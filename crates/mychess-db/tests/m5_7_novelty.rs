//! Contract tests of `identify_opening`, `find_novelty` and `Eco::last_sub_code`
//! (M5-7a, D1189 to D1193, D1202, D1203; Scid `sc_eco_game` and
//! `sc_game_novelty`, tkscid.cpp 4851-4891 and 8342-8527). The oracle used for
//! the equivalence test is written here, from the Scid algorithm (one scan of
//! the database per position of the game).
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::assert_is_empty,
    clippy::type_complexity
)]

#[path = "common/si4_builder.rs"]
mod si4_builder;
#[path = "common/m5_7_support.rs"]
mod support;

use mychess_core::{Color, Eco, EcoBook, Game, Position, SanMode};
use mychess_db::si4::Si4Database;
use mychess_db::{
    CancelToken, GameSource, NoProgress, Novelty, NoveltyError, NoveltyOptions, find_novelty,
    identify_opening,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;
use si4_builder::{EntrySpec, IndexHeader, Parts, default_names, index_file, open_parts, rec};
use support::{Spec, book, date, deep_book, eco, long_line, open, san_game, spec, write_db};

const E4: u8 = 0xCF;
const NF3: u8 = 0x67;
const END_GAME: u8 = 0x0F;

fn nov(
    db: &mut impl GameSource,
    game: &Game,
    book: Option<&EcoBook>,
    older_only: bool,
    current_game: Option<u32>,
) -> Result<Novelty, NoveltyError> {
    let cancel = CancelToken::new();
    find_novelty(
        db,
        game,
        book,
        NoveltyOptions {
            older_only,
            current_game,
            cancel: &cancel,
            progress: &mut NoProgress,
        },
    )
}

fn db_of(specs: &[Spec]) -> (tempfile::TempDir, Si4Database) {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), specs);
    let db = open(&base);
    (dir, db)
}

fn key(n: &Novelty) -> (u32, &str, u32, Color) {
    (n.ply, n.san.as_str(), n.move_number, n.color)
}

fn from_fen(fen: &str, sans: &[&str]) -> Game {
    let mut game = Game::from_position(Position::from_fen(fen).unwrap());
    let mut node = game.root();
    for san in sans {
        let position = game.position_at(node).unwrap().clone();
        node = game
            .add_move(node, position.parse_san(san, SanMode::Strict).unwrap().mv)
            .unwrap();
    }
    game
}

// ============================================================ identify_opening

#[test]
fn identify_returns_the_deepest_extended_code_with_ply_and_name() {
    let found = identify_opening(
        &book(),
        &san_game(&["e4", "e5", "Nf3", "Nc6", "d4", "exd4", "Nxd4"]),
    )
    .unwrap();
    assert_eq!(found.eco, eco("C44b"));
    assert_eq!(found.ply, 5);
    assert_eq!(found.name, "Scotch-ish");
}

#[test]
fn identify_matches_transpositions_by_position() {
    let found = identify_opening(&book(), &san_game(&["Nf3", "e5", "e4", "Nc6", "d4"])).unwrap();
    assert_eq!((found.eco, found.ply), (eco("C44b"), 5));
}

#[test]
fn identify_falls_back_to_the_start_entry_at_ply_zero() {
    let found = identify_opening(&book(), &san_game(&["h4", "h5"])).unwrap();
    assert_eq!(
        (found.eco, found.ply, found.name.as_str()),
        (eco("A00a"), 0, "Start")
    );
}

#[test]
fn identify_returns_none_when_nothing_matches() {
    let no_start = EcoBook::parse("C20 \"KP\" 1.e4 e5 *\n").unwrap();
    assert_eq!(identify_opening(&no_start, &san_game(&["d4", "d5"])), None);
    assert_eq!(identify_opening(&no_start, &Game::new()), None);
}

#[test]
fn identify_does_not_change_the_game() {
    let game = san_game(&["e4", "e5", "Nf3"]);
    let before = game.clone();
    drop(identify_opening(&book(), &game));
    assert!(game.same_content(&before));
}

#[test]
fn identify_follows_only_the_main_line() {
    // Main line 1.e4 e5; a variation 1.d4 Nf6 must not matter.
    let book =
        EcoBook::parse("A00a \"Start\" *\nC20 \"KP\" 1.e4 e5 *\nA45 \"Indian\" 1.d4 Nf6 *\n")
            .unwrap();
    let mut game = san_game(&["e4", "e5"]);
    let root = game.root();
    let d4 = Position::new().parse_san("d4", SanMode::Strict).unwrap().mv;
    let var = game.add_move(root, d4).unwrap();
    let after = game.position_at(var).unwrap().clone();
    game.add_move(var, after.parse_san("Nf6", SanMode::Strict).unwrap().mv)
        .unwrap();
    let found = identify_opening(&book, &game).unwrap();
    assert_eq!((found.eco, found.ply), (eco("C20"), 2));
}

#[test]
fn identify_scans_beyond_ply_60_unlike_the_batch_classification() {
    let line = long_line(70);
    let book = deep_book(&line);
    let sans: Vec<&str> = line.iter().map(String::as_str).collect();
    let found = identify_opening(&book, &san_game(&sans)).unwrap();
    assert_eq!(
        (found.eco, found.ply, found.name.as_str()),
        (eco("C31"), 61, "m61")
    );
}

#[test]
fn identify_starts_from_a_standard_game_with_its_own_position() {
    let game = from_fen(
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2",
        &["Nf3"],
    );
    let found = identify_opening(&book(), &game).unwrap();
    assert_eq!((found.eco, found.ply), (eco("C40a"), 1));
}

// ============================================================ last_sub_code

#[rstest]
#[case("B91", "B91z4")]
#[case("B91a", "B91a4")]
#[case("B91a1", "B91a1")]
#[case("B91a2", "B91a2")]
#[case("B91a4", "B91a4")]
#[case("B91z", "B91z4")]
#[case("B91z4", "B91z4")]
#[case("A00", "A00z4")]
#[case("E99", "E99z4")]
#[case("E99z4", "E99z4")]
#[case("D00b", "D00b4")]
fn last_sub_code_is_the_upper_end_of_a_range(#[case] from: &str, #[case] expected: &str) {
    let got: Eco = eco(from).last_sub_code();
    assert_eq!(got, eco(expected));
    assert!(got >= eco(from));
}

#[test]
fn last_sub_code_is_idempotent_for_every_code() {
    for code in 1..=65_500u16 {
        let e = Eco::from_scid_code(code).unwrap();
        let last = e.last_sub_code();
        assert_eq!(last.last_sub_code(), last, "{e}");
        assert!(last >= e, "{e}");
        assert_eq!(last.basic(), e.basic(), "{e}");
    }
}

// ============================================================ novelty: spec example

#[test]
fn spec_example_the_novelty_is_3_nf6() {
    let (_d, mut db) = db_of(&[spec(&["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"])]);
    let current = san_game(&["e4", "e5", "Nf3", "Nc6", "Bb5", "Nf6"]);
    let found = nov(&mut db, &current, None, false, None).unwrap();
    assert_eq!(key(&found), (6, "Nf6", 3, Color::Black));
}

#[test]
fn a_white_novelty_has_the_white_color_and_the_right_move_number() {
    let (_d, mut db) = db_of(&[spec(&["e4", "e5", "Nf3", "Nc6"])]);
    let current = san_game(&["e4", "e5", "Nf3", "Nc6", "Bb5"]);
    let found = nov(&mut db, &current, None, false, None).unwrap();
    assert_eq!(key(&found), (5, "Bb5", 3, Color::White));
}

#[test]
fn novelty_san_carries_check_and_capture_marks() {
    let (_d, mut db) = db_of(&[spec(&["e4", "e5", "Qh5", "Nc6", "Bc4", "Nf6"])]);
    let current = san_game(&["e4", "e5", "Qh5", "Nc6", "Bc4", "Nf6", "Qxf7"]);
    let found = nov(&mut db, &current, None, false, None).unwrap();
    assert_eq!(found.san, "Qxf7#");
    assert_eq!(found.ply, 7);
}

// ============================================================ novelty: no novelty

#[test]
fn an_identical_game_has_no_novelty() {
    let (_d, mut db) = db_of(&[spec(&["e4", "e5", "Nf3"])]);
    let current = san_game(&["e4", "e5", "Nf3"]);
    assert!(matches!(
        nov(&mut db, &current, None, false, None),
        Err(NoveltyError::NoNovelty)
    ));
}

#[test]
fn a_prefix_of_a_reference_game_has_no_novelty() {
    let (_d, mut db) = db_of(&[spec(&["e4", "e5", "Nf3", "Nc6"])]);
    assert!(matches!(
        nov(&mut db, &san_game(&["e4", "e5"]), None, false, None),
        Err(NoveltyError::NoNovelty)
    ));
}

#[test]
fn a_game_without_moves_has_no_novelty() {
    let (_d, mut db) = db_of(&[spec(&["e4"])]);
    assert!(matches!(
        nov(&mut db, &Game::new(), None, false, None),
        Err(NoveltyError::NoNovelty)
    ));
}

#[test]
fn positions_reached_by_transposition_count_as_seen() {
    let (_d, mut db) = db_of(&[
        spec(&["e4", "e5", "Nf3", "Nc6", "Bc4"]),
        spec(&["Nf3", "e5", "e4", "Nc6", "Bb5"]),
    ]);
    let current = san_game(&["e4", "e5", "Nf3", "Nc6", "Bb5"]);
    assert!(matches!(
        nov(&mut db, &current, None, false, None),
        Err(NoveltyError::NoNovelty)
    ));
}

#[test]
fn the_first_unseen_position_wins_even_if_a_later_one_was_seen() {
    // After 1.Nf3 d5 2.d4 the board equals 1.d4 d5 2.Nf3, but the position
    // after 1.Nf3 alone is in no game: the novelty is the first move.
    let (_d, mut db) = db_of(&[spec(&["d4", "d5", "Nf3"])]);
    let current = san_game(&["Nf3", "d5", "d4"]);
    let found = nov(&mut db, &current, None, false, None).unwrap();
    assert_eq!(key(&found), (1, "Nf3", 1, Color::White));
}

#[test]
fn each_position_may_be_seen_in_a_different_reference_game() {
    let (_d, mut db) = db_of(&[
        spec(&["e4"]),
        spec(&["e4", "e5"]),
        spec(&["d4", "d5"]),
        spec(&["e4", "e5", "Nf3"]),
    ]);
    let current = san_game(&["e4", "e5", "Nf3", "Nc6"]);
    let found = nov(&mut db, &current, None, false, None).unwrap();
    assert_eq!(key(&found), (4, "Nc6", 2, Color::Black));
}

// ============================================================ novelty: empty / first move

#[test]
fn with_no_reference_game_passing_through_the_start_the_first_move_is_reported() {
    let (_d, mut db) = db_of(&[]);
    let found = nov(&mut db, &san_game(&["e4", "e5"]), None, false, None).unwrap();
    assert_eq!(key(&found), (1, "e4", 1, Color::White));
}

// ============================================================ novelty: current game / eligibility

#[test]
fn the_current_game_is_ignored_when_it_is_in_the_reference_base() {
    let (_d, mut db) = db_of(&[spec(&["e4", "e5"])]);
    let current = san_game(&["e4", "e5"]);
    assert!(matches!(
        nov(&mut db, &current, None, false, None),
        Err(NoveltyError::NoNovelty)
    ));
    let found = nov(&mut db, &current, None, false, Some(1)).unwrap();
    assert_eq!(key(&found), (1, "e4", 1, Color::White));
}

#[test]
fn only_the_numbered_game_is_ignored() {
    let (_d, mut db) = db_of(&[spec(&["e4", "e5"]), spec(&["e4", "e5"])]);
    let current = san_game(&["e4", "e5"]);
    for own in [1, 2] {
        assert!(
            matches!(
                nov(&mut db, &current, None, false, Some(own)),
                Err(NoveltyError::NoNovelty)
            ),
            "the other copy still matches (own = {own})"
        );
    }
}

#[rstest]
#[case(0)]
#[case(99)]
#[case(u32::MAX)]
fn a_current_game_number_outside_the_base_ignores_nothing(#[case] own: u32) {
    let (_d, mut db) = db_of(&[spec(&["e4", "e5"])]);
    let current = san_game(&["e4", "e5"]);
    assert!(matches!(
        nov(&mut db, &current, None, false, Some(own)),
        Err(NoveltyError::NoNovelty)
    ));
}

#[test]
fn reference_games_with_their_own_start_position_are_ignored() {
    // The FEN game passes through the same boards but must not count.
    let fen_game = from_fen(
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2",
        &["Nf3", "Nc6"],
    );
    let (_d, mut db) = db_of(&[
        spec(&["e4"]),
        Spec {
            game: fen_game,
            ..spec(&[])
        },
    ]);
    let current = san_game(&["e4", "e5", "Nf3", "Nc6"]);
    let found = nov(&mut db, &current, None, false, None).unwrap();
    assert_eq!(key(&found), (2, "e5", 1, Color::Black));
}

#[test]
fn a_current_game_with_its_own_start_position_is_refused() {
    let (_d, mut db) = db_of(&[spec(&["e4"])]);
    let game = from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1", &["Rh2"]);
    assert!(matches!(
        nov(&mut db, &game, None, false, None),
        Err(NoveltyError::NonStandardStart)
    ));
    // Also when it has no moves.
    let empty = from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1", &[]);
    assert!(matches!(
        nov(&mut db, &empty, None, false, None),
        Err(NoveltyError::NonStandardStart)
    ));
}

#[test]
fn castling_rights_do_not_distinguish_positions() {
    // The reference reaches the position after 2.Nf3 having lost all
    // castling rights (Ke2 Ke7 Ke1 Ke8); the board and the side to move
    // are the same.
    let (_d, mut db) = db_of(&[spec(&["e4", "e5", "Ke2", "Ke7", "Ke1", "Ke8", "Nf3"])]);
    let current = san_game(&["e4", "e5", "Nf3", "Nc6"]);
    let found = nov(&mut db, &current, None, false, None).unwrap();
    assert_eq!(key(&found), (4, "Nc6", 2, Color::Black));
}

#[test]
fn the_side_to_move_distinguishes_positions() {
    // 1.Nf3 Nf6 2.Ng1 Ng8 returns to the start with White to move;
    // 1.Nf3 Nf6 2.Ng1 has the knight back with Black to move: a different
    // position from the start.
    let (_d, mut db) = db_of(&[spec(&["Nf3", "Nf6", "Ng1", "Ng8"])]);
    let current = san_game(&["Nf3", "Nf6", "Ng1", "Nc6"]);
    let found = nov(&mut db, &current, None, false, None).unwrap();
    assert_eq!(key(&found), (4, "Nc6", 2, Color::Black));
}

// ============================================================ older_only

fn dated(sans: &[&str], d: Option<(u16, u8, u8)>) -> Spec {
    let s = spec(sans);
    match d {
        Some((y, m, dd)) => s.date(date(y, m, dd)),
        None => s,
    }
}

#[test]
fn older_only_ignores_games_with_the_same_or_a_later_date() {
    // Arrange: current game dated 2000.06.15.
    let sans = ["e4", "e5", "Nf3"];
    let mut current = san_game(&sans);
    current.tags_mut().date = date(2000, 6, 15);
    for (ref_date, ignored) in [
        (Some((2000, 6, 14)), false), // older: counts
        (Some((2000, 6, 15)), true),  // same date: ignored
        (Some((2000, 6, 16)), true),  // newer: ignored
        (Some((1999, 1, 1)), false),
        (None, false), // unknown date (0) is older than a known one
    ] {
        let (_d, mut db) = db_of(&[dated(&sans, ref_date)]);
        // Act
        let with_flag = nov(&mut db, &current, None, true, None);
        let without = nov(&mut db, &current, None, false, None);
        // Assert
        assert!(
            matches!(without, Err(NoveltyError::NoNovelty)),
            "{ref_date:?} without older_only"
        );
        if ignored {
            assert_eq!(
                key(&with_flag.unwrap()),
                (1, "e4", 1, Color::White),
                "{ref_date:?}"
            );
        } else {
            assert!(
                matches!(with_flag, Err(NoveltyError::NoNovelty)),
                "{ref_date:?} should count"
            );
        }
    }
}

#[test]
fn older_only_with_an_unknown_current_date_ignores_every_game() {
    // Scid: ie->GetDate() >= currentDate with currentDate == 0 is always true.
    let (_d, mut db) = db_of(&[
        dated(&["e4", "e5"], Some((1990, 1, 1))),
        dated(&["e4", "e5"], None),
    ]);
    let current = san_game(&["e4", "e5"]);
    let found = nov(&mut db, &current, None, true, None).unwrap();
    assert_eq!(found.ply, 1);
}

#[test]
fn older_only_uses_the_older_game_even_if_a_newer_one_is_listed_first() {
    let sans = ["e4", "e5", "Nf3"];
    let (_d, mut db) = db_of(&[
        dated(&sans, Some((2010, 1, 1))),
        dated(&sans, Some((1999, 1, 1))),
    ]);
    let mut current = san_game(&sans);
    current.tags_mut().date = date(2000, 1, 1);
    assert!(matches!(
        nov(&mut db, &current, None, true, None),
        Err(NoveltyError::NoNovelty)
    ));
}

#[test]
fn older_only_compares_partial_dates_with_scid_encoding() {
    // 2000.??.?? encodes as year<<9 with zero month and day: it is before
    // 2000.01.01.
    let sans = ["e4", "e5"];
    let year_only =
        spec(&sans).date(mychess_core::PartialDate::new(Some(2000), None, None).unwrap());
    let (_d, mut db) = db_of(&[year_only]);
    let mut current = san_game(&sans);
    current.tags_mut().date = date(2000, 1, 1);
    assert!(matches!(
        nov(&mut db, &current, None, true, None),
        Err(NoveltyError::NoNovelty)
    ));
}

// ============================================================ the book

#[test]
fn the_search_starts_at_the_deepest_book_position() {
    // Book: C20 = 1.e4 e5. The reference does not pass through it.
    let book = EcoBook::parse("A00a \"Start\" *\nC20 \"KP\" 1.e4 e5 *\n").unwrap();
    let (_d, mut db) = db_of(&[spec(&["d4", "d5"])]);
    let current = san_game(&["e4", "e5", "Nf3", "Nc6"]);
    // Without a book the start matches and the novelty is the first move.
    let plain = nov(&mut db, &current, None, false, None).unwrap();
    assert_eq!(key(&plain), (1, "e4", 1, Color::White));
    // With the book the search begins at ply 2: that position is unseen.
    let booked = nov(&mut db, &current, Some(&book), false, None).unwrap();
    assert_eq!(key(&booked), (2, "e5", 1, Color::Black));
}

#[test]
fn with_a_book_the_positions_before_the_book_position_are_not_examined() {
    // The reference reaches 1.e4 e5 but not through 1.e4 alone being
    // missing: only positions from ply 2 on are checked.
    let book = EcoBook::parse("A00a \"Start\" *\nC20 \"KP\" 1.e4 e5 *\n").unwrap();
    let (_d, mut db) = db_of(&[
        spec(&["Nf3", "Nf6", "e4"]),
        spec(&["e4", "e5", "Nf3", "Nc6"]),
    ]);
    let current = san_game(&["e4", "e5", "Nf3", "Nc6", "Bb5"]);
    let found = nov(&mut db, &current, Some(&book), false, None).unwrap();
    assert_eq!(key(&found), (5, "Bb5", 3, Color::White));
}

#[test]
fn a_book_that_matches_nothing_starts_at_ply_zero() {
    let book = EcoBook::parse("C20 \"KP\" 1.e4 e5 *\n").unwrap();
    let (_d, mut db) = db_of(&[spec(&["d4"])]);
    let found = nov(&mut db, &san_game(&["d4", "d5"]), Some(&book), false, None).unwrap();
    assert_eq!(key(&found), (2, "d5", 1, Color::Black));
    let found = nov(&mut db, &san_game(&["h4"]), Some(&book), false, None).unwrap();
    assert_eq!(found.ply, 1);
}

#[test]
fn when_the_deepest_book_position_is_the_end_of_the_game_it_is_checked_alone() {
    let book = EcoBook::parse("A00a \"Start\" *\nC20 \"KP\" 1.e4 e5 *\n").unwrap();
    let (_d, mut db) = db_of(&[spec(&["d4"])]);
    let found = nov(&mut db, &san_game(&["e4", "e5"]), Some(&book), false, None).unwrap();
    assert_eq!(key(&found), (2, "e5", 1, Color::Black));
    let (_d2, mut db2) = db_of(&[spec(&["e4", "e5", "Nf3"])]);
    assert!(matches!(
        nov(&mut db2, &san_game(&["e4", "e5"]), Some(&book), false, None),
        Err(NoveltyError::NoNovelty)
    ));
}

// ============================================================ cancel / progress / errors

fn mem_of(records: &[Vec<u8>]) -> si4_builder::MemDb {
    si4_builder::mem_db(records)
}

#[test]
fn a_cancelled_token_interrupts_the_search() {
    let (_d, mut db) = db_of(&[spec(&["e4"])]);
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = find_novelty(
        &mut db,
        &san_game(&["e4"]),
        None,
        NoveltyOptions {
            older_only: false,
            current_game: None,
            cancel: &cancel,
            progress: &mut NoProgress,
        },
    );
    assert!(matches!(result, Err(NoveltyError::Interrupted)));
}

#[test]
fn cancelling_from_the_progress_sink_interrupts_a_long_scan() {
    // 70000 one-move games: the position after 1.d4 is never found, so the
    // scan runs on until the check at game 65536.
    let records: Vec<Vec<u8>> = (0..70_000).map(|_| rec(&[E4, END_GAME], &[])).collect();
    let mut db = mem_of(&records);
    let cancel = CancelToken::new();
    let in_sink = cancel.clone();
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |done: u64, total: u64| {
        seen.push((done, total));
        in_sink.cancel();
    };
    let result = find_novelty(
        &mut db,
        &san_game(&["d4"]),
        None,
        NoveltyOptions {
            older_only: false,
            current_game: None,
            cancel: &cancel,
            progress: &mut sink,
        },
    );
    assert!(matches!(result, Err(NoveltyError::Interrupted)));
    assert!(!seen.is_empty());
    assert!(seen.iter().all(|&(d, t)| d <= t && t == 70_000), "{seen:?}");
}

#[test]
fn a_finished_search_reports_progress_up_to_the_total() {
    let (_d, mut db) = db_of(&[spec(&["e4"]), spec(&["d4"])]);
    let cancel = CancelToken::new();
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |done: u64, total: u64| seen.push((done, total));
    drop(find_novelty(
        &mut db,
        &san_game(&["c4"]),
        None,
        NoveltyOptions {
            older_only: false,
            current_game: None,
            cancel: &cancel,
            progress: &mut sink,
        },
    ));
    assert_eq!(seen.last(), Some(&(2, 2)));
}

#[test]
fn an_unreadable_reference_record_is_a_read_error_naming_the_game() {
    // One entry whose record lies beyond the end of the .sg4.
    let mut sg4 = rec(&[E4, END_GAME], &[]);
    let first = EntrySpec::new(0, sg4.len() as u32);
    let second = EntrySpec::new(10_000, 40);
    sg4.extend_from_slice(&[0; 4]);
    let mut db = open_parts(&Parts {
        index: index_file(&IndexHeader::default(), &[first.encode(), second.encode()]),
        names: default_names(),
        sg4,
    })
    .unwrap();
    let result = nov(&mut db, &san_game(&["d4"]), None, false, None);
    assert!(
        matches!(result, Err(NoveltyError::Read { game: 2, .. })),
        "{result:?}"
    );
}

#[test]
fn damaged_reference_records_do_not_fail_the_search() {
    // D1202: the walk of a record stops at the damage; positions up to the
    // last decodable move count. The position just before the broken move is
    // not visited (documented deviation), so 1.e4 e5 2.Nf3 is novel here.
    let damaged = rec(&[E4, E4, NF3, 0x20, END_GAME], &[]);
    let broken_first = rec(&[0x20, E4, END_GAME], &[]);
    let mut db = mem_of(&[broken_first, damaged]);
    let found = nov(&mut db, &san_game(&["e4", "e5", "Nf3"]), None, false, None).unwrap();
    assert_eq!(key(&found), (3, "Nf3", 2, Color::White));
    assert!(matches!(
        nov(&mut db, &san_game(&["e4", "e5"]), None, false, None),
        Err(NoveltyError::NoNovelty)
    ));
}

#[test]
fn novelty_error_messages_are_lowercase_without_final_dot() {
    for e in [
        NoveltyError::NoNovelty,
        NoveltyError::NonStandardStart,
        NoveltyError::Interrupted,
    ] {
        let m = e.to_string();
        assert!(
            !m.ends_with('.') && m.chars().next().unwrap().is_lowercase(),
            "{m}"
        );
    }
}

// ============================================================ equivalence with the naive algorithm

/// A small xorshift generator.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize % n.max(1)
    }
}

fn random_game(rng: &mut Rng, max_plies: usize, branching: usize) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    for _ in 0..rng.below(max_plies + 1) {
        let position = game.position_at(node).unwrap().clone();
        let moves = position.legal_moves();
        if moves.is_empty() {
            break;
        }
        let mv = moves[rng.below(moves.len().min(branching))];
        node = game.add_move(node, mv).unwrap();
    }
    game
}

/// Board and side to move only: castling rights and en passant are ignored.
fn board_key(position: &Position) -> String {
    position
        .fen()
        .split(' ')
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

fn boards(game: &Game) -> Vec<(Position, Option<mychess_core::Move>)> {
    let mut out = vec![(game.start_position().clone(), None)];
    for node in game.mainline() {
        out.push((
            game.position_at(node).unwrap().clone(),
            Some(game.mv(node).unwrap()),
        ));
    }
    out
}

/// Scid's algorithm, literally: for each position of the game, from the
/// deepest book position, scan every eligible reference game.
fn oracle(
    refs: &[Spec],
    current: &Game,
    book: Option<&EcoBook>,
    older_only: bool,
    current_game: Option<usize>,
) -> Result<(u32, String, u32, Color), &'static str> {
    let list = boards(current);
    if list.len() == 1 {
        return Err("none");
    }
    let first = book
        .and_then(|b| {
            list.iter()
                .enumerate()
                .rev()
                .find(|(_, (p, _))| b.lookup(p).is_some())
                .map(|(i, _)| i)
        })
        .unwrap_or(0);
    let cur_date = current.tags().date.to_scid_u32();
    for i in first..list.len() {
        let target = board_key(&list[i].0);
        let seen = refs.iter().enumerate().any(|(idx, r)| {
            if current_game == Some(idx) {
                return false;
            }
            if older_only && r.date.map_or(0, mychess_core::PartialDate::to_scid_u32) >= cur_date {
                return false;
            }
            boards(&r.game).iter().any(|(p, _)| board_key(p) == target)
        });
        if !seen {
            let ply = i.max(1);
            let before = &list[ply - 1].0;
            let mv = list[ply].1.unwrap();
            return Ok((
                u32::try_from(ply).unwrap(),
                before.san(mv).unwrap(),
                before.fullmove_number(),
                before.turn(),
            ));
        }
    }
    Err("none")
}

const EQ_BOOK: &str = "A00a \"Start\" *\nB00 \"KP\" 1.e4 *\nC20 \"KP\" 1.e4 e5 *\nA40 \"QP\" 1.d4 *\nA46 \"Indian\" 1.d4 Nf6 2.Nf3 *\n";

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn one_pass_search_equals_the_scid_scan_per_position(
        seed in 1u64..u64::MAX,
        refs_n in 0usize..14,
        branching in 1usize..4,
        older_only in any::<bool>(),
        use_book in any::<bool>(),
        own in any::<bool>(),
    ) {
        // Arrange
        let mut rng = Rng(seed | 1);
        let dates = [None, Some((2000, 1, 1)), Some((2001, 5, 5)), Some((2001, 5, 5)), Some((1999, 12, 31))];
        let pick_date = |rng: &mut Rng| dates[rng.below(dates.len())];
        let refs: Vec<Spec> = (0..refs_n)
            .map(|_| {
                let game = random_game(&mut rng, 12, branching + 1);
                let mut s = Spec { game, ..spec(&[]) };
                if let Some((y, m, d)) = pick_date(&mut rng) {
                    s.date = Some(date(y, m, d));
                }
                s
            })
            .collect();
        let own_idx = (own && refs_n > 0).then(|| rng.below(refs_n));
        let mut current = match own_idx {
            Some(i) => refs[i].game.clone(),
            None => random_game(&mut rng, 12, branching + 1),
        };
        if let Some((y, m, d)) = pick_date(&mut rng) {
            current.tags_mut().date = date(y, m, d);
        }
        let book = EcoBook::parse(EQ_BOOK).unwrap();
        let book = use_book.then_some(&book);
        let dir = tempfile::tempdir().unwrap();
        let base = write_db(dir.path(), &refs);
        let mut db = open(&base);
        // Act
        let got = nov(&mut db, &current, book, older_only, own_idx.map(|i| u32::try_from(i).unwrap() + 1));
        let want = oracle(&refs, &current, book, older_only, own_idx);
        // Assert
        match (got, want) {
            (Ok(n), Ok((ply, san, num, color))) => {
                prop_assert_eq!((n.ply, n.san, n.move_number, n.color), (ply, san, num, color));
            }
            (Err(NoveltyError::NoNovelty), Err("none")) => {}
            (g, w) => prop_assert!(false, "got {:?}, oracle {:?}", g, w),
        }
    }
}

// ============================================================ compat (Scid reference data)

fn scid_reference() -> std::path::PathBuf {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27");
    assert!(
        dir.is_dir(),
        "Scid vs PC reference not found at {} (see docs/referencia-scid.md)",
        dir.display()
    );
    dir
}

fn scid_eco() -> EcoBook {
    let bytes = std::fs::read(scid_reference().join("scid.eco")).unwrap();
    EcoBook::parse(&String::from_utf8(bytes).unwrap()).unwrap()
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_summary_b9_matches_the_scid_thinning() {
    // Expected values computed by hand-simulating `PBook::EcoSummary`
    // (pbook.cpp 421-450) over the entries of scid.eco: with a prefix shorter
    // than 3 characters an entry that shares `len + 1` characters with the
    // last one shown is skipped.
    let book = scid_eco();
    let b9 = book.summary("B9");
    assert_eq!(b9.len(), 10);
    let labels: Vec<(String, &str)> = b9.iter().map(|e| (e.eco().to_string(), e.name())).collect();
    assert_eq!(labels[0], ("B90a".to_owned(), "Sicilian: Najdorf"));
    assert_eq!(labels[1], ("B91a".to_owned(), "Sicilian: Najdorf, 6.g3"));
    assert_eq!(labels[2].0, "B92a");
    assert_eq!(labels[9].0.chars().take(3).collect::<String>(), "B99");
    // A 3-character prefix is not thinned.
    assert_eq!(book.summary("B90").len(), 25);
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_eco_find_sicilian_counts_the_entries_with_that_name() {
    // 1561 entries of scid.eco have "Sicilian" in `code [name]` (counted
    // from the file text, case sensitive, as `PBook::EcoFind`).
    let book = scid_eco();
    let hits = book.find("Sicilian");
    assert_eq!(hits.len(), 1561);
    assert!(hits.iter().all(|e| e.name().contains("Sicilian")));
    assert_eq!(hits[0].eco().to_string(), "A00l");
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_classify_a_copy_of_a_scid_database_and_reopen() {
    use mychess_core::ClassifyOptions;
    use mychess_db::si4::Si4Writer;
    use mychess_db::{ClassifySelection, classify_database};

    let book = scid_eco();
    let bases = scid_reference().join("bases");
    for name in ["endings", "tactics"] {
        // Copy the three files; the reference stays untouched.
        let dir = tempfile::tempdir().unwrap();
        for ext in ["si4", "sn4", "sg4"] {
            std::fs::copy(
                bases.join(format!("{name}.{ext}")),
                dir.path().join(format!("{name}.{ext}")),
            )
            .unwrap();
        }
        let copy = dir.path().join(name);
        // Act
        let mut writer = Si4Writer::open(&copy).unwrap();
        let report = classify_database(
            &mut writer,
            &book,
            ClassifySelection::All,
            true,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
        drop(writer);
        // Assert: every stored code equals the core's classification of the
        // decoded game (an independent path), after reopening.
        let mut db = Si4Database::open(&copy).unwrap();
        let mut classified = 0u32;
        for n in 1..=db.len() {
            let entry = db.entry(n).unwrap();
            if entry.length == 0 {
                continue;
            }
            let game = db.game(n).unwrap();
            let want = book
                .classify(&game, ClassifyOptions::batch())
                .map(|m| m.eco(true));
            assert_eq!(entry.eco(), want, "{name} game {n}");
            classified += 1;
        }
        assert_eq!(report.classified, classified, "{name}");
        // Idempotent on the copy.
        let mut writer = Si4Writer::open(&copy).unwrap();
        let again = classify_database(
            &mut writer,
            &book,
            ClassifySelection::All,
            true,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
        assert_eq!(again.changed, 0, "{name}");
    }
}
