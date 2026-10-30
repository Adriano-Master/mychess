//! Contract tests of the search by position (spec
//! `docs/specs/m6-1-buscas-avancadas.md`, comportamento 5 a 11 and regras
//! 10 a 16 of "Posição"; D3208 to D3214).
#![allow(
    clippy::assert_is_empty,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    unused_qualifications,
    clippy::field_reassign_with_default,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m6_1a_support.rs"]
mod support;

use mychess_core::Position;
use mychess_db::{
    BoardMatch, BoardSearch, CancelToken, Filter, FilterOp, NoProgress, search_board,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use support::{
    arb_games, db_of, game, game_fen, numbers, oracle_board, pgn_fixture, pos_after, position_in,
    run_board, values,
};

fn exact(pos: Position) -> BoardSearch {
    BoardSearch::new(pos)
}

fn kind(pos: Position, kind: BoardMatch) -> BoardSearch {
    BoardSearch {
        kind,
        ..BoardSearch::new(pos)
    }
}

fn fen(text: &str) -> Position {
    Position::from_fen(text).unwrap()
}

// ------------------------------------------------------- what matches

#[test]
fn the_standard_start_matches_every_standard_game_even_the_empty_one() {
    let (_fx, mut db) = db_of(vec![
        game(""),
        game("e2e4 e7e5"),
        game_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", "e2e4"),
    ]);
    let (filter, report) = run_board(&mut db, &exact(Position::new()));
    assert_eq!(values(&filter), vec![(1, 1), (2, 1)]);
    assert_eq!(report.matched, 2);
}

#[test]
fn exact_finds_the_position_with_the_ply_plus_one() {
    let (_fx, mut db) = db_of(vec![
        game("e2e4 e7e5 g1f3"),
        game("e2e4 c7c5"),
        // The same squares reached by a transposition, later.
        game("g1f3 g8f6 f3g1 f6g8 e2e4 e7e5"),
    ]);
    let (filter, _) = run_board(&mut db, &exact(pos_after("e2e4 e7e5")));
    assert_eq!(values(&filter), vec![(1, 3), (3, 7)]);
}

#[test]
fn castling_rights_and_en_passant_never_count() {
    let (_fx, mut db) = db_of(vec![game("e2e4 e7e5")]);
    let with_rights = pos_after("e2e4 e7e5");
    let without = fen("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w - - 0 2");
    let with_ep = fen("rnbqkbnr/pppp1ppp/8/4pP2/8/8/PPPPP1PP/RNBQKBNR w KQkq e6 0 3");
    assert_eq!(numbers(&run_board(&mut db, &exact(with_rights)).0), vec![1]);
    assert_eq!(numbers(&run_board(&mut db, &exact(without)).0), vec![1]);
    // A different position that merely has an en passant square.
    assert_eq!(
        numbers(&run_board(&mut db, &exact(with_ep)).0),
        Vec::<u32>::new()
    );
}

#[test]
fn the_other_side_to_move_does_not_match() {
    let (_fx, mut db) = db_of(vec![game("e2e4 e7e5")]);
    let black_to_move = fen("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 2");
    let (filter, _) = run_board(&mut db, &exact(black_to_move));
    assert!(numbers(&filter).is_empty());
}

#[test]
fn exact_distinguishes_pieces_but_pawns_files_and_material_do_not() {
    let (_fx, mut db) = db_of(vec![
        game("e2e4 e7e5 g1f3 b8c6"),
        game("e2e4 e7e5 b1c3 b8c6"),
    ]);
    let target = pos_after("e2e4 e7e5 g1f3 b8c6");
    let mut found = |k| numbers(&run_board(&mut db, &kind(target.clone(), k)).0);
    assert_eq!(found(BoardMatch::Exact), vec![1]);
    // Game 2 never has the Nf3 position, but its pawns are the same; the
    // pieces may be anywhere with Pawns, Files and Material, at any ply.
    assert_eq!(found(BoardMatch::Pawns), vec![1, 2]);
    assert_eq!(found(BoardMatch::Files), vec![1, 2]);
    assert_eq!(found(BoardMatch::Material), vec![1, 2]);
}

#[test]
fn a_pawn_on_another_square_of_the_same_file_separates_pawns_from_files() {
    let (_fx, mut db) = db_of(vec![game("e2e3 e7e6"), game("e2e4 e7e5")]);
    let target = pos_after("e2e4 e7e5");
    let mut found = |k| numbers(&run_board(&mut db, &kind(target.clone(), k)).0);
    assert_eq!(found(BoardMatch::Exact), vec![2]);
    assert_eq!(found(BoardMatch::Pawns), vec![2]);
    // Same number of pawns on every file: game 1 matches by files.
    assert_eq!(found(BoardMatch::Files), vec![1, 2]);
    assert_eq!(found(BoardMatch::Material), vec![1, 2]);
}

#[test]
fn another_piece_count_separates_all_four_kinds() {
    let (_fx, mut db) = db_of(vec![
        game("e2e4 d7d5 e4d5 d8d5"),
        game("e2e4 d7d5 e4e5 e7e6"),
    ]);
    // After 1.e4 d5 2.exd5 Qxd5: white has 7 pawns, black 7 pawns.
    let target = pos_after("e2e4 d7d5 e4d5 d8d5");
    for k in [
        BoardMatch::Exact,
        BoardMatch::Pawns,
        BoardMatch::Files,
        BoardMatch::Material,
    ] {
        let (filter, _) = run_board(&mut db, &kind(target.clone(), k));
        assert_eq!(numbers(&filter), vec![1], "{k:?}");
    }
}

#[test]
fn ignore_colors_finds_the_position_turned_upside_down() {
    let target = fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1");
    let upside_down = "4k3/4p3/8/8/8/8/8/4K3 b - - 0 1";
    let (_fx, mut db) = db_of(vec![
        game_fen(upside_down, "e7e6"),
        game_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", "e2e3"),
    ]);
    let mut q = exact(target.clone());
    let (filter, _) = run_board(&mut db, &q);
    assert_eq!(numbers(&filter), vec![2]);
    q.ignore_colors = true;
    let (filter, _) = run_board(&mut db, &q);
    assert_eq!(values(&filter), vec![(1, 1), (2, 1)]);
    // And the other way round: searching the upside-down position finds the
    // normal game.
    let mut q = exact(fen(upside_down));
    q.ignore_colors = true;
    assert_eq!(numbers(&run_board(&mut db, &q).0), vec![1, 2]);
}

#[test]
fn the_normal_position_wins_over_the_flipped_one_even_if_later() {
    // White Ke1 Ra1 against Black Ke8 Ra8 is its own upside-down image.
    // Black to move at ply 0 (the flipped target), a null move, then White to
    // move at ply 1 (the normal target).
    let start = "r3k3/8/8/8/8/8/8/R3K3 b - - 0 1";
    let (_fx, mut db) = db_of(vec![game_fen(start, "0000 a1a2")]);
    let mut q = exact(fen("r3k3/8/8/8/8/8/8/R3K3 w - - 0 1"));
    q.ignore_colors = true;
    let (filter, _) = run_board(&mut db, &q);
    assert_eq!(values(&filter), vec![(1, 2)]);
    q.ignore_colors = false;
    assert_eq!(values(&run_board(&mut db, &q).0), vec![(1, 2)]);
}

// ---------------------------------------------------------- variations

#[test]
fn a_position_only_in_a_variation_needs_the_option() {
    let (_fx, mut db) = db_of(vec![
        game("e2e4 e7e5 (c7c5 g1f3) g1f3"),
        game("e2e4 e7e5 g1f3"),
    ]);
    let target = pos_after("e2e4 c7c5 g1f3");
    let mut q = exact(target);
    assert!(numbers(&run_board(&mut db, &q).0).is_empty());
    q.in_variations = true;
    // The variation replaces 1...e5, which is played from ply 1: value 2.
    assert_eq!(values(&run_board(&mut db, &q).0), vec![(1, 2)]);
}

#[test]
fn the_ply_of_a_nested_variation_is_the_one_of_the_outermost() {
    let (_fx, mut db) = db_of(vec![game("e2e4 e7e5 (c7c5 g1f3 (b1c3 d7d5) d7d6) g1f3")]);
    let mut q = exact(pos_after("e2e4 c7c5 b1c3 d7d5"));
    q.in_variations = true;
    assert_eq!(values(&run_board(&mut db, &q).0), vec![(1, 2)]);
}

#[test]
fn the_main_line_wins_over_an_earlier_variation() {
    // Position after 1.e4 e5: in the variation at ply 2 (branch ply 0), in
    // the main line only at ply 6.
    let (_fx, mut db) = db_of(vec![game(
        "g1f3 (e2e4 e7e5 d2d4) g8f6 f3g1 f6g8 e2e4 e7e5 d2d4",
    )]);
    let mut q = exact(pos_after("e2e4 e7e5"));
    q.in_variations = true;
    assert_eq!(values(&run_board(&mut db, &q).0), vec![(1, 7)]);
}

#[test]
fn games_without_variations_give_the_same_result_with_and_without_the_option() {
    let (_fx, mut db) = db_of(vec![game("e2e4 e7e5 g1f3"), game("d2d4"), game("")]);
    for k in [BoardMatch::Exact, BoardMatch::Material] {
        let mut q = kind(pos_after("e2e4 e7e5"), k);
        let plain = values(&run_board(&mut db, &q).0);
        q.in_variations = true;
        assert_eq!(plain, values(&run_board(&mut db, &q).0));
    }
}

// ---------------------------------------------------- operation, errors

#[test]
fn and_or_and_reset_follow_the_filter_operations() {
    let (_fx, mut db) = db_of(vec![
        game("e2e4 e7e5"),
        game("e2e4 c7c5"),
        game("e2e4 e7e5 g1f3"),
    ]);
    let target = pos_after("e2e4 e7e5");
    let mut filter = Filter::new(3);
    filter.fill(0);
    filter.set(1, 9);
    filter.set(2, 9);
    let mut q = exact(target);
    q.op = FilterOp::And;
    let mut f = filter.clone();
    search_board(&mut db, &q, &mut f, &mut NoProgress, &CancelToken::new()).unwrap();
    assert_eq!(values(&f), vec![(1, 3)]);
    q.op = FilterOp::Or;
    let mut f = filter.clone();
    search_board(&mut db, &q, &mut f, &mut NoProgress, &CancelToken::new()).unwrap();
    // Game 1 and 2 were in: untouched. Game 3 is added.
    assert_eq!(values(&f), vec![(1, 9), (2, 9), (3, 3)]);
    q.op = FilterOp::Reset;
    let mut f = filter;
    search_board(&mut db, &q, &mut f, &mut NoProgress, &CancelToken::new()).unwrap();
    assert_eq!(values(&f), vec![(1, 3), (3, 3)]);
}

#[test]
fn the_most_crowded_position_the_core_allows_is_searchable() {
    // The core refuses more than 16 pieces for a side, the room of the SI4
    // piece list, so `SearchError::UnsupportedPosition` is a defensive error
    // that a `Position` cannot trigger; the boundary must work.
    assert!(Position::from_fen("4k3/8/8/8/8/Q7/PPPPPPPP/QQQQQQQK w - - 0 1").is_err());
    let full = fen("4k3/8/8/8/8/Q7/PPPPPPPP/QQQQQQ1K w - - 0 1");
    let (_fx, mut db) = db_of(vec![game("e2e4")]);
    let (filter, report) = run_board(&mut db, &exact(full));
    assert!(numbers(&filter).is_empty());
    assert!(!report.interrupted);
}

#[test]
fn a_late_match_is_limited_to_255_and_does_not_wrap() {
    // 280 plies of knight shuffling, then a capture: the new position is at
    // ply 283, and 283 + 1 would wrap to 28 in a byte.
    let mut moves = String::new();
    for _ in 0..70 {
        moves.push_str("g1f3 g8f6 f3g1 f6g8 ");
    }
    moves.push_str("e2e4 d7d5 e4d5");
    let (_fx, mut db) = db_of(vec![game(&moves)]);
    let (filter, _) = run_board(&mut db, &exact(pos_after("e2e4 d7d5 e4d5")));
    assert_eq!(values(&filter), vec![(1, 255)]);
}

// ----------------------------------------------- stale index (START)

/// Overwrites `FinalMatSig` (bytes 34 to 36, after the stored-line code) with
/// the most material and the home-pawn data (38 to 46) with 16 changes of
/// the first pawn, as an old Scid may leave stale values: a search that
/// trusted them would discard the game.
fn spoil_index(fx: &base::Fixture, n: u32) {
    base::patch_entry(fx, n, |e| {
        e[34..37].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
        e[38] = (e[38] & 0xC0) | 16;
        e[39..47].fill(0);
    });
}

#[test]
fn a_game_with_its_own_start_position_ignores_a_stale_index() {
    let games = vec![
        game_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", "e2e4 e8d7"),
        game_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", "e2e3 e8d7"),
    ];
    let (fx, db) = db_of(games);
    drop(db);
    spoil_index(&fx, 1);
    spoil_index(&fx, 2);
    let mut db = fx.open();
    for (k, expected) in [
        (BoardMatch::Exact, vec![1]),
        (BoardMatch::Pawns, vec![1]),
        (BoardMatch::Files, vec![1, 2]),
        (BoardMatch::Material, vec![1, 2]),
    ] {
        let q = kind(fen("8/3k4/8/8/4P3/8/8/4K3 w - - 1 2"), k);
        let (filter, _) = run_board(&mut db, &q);
        assert_eq!(numbers(&filter), expected, "{k:?}");
    }
}

#[test]
fn a_standard_game_with_a_spoiled_index_is_skipped_only_by_the_shortcuts() {
    let (fx, db) = db_of(vec![game("e2e4 e7e5")]);
    drop(db);
    spoil_index(&fx, 1);
    let mut db = fx.open();
    let mut q = exact(pos_after("e2e4 e7e5"));
    assert!(numbers(&run_board(&mut db, &q).0).is_empty());
    q.shortcuts = false;
    assert_eq!(numbers(&run_board(&mut db, &q).0), vec![1]);
}

#[test]
fn the_shortcuts_skip_games_that_cannot_match_without_changing_the_answer() {
    let (_fx, mut db) = db_of(vec![
        game("e2e4 e7e5 g1f3"),
        game("d2d4 d7d5"),
        game("e2e4 d7d5 e4d5 d8d5"),
    ]);
    for target in [
        pos_after("e2e4 e7e5"),
        pos_after("d2d4"),
        pos_after("e2e4 d7d5 e4d5"),
        Position::new(),
    ] {
        for k in [
            BoardMatch::Exact,
            BoardMatch::Pawns,
            BoardMatch::Files,
            BoardMatch::Material,
        ] {
            let mut q = kind(target.clone(), k);
            q.ignore_colors = true;
            let fast = run_board(&mut db, &q).0;
            q.shortcuts = false;
            let slow = run_board(&mut db, &q).0;
            assert_eq!(fast.as_bytes(), slow.as_bytes(), "{k:?}");
        }
    }
}

// ----------------------------------------------------- fixture in PGN

#[test]
fn searches_a_pgn_database_with_variations_and_a_start_position() {
    let mut db = pgn_fixture("m6_1a_search.pgn");
    // Game 3 reaches 1.e4 c5 2.Nf3 only in its variation.
    let mut q = exact(pos_after("e2e4 c7c5 g1f3"));
    assert!(numbers(&run_board(&mut db, &q).0).is_empty());
    q.in_variations = true;
    assert_eq!(values(&run_board(&mut db, &q).0), vec![(3, 2)]);
    // 1.e4 e5 2.Nf3 Nc6 is in games 1 and 3 (main line), value 5.
    let (filter, _) = run_board(&mut db, &exact(pos_after("e2e4 e7e5 g1f3 b8c6")));
    assert_eq!(values(&filter), vec![(1, 5), (3, 5)]);
    // The standard start matches the standard games, empty one included, not
    // the games with a FEN.
    let (filter, _) = run_board(&mut db, &exact(Position::new()));
    assert_eq!(numbers(&filter), vec![1, 2, 3, 5]);
}

// -------------------------------------------------------- properties

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// The filter equals the oracle written on the decoded games, with and
    /// without the shortcuts of the index, with one and with many threads.
    #[test]
    fn matches_the_oracle_with_and_without_shortcuts_on_any_thread_count(
        games in arb_games(10),
        which in any::<usize>(),
        at in any::<usize>(),
        kind_index in 0usize..4,
        flip in any::<bool>(),
        vars in any::<bool>(),
    ) {
        let target = position_in(&games[which % games.len()], at);
        let kind = [BoardMatch::Exact, BoardMatch::Pawns, BoardMatch::Files, BoardMatch::Material][kind_index];
        let expected: Vec<(u32, u8)> = games
            .iter()
            .enumerate()
            .filter_map(|(i, g)| oracle_board(g, kind, &target, flip, vars).map(|v| (i as u32 + 1, v)))
            .collect();
        let (_fx, mut db) = db_of(games);
        let mut q = BoardSearch { kind, in_variations: vars, ignore_colors: flip, ..BoardSearch::new(target) };
        let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap()
            .install(|| run_board(&mut db, &q).0);
        let many = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap()
            .install(|| run_board(&mut db, &q).0);
        q.shortcuts = false;
        let naive = run_board(&mut db, &q).0;
        prop_assert_eq!(values(&one), expected);
        prop_assert_eq!(one.as_bytes(), many.as_bytes());
        prop_assert_eq!(one.as_bytes(), naive.as_bytes());
    }
}

#[test]
fn per_target_index_discard_does_not_change_the_result_with_ignore_colors() {
    // Only the flipped target is reachable by the index in game 1 (the
    // normal one needs a white pawn that is not there); game 2 is the other
    // way round. The shortcuts, which now mark the discarded target as dead
    // inside the walk, must agree with the walk without them.
    let (_fx, mut db) = db_of(vec![
        game_fen("4k3/4p3/8/8/8/8/8/4K3 b - - 0 1", "e7e6"),
        game_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", "e2e3"),
        game("e2e4 e7e5"),
    ]);
    let mut q = exact(fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1"));
    q.ignore_colors = true;
    let (with, _) = run_board(&mut db, &q);
    q.shortcuts = false;
    let (without, _) = run_board(&mut db, &q);
    assert_eq!(values(&with), values(&without));
    assert_eq!(numbers(&with), vec![1, 2]);
}
