//! Contract tests of the opening tree engine, part 1: counting, statistics
//! and the matching rules (spec `docs/specs/m5-1-motor-da-arvore.md`, rules
//! 3 to 13, D943, D947, D948, D954, D956). Expected values are computed by
//! hand from the formulas of the spec.
#![allow(
    clippy::unwrap_used,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::cast_possible_truncation
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m5_1_support.rs"]
mod support;

use mychess_core::{Game, GameResult, Position};
use mychess_db::si4::GameFlags;
use mychess_db::tree::{TreeError, TreeOptions, TreeState, performance};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{db_of, gb, pos_after, repeat, row, run, sans, tree};

const W: GameResult = GameResult::White;
const BL: GameResult = GameResult::Black;
const D: GameResult = GameResult::Draw;
const NONE: GameResult = GameResult::Unknown;

// ------------------------------------------------------------ counting

#[test]
fn start_position_has_one_node_per_first_move_with_counts() {
    // Arrange
    let (_fx, mut db) = db_of(vec![
        gb("e2e4 e7e5 g1f3").result(W),
        gb("e2e4 c7c5").result(BL),
        gb("d2d4 d7d5").result(D),
        gb("e2e4 e7e5").result(NONE),
        gb("c2c4").result(W),
    ]);
    // Act
    let r = tree(&mut db, &Position::new());
    // Assert
    let mut names = sans(&r);
    names.sort_unstable();
    assert_eq!(names, vec!["c4", "d4", "e4"]);
    let e4 = row(&r, "e4");
    assert_eq!((e4.total, e4.freq), (3, [1, 1, 0, 1]));
    let d4 = row(&r, "d4");
    assert_eq!((d4.total, d4.freq), (1, [0, 0, 1, 0]));
    let c4 = row(&r, "c4");
    assert_eq!((c4.total, c4.freq), (1, [1, 0, 0, 0]));
    let total = r.total.as_ref().unwrap();
    assert_eq!(total.total, 5);
    assert_eq!(total.freq, [2, 1, 1, 1]);
    assert_eq!(r.rows.iter().map(|x| x.total).sum::<u32>(), total.total);
    assert_eq!(r.turn, mychess_core::Color::White);
}

#[test]
fn tree_filter_holds_ply_plus_one_of_the_match() {
    let (_fx, mut db) = db_of(vec![
        gb("e2e4 e7e5"),
        gb("d2d4 d7d5"),
        gb("g1f3 d7d5 e2e4 e7e5"),
    ]);
    let mut state = TreeState::default();
    // After 1.e4 e5 (2 half moves) the first and no other game matches.
    run(
        &mut db,
        &pos_after("e2e4 e7e5"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let f = state.tree_filter().unwrap();
    assert_eq!(f.as_bytes(), &[3, 0, 0]);
    // After 1.e4 (1 half move): game 1 at ply 1 -> value 2.
    run(
        &mut db,
        &pos_after("e2e4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert_eq!(state.tree_filter().unwrap().as_bytes(), &[2, 0, 0]);
}

#[test]
fn no_game_reaches_the_position_gives_no_rows_and_no_total() {
    let (_fx, mut db) = db_of(vec![gb("e2e4 e7e5"), gb("d2d4")]);
    let r = tree(&mut db, &pos_after("c2c4"));
    assert!(r.rows.is_empty());
    assert!(r.total.is_none());
}

#[test]
fn empty_database_gives_empty_result() {
    let (_fx, mut db) = db_of(vec![]);
    let r = tree(&mut db, &Position::new());
    assert!(r.rows.is_empty());
    assert!(r.total.is_none());
}

// ------------------------------------------------------------ score

#[test]
fn score_is_truncated_from_the_white_point_of_view() {
    // e4: W, W, B -> (2*2 + 0 + 0) * 500 / 3 = 666; d4: W, B, B -> 333.
    let mut games = vec![
        gb("e2e4").result(W),
        gb("e2e4").result(W),
        gb("e2e4").result(BL),
    ];
    games.extend([
        gb("d2d4").result(W),
        gb("d2d4").result(BL),
        gb("d2d4").result(BL),
    ]);
    // c4: one game without result counts half a point; g3: only draws.
    games.push(gb("c2c4").result(NONE));
    games.extend(repeat(3, |_| gb("g2g3").result(D)));
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &Position::new());
    assert_eq!(row(&r, "e4").score_permille, 666);
    assert_eq!(row(&r, "d4").score_permille, 333);
    assert_eq!(row(&r, "c4").score_permille, 500);
    assert_eq!(row(&r, "g3").score_permille, 500);
    assert_eq!(row(&r, "e4").score_tenths(), (66, 6));
    assert_eq!(row(&r, "d4").score_tenths(), (33, 3));
    // Total: wins W=3, B=3, D=3, None=1 -> (6 + 3 + 1) * 500 / 10 = 500.
    assert_eq!(r.total.as_ref().unwrap().score_permille, 500);
}

#[rstest]
#[case(W, 1000)]
#[case(BL, 0)]
#[case(D, 500)]
#[case(NONE, 500)]
fn score_of_a_single_game_by_result(#[case] result: GameResult, #[case] expected: u32) {
    let (_fx, mut db) = db_of(vec![gb("e2e4").result(result)]);
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.rows[0].score_permille, expected);
    assert_eq!(r.total.as_ref().unwrap().score_permille, expected);
}

// ------------------------------------------------------------ Elo

fn rated_games(count: usize, white: u16, black: u16) -> Vec<base::B> {
    repeat(count, |_| gb("e2e4").elo(white, black).result(D))
}

#[rstest]
#[case(9, None)]
#[case(10, Some(2000))]
#[case(11, Some(2000))]
fn average_elo_needs_ten_rated_games(#[case] n: usize, #[case] expected: Option<u32>) {
    let (_fx, mut db) = db_of(rated_games(n, 2000, 2400));
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.rows[0].avg_elo, expected);
    assert_eq!(r.total.as_ref().unwrap().avg_elo, expected);
}

#[rstest]
#[case(9, None)]
#[case(10, Some(2400))]
fn performance_needs_ten_rated_opponents(#[case] n: usize, #[case] expected: Option<u32>) {
    // All draws: 50% -> performance equals the opponents' average.
    let (_fx, mut db) = db_of(rated_games(n, 2000, 2400));
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.rows[0].perf, expected);
    assert_eq!(r.total.as_ref().unwrap().perf, expected);
}

#[test]
fn elo_and_performance_counts_are_independent() {
    // 10 games with the white Elo, only 9 with the black Elo.
    let mut games = rated_games(9, 2000, 2400);
    games.push(gb("e2e4").elo(2000, 0).result(D));
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.rows[0].avg_elo, Some(2000));
    assert_eq!(r.rows[0].perf, None);
}

#[test]
fn games_without_elo_are_left_out_of_the_average() {
    // 10 rated at 2000 plus 5 unrated: the average stays 2000.
    let mut games = rated_games(10, 2000, 2400);
    games.extend(repeat(5, |_| gb("e2e4").elo(0, 0).result(D)));
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.rows[0].total, 15);
    assert_eq!(r.rows[0].avg_elo, Some(2000));
}

#[test]
fn average_elo_is_truncated() {
    // 5 x 2000 + 5 x 2001 = 20005; 20005 / 10 = 2000.
    let mut games = repeat(5, |_| gb("e2e4").elo(2000, 2000).result(D));
    games.extend(repeat(5, |_| gb("e2e4").elo(2001, 2000).result(D)));
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.rows[0].avg_elo, Some(2000));
}

#[test]
fn total_applies_the_threshold_to_the_global_sums() {
    // Each node has 5 rated games (None), the total has 10: 20010 / 10.
    let mut games = repeat(5, |_| gb("e2e4").elo(2000, 2400).result(D));
    games.extend(repeat(5, |_| gb("d2d4").elo(2002, 2400).result(D)));
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &Position::new());
    assert_eq!(row(&r, "e4").avg_elo, None);
    assert_eq!(row(&r, "d4").avg_elo, None);
    let total = r.total.as_ref().unwrap();
    assert_eq!(total.avg_elo, Some(2001));
    assert_eq!(total.perf, Some(2400));
}

#[test]
fn elo_is_taken_from_the_side_that_moves() {
    // After 1.e4 Black moves: the "Elo" is the black one, the performance
    // uses the white one.
    let games = repeat(10, |_| gb("e2e4 e7e5").elo(2300, 2100).result(D));
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &pos_after("e2e4"));
    assert_eq!(r.turn, mychess_core::Color::Black);
    assert_eq!(r.rows[0].avg_elo, Some(2100));
    assert_eq!(r.rows[0].perf, Some(2300));
}

// ------------------------------------------------------------ performance

#[test]
fn performance_at_the_spec_points() {
    assert_eq!(performance(2500, 50), 2500);
    // D956: the table gives 193 for 75%, not 202.
    assert_eq!(performance(2500, 75), 2693);
    assert_eq!(performance(2500, 100), 3300);
    assert_eq!(performance(2500, 0), 1700);
}

#[test]
fn performance_saturates_at_zero_and_clamps_percent() {
    assert_eq!(performance(0, 0), 0);
    assert_eq!(performance(500, 0), 0);
    assert_eq!(performance(800, 0), 0);
    assert_eq!(performance(801, 0), 1);
    assert_eq!(performance(2500, 101), 3300);
    assert_eq!(performance(2500, u32::MAX), 3300);
}

#[test]
fn performance_follows_the_whole_scid_table() {
    // perf_elodiff, spec rule 9.
    const TABLE: [u32; 51] = [
        0, 7, 14, 21, 29, 36, 43, 50, 57, 65, 72, 80, 87, 95, 102, 110, 117, 125, 133, 141, 149,
        158, 166, 175, 184, 193, 202, 211, 220, 230, 240, 251, 262, 273, 284, 296, 309, 322, 336,
        351, 366, 383, 401, 422, 444, 470, 501, 538, 589, 677, 800,
    ];
    for pct in 0..=100u32 {
        let expected = if pct < 50 {
            3000 - TABLE[(50 - pct) as usize]
        } else {
            3000 + TABLE[(pct - 50) as usize]
        };
        assert_eq!(performance(3000, pct), expected, "pct {pct}");
    }
}

#[test]
fn row_performance_uses_rounded_percent_for_white() {
    // 12 games: 5 white wins, 7 black wins -> score 10*500/12 = 416,
    // pct = (416 + 5) / 10 = 42, perf = 2400 - elodiff[8] = 2343.
    let mut games = repeat(5, |_| gb("e2e4").elo(2000, 2400).result(W));
    games.extend(repeat(7, |_| gb("e2e4").elo(2000, 2400).result(BL)));
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.rows[0].score_permille, 416);
    assert_eq!(r.rows[0].perf, Some(2343));
}

#[test]
fn row_performance_for_all_wins_and_for_a_low_score() {
    // 10 white wins: score 1000, pct 100 -> 2400 + 800.
    let (_fx, mut db) = db_of(repeat(10, |_| gb("e2e4").elo(2000, 2400).result(W)));
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.rows[0].perf, Some(3200));
    // 3 white wins, 7 black wins: score 300 -> pct 30 -> 2400 - elodiff[20].
    let mut games = repeat(3, |_| gb("e2e4").elo(2000, 2400).result(W));
    games.extend(repeat(7, |_| gb("e2e4").elo(2000, 2400).result(BL)));
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.rows[0].perf, Some(2400 - 149));
}

#[test]
fn row_performance_is_inverted_when_black_moves() {
    // After 1.e4: 3 white wins and 7 black wins. Score (White) = 300; for
    // Black pct = 100 - 30 = 70 -> perf = 2300 (white Elo) + elodiff[20].
    let mut games = repeat(3, |_| gb("e2e4 e7e5").elo(2300, 2100).result(W));
    games.extend(repeat(7, |_| gb("e2e4 e7e5").elo(2300, 2100).result(BL)));
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &pos_after("e2e4"));
    assert_eq!(r.rows[0].score_permille, 300);
    assert_eq!(r.rows[0].perf, Some(2300 + 149));
    assert_eq!(r.total.as_ref().unwrap().perf, Some(2300 + 149));
}

// ------------------------------------------------------------ year

fn year_games(dates: &[&str]) -> Vec<base::B> {
    dates.iter().map(|d| gb("e2e4").date(d)).collect()
}

#[rstest]
#[case(&["2000.01.01", "2001.01.01"], Some(2001))] // (4001 + 1) / 2
#[case(&["2000.01.01", "2000.01.01", "2001.01.01"], Some(2000))] // (6001 + 1) / 3
#[case(&["2000.01.01", "2001.01.01", "2001.01.01"], Some(2001))] // (6002 + 1) / 3
#[case(&["2000.01.01", "????.??.??", "2001.01.01"], Some(2001))] // year 0 ignored
#[case(&["2000.??.??", "2000.??.??"], Some(2000))]
#[case(&["????.??.??", "????.??.??"], None)]
fn average_year_is_rounded_and_ignores_unknown(
    #[case] dates: &[&str],
    #[case] expected: Option<u32>,
) {
    let (_fx, mut db) = db_of(year_games(dates));
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.rows[0].avg_year, expected);
    assert_eq!(r.total.as_ref().unwrap().avg_year, expected);
}

#[test]
fn total_year_uses_the_global_sums() {
    // e4: 2000; d4: 2001, 2003 -> sum 6004, count 3: (6004 + 1) / 3 = 2001.
    let mut games = year_games(&["2000.01.01"]);
    games.extend(["2001.01.01", "2003.01.01"].map(|d| gb("d2d4").date(d)));
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &Position::new());
    assert_eq!(row(&r, "e4").avg_year, Some(2000));
    assert_eq!(row(&r, "d4").avg_year, Some(2002));
    assert_eq!(r.total.as_ref().unwrap().avg_year, Some(2001));
}

// ------------------------------------------------------------ rounding helpers

#[test]
fn display_helpers_round_as_the_spec_says() {
    // 1 of 3 games: share 33.3, draws 1 of 3 -> 333 + 5 -> 33.
    let mut games = vec![gb("e2e4").result(D)];
    games.extend(repeat(2, |_| gb("d2d4").result(W)));
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &Position::new());
    let n = r.total.as_ref().unwrap().total;
    assert_eq!(n, 3);
    assert_eq!(row(&r, "e4").share_tenths(n), (33, 3));
    assert_eq!(row(&r, "d4").share_tenths(n), (66, 6));
    assert_eq!(row(&r, "e4").draw_percent(), 100);
    assert_eq!(row(&r, "d4").draw_percent(), 0);
    // Total: 1 draw of 3 -> (333 + 5) / 10 = 33.
    assert_eq!(r.total.as_ref().unwrap().draw_percent(), 33);
    // Total score: (2*2 + 1) * 500 / 3 = 833.
    assert_eq!(r.total.as_ref().unwrap().score_tenths(), (83, 3));
    // Rounding up: 2 draws of 3 -> (666 + 5) / 10 = 67.
    let (_fx, mut db) = db_of(vec![
        gb("e2e4").result(D),
        gb("e2e4").result(D),
        gb("e2e4").result(W),
    ]);
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.rows[0].draw_percent(), 67);
}

// ------------------------------------------------------------ matching

#[test]
fn repeated_position_counts_once_per_game_at_the_first_occurrence() {
    // 1.Nf3 Nf6 2.Ng1 Ng8 3.e4: the start position occurs at ply 0 and 4.
    let (_fx, mut db) = db_of(vec![gb("g1f3 g8f6 f3g1 f6g8 e2e4")]);
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert_eq!(sans(&r), vec!["Nf3"]);
    assert_eq!(r.rows[0].total, 1);
    assert_eq!(r.total.as_ref().unwrap().total, 1);
    assert_eq!(state.tree_filter().unwrap().as_bytes(), &[1]);
}

#[test]
fn repeated_position_in_the_middle_uses_the_first_occurrence_ply() {
    // After 1.Nf3 Nf6 (ply 2) again at ply 6; the first one counts.
    let (_fx, mut db) = db_of(vec![gb("g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 e2e4")]);
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &pos_after("g1f3 g8f6"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert_eq!(sans(&r), vec!["Ng1"]);
    assert_eq!(r.total.as_ref().unwrap().total, 1);
    assert_eq!(state.tree_filter().unwrap().get(1), 3);
}

#[test]
fn game_ending_at_the_position_produces_the_end_node() {
    let (_fx, mut db) = db_of(vec![
        gb("e2e4 e7e5").result(W),
        gb("e2e4 e7e5 g1f3").result(D),
    ]);
    let r = tree(&mut db, &pos_after("e2e4 e7e5"));
    let end = row(&r, "[end]");
    assert!(end.is_end());
    assert_eq!(end.mv, None);
    assert_eq!(end.uci, None);
    assert_eq!((end.total, end.freq), (1, [1, 0, 0, 0]));
    assert_eq!(r.total.as_ref().unwrap().total, 2);
    assert_eq!(row(&r, "Nf3").total, 1);
}

#[test]
fn zero_move_game_is_an_end_node_only_at_the_standard_start() {
    let (_fx, mut db) = db_of(vec![base::B::of(Game::new()).result(D), gb("e2e4 e7e5")]);
    // At the standard start the empty game ends there.
    let r = tree(&mut db, &Position::new());
    assert_eq!(row(&r, "[end]").total, 1);
    assert_eq!(row(&r, "e4").total, 1);
    // Elsewhere it is not counted.
    let r = tree(&mut db, &pos_after("e2e4"));
    assert_eq!(sans(&r), vec!["e5"]);
    assert_eq!(r.total.as_ref().unwrap().total, 1);
}

#[test]
fn zero_move_game_from_a_fen_is_ignored_even_at_its_own_position() {
    let fen = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
    let (_fx, mut db) = db_of(vec![
        base::B::of(Game::from_position(Position::from_fen(fen).unwrap())),
        base::B::of(base::fen_game(fen, &["e2e4"])),
    ]);
    let r = tree(&mut db, &Position::from_fen(fen).unwrap());
    assert_eq!(sans(&r), vec!["e4"]);
    assert_eq!(r.total.as_ref().unwrap().total, 1);
}

#[test]
fn game_without_data_is_skipped() {
    let list = vec![gb("e2e4").done(), gb("e2e4").done(), gb("d2d4").done()];
    let fx = base::build(&list);
    base::zero_length(&fx, 2);
    let mut db = fx.open();
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert_eq!(row(&r, "e4").total, 1);
    assert_eq!(r.total.as_ref().unwrap().total, 2);
    assert_eq!(state.tree_filter().unwrap().get(2), 0);
    assert!(r.unreadable.is_empty(), "no data is not a read failure");
}

#[test]
fn game_with_its_own_start_position_is_searched_from_it() {
    let fen = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
    let (_fx, mut db) = db_of(vec![
        base::B::of(base::fen_game(fen, &["e2e4"])),
        base::B::of(base::fen_game(fen, &["e2e3"])),
        gb("e2e4"),
    ]);
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &Position::from_fen(fen).unwrap(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let mut names = sans(&r);
    names.sort_unstable();
    assert_eq!(names, vec!["e3", "e4"]);
    assert_eq!(state.tree_filter().unwrap().as_bytes(), &[1, 1, 0]);
    // The standard start does not match the FEN games.
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.total.as_ref().unwrap().total, 1);
}

#[test]
fn variations_are_not_searched() {
    // Main line 1.e4 e5; variation 1.e4 c5. Searching after 1.e4 c5 finds
    // nothing; after 1.e4 the only move is e5.
    let mut game = Game::new();
    let root = game.root();
    let n1 = base::play(&mut game, root, "e2e4");
    base::play(&mut game, n1, "e7e5");
    base::play(&mut game, n1, "c7c5");
    let (_fx, mut db) = db_of(vec![base::B::of(game)]);
    assert!(
        tree(&mut db, &pos_after("e2e4 c7c5")).rows.is_empty(),
        "variation position must not match"
    );
    let r = tree(&mut db, &pos_after("e2e4"));
    assert_eq!(sans(&r), vec!["e5"]);
}

#[test]
fn deleted_games_count() {
    let list = vec![
        gb("e2e4").done(),
        gb("e2e4").flags(GameFlags::DELETE),
        gb("d2d4").flags(GameFlags::DELETE),
    ];
    let fx = base::build(&list);
    let mut db = fx.open();
    let r = tree(&mut db, &Position::new());
    assert_eq!(row(&r, "e4").total, 2);
    assert_eq!(row(&r, "d4").total, 1);
}

#[test]
fn castling_rights_and_en_passant_do_not_prevent_a_match() {
    // Game 1 starts from the standard board but without castling rights;
    // game 2 plays from the standard start. Searching the standard start
    // (with rights) must find both.
    let no_rights = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1";
    let (_fx, mut db) = db_of(vec![
        base::B::of(base::fen_game(no_rights, &["e2e4"])),
        gb("e2e4"),
        gb("d2d4"),
    ]);
    let r = tree(&mut db, &Position::new());
    assert_eq!(row(&r, "e4").total, 2);
    assert_eq!(r.total.as_ref().unwrap().total, 3);
    // And the reverse: searching the position without rights.
    let r = tree(&mut db, &Position::from_fen(no_rights).unwrap());
    assert_eq!(r.total.as_ref().unwrap().total, 3);
}

#[test]
fn en_passant_square_does_not_prevent_a_match() {
    // After 1.e4, with the e3 square in the FEN of game 1 and not in game 2.
    let with_ep = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
    let (_fx, mut db) = db_of(vec![
        base::B::of(base::fen_game(with_ep, &["e7e5"])),
        gb("e2e4 e7e5"),
    ]);
    let r = tree(&mut db, &pos_after("e2e4"));
    assert_eq!(row(&r, "e5").total, 2);
}

#[test]
fn transposition_into_the_position_is_matched() {
    // Both reach the same board with Black to move after 1.e4 e5 2.Nf3.
    let (_fx, mut db) = db_of(vec![gb("e2e4 e7e5 g1f3 b8c6"), gb("g1f3 e7e5 e2e4 b8c6")]);
    let r = tree(&mut db, &pos_after("e2e4 e7e5 g1f3"));
    assert_eq!(row(&r, "Nc6").total, 2);
}

#[test]
fn filter_value_saturates_at_255() {
    // 252 + 1 and 256 + 1 half moves before 1.e4 is first reached.
    let shuffle = |n: usize| {
        let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
        let mut moves: Vec<&str> = (0..n).map(|i| cycle[i % 4]).collect();
        moves.push("e2e4");
        moves.join(" ")
    };
    let (_fx, mut db) = db_of(vec![gb(&shuffle(252)), gb(&shuffle(256))]);
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &pos_after("e2e4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    // Both end at the position: an [end] node.
    assert_eq!(row(&r, "[end]").total, 2);
    let f = state.tree_filter().unwrap();
    assert_eq!(f.get(1), 254, "ply 253 + 1");
    assert_eq!(f.get(2), 255, "ply 257 + 1 saturates");
}

// ------------------------------------------------------------ node limit

fn many_moves_fen() -> &'static str {
    // 218 legal moves for White.
    "R6R/3Q4/1Q4Q1/4Q3/2Q4Q/Q4Q2/pp1Q4/kBNN1KB1 w - - 0 1"
}

fn games_with_distinct_first_moves(n: usize) -> Vec<base::B> {
    let pos = Position::from_fen(many_moves_fen()).unwrap();
    let moves = pos.legal_moves();
    assert!(moves.len() >= 61);
    moves
        .into_iter()
        .take(n)
        .map(|mv| {
            let mut game = Game::from_position(pos.clone());
            let root = game.root();
            game.add_move(root, mv).unwrap();
            base::B::of(game)
        })
        .collect()
}

#[test]
fn sixty_distinct_moves_are_accepted() {
    let (_fx, mut db) = db_of(games_with_distinct_first_moves(60));
    let r = tree(&mut db, &Position::from_fen(many_moves_fen()).unwrap());
    assert_eq!(r.rows.len(), 60);
    assert_eq!(r.total.as_ref().unwrap().total, 60);
}

#[test]
fn sixty_one_distinct_moves_fail_with_too_many_moves_and_keep_the_state() {
    let (_fx, mut db) = db_of(games_with_distinct_first_moves(61));
    let mut state = TreeState::default();
    let err = run(
        &mut db,
        &Position::from_fen(many_moves_fen()).unwrap(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap_err();
    assert!(matches!(err, TreeError::TooManyMoves), "{err:?}");
    assert!(state.tree_filter().is_none());
    assert!(state.db_filter().is_none());
}

#[test]
fn promotions_are_distinct_nodes() {
    // Pawn on a7 can promote four ways; the key includes the promotion.
    let fen = "4k3/P7/8/8/8/8/8/4K3 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let games: Vec<_> = ["a7a8q", "a7a8r", "a7a8b", "a7a8n"]
        .iter()
        .map(|m| base::B::of(base::fen_game(fen, &[m])))
        .collect();
    let (_fx, mut db) = db_of(games);
    let r = tree(&mut db, &pos);
    assert_eq!(r.rows.len(), 4);
    let mut s: Vec<_> = sans(&r).iter().map(|s| (*s).to_owned()).collect();
    s.sort();
    assert_eq!(s, vec!["a8=B", "a8=N", "a8=Q+", "a8=R+"]);
}

// ------------------------------------------------------------ passes searched

#[test]
fn rows_expose_uci_and_hidden_flag() {
    let (_fx, mut db) = db_of(vec![gb("g1f3")]);
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert_eq!(r.rows[0].san, "Nf3");
    assert_eq!(r.rows[0].uci.as_deref(), Some("g1f3"));
    assert!(!r.rows[0].hidden);
    let hide = TreeOptions {
        hide_moves: true,
        ..TreeOptions::default()
    };
    let r = run(&mut db, &Position::new(), &hide, &mut TreeState::default()).unwrap();
    assert!(r.rows[0].hidden);
    assert_eq!(r.rows[0].san, "Nf3", "training only marks the row");
    assert_eq!(r.rows[0].total, 1);
}

#[test]
fn unreadable_record_is_counted_and_does_not_enter_the_tree() {
    let list = vec![gb("e2e4").done(), gb("e2e4").done(), gb("e2e4").done()];
    let fx = base::build(&list);
    base::break_offset(&fx, 2);
    let mut db = fx.open();
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert_eq!(r.rows[0].total, 2);
    assert_eq!(r.unreadable_total, 1);
    assert_eq!(r.unreadable, vec![2]);
    assert_eq!(state.tree_filter().unwrap().as_bytes(), &[1, 0, 1]);
}

#[test]
fn unreadable_index_entry_is_counted() {
    let list = vec![gb("e2e4").done(), gb("e2e4").done()];
    let fx = base::build(&list);
    base::break_entry(&fx, 1);
    let mut db = fx.open();
    let r = tree(&mut db, &Position::new());
    assert_eq!(r.unreadable_total, 1);
    assert_eq!(r.unreadable, vec![1]);
    assert_eq!(r.rows[0].total, 1);
}
