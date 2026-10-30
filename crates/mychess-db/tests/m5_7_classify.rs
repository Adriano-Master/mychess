//! Contract tests of `classify_database` and `classify_preview` (M5-7a, D1186,
//! D1199 to D1201; Scid `sc_eco_base`, tkscid.cpp 4699-4843). Expected codes
//! are derived by hand from the book in `support::BOOK_TEXT`.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::assert_is_empty,
    clippy::unnecessary_wraps,
    clippy::manual_assert_eq
)]

#[path = "common/si4_builder.rs"]
mod si4_builder;
#[path = "common/m5_7_support.rs"]
mod support;

use std::collections::HashSet;

use mychess_core::{CastlingMode, EcoBook, Game, GameResult, PartialDate, Position};
use mychess_db::si4::{GameFlags, Si4Writer};
use mychess_db::{
    CancelToken, ClassifyError, ClassifyReport, ClassifySelection, Filter, GameSource, NoProgress,
    classify_database, classify_preview,
};
use pretty_assertions::assert_eq;
use si4_builder::{
    EntrySpec, IndexHeader, MemDb, Parts, default_names, index_file, open_parts, rec, rec_fen,
};
use support::{
    Spec, book, code, codes, date, deep_book, eco_offset, index_bytes, long_line, open, san_game,
    spec, write_db,
};

const E4: u8 = 0xCF;
const NF3: u8 = 0x67;
const NC6: u8 = 0x22;
const END_GAME: u8 = 0x0F;

fn run(base: &std::path::Path, selection: ClassifySelection<'_>, extended: bool) -> ClassifyReport {
    let mut writer = Si4Writer::open(base).unwrap();
    classify_database(
        &mut writer,
        &book(),
        selection,
        extended,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap()
}

fn preview(
    db: &mut impl GameSource,
    selection: ClassifySelection<'_>,
    extended: bool,
) -> ClassifyReport {
    classify_preview(
        db,
        &book(),
        selection,
        extended,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap()
}

/// Seven games with known codes under `BOOK_TEXT`:
/// C44b, C44b (transposition), A45, A00a, B01, C40a, A00a (no moves).
fn sample_specs() -> Vec<Spec> {
    vec![
        spec(&["e4", "e5", "Nf3", "Nc6", "d4", "exd4"]),
        spec(&["Nf3", "e5", "e4", "Nc6", "d4"]),
        spec(&["d4", "Nf6", "c4"]),
        spec(&["d4", "d5"]),
        spec(&["e4", "d5", "exd5"]),
        spec(&["e4", "e5", "Nf3", "d6"]),
        spec(&["h4"]),
    ]
}

fn expected_extended() -> Vec<u16> {
    ["C44b", "C44b", "A45", "A00a", "B01", "C40a", "A00a"]
        .iter()
        .map(|c| code(c))
        .collect()
}

// ------------------------------------------------------------ All

#[test]
fn all_classifies_by_deepest_book_position_with_transpositions() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &sample_specs());
    let report = run(&base, ClassifySelection::All, true);
    assert_eq!(codes(&base), expected_extended());
    assert_eq!(
        report,
        ClassifyReport {
            classified: 7,
            changed: 7,
            written: 7,
            unreadable: 0,
            cancelled: false
        }
    );
}

#[test]
fn not_extended_writes_only_basic_codes() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &sample_specs());
    run(&base, ClassifySelection::All, false);
    let expected: Vec<u16> = ["C44", "C44", "A45", "A00", "B01", "C40", "A00"]
        .iter()
        .map(|c| code(c))
        .collect();
    assert_eq!(codes(&base), expected);
}

#[test]
fn all_overwrites_a_wrong_old_code() {
    let dir = tempfile::tempdir().unwrap();
    let mut specs = sample_specs();
    specs[2] = spec(&["d4", "Nf6", "c4"]).eco("E99z4");
    let base = write_db(dir.path(), &specs);
    let report = run(&base, ClassifySelection::All, true);
    assert_eq!(codes(&base)[2], code("A45"));
    assert_eq!(report.changed, 7);
}

#[test]
fn second_run_changes_and_writes_nothing_and_leaves_the_file_identical() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &sample_specs());
    run(&base, ClassifySelection::All, true);
    let bytes = index_bytes(&base);
    let again = run(&base, ClassifySelection::All, true);
    assert_eq!(
        (again.classified, again.changed, again.written),
        (7, 0, 0),
        "{again:?}"
    );
    assert_eq!(bytes, index_bytes(&base));
}

#[test]
fn classification_touches_only_the_eco_bytes_of_the_index() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &sample_specs());
    let before = index_bytes(&base);
    let sg4 = std::fs::read(base.with_extension("sg4")).unwrap();
    run(&base, ClassifySelection::All, true);
    let after = index_bytes(&base);
    assert_eq!(before.len(), after.len());
    let eco_bytes: HashSet<usize> = (1..=7)
        .flat_map(|n| [eco_offset(n), eco_offset(n) + 1])
        .collect();
    for i in 0..before.len() {
        if before[i] != after[i] {
            assert!(
                eco_bytes.contains(&i),
                "byte {i} changed outside the ECO field"
            );
        }
    }
    assert_eq!(sg4, std::fs::read(base.with_extension("sg4")).unwrap());
}

#[test]
fn deleted_games_are_classified_too() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &[spec(&["d4", "Nf6"]).deleted()]);
    run(&base, ClassifySelection::All, true);
    let entry = open(&base).entry(1).unwrap();
    assert_eq!(entry.eco_code, code("A45"));
    assert!(entry.flags.contains(GameFlags::DELETE));
}

#[test]
fn game_with_no_moves_matches_the_start_entry_when_the_record_exists() {
    // A game without moves still has a record; the start entry matches at
    // ply 0 (spec rule 4).
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(
        dir.path(),
        &[Spec {
            game: Game::new(),
            ..spec(&[])
        }],
    );
    let report = run(&base, ClassifySelection::All, true);
    assert_eq!(codes(&base), [code("A00a")]);
    assert_eq!(report.classified, 1);
}

#[test]
fn all_erases_the_code_of_a_game_that_matches_nothing_in_the_book() {
    // A book with no start entry: 1.h4 matches nothing.
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(
        dir.path(),
        &[spec(&["h4"]).eco("B01"), spec(&["e4", "e5"]).eco("E99")],
    );
    let no_start = EcoBook::parse("C20 \"KP\" 1.e4 e5 *\n").unwrap();
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = classify_database(
        &mut writer,
        &no_start,
        ClassifySelection::All,
        true,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    drop(writer);
    assert_eq!(codes(&base), [0, code("C20")]);
    assert_eq!(
        (report.classified, report.changed, report.written),
        (2, 2, 2)
    );
}

#[test]
fn a_game_that_matches_nothing_and_has_no_code_is_classified_but_not_changed() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &[spec(&["h4"])]);
    let no_start = EcoBook::parse("C20 \"KP\" 1.e4 e5 *\n").unwrap();
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = classify_database(
        &mut writer,
        &no_start,
        ClassifySelection::NoCode,
        true,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(
        (report.classified, report.changed, report.written),
        (1, 0, 0)
    );
}

// ------------------------------------------------------------ selections

#[test]
fn nocode_never_touches_games_that_already_have_a_code() {
    let dir = tempfile::tempdir().unwrap();
    let mut specs = sample_specs();
    specs[0] = spec(&["e4", "e5", "Nf3", "Nc6", "d4"]).eco("E00"); // wrong on purpose
    specs[4] = spec(&["e4", "d5"]).eco("A00");
    let base = write_db(dir.path(), &specs);
    let report = run(&base, ClassifySelection::NoCode, true);
    let got = codes(&base);
    assert_eq!(got[0], code("E00"));
    assert_eq!(got[4], code("A00"));
    assert_eq!(got[1], code("C44b"));
    assert_eq!(got[2], code("A45"));
    assert_eq!(report.classified, 5, "only the five without a code");
    assert_eq!(report.changed, 5);
}

#[test]
fn nocode_with_basic_codes() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(
        dir.path(),
        &[
            spec(&["e4", "e5", "Nf3"]).eco("A00"),
            spec(&["e4", "e5", "Nf3"]),
        ],
    );
    run(&base, ClassifySelection::NoCode, false);
    assert_eq!(codes(&base), [code("A00"), code("C40")]);
}

#[test]
fn filter_selection_classifies_only_games_in_the_filter() {
    let dir = tempfile::tempdir().unwrap();
    let mut specs = sample_specs();
    specs[3] = spec(&["d4", "d5"]).eco("E01");
    let base = write_db(dir.path(), &specs);
    let mut filter = Filter::new(7);
    for n in [2, 4, 6] {
        filter.set(n, 0);
    }
    let report = run(&base, ClassifySelection::Filter(&filter), true);
    let expected = expected_extended();
    let got = codes(&base);
    for n in [1usize, 3, 5, 7] {
        assert_eq!(got[n - 1], expected[n - 1], "game {n} is in the filter");
    }
    for n in [2usize, 6] {
        assert_eq!(got[n - 1], 0, "game {n} is outside the filter");
    }
    assert_eq!(got[3], code("E01"), "game 4 keeps its old code");
    assert_eq!(report.classified, 4);
}

#[test]
fn filter_selection_with_an_empty_filter_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &sample_specs());
    let mut filter = Filter::new(7);
    filter.fill(0);
    let before = index_bytes(&base);
    let report = run(&base, ClassifySelection::Filter(&filter), true);
    assert_eq!(report.classified, 0);
    assert_eq!(before, index_bytes(&base));
}

#[test]
fn since_skips_earlier_games_and_games_with_unknown_date() {
    // Arrange: dates around the cut-off 2000.01.01.
    let dir = tempfile::tempdir().unwrap();
    let specs = vec![
        spec(&["d4", "Nf6"]).date(date(2000, 1, 1)), // on the date: in
        spec(&["d4", "Nf6"]).date(date(1999, 12, 31)), // before: out
        spec(&["d4", "Nf6"]),                        // unknown date: out
        spec(&["d4", "Nf6"]).date(date(2024, 6, 1)), // after: in
        spec(&["d4", "Nf6"]).date(PartialDate::new(Some(2001), None, None).unwrap()), // year only: in
        spec(&["d4", "Nf6"]).date(PartialDate::new(Some(2000), None, None).unwrap()), // 2000.??.??: before 2000.01.01 in Scid's encoding
    ];
    let base = write_db(dir.path(), &specs);
    // Act
    let report = run(&base, ClassifySelection::Since(date(2000, 1, 1)), true);
    // Assert
    let a45 = code("A45");
    assert_eq!(codes(&base), [a45, 0, 0, a45, a45, 0]);
    assert_eq!(report.classified, 3);
}

#[test]
fn since_with_a_year_only_start_includes_that_whole_year() {
    let dir = tempfile::tempdir().unwrap();
    let specs = vec![
        spec(&["d4", "Nf6"]).date(date(1999, 12, 31)),
        spec(&["d4", "Nf6"]).date(date(2000, 1, 1)),
        spec(&["d4", "Nf6"]).date(PartialDate::new(Some(2000), None, None).unwrap()),
    ];
    let base = write_db(dir.path(), &specs);
    run(
        &base,
        ClassifySelection::Since(PartialDate::new(Some(2000), None, None).unwrap()),
        true,
    );
    let a45 = code("A45");
    assert_eq!(codes(&base), [0, a45, a45]);
}

// ------------------------------------------------------------ preview

#[test]
fn preview_reports_what_would_change_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut specs = sample_specs();
    specs[2] = spec(&["d4", "Nf6", "c4"]).eco("A45"); // already right
    let base = write_db(dir.path(), &specs);
    let before = index_bytes(&base);
    let mut db = open(&base);
    let report = preview(&mut db, ClassifySelection::All, true);
    assert_eq!(
        report,
        ClassifyReport {
            classified: 7,
            changed: 6,
            written: 0,
            unreadable: 0,
            cancelled: false
        }
    );
    assert_eq!(before, index_bytes(&base));
}

#[test]
fn preview_changed_equals_what_the_real_run_changes() {
    for selection_kind in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let mut specs = sample_specs();
        specs[1] = spec(&["Nf3", "e5", "e4", "Nc6", "d4"]).eco("C44b");
        specs[4] = spec(&["e4", "d5", "exd5"]).eco("A00");
        let base = write_db(dir.path(), &specs);
        let mut filter = Filter::new(7);
        filter.set(1, 0);
        let selection = match selection_kind {
            0 => ClassifySelection::All,
            1 => ClassifySelection::NoCode,
            _ => ClassifySelection::Filter(&filter),
        };
        let mut db = open(&base);
        let would = preview(&mut db, selection, true);
        drop(db);
        let did = run(&base, selection, true);
        assert_eq!(would.classified, did.classified, "kind {selection_kind}");
        assert_eq!(would.changed, did.changed, "kind {selection_kind}");
        assert_eq!(would.changed, did.written, "kind {selection_kind}");
        assert_eq!(would.written, 0);
    }
}

#[test]
fn preview_works_on_a_database_that_cannot_be_written() {
    let mut db = mem(&[(rec(&[E4, E4, NF3, END_GAME], &[]), None)]);
    let report = preview(&mut db, ClassifySelection::All, true);
    assert_eq!(
        (report.classified, report.changed, report.written),
        (1, 1, 0)
    );
}

// ------------------------------------------------------------ errors

#[test]
fn an_empty_book_is_rejected_by_both_entry_points() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &sample_specs());
    let before = index_bytes(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    let result = classify_database(
        &mut writer,
        &EcoBook::default(),
        ClassifySelection::All,
        true,
        &CancelToken::new(),
        &mut NoProgress,
    );
    assert!(matches!(result, Err(ClassifyError::EmptyBook)));
    drop(writer);
    let mut db = open(&base);
    let result = classify_preview(
        &mut db,
        &EcoBook::default(),
        ClassifySelection::All,
        true,
        &CancelToken::new(),
        &mut NoProgress,
    );
    assert!(matches!(result, Err(ClassifyError::EmptyBook)));
    assert_eq!(before, index_bytes(&base), "an empty book erases nothing");
}

#[test]
fn an_empty_database_classifies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &[]);
    let report = run(&base, ClassifySelection::All, true);
    assert_eq!(report, ClassifyReport::default());
}

// ------------------------------------------------------------ cancel / progress

fn many(n: usize) -> Vec<Spec> {
    (0..n).map(|_| spec(&["d4", "Nf6"])).collect()
}

#[test]
fn a_token_cancelled_before_the_start_returns_ok_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &many(2500));
    let before = index_bytes(&base);
    let cancel = CancelToken::new();
    cancel.cancel();
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = classify_database(
        &mut writer,
        &book(),
        ClassifySelection::All,
        true,
        &cancel,
        &mut NoProgress,
    )
    .unwrap();
    drop(writer);
    assert!(report.cancelled);
    assert_eq!(report.written, 0);
    assert!(report.classified < 2500, "{report:?}");
    assert_eq!(before, index_bytes(&base));
}

#[test]
fn cancel_during_progress_returns_ok_keeps_what_was_written_and_stops_early() {
    // Arrange: more games than one check interval (1000); the sink cancels
    // at the first progress report.
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &many(2500));
    let cancel = CancelToken::new();
    let cancel_in_sink = cancel.clone();
    let mut calls = Vec::new();
    let mut sink = |done: u64, total: u64| {
        calls.push((done, total));
        cancel_in_sink.cancel();
    };
    // Act
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = classify_database(
        &mut writer,
        &book(),
        ClassifySelection::All,
        true,
        &cancel,
        &mut sink,
    )
    .unwrap();
    drop(writer);
    // Assert
    assert!(report.cancelled);
    assert!(!calls.is_empty());
    assert!(calls.iter().all(|&(d, t)| d <= t && t == 2500), "{calls:?}");
    assert!(
        report.classified > 0 && report.classified < 2500,
        "{report:?}"
    );
    assert_eq!(report.written, report.changed);
    let all = codes(&base);
    let written_codes = all.iter().filter(|&&c| c != 0).count();
    assert_eq!(
        written_codes as u32, report.written,
        "what was flushed stays"
    );
    assert_eq!(all[0], code("A45"), "the first games were stored");
    assert_eq!(*all.last().unwrap(), 0, "the tail was not reached");
    // A second run finishes the job: only the missing games change.
    let rest = run(&base, ClassifySelection::NoCode, true);
    assert_eq!(rest.classified + report.written, 2500);
    assert!(codes(&base).iter().all(|&c| c == code("A45")));
}

#[test]
fn progress_ends_with_done_equal_to_total_when_not_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &many(1500));
    let mut calls: Vec<(u64, u64)> = Vec::new();
    let mut sink = |done: u64, total: u64| calls.push((done, total));
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = classify_database(
        &mut writer,
        &book(),
        ClassifySelection::All,
        true,
        &CancelToken::new(),
        &mut sink,
    )
    .unwrap();
    assert!(!report.cancelled);
    assert_eq!(report.classified, 1500);
    assert_eq!(calls.last(), Some(&(1500, 1500)));
    assert!(
        calls.windows(2).all(|w| w[0].0 <= w[1].0),
        "monotonic: {calls:?}"
    );
    assert!(codes(&base).iter().all(|&c| c == code("A45")));
}

// ------------------------------------------------------------ plies (60)

#[test]
fn batch_classification_looks_only_at_the_first_60_plies() {
    // Arrange
    let line = long_line(70);
    let book = deep_book(&line);
    let as_strs = |n: usize| line[..n].iter().map(String::as_str).collect::<Vec<_>>();
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(
        dir.path(),
        &[
            spec(&as_strs(40)),
            spec(&as_strs(59)),
            spec(&as_strs(60)),
            spec(&as_strs(61)),
            spec(&as_strs(70)),
        ],
    );
    // Act
    let mut writer = Si4Writer::open(&base).unwrap();
    classify_database(
        &mut writer,
        &book,
        ClassifySelection::All,
        true,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    drop(writer);
    // Assert: ply 60 still counts; the entry at ply 61 is never reached.
    assert_eq!(
        codes(&base),
        [
            code("C20"),
            code("C20"),
            code("C30"),
            code("C30"),
            code("C30")
        ]
    );
}

// ------------------------------------------------------------ record damage / FEN / empty

fn mem(items: &[(Vec<u8>, Option<EntrySpec>)]) -> MemDb {
    let mut sg4 = Vec::new();
    let mut entries = Vec::new();
    for (record, spec) in items {
        let mut e = spec.clone().unwrap_or_else(|| EntrySpec::new(0, 0));
        e.offset = sg4.len() as u32;
        if e.length == 0 && !record.is_empty() {
            e.length = record.len() as u32;
        }
        e.half_moves = 4;
        entries.push(e.encode());
        sg4.extend_from_slice(record);
    }
    open_parts(&Parts {
        index: index_file(&IndexHeader::default(), &entries),
        names: default_names(),
        sg4,
    })
    .unwrap()
}

fn entry_with(f: impl FnOnce(&mut EntrySpec)) -> Option<EntrySpec> {
    let mut e = EntrySpec::new(0, 0);
    f(&mut e);
    Some(e)
}

#[test]
fn a_record_that_breaks_after_some_moves_is_classified_by_the_moves_read() {
    // Arrange: 1.e4 e5 2.Nf3 then an invalid code, an illegal move and a cut.
    let mut cut = rec(&[], &[]);
    cut.extend_from_slice(&[E4, E4, NF3]);
    let mut db = mem(&[
        (rec(&[E4, E4, NF3, 0x20, END_GAME], &[]), None),
        (rec(&[E4, E4, NF3, 0x11, END_GAME], &[]), None),
        (cut, None),
    ]);
    // Act
    let report = preview(&mut db, ClassifySelection::All, true);
    // Assert
    assert_eq!((report.classified, report.unreadable), (3, 0));
    assert_eq!(report.changed, 3);
}

#[test]
fn a_record_that_breaks_before_the_first_move_is_unreadable_and_skipped() {
    // An undecodable first move code, and a record cut right after the
    // prelude: both give nothing to classify.
    let mut db = mem(&[
        (rec(&[0x20, E4, END_GAME], &[]), entry_with(|e| e.eco = 5)),
        (rec(&[], &[]), None),
        (rec(&[E4, E4, NF3, NC6, END_GAME], &[]), None),
    ]);
    let report = preview(&mut db, ClassifySelection::All, true);
    assert_eq!(report.unreadable, 2);
    assert_eq!(report.classified, 1);
}

#[test]
fn an_illegal_first_move_is_classified_as_if_no_move_was_played() {
    // Scid: DecodeNextMove fails at once and the game is looked up at its
    // start position (A00a here).
    let mut db = mem(&[(
        rec(&[0x11, E4, END_GAME], &[]),
        entry_with(|e| e.eco = code("A00a")),
    )]);
    let report = preview(&mut db, ClassifySelection::All, true);
    assert_eq!((report.classified, report.changed), (1, 0), "{report:?}");
}

#[test]
fn damaged_game_codes_are_the_deepest_match_before_the_damage() {
    // Writing run over a file database built from damaged raw records is not
    // possible with Si4Writer, so check the preview counts instead: the game
    // damaged after 1.e4 e5 2.Nf3 must differ from "no code" (C40a != 0).
    let mut db = mem(&[(rec(&[E4, E4, NF3, 0x20, END_GAME], &[]), None)]);
    let report = preview(&mut db, ClassifySelection::NoCode, true);
    assert_eq!(report.changed, 1);
    // And with the stored code already equal to C40a nothing would change.
    let mut db = mem(&[(
        rec(&[E4, E4, NF3, 0x20, END_GAME], &[]),
        entry_with(|e| e.eco = code("C40a")),
    )]);
    let report = preview(&mut db, ClassifySelection::All, true);
    assert_eq!((report.classified, report.changed), (1, 0));
}

#[test]
fn entries_with_length_zero_are_ignored_not_counted() {
    let mut db = mem(&[
        (Vec::new(), entry_with(|e| e.eco = code("B01"))),
        (rec(&[E4, E4, NF3, END_GAME], &[]), None),
    ]);
    let report = preview(&mut db, ClassifySelection::All, true);
    assert_eq!(
        (report.classified, report.changed, report.unreadable),
        (1, 1, 0)
    );
}

const AFTER_E4_E5: &str = "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2";

fn fen_game(fen: &str, stream: &[u8], eco: u16) -> (Vec<u8>, Option<EntrySpec>) {
    (
        rec_fen(fen, stream, &[]),
        entry_with(|e| {
            e.flags = 1; // START
            e.eco = eco;
        }),
    )
}

fn from_fen_spec(fen: &str, sans: &[&str]) -> Spec {
    let mut game = Game::from_position(Position::from_fen(fen).unwrap());
    let mut node = game.root();
    for san in sans {
        let position = game.position_at(node).unwrap().clone();
        let parsed = position
            .parse_san(san, mychess_core::SanMode::Strict)
            .unwrap();
        node = game.add_move(node, parsed.mv).unwrap();
    }
    Spec { game, ..spec(&[]) }
}

#[test]
fn standard_game_with_its_own_start_position_is_classified_from_it() {
    // Arrange: the start FEN is the position after 1.e4 e5 (C20); with Nf3
    // the game reaches C40a; with no moves it is C20 at ply 0.
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(
        dir.path(),
        &[
            from_fen_spec(AFTER_E4_E5, &["Nf3"]),
            from_fen_spec(AFTER_E4_E5, &[]),
            from_fen_spec(AFTER_E4_E5, &["Nf3", "Nc6", "d4"]),
        ],
    );
    assert!(
        open(&base)
            .entry(1)
            .unwrap()
            .flags
            .contains(GameFlags::START)
    );
    // Act
    let report = run(&base, ClassifySelection::All, true);
    // Assert
    assert_eq!(codes(&base), [code("C40a"), code("C20"), code("C44b")]);
    assert_eq!(report.classified, 3);
}

#[test]
fn start_fen_game_that_matches_no_book_position_gets_no_code() {
    // Not even the start entry applies: the game does not start there.
    let dir = tempfile::tempdir().unwrap();
    let mut with_old = from_fen_spec("4k3/8/8/8/8/8/8/4K2R w K - 0 1", &["Rh2"]);
    with_old.eco = Some(support::eco("A00a"));
    let base = write_db(dir.path(), &[with_old]);
    let report = run(&base, ClassifySelection::All, true);
    assert_eq!(codes(&base), [0]);
    assert_eq!((report.classified, report.changed), (1, 1));
}

#[test]
fn chess960_start_is_not_classified() {
    let fen = "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w HFhf - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    assert_eq!(pos.castling_mode(), CastlingMode::Chess960, "fixture guard");
    let mut db = mem(&[fen_game(fen, &[END_GAME], 0)]);
    let report = preview(&mut db, ClassifySelection::All, true);
    // Nothing found and nothing stored: classified, but no change.
    assert_eq!((report.classified, report.changed), (1, 0));
}

#[test]
fn unreadable_start_fen_is_counted_unreadable_and_keeps_the_old_code() {
    // D1201/D1205: a start FEN the core rejects makes the game unreadable.
    let mut db = mem(&[fen_game("not a fen", &[END_GAME], code("B01"))]);
    let report = preview(&mut db, ClassifySelection::All, true);
    assert_eq!(
        (report.classified, report.unreadable, report.changed),
        (0, 1, 0),
        "{report:?}"
    );
}

#[test]
fn a_token_cancelled_before_the_start_stops_even_a_small_database() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &sample_specs());
    let before = index_bytes(&base);
    let cancel = CancelToken::new();
    cancel.cancel();
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = classify_database(
        &mut writer,
        &book(),
        ClassifySelection::All,
        true,
        &cancel,
        &mut NoProgress,
    )
    .unwrap();
    drop(writer);
    assert!(report.cancelled);
    assert_eq!((report.classified, report.written), (0, 0));
    assert_eq!(before, index_bytes(&base));
}

#[test]
fn set_eco_batch_counted_reports_progress_before_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &sample_specs());
    let mut writer = Si4Writer::open(&base).unwrap();
    let (changed, result) = writer.set_eco_batch_counted(&[
        (1, Some(support::eco("C20"))),
        (2, Some(support::eco("C21"))),
        (99, Some(support::eco("C22"))),
        (3, Some(support::eco("C23"))),
    ]);
    drop(writer);
    assert_eq!(changed, 2);
    assert!(result.is_err());
    assert_eq!(&codes(&base)[..3], &[code("C20"), code("C21"), 0]);
}

#[test]
fn san_game_helper_matches_the_book_semantics() {
    // Guard: the transposition used in the samples really reaches the same
    // board as the Scotch entry.
    let a = san_game(&["e4", "e5", "Nf3", "Nc6", "d4"]);
    let b = san_game(&["Nf3", "e5", "e4", "Nc6", "d4"]);
    let end = |g: &Game| g.position_at(g.mainline().last().unwrap()).unwrap().clone();
    assert!(end(&a) == end(&b));
    assert_eq!(GameResult::default(), GameResult::Unknown);
}

#[test]
fn since_an_unknown_date_still_skips_games_with_unknown_date() {
    // D1186: games of unknown date are skipped even when the start is
    // itself unknown (Scid's plain `<` comparison would let them through).
    let dir = tempfile::tempdir().unwrap();
    let specs = vec![
        spec(&["d4", "Nf6"]),
        spec(&["d4", "Nf6"]).date(date(1990, 1, 1)),
    ];
    let base = write_db(dir.path(), &specs);
    run(
        &base,
        ClassifySelection::Since(PartialDate::new(None, None, None).unwrap()),
        true,
    );
    assert_eq!(codes(&base), [0, code("A45")]);
}
