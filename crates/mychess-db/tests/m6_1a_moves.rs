//! Contract tests of the search by a sequence of moves (spec
//! `docs/specs/m6-1-buscas-avancadas.md`, comportamento 25 a 29, regras 25 a
//! 29 de "Lances"; D3225 to D3230).
#![allow(
    clippy::assert_is_empty,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    unused_qualifications,
    clippy::field_reassign_with_default,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m6_1a_support.rs"]
mod support;

use mychess_db::{CheckTest, FilterOp, MovesSearch, SearchError, SideFilter};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use support::{
    arb_games, db_of, game, game_fen, mainline_san_list, numbers, oracle_moves, run_moves, values,
};

fn q(text: &str) -> MovesSearch {
    MovesSearch::parse(text, SideFilter::Both, CheckTest::No, FilterOp::Reset).unwrap()
}

fn with(text: &str, side: SideFilter, test: CheckTest) -> MovesSearch {
    MovesSearch::parse(text, side, test, FilterOp::Reset).unwrap()
}

#[test]
fn a_sequence_matches_consecutive_moves_and_the_value_is_the_first_ply_plus_two() {
    let (_fx, mut db) = db_of(vec![
        game("e2e4 e7e5 g1f3"),
        game("e2e4 e7e5 b1c3"),
        game("d2d4 e7e5 g1f3"),
        game("b1c3 d7d6 e2e4 e7e5 g1f3"),
    ]);
    assert_eq!(
        values(&run_moves(&mut db, &q("e4 e5 Nf3")).0),
        vec![(1, 2), (4, 4)]
    );
    // The wildcard is any move.
    assert_eq!(
        values(&run_moves(&mut db, &q("e4 ? Nf3")).0),
        vec![(1, 2), (4, 4)]
    );
    assert_eq!(numbers(&run_moves(&mut db, &q("e5 Nf3")).0), vec![1, 3, 4]);
}

#[test]
fn the_side_restricts_only_the_first_move() {
    let (_fx, mut db) = db_of(vec![game("g1f3 g8f6"), game("e2e4 g8f6 b1c3")]);
    // Nf3 for Black: no game has a black Nf3.
    assert!(
        numbers(&run_moves(&mut db, &with("Nf3", SideFilter::Black, CheckTest::No)).0).is_empty()
    );
    assert_eq!(
        numbers(&run_moves(&mut db, &with("Nf3", SideFilter::White, CheckTest::No)).0),
        vec![1]
    );
    // Nf6 is a Black move; the following wildcard belongs to White.
    let black_then_any = with("Nf6 ?", SideFilter::Black, CheckTest::No);
    assert_eq!(values(&run_moves(&mut db, &black_then_any).0), vec![(2, 3)]);
}

#[test]
fn the_wildcard_needs_a_real_move_and_does_not_match_the_end_of_the_game() {
    let (_fx, mut db) = db_of(vec![game("e2e4"), game("e2e4 e7e5")]);
    assert_eq!(numbers(&run_moves(&mut db, &q("e4 ?")).0), vec![2]);
    assert_eq!(numbers(&run_moves(&mut db, &q("e4")).0), vec![1, 2]);
}

#[test]
fn the_first_word_cannot_be_a_wildcard_and_an_empty_list_is_refused() {
    let first = MovesSearch::parse("? e5", SideFilter::Both, CheckTest::No, FilterOp::Reset);
    assert_eq!(first.unwrap_err(), SearchError::FirstMoveIsWildcard);
    let empty = MovesSearch::parse("   ", SideFilter::Both, CheckTest::No, FilterOp::Reset);
    assert_eq!(empty.unwrap_err(), SearchError::EmptyMoveList);
}

#[test]
fn the_check_test_decides_whether_plus_and_hash_are_in_the_text() {
    let check = game_fen("4k3/8/8/8/8/8/8/3QK3 w - - 0 1", "d1d8");
    let mate = game_fen("6k1/5ppp/8/8/8/8/8/K3R3 w - - 0 1", "e1e8");
    let (_fx, mut db) = db_of(vec![check, mate]);
    let mut found = |text: &str, test| {
        let search = with(text, SideFilter::Both, test);
        (search.check_test, numbers(&run_moves(&mut db, &search).0))
    };
    // No marker: Qd8 matches Qd8+ and Re8 matches Re8#.
    assert_eq!(found("Qd8", CheckTest::No), (CheckTest::No, vec![1]));
    assert_eq!(found("Re8", CheckTest::No), (CheckTest::No, vec![2]));
    // With the check test the generated text carries the marker: no match.
    assert_eq!(found("Qd8", CheckTest::Check), (CheckTest::Check, vec![]));
    // A marker typed with the test off raises it (instead of never matching).
    assert_eq!(found("Qd8+", CheckTest::No), (CheckTest::Check, vec![1]));
    assert_eq!(found("Re8#", CheckTest::No), (CheckTest::Mate, vec![2]));
    // Check marks mate with + too; Mate distinguishes.
    assert_eq!(found("Re8+", CheckTest::Check), (CheckTest::Check, vec![2]));
    assert_eq!(found("Re8+", CheckTest::Mate), (CheckTest::Mate, vec![]));
}

#[test]
fn words_are_read_like_the_san_the_core_writes() {
    let castle = game_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1g1 e8c8");
    let promotion = game_fen("k7/4P3/8/8/8/8/8/K7 w - - 0 1", "e7e8q");
    let capture = game("e2e4 d7d5 e4d5");
    let (_fx, mut db) = db_of(vec![castle, promotion, capture]);
    let mut found = |text: &str| numbers(&run_moves(&mut db, &q(text)).0);
    assert_eq!(found("0-0 0-0-0"), vec![1]);
    assert_eq!(found("O-O O-O-O"), vec![1]);
    assert_eq!(found("e8=Q"), vec![2]);
    assert_eq!(found("e8Q"), vec![2]);
    assert_eq!(found("exd5!?"), vec![3]);
    assert_eq!(found("1.e4 1...d5 2.exd5"), vec![3]);
    // The capture mark is mandatory, as in the SAN that is generated.
    assert!(matches!(
        MovesSearch::parse("ed5", SideFilter::Both, CheckTest::No, FilterOp::Reset),
        Err(SearchError::BadMoveToken { .. })
    ));
    assert!(matches!(
        MovesSearch::parse("Ng1-f3", SideFilter::Both, CheckTest::No, FilterOp::Reset),
        Err(SearchError::BadMoveToken { .. })
    ));
}

#[test]
fn disambiguation_has_to_be_the_one_that_is_generated() {
    let by_file = game_fen("4k3/8/8/8/8/8/4K3/R6R w - - 0 1", "a1d1");
    let by_rank = game_fen("4k3/8/8/R7/8/8/8/R3K3 w - - 0 1", "a1a3");
    let (_fx, mut db) = db_of(vec![by_file, by_rank]);
    let mut found = |text: &str| numbers(&run_moves(&mut db, &q(text)).0);
    assert_eq!(found("Rad1"), vec![1]);
    assert_eq!(found("Rd1"), Vec::<u32>::new());
    assert_eq!(found("R1d1"), Vec::<u32>::new());
    assert_eq!(found("R1a3"), vec![2]);
    assert_eq!(found("Raa3"), Vec::<u32>::new());
}

#[test]
fn the_first_move_can_be_at_any_ply_up_to_254_and_not_beyond() {
    // 252 plies of knight shuffling, then e4 at ply 254 (the last start).
    let shuffle = "g1f3 g8f6 f3g1 f6g8 ".repeat(63);
    let at_254 = game(&format!("{shuffle}g1f3 g8f6 e2e4"));
    // ... and e5 at ply 255: too late.
    let at_255 = game(&format!("{shuffle}g1f3 g8f6 f3g1 e7e5"));
    let (_fx, mut db) = db_of(vec![at_254, at_255]);
    let hit = run_moves(&mut db, &q("e4")).0;
    assert_eq!(values(&hit), vec![(1, 255)]);
    assert!(numbers(&run_moves(&mut db, &q("e5")).0).is_empty());
    // The range is an option.
    let mut wide = q("e5");
    wide.start_plies = (0, 400);
    assert_eq!(values(&run_moves(&mut db, &wide).0), vec![(2, 255)]);
    // The sequence itself may go past the limit.
    let mut long = q("e4 ?");
    long.start_plies = (0, 254);
    let (_fx2, mut db2) = db_of(vec![game(&format!("{shuffle}g1f3 g8f6 e2e4 e7e5"))]);
    assert_eq!(numbers(&run_moves(&mut db2, &long).0), vec![1]);
}

#[test]
fn the_start_range_limits_the_first_move() {
    let (_fx, mut db) = db_of(vec![game("e2e4 e7e5 g1f3 b8c6 f1c4")]);
    let mut search = q("Nf3");
    search.start_plies = (3, 10);
    assert!(numbers(&run_moves(&mut db, &search).0).is_empty());
    search.start_plies = (2, 2);
    assert_eq!(values(&run_moves(&mut db, &search).0), vec![(1, 4)]);
}

#[test]
fn variations_are_searched_only_when_asked() {
    let (_fx, mut db) = db_of(vec![
        game("e2e4 e7e5 (c7c5 g1f3 d7d6) g1f3"),
        game("e2e4 e7e5 g1f3"),
    ]);
    let mut search = q("c5 Nf3 d6");
    assert!(numbers(&run_moves(&mut db, &search).0).is_empty());
    search.in_variations = true;
    // The first move c5 is inside the variation that starts at ply 1.
    assert_eq!(values(&run_moves(&mut db, &search).0), vec![(1, 3)]);
    // A sequence that starts in the main line and continues in the variation.
    let mut cross = q("e4 c5 Nf3");
    cross.in_variations = true;
    assert_eq!(values(&run_moves(&mut db, &cross).0), vec![(1, 2)]);
}

#[test]
fn and_or_follow_the_filter_operation_and_unplayable_games_are_listed() {
    let (_fx, mut db) = db_of(vec![game("e2e4"), game("d2d4"), game("e2e4 e7e5")]);
    let mut filter = mychess_db::Filter::new(3);
    filter.fill(0);
    filter.set(2, 8);
    filter.set(3, 8);
    let mut search = q("e4");
    search.op = FilterOp::And;
    let report = mychess_db::search_moves(
        &mut db,
        &search,
        &mut filter,
        &mut mychess_db::NoProgress,
        &mychess_db::CancelToken::new(),
    )
    .unwrap();
    assert_eq!(values(&filter), vec![(3, 2)]);
    assert_eq!(report.start_count, 2);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// The pre-test of the shapes (D3230) gives the same answer as comparing
    /// the SAN of every move, with one or many threads, and both equal the
    /// oracle written on the decoded games.
    #[test]
    fn matches_the_oracle_with_and_without_the_pre_test(
        games in arb_games(10),
        which in any::<usize>(),
        start in any::<usize>(),
        len in 1usize..4,
        wild in prop::collection::vec(any::<bool>(), 4),
        test_index in 0usize..3,
        side_index in 0usize..3,
        vars in any::<bool>(),
        spoil in any::<bool>(),
    ) {
        let test = [CheckTest::No, CheckTest::Check, CheckTest::Mate][test_index];
        let side = [SideFilter::Both, SideFilter::White, SideFilter::Black][side_index];
        let source = &games[which % games.len()];
        let sans = mainline_san_list(source, test);
        prop_assume!(!sans.is_empty());
        let from = start % sans.len();
        let mut words: Vec<String> = sans[from..].iter().take(len).cloned().collect();
        for (i, w) in words.iter_mut().enumerate().skip(1) {
            if wild[i] {
                "?".clone_into(w);
            }
        }
        if spoil {
            // A word that matches nothing.
            words.push("Qh5xg7".to_owned());
        }
        let Ok(mut q) = MovesSearch::parse(&words.join(" "), side, test, FilterOp::Reset) else {
            return Ok(());
        };
        q.in_variations = vars;
        let expected: Vec<(Option<u8>, bool)> = games.iter().map(|g| oracle_moves(g, &q)).collect();
        let (_fx, mut db) = db_of(games);
        let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap()
            .install(|| run_moves(&mut db, &q).0);
        let many = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap()
            .install(|| run_moves(&mut db, &q).0);
        q.fast_path = false;
        let naive = run_moves(&mut db, &q).0;
        prop_assert_eq!(one.as_bytes(), many.as_bytes());
        prop_assert_eq!(one.as_bytes(), naive.as_bytes());
        for (i, (main, any)) in expected.iter().enumerate() {
            let got = one.get(i as u32 + 1);
            match (main, vars && *any) {
                (Some(v), _) => prop_assert_eq!(got, *v, "game {}", i + 1),
                (None, true) => prop_assert!(got > 0, "game {} should match in a variation", i + 1),
                (None, false) => prop_assert_eq!(got, 0, "game {}", i + 1),
            }
        }
    }
}
