//! Contract tests of the PGN (and `.pgn.gz`) opened as a read-only database
//! (spec M3-3, "Abrir PGN ou `.pgn.gz` como banco somente leitura", rules
//! 18-21 and 52-54, D351-D355, D403-D411 and the acceptance criteria "PGN
//! como banco"). Derived from the spec and from Scid's `file.tcl` /
//! `sc_base import`, not from the implementation.
#![allow(
    clippy::assertions_on_result_states,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::needless_pass_by_value,
    clippy::field_reassign_with_default
)]

#[path = "common/m3_3b_support.rs"]
mod support;

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use mychess_core::{Eco, Game, GameResult, Nag, NodeId, PartialDate};
use mychess_db::si4::{NameKind, Si4Writer};
use mychess_db::{
    CancelToken, EcoRange, ExportItem, ExportOptions, Filter, GameSource, HeaderSearch,
    ImportError, ImportOptions, MinMax, NoProgress, OpenOptions, OpenReport, PgnDatabase,
    PgnOpenError, PlayerStats, READ_ONLY_DESCRIPTION, ResultSet, UnsupportedKind, import_pgn,
    search_header,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;
use support::{THREE, canon, file_names, gzip_stored, import_bytes, import_str};

// ====================================================================
// Helpers
// ====================================================================

/// `n` varied games: names, Elo, dates, rounds, ECO tags, comments,
/// variations and the four results.
fn corpus(n: usize) -> String {
    let results = ["1-0", "0-1", "1/2-1/2", "*"];
    let mut pgn = String::new();
    for i in 0..n {
        let res = results[i % 4];
        write!(
            pgn,
            "[Event \"Ev{}\"]\n[Site \"S{}\"]\n[Date \"{}.{:02}.{:02}\"]\n[Round \"{}\"]\n\
             [White \"P{}, X\"]\n[Black \"Q{}, Y\"]\n[Result \"{res}\"]\n\
             [WhiteElo \"{}\"]\n[BlackElo \"{}\"]\n",
            i % 11,
            i % 3,
            2000 + i % 20,
            1 + i % 12,
            1 + i % 28,
            1 + i % 4,
            i % 7,
            i % 5,
            2000 + (i % 50) * 10,
            1900 + (i % 40) * 10,
        )
        .unwrap();
        if i % 4 == 0 {
            pgn.push_str("[ECO \"B20\"]\n");
        }
        let comment = if i % 3 == 0 {
            format!(" {{good move {i}}}")
        } else {
            String::new()
        };
        let moves = match i % 4 {
            0 => format!("1. e4 c5 2. Nf3 d6 3. d4 cxd4{comment} 4. Nxd4 Nf6"),
            1 => format!("1. d4 d5 2. c4 e6{comment} 3. Nc3 Nf6"),
            2 => format!("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6{comment}"),
            _ => format!("1. c4 e5 (1... c5 2. Nf3) 2. Nc3 Nf6{comment}"),
        };
        write!(pgn, "\n{moves} {res}\n\n").unwrap();
    }
    pgn
}

fn put(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn open_with(
    path: &Path,
    options: &OpenOptions,
) -> Result<(PgnDatabase, OpenReport), PgnOpenError> {
    PgnDatabase::open(path, options, &mut NoProgress, &CancelToken::new())
}

fn open(path: &Path) -> (PgnDatabase, OpenReport) {
    open_with(path, &OpenOptions::default()).unwrap()
}

fn open_err(path: &Path, options: &OpenOptions) -> PgnOpenError {
    match open_with(path, options) {
        Ok((db, _)) => panic!("expected an error, got a database of {} games", db.len()),
        Err(e) => e,
    }
}

fn assert_same<A: GameSource, B: GameSource>(a: &mut A, b: &mut B) {
    assert_eq!(a.len(), b.len(), "number of games");
    for n in 1..=a.len() {
        assert_eq!(a.entry(n).unwrap(), b.entry(n).unwrap(), "entry {n}");
        assert_eq!(
            a.record_bytes(n).unwrap().to_vec(),
            b.record_bytes(n).unwrap().to_vec(),
            "record bytes {n}"
        );
        assert_eq!(a.raw_game(n).unwrap(), b.raw_game(n).unwrap(), "raw {n}");
        let (ga, gb) = (a.game(n).unwrap(), b.game(n).unwrap());
        assert!(
            ga.same_content(&gb),
            "game {n}:\n{}\n{}",
            canon(&ga),
            canon(&gb)
        );
    }
    for kind in NameKind::ALL {
        let count = a.names().num_names(kind);
        assert_eq!(count, b.names().num_names(kind), "{kind:?} count");
        assert_eq!(a.names().max_frequency(kind), b.names().max_frequency(kind));
        for id in 0..count {
            assert_eq!(
                a.names().get(kind, id),
                b.names().get(kind, id),
                "{kind:?} {id}"
            );
            assert_eq!(
                a.names().frequency(kind, id),
                b.names().frequency(kind, id),
                "{kind:?} {id} frequency"
            );
        }
    }
}

fn run_search<D: GameSource>(db: &mut D, query: &HeaderSearch) -> Vec<u32> {
    let stats = PlayerStats::from_database(&*db);
    let mut filter = Filter::new(db.len());
    search_header(
        db,
        query,
        &mut filter,
        &stats,
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    filter.iter_games().collect()
}

fn year(y: u16) -> PartialDate {
    PartialDate::new(Some(y), None, None).unwrap()
}

fn eco(code: &str) -> Eco {
    Eco::parse_prefix(code).unwrap().0
}

fn mainline_moves(game: &Game) -> Vec<mychess_core::Move> {
    game.mainline().map(|id| game.mv(id).unwrap()).collect()
}

/// A reader that gives `data` and then fails.
struct FailingReader {
    data: io::Cursor<Vec<u8>>,
}

impl Read for FailingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.data.read(buf)?;
        if n == 0 {
            return Err(io::Error::other("disk on fire"));
        }
        Ok(n)
    }
}

// ====================================================================
// .pgn and .pgn.gz
// ====================================================================

#[test]
fn pgn_and_pgn_gz_with_more_than_one_batch_open_identical_databases() {
    let text = corpus(250);
    let dir = tempfile::tempdir().unwrap();
    let plain = put(dir.path(), "x.pgn", text.as_bytes());
    let gz = put(dir.path(), "x.pgn.gz", &gzip_stored(text.as_bytes()));
    let (mut a, ra) = open(&plain);
    let (mut b, rb) = open(&gz);
    assert_eq!(a.len(), 250);
    assert_eq!(ra.games_added, 250);
    assert_eq!(rb.games_added, 250);
    assert_eq!(ra.games_read, rb.games_read);
    assert_same(&mut a, &mut b);
    assert_eq!(ra.to_string(), rb.to_string());
}

#[test]
fn gz_is_detected_by_content_not_by_extension() {
    let dir = tempfile::tempdir().unwrap();
    let gz = gzip_stored(THREE.as_bytes());
    for name in ["a.pgn", "a.txt", "a", "a.zip", "a.gz"] {
        let path = put(dir.path(), name, &gz);
        let (db, report) = open(&path);
        assert_eq!(db.len(), 3, "{name}");
        assert_eq!(report.games_added, 3);
    }
}

#[test]
fn concatenated_gzip_members_are_read_as_one_stream() {
    let all = corpus(30);
    let (first, second) = all.split_at(all.find("[Event \"Ev5\"]").unwrap());
    let mut gz = gzip_stored(first.as_bytes());
    gz.extend(gzip_stored(second.as_bytes()));
    let dir = tempfile::tempdir().unwrap();
    let multi = put(dir.path(), "m.pgn.gz", &gz);
    let plain = put(dir.path(), "m.pgn", all.as_bytes());
    let (mut a, _) = open(&multi);
    let (mut b, _) = open(&plain);
    assert_eq!(a.len(), 30);
    assert_same(&mut a, &mut b);
}

#[rstest]
#[case::no_extension("noext")]
#[case::txt("games.txt")]
#[case::dat("games.dat")]
#[case::upper("GAMES.PGN")]
#[case::pgn("games.pgn")]
fn any_extension_with_text_content_opens_as_pgn(#[case] name: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), name, THREE.as_bytes());
    let (db, _) = open(&path);
    assert_eq!(db.len(), 3);
}

#[test]
fn a_pgn_named_zip_opens_and_a_zip_named_pgn_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let pgn_as_zip = put(dir.path(), "real.zip", THREE.as_bytes());
    assert_eq!(open(&pgn_as_zip).0.len(), 3);

    let mut zip = b"PK\x03\x04".to_vec();
    zip.extend_from_slice(&[0x14, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(THREE.as_bytes());
    let zip_as_pgn = put(dir.path(), "fake.pgn", &zip);
    match open_err(&zip_as_pgn, &OpenOptions::default()) {
        PgnOpenError::UnsupportedFormat { kind, path } => {
            assert_eq!(kind, UnsupportedKind::Zip);
            assert_eq!(path, zip_as_pgn);
        }
        other => panic!("{other:?}"),
    }
}

// ====================================================================
// Properties of the opened database
// ====================================================================

#[test]
fn opened_database_is_type_3_read_only_with_the_scid_description_and_full_filter() {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", corpus(120).as_bytes());
    let (db, _) = open(&path);
    assert_eq!(db.base_type(), 3);
    assert!(db.is_read_only());
    assert_eq!(db.header().base_type(), 3);
    let description = String::from_utf8_lossy(db.header().description()).into_owned();
    assert_eq!(description.trim_end_matches('\0'), READ_ONLY_DESCRIPTION);
    assert_eq!(
        READ_ONLY_DESCRIPTION,
        "This database is read-only and can't be altered."
    );
    let filter = db.new_filter();
    assert_eq!(filter.len(), 120);
    assert_eq!(filter.count(), 120);
    assert!((1..=120).all(|n| filter.get(n) == 1));
}

#[rstest]
#[case::plain(false)]
#[case::gz(true)]
fn opening_creates_no_scid_file_and_leaves_the_source_untouched(#[case] gz: bool) {
    let dir = tempfile::tempdir().unwrap();
    let text = corpus(150);
    let (name, bytes) = if gz {
        ("x.pgn.gz", gzip_stored(text.as_bytes()))
    } else {
        ("x.pgn", text.into_bytes())
    };
    let path = put(dir.path(), name, &bytes);
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    let (mut db, _) = open(&path);
    drop(db.game(1).unwrap());
    drop(db);
    assert_eq!(file_names(dir.path()), vec![name.to_owned()]);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
}

#[test]
fn failed_openings_leave_no_file_in_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", THREE.as_bytes());
    let opts = OpenOptions {
        max_games: 1,
        ..OpenOptions::default()
    };
    drop(open_err(&path, &opts));
    assert_eq!(file_names(dir.path()), vec!["x.pgn".to_owned()]);
}

#[test]
fn report_is_never_interrupted_or_stopped_and_added_covers_every_game() {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", corpus(130).as_bytes());
    let (db, report) = open(&path);
    assert!(!report.interrupted);
    assert!(report.stopped.is_none());
    assert_eq!(report.added, 1..131);
    assert_eq!(report.games_added, db.len());
    assert_eq!(report.games_skipped_total, 0);
    assert!(!report.has_warnings(), "{report}");
}

#[test]
fn reading_warnings_do_not_prevent_opening_and_cite_game_and_line() {
    let pgn = THREE.replace("1. d4 d5 2. c4 0-1", "1. d4 d5 2. Ke5 0-1");
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", pgn.as_bytes());
    let (mut db, report) = open(&path);
    assert_eq!(db.len(), 3);
    assert!(report.has_warnings());
    let text = report.to_string();
    let first = text.lines().next().unwrap();
    let rest = first
        .strip_prefix("(game 2, line ")
        .unwrap_or_else(|| panic!("{text}"));
    let line: usize = rest.split_once(')').unwrap().0.parse().unwrap();
    let game2 = pgn
        .lines()
        .position(|l| l.starts_with("[Event \"Two"))
        .unwrap()
        + 1;
    let game3 = pgn
        .lines()
        .position(|l| l.starts_with("[Event \"Three"))
        .unwrap()
        + 1;
    assert!((game2..game3).contains(&line), "{text}");
    assert!(!report.interrupted && report.stopped.is_none());
    // what was read before the error is kept, as in an import
    assert_eq!(db.game(2).unwrap().mainline().count(), 2);
    // the report equals the one of the import (D407)
    let imp = import_str(&pgn, &ImportOptions::default());
    assert_eq!(text, imp.report.to_string());
}

#[test]
fn path_is_canonical_and_stable_across_spellings() {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", THREE.as_bytes());
    let canonical = fs::canonicalize(&path).unwrap();
    let (db, _) = open(&path);
    assert_eq!(db.path(), canonical);
    assert_eq!(db.path(), db.path());

    fs::create_dir(dir.path().join("sub")).unwrap();
    let dotted = dir.path().join("sub").join("..").join("x.pgn");
    let (db2, _) = open(&dotted);
    assert_eq!(db2.path(), canonical);
    assert!(db2.path().is_absolute());
}

#[cfg(unix)]
#[test]
fn path_through_a_symlink_is_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", THREE.as_bytes());
    let link = dir.path().join("link.pgn");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let (a, _) = open(&link);
    let (b, _) = open(&path);
    assert_eq!(a.path(), b.path());
    assert_eq!(a.path(), fs::canonicalize(&path).unwrap());
}

#[test]
fn memory_estimate_is_positive_and_grows_with_the_games() {
    let dir = tempfile::tempdir().unwrap();
    let small = put(dir.path(), "s.pgn", THREE.as_bytes());
    let big = put(dir.path(), "b.pgn", corpus(300).as_bytes());
    let (a, _) = open(&small);
    let (b, _) = open(&big);
    assert!(a.memory_estimate() > 0);
    assert!(b.memory_estimate() > a.memory_estimate());
}

#[test]
fn import_into_a_read_only_writer_is_still_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut writer = Si4Writer::create(&dir.path().join("ro"), 3).unwrap();
    let result = import_pgn(
        THREE.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert!(matches!(result, Err(ImportError::ReadOnly)));
}

// ====================================================================
// Equivalence with an imported database
// ====================================================================

#[test]
fn pgn_database_equals_the_imported_si4_database() {
    let text = corpus(260);
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", text.as_bytes());
    let (mut pgn, _) = open(&path);
    let imported = import_str(&text, &ImportOptions::default());
    let mut si4 = imported.open();
    assert_same(&mut pgn, &mut si4);
}

#[test]
fn equivalence_holds_with_illegal_moves_latin1_and_odd_tags() {
    let mut bytes = b"[Event \"Caf".to_vec();
    bytes.push(0xE9);
    bytes.extend_from_slice(
        b"\"]\n[White \"M\xFCller\"]\n[Foo \"bar\"]\n[ECO \"C44\"]\n\n1. e4 e5 2. Ke5 *\n\n",
    );
    bytes.extend_from_slice(THREE.as_bytes());
    bytes.extend_from_slice(corpus(10).as_bytes());
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", &bytes);
    let (mut pgn, report) = open(&path);
    let imported = import_bytes(&bytes, &ImportOptions::default());
    let mut si4 = imported.open();
    assert_same(&mut pgn, &mut si4);
    assert_eq!(report.to_string(), imported.report.to_string());
    assert_eq!(pgn.game(1).unwrap().tags().event, "Café");
    assert_eq!(pgn.game(1).unwrap().tags().white, "Müller");
}

#[test]
fn search_and_filter_over_the_pgn_match_the_si4_equivalent() {
    let text = corpus(260);
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", text.as_bytes());
    let (mut pgn, _) = open(&path);
    let imported = import_str(&text, &ImportOptions::default());
    let mut si4 = imported.open();

    let mut queries: Vec<(&str, HeaderSearch)> = Vec::new();
    let mut s = HeaderSearch::default();
    s.white = "P3".into();
    queries.push(("white name", s));
    let mut s = HeaderSearch::default();
    s.black = "Q2".into();
    s.event = "Ev4".into();
    queries.push(("black and event", s));
    let mut s = HeaderSearch::default();
    s.site = "S1".into();
    s.round = "2".into();
    queries.push(("site and round", s));
    let mut s = HeaderSearch::default();
    s.white_elo = MinMax {
        min: 2100,
        max: 2200,
    };
    queries.push(("white elo", s));
    let mut s = HeaderSearch::default();
    s.date = MinMax {
        min: year(2005),
        max: year(2009),
    };
    queries.push(("dates", s));
    let mut s = HeaderSearch::default();
    s.eco = Some(EcoRange {
        min: eco("B00"),
        max: eco("B99"),
        include_none: false,
    });
    queries.push(("eco", s));
    let mut s = HeaderSearch::default();
    s.results = ResultSet {
        white: true,
        draw: false,
        black: false,
        none: false,
    };
    queries.push(("white wins", s));
    let mut s = HeaderSearch::default();
    s.pgn_text = vec!["good move".into()];
    queries.push(("pgn text", s));
    let mut s = HeaderSearch::default();
    s.pgn_text = vec!["GOOD MOVE 9".into()];
    s.ignore_case = true;
    queries.push(("pgn text, ignore case", s));
    let mut s = HeaderSearch::default();
    s.length = Some(MinMax { min: 10, max: 12 });
    queries.push(("length", s));
    let mut s = HeaderSearch::default();
    s.white = "Q1".into();
    s.black = "P1".into();
    s.ignore_colors = true;
    queries.push(("ignore colors", s));

    let mut non_trivial = 0;
    for (label, query) in &queries {
        let a = run_search(&mut pgn, query);
        let b = run_search(&mut si4, query);
        assert_eq!(a, b, "{label}");
        if !a.is_empty() && a.len() < 260 {
            non_trivial += 1;
        }
    }
    assert!(non_trivial >= 7, "queries must discriminate: {non_trivial}");

    // text find / text filter
    for (needle, ignore_case) in [("P3", false), ("p3", true), ("p3", false), ("Ev9", true)] {
        for from in [0, 1, 40] {
            let f1 = Filter::new(pgn.len());
            let f2 = Filter::new(si4.len());
            assert_eq!(
                f1.textfind(&pgn, needle, ignore_case, from),
                f2.textfind(&si4, needle, ignore_case, from),
                "textfind {needle} {ignore_case} {from}"
            );
        }
        let mut f1 = pgn.new_filter();
        let mut f2 = Filter::new(si4.len());
        let r1 = f1.textfilter(&pgn, needle, ignore_case);
        let r2 = f2.textfilter(&si4, needle, ignore_case);
        assert_eq!(r1, r2, "textfilter {needle}");
        assert_eq!(
            f1.iter_games().collect::<Vec<_>>(),
            f2.iter_games().collect::<Vec<_>>()
        );
    }
    let mut f = pgn.new_filter();
    let kept = f.textfilter(&pgn, "good move", true);
    assert!(kept > 0 && f.count() < 260);

    // player statistics
    let s1 = PlayerStats::from_database(&pgn);
    let s2 = PlayerStats::from_database(&si4);
    for id in 0..pgn.names().num_names(NameKind::Player) {
        assert_eq!(s1.max_elo(id), s2.max_elo(id), "player {id}");
    }
    assert!((0..pgn.names().num_names(NameKind::Player)).any(|id| s1.max_elo(id) > 0));
}

#[test]
fn games_exported_from_the_pgn_database_reimport_to_the_same_games() {
    let text = corpus(120);
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", text.as_bytes());
    let (mut pgn, _) = open(&path);
    let items: Vec<ExportItem> = (1..=pgn.len())
        .map(|n| ExportItem::stored(&mut pgn, n))
        .collect();
    let out = dir.path().join("out.pgn");
    let report = mychess_db::export_pgn(
        items,
        &out,
        &ExportOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(report.written, 120);
    let again = import_bytes(&fs::read(&out).unwrap(), &ImportOptions::default());
    assert_eq!(again.report.games_added, 120);
    let mut db2 = again.open();
    for n in 1..=120 {
        let g1 = pgn.game(n).unwrap();
        let g2 = db2.game(n).unwrap();
        assert!(
            g1.same_content(&g2),
            "game {n}:\n{}\n{}",
            canon(&g1),
            canon(&g2)
        );
    }
}

// ====================================================================
// Cancellation, progress and errors
// ====================================================================

#[test]
fn cancelling_before_the_start_returns_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", corpus(250).as_bytes());
    let token = CancelToken::new();
    token.cancel();
    let result = PgnDatabase::open(&path, &OpenOptions::default(), &mut NoProgress, &token);
    assert!(matches!(result, Err(PgnOpenError::Interrupted)));
    assert_eq!(file_names(dir.path()), vec!["x.pgn".to_owned()]);
}

#[rstest]
#[case::plain(false)]
#[case::gz(true)]
fn cancelling_in_the_middle_returns_interrupted_and_no_database(#[case] gz: bool) {
    let dir = tempfile::tempdir().unwrap();
    let text = corpus(450);
    let path = if gz {
        put(dir.path(), "x.pgn.gz", &gzip_stored(text.as_bytes()))
    } else {
        put(dir.path(), "x.pgn", text.as_bytes())
    };
    let token = CancelToken::new();
    let t = token.clone();
    let mut calls = 0u32;
    let mut sink = |_done: u64, _total: u64| {
        calls += 1;
        t.cancel();
    };
    let result = PgnDatabase::open(&path, &OpenOptions::default(), &mut sink, &token);
    assert!(matches!(result, Err(PgnOpenError::Interrupted)));
    assert!(calls >= 1);
    assert!(
        calls < 5,
        "stopped after the first batch, got {calls} reports"
    );
}

#[rstest]
#[case::plain(false)]
#[case::gz(true)]
fn progress_reports_bytes_of_the_file_over_its_size_per_batch(#[case] gz: bool) {
    let dir = tempfile::tempdir().unwrap();
    let text = corpus(250);
    let path = if gz {
        put(dir.path(), "x.pgn.gz", &gzip_stored(text.as_bytes()))
    } else {
        put(dir.path(), "x.pgn", text.as_bytes())
    };
    let size = fs::metadata(&path).unwrap().len();
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |done: u64, total: u64| seen.push((done, total));
    PgnDatabase::open(
        &path,
        &OpenOptions::default(),
        &mut sink,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(seen.len() >= 3, "250 games = at least 3 batches: {seen:?}");
    assert!(
        seen.iter().all(|&(d, t)| t == size && d <= size),
        "{seen:?}"
    );
    assert!(
        seen.windows(2).all(|w| w[0].0 <= w[1].0),
        "monotone: {seen:?}"
    );
}

#[test]
fn io_error_in_the_middle_is_a_read_error_without_database() {
    let data = corpus(150).into_bytes();
    let reader = FailingReader {
        data: io::Cursor::new(data),
    };
    let result = PgnDatabase::from_reader(
        reader,
        Path::new("boom.pgn"),
        &OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    );
    match result {
        Err(PgnOpenError::Read { path, .. }) => assert_eq!(path, Path::new("boom.pgn")),
        Err(other) => panic!("{other:?}"),
        Ok((db, _)) => panic!("partial database of {} games", db.len()),
    }
}

#[test]
fn truncated_gzip_fails_without_a_partial_database() {
    let gz = gzip_stored(corpus(250).as_bytes());
    let dir = tempfile::tempdir().unwrap();
    for cut in [10, 8, 1, gz.len() / 2] {
        let path = put(dir.path(), "t.pgn.gz", &gz[..gz.len() - cut]);
        assert!(
            matches!(
                open_err(&path, &OpenOptions::default()),
                PgnOpenError::Read { .. }
            ),
            "cut {cut}"
        );
    }
}

#[test]
fn corrupted_gzip_fails_without_a_partial_database() {
    let gz = gzip_stored(corpus(250).as_bytes());
    let dir = tempfile::tempdir().unwrap();
    // wrong CRC
    let mut bad_crc = gz.clone();
    let n = bad_crc.len();
    bad_crc[n - 8] ^= 0xFF;
    // wrong ISIZE
    let mut bad_size = gz.clone();
    bad_size[n - 1] ^= 0x7F;
    // unknown compression method
    let mut bad_method = gz.clone();
    bad_method[2] = 7;
    // invalid deflate block type (3) at the start of the data
    let mut bad_block = gz.clone();
    bad_block[10] = 0x07;
    for (label, bytes) in [
        ("crc", bad_crc),
        ("isize", bad_size),
        ("method", bad_method),
        ("block", bad_block),
    ] {
        let path = put(dir.path(), "c.pgn.gz", &bytes);
        assert!(
            matches!(
                open_err(&path, &OpenOptions::default()),
                PgnOpenError::Read { .. }
            ),
            "{label}"
        );
    }
}

#[test]
fn missing_file_is_an_open_error_with_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nope.pgn");
    match open_err(&path, &OpenOptions::default()) {
        PgnOpenError::Open { path: p, source } => {
            assert_eq!(p, path);
            assert_eq!(source.kind(), io::ErrorKind::NotFound);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_directory_is_not_a_database() {
    let dir = tempfile::tempdir().unwrap();
    let result = open_with(dir.path(), &OpenOptions::default());
    assert!(result.is_err());
}

// ====================================================================
// Limits
// ====================================================================

#[test]
fn max_games_two_refuses_three_games_and_accepts_two() {
    let dir = tempfile::tempdir().unwrap();
    let three = put(dir.path(), "three.pgn", THREE.as_bytes());
    let two_text = &THREE[..THREE.find("[Event \"Three\"]").unwrap()];
    let two = put(dir.path(), "two.pgn", two_text.as_bytes());
    let options = OpenOptions {
        max_games: 2,
        ..OpenOptions::default()
    };
    match open_err(&three, &options) {
        PgnOpenError::TooManyGames { limit } => assert_eq!(limit, 2),
        other => panic!("{other:?}"),
    }
    let (db, report) = open_with(&two, &options).unwrap();
    assert_eq!(db.len(), 2);
    assert_eq!(report.games_added, 2);
}

#[test]
fn max_games_zero_accepts_an_empty_file_only() {
    let dir = tempfile::tempdir().unwrap();
    let empty = put(dir.path(), "e.pgn", b"");
    let one = put(dir.path(), "o.pgn", corpus(1).as_bytes());
    let options = OpenOptions {
        max_games: 0,
        ..OpenOptions::default()
    };
    assert_eq!(open_with(&empty, &options).unwrap().0.len(), 0);
    assert!(matches!(
        open_err(&one, &options),
        PgnOpenError::TooManyGames { limit: 0 }
    ));
}

#[test]
fn default_limit_is_five_million_games() {
    assert_eq!(mychess_db::DEFAULT_MAX_GAMES, 5_000_000);
    assert_eq!(OpenOptions::default().max_games, 5_000_000);
    assert_eq!(OpenOptions::default().import, ImportOptions::default());
}

#[test]
fn skipped_games_do_not_count_for_max_games() {
    // The middle game is bigger than max_game_bytes: it is skipped (D347 e).
    let big = format!(
        "[Event \"Big\"]\n[Result \"*\"]\n\n1. e4 {{{}}} *\n\n",
        "x".repeat(20_000)
    );
    let pgn = format!("{}{big}{}", corpus(1), corpus(1).replace("Ev0", "Ev1"));
    let mut options = OpenOptions {
        max_games: 2,
        ..OpenOptions::default()
    };
    options.import.read.max_game_bytes = 4_000;
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", pgn.as_bytes());
    let (db, report) = open_with(&path, &options).unwrap();
    assert_eq!(db.len(), 2);
    assert_eq!(report.games_read, 3);
    assert_eq!(report.games_skipped_total, 1);
}

#[test]
fn round_name_limit_fails_the_opening_without_a_database() {
    let limit = 262_143usize;
    let total = limit + 7;
    let mut pgn = String::with_capacity(total * 32);
    for i in 0..total {
        write!(pgn, "[Round \"r{i}\"]\n\n1. e4 *\n\n").unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "r.pgn", pgn.as_bytes());
    match open_err(&path, &OpenOptions::default()) {
        PgnOpenError::NameLimit {
            kind,
            limit: l,
            game,
        } => {
            assert_eq!(kind, NameKind::Round);
            assert_eq!(l as usize, limit);
            assert_eq!(game, limit as u64 + 1, "the first game that did not fit");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(file_names(dir.path()), vec!["r.pgn".to_owned()]);
}

// ====================================================================
// Formats that are not PGN
// ====================================================================

fn with_filler(magic: &[u8]) -> Vec<u8> {
    let mut v = magic.to_vec();
    v.extend_from_slice(&[b'a'; 200]);
    v
}

#[rstest]
#[case::zip_local(b"PK\x03\x04".as_slice(), UnsupportedKind::Zip)]
#[case::zip_empty(b"PK\x05\x06".as_slice(), UnsupportedKind::Zip)]
#[case::seven_zip(b"7z\xBC\xAF\x27\x1C".as_slice(), UnsupportedKind::SevenZip)]
#[case::rar4(b"Rar!\x1A\x07\x00".as_slice(), UnsupportedKind::Rar)]
#[case::rar5(b"Rar!\x1A\x07\x01\x00".as_slice(), UnsupportedKind::Rar)]
#[case::bzip2(b"BZh9".as_slice(), UnsupportedKind::Bzip2)]
#[case::xz(b"\xFD7zXZ\x00".as_slice(), UnsupportedKind::Xz)]
#[case::zstd(b"\x28\xB5\x2F\xFD".as_slice(), UnsupportedKind::Zstd)]
fn archives_and_compressed_streams_are_unsupported_whatever_the_extension(
    #[case] magic: &[u8],
    #[case] expected: UnsupportedKind,
) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["x.pgn", "x.bin", "x"] {
        let path = put(dir.path(), name, &with_filler(magic));
        match open_err(&path, &OpenOptions::default()) {
            PgnOpenError::UnsupportedFormat { kind, path: p } => {
                assert_eq!(kind, expected, "{name}");
                assert_eq!(p, path);
            }
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[test]
fn binary_content_with_nul_is_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", b"[Event \"x\"]\n\0\n1. e4 *\n");
    assert!(matches!(
        open_err(&path, &OpenOptions::default()),
        PgnOpenError::UnsupportedFormat {
            kind: UnsupportedKind::Binary,
            ..
        }
    ));
}

#[test]
fn utf16_text_is_refused_instead_of_opening_an_empty_database() {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in THREE.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", &bytes);
    assert!(matches!(
        open_err(&path, &OpenOptions::default()),
        PgnOpenError::UnsupportedFormat {
            kind: UnsupportedKind::Binary,
            ..
        }
    ));
}

#[test]
fn a_real_si4_file_is_refused_as_binary_even_named_pgn() {
    let dir = tempfile::tempdir().unwrap();
    let imported = import_str(THREE, &ImportOptions::default());
    let si4 = fs::read(imported.base.with_extension("si4")).unwrap();
    for name in ["db.si4", "db.pgn", "db"] {
        let path = put(dir.path(), name, &si4);
        assert!(
            matches!(
                open_err(&path, &OpenOptions::default()),
                PgnOpenError::UnsupportedFormat {
                    kind: UnsupportedKind::Binary,
                    ..
                }
            ),
            "{name}"
        );
    }
}

#[test]
fn a_nul_after_the_first_4096_bytes_is_tolerated() {
    // D409: not detected; the reader must not panic and the games survive.
    let mut bytes = corpus(2).into_bytes();
    bytes.extend_from_slice(&[b' '; 5000]);
    bytes.extend_from_slice(b"\0\n");
    bytes.extend_from_slice(corpus(1).replace("Ev0", "EvZ").as_bytes());
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "x.pgn", &bytes);
    let (db, _) = open(&path);
    assert!(db.len() >= 2);
}

// ====================================================================
// Hostile and odd inputs
// ====================================================================

#[test]
fn empty_file_opens_as_an_empty_read_only_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "e.pgn", b"");
    let (db, report) = open(&path);
    assert_eq!(db.len(), 0);
    assert!(db.is_empty());
    assert_eq!(db.base_type(), 3);
    assert_eq!(report.games_added, 0);
    assert!(report.added.is_empty());
    assert_eq!(db.new_filter().count(), 0);
}

#[test]
fn empty_gzip_opens_as_an_empty_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "e.pgn.gz", &gzip_stored(b""));
    let (db, _) = open(&path);
    assert_eq!(db.len(), 0);
    assert_eq!(db.base_type(), 3);
}

#[rstest]
#[case::prose(b"hello world\nthis is not chess\n\nsome more text\n".as_slice())]
#[case::whitespace(b"\n\n   \n\t\n".as_slice())]
#[case::bom_only("\u{feff}".as_bytes())]
fn text_outside_games_gives_an_empty_database(#[case] bytes: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "t.pgn", bytes);
    let (db, report) = open(&path);
    assert_eq!(db.len(), 0, "{report}");
    assert_eq!(report.games_added, 0);
}

#[test]
fn bom_latin1_crlf_and_cr_give_the_same_games_as_plain_utf8() {
    let base = THREE.replace("Paris", "Zürich");
    let dir = tempfile::tempdir().unwrap();
    let utf8 = put(dir.path(), "a.pgn", base.as_bytes());
    let (mut reference, _) = open(&utf8);
    assert_eq!(reference.game(1).unwrap().tags().site, "Zürich");

    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(base.as_bytes());
    let latin1: Vec<u8> = base.chars().map(|c| c as u32 as u8).collect();
    let variants: [(&str, Vec<u8>); 4] = [
        ("bom", bom),
        ("latin1", latin1),
        ("crlf", base.replace('\n', "\r\n").into_bytes()),
        ("cr", base.replace('\n', "\r").into_bytes()),
    ];
    for (label, bytes) in variants {
        let path = put(dir.path(), &format!("{label}.pgn"), &bytes);
        let (mut db, report) = open(&path);
        assert_eq!(db.len(), 3, "{label}");
        assert!(!report.has_warnings(), "{label}: {report}");
        assert_same(&mut reference, &mut db);
    }
}

#[test]
fn a_huge_line_does_not_panic_or_allocate_disproportionately() {
    let mut pgn = String::from("[Event \"A\"]\n\n1. e4 {");
    pgn.push_str(&"a".repeat(6_000_000));
    pgn.push_str("} e5 *\n\n");
    pgn.push_str(&corpus(1));
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "h.pgn", pgn.as_bytes());
    let (db, _) = open(&path);
    assert!(db.len() <= 2);
    assert!(
        db.memory_estimate() < 16 * 1024 * 1024,
        "{}",
        db.memory_estimate()
    );
}

#[test]
fn a_huge_tag_value_is_shortened_not_stored_whole() {
    let pgn = format!(
        "[Event \"{}\"]\n[Result \"*\"]\n\n1. e4 *\n\n{}",
        "x".repeat(3_000_000),
        corpus(1)
    );
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "h.pgn", pgn.as_bytes());
    let (mut db, _) = open(&path);
    assert!(db.len() >= 1);
    assert!(db.game(1).unwrap().tags().event.len() <= 255);
    assert!(db.memory_estimate() < 4 * 1024 * 1024);
}

#[test]
fn a_huge_game_does_not_panic() {
    let mut pgn = String::from("[Event \"G\"]\n\n");
    for i in 0..150_000 {
        pgn.push_str(if i % 2 == 0 {
            "1. Nf3 Nf6 2. Ng1 Ng8 "
        } else {
            ""
        });
    }
    pgn.push_str("*\n\n");
    pgn.push_str(&corpus(1));
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "g.pgn", pgn.as_bytes());
    let (db, report) = open(&path);
    assert!(db.len() >= 1 && db.len() <= 2);
    assert!(db.memory_estimate() < 64 * 1024 * 1024);
    assert!(!report.interrupted);
}

#[test]
fn hundreds_of_thousands_of_tags_are_limited() {
    let mut pgn = String::from("[Event \"T\"]\n");
    for i in 0..400_000 {
        writeln!(pgn, "[Tag{i} \"v\"]").unwrap();
    }
    pgn.push_str("\n1. e4 *\n\n");
    pgn.push_str(&corpus(1));
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "t.pgn", pgn.as_bytes());
    let (mut db, _) = open(&path);
    assert!(db.len() >= 1 && db.len() <= 2);
    assert!(
        db.memory_estimate() < 8 * 1024 * 1024,
        "{}",
        db.memory_estimate()
    );
    drop(db.game(1).unwrap());
}

#[test]
fn binary_garbage_without_nul_in_the_head_does_not_panic() {
    let mut state = 0x1234_5678_u32;
    let mut bytes: Vec<u8> = (0..1_500_000)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect();
    bytes[0] = b'x';
    for b in bytes.iter_mut().take(4096) {
        if *b == 0 {
            *b = 1;
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "g.pgn", &bytes);
    if let Ok((db, report)) = open_with(&path, &OpenOptions::default()) {
        assert!(!report.interrupted);
        assert!(db.memory_estimate() < 128 * 1024 * 1024);
    }
}

#[test]
fn garbage_inside_gzip_does_not_panic() {
    let mut data = vec![0xFFu8; 70_000];
    data[0] = 0;
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "g.pgn.gz", &gzip_stored(&data));
    if let Ok((db, _)) = open_with(&path, &OpenOptions::default()) {
        assert!(db.len() <= 1);
    }
}

// ====================================================================
// Property: random legal games survive PGN -> PgnDatabase -> PGN -> SI4
// ====================================================================

type Op = (u8, u8, u8, u8);

fn build(ops: &[Op], tag_seed: u8) -> Game {
    let mut game = Game::new();
    let mut nodes: Vec<NodeId> = vec![game.root()];
    for &(p, m, kind, val) in ops {
        let parent = nodes[usize::from(p) % nodes.len()];
        let legal = game.position_at(parent).unwrap().legal_moves();
        if legal.is_empty() {
            continue;
        }
        let mv = legal[usize::from(m) % legal.len()];
        let id = game.add_move(parent, mv).unwrap();
        if !nodes.contains(&id) {
            nodes.push(id);
        }
        if kind & 1 == 1 {
            game.set_comment(id, &format!("note {val} \u{e9}")).unwrap();
        }
        if kind & 2 == 2
            && game.nags(id).unwrap().len() < 3
            && let Ok(nag) = Nag::new(u16::from(val % 139) + 1)
        {
            game.add_nag(id, nag).unwrap();
        }
    }
    let names = ["Anand, V", "Carlsen", "\u{c5}sa", "Zed", "Kasparov, Garry"];
    let t = game.tags_mut();
    t.white = names[usize::from(tag_seed) % names.len()].into();
    t.black = names[usize::from(tag_seed / 7) % names.len()].into();
    t.event = format!("Ev{}", tag_seed % 5);
    t.result = [
        GameResult::Unknown,
        GameResult::White,
        GameResult::Black,
        GameResult::Draw,
    ][usize::from(tag_seed) % 4];
    game
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

    #[test]
    fn random_games_keep_their_moves_through_pgn_database_export_and_import(
        specs in prop::collection::vec(
            (prop::collection::vec(any::<Op>(), 0..30), any::<u8>()),
            1..6,
        ),
        gz in any::<bool>(),
    ) {
        let games: Vec<Game> = specs.iter().map(|(ops, seed)| build(ops, *seed)).collect();
        let text: String = games.iter().map(|g| canon(g) + "\n").collect();
        let dir = tempfile::tempdir().unwrap();
        let path = if gz {
            put(dir.path(), "p.pgn.gz", &gzip_stored(text.as_bytes()))
        } else {
            put(dir.path(), "p.pgn", text.as_bytes())
        };
        let (mut pgn, report) = open_with(&path, &OpenOptions::default()).unwrap();
        prop_assert_eq!(pgn.len() as usize, games.len(), "{}", report);

        let imported = import_str(&text, &ImportOptions::default());
        let mut si4 = imported.open();
        for n in 1..=pgn.len() {
            prop_assert_eq!(
                pgn.record_bytes(n).unwrap().to_vec(),
                si4.record_bytes(n).unwrap().to_vec()
            );
        }

        let items: Vec<ExportItem> = (1..=pgn.len()).map(|n| ExportItem::stored(&mut pgn, n)).collect();
        let out = dir.path().join("out.pgn");
        mychess_db::export_pgn(
            items,
            &out,
            &ExportOptions::scid_dialog(),
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        let second = import_bytes(&fs::read(&out).unwrap(), &ImportOptions::default());
        prop_assert_eq!(second.report.games_added as usize, games.len());
        let mut db2 = second.open();
        for (i, g) in games.iter().enumerate() {
            let n = u32::try_from(i).unwrap() + 1;
            let from_pgn = pgn.game(n).unwrap();
            prop_assert_eq!(mainline_moves(&from_pgn), mainline_moves(g), "game {}", n);
            prop_assert_eq!(mainline_moves(&db2.game(n).unwrap()), mainline_moves(g), "game {}", n);
            prop_assert!(from_pgn.same_content(&db2.game(n).unwrap()), "game {}", n);
        }
    }
}
