//! Contract tests of the search by material and patterns (spec
//! `docs/specs/m6-1-buscas-avancadas.md`, comportamento 12 a 24, regras 17 a
//! 24 de "Material e padrões"; D3215 to D3224).
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

use mychess_core::{Color, File, Piece, Position, Rank, Role};
use mychess_db::{
    BishopColors, CancelToken, Filter, MaterialSearch, NoProgress, Pattern, PieceRange,
    SearchError, SideRanges, material_presets, pattern_presets, search_material,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use support::{
    arb_games, db_of, game, game_fen, material_around, numbers, oracle_material, position_in,
    run_material, values,
};

const WP: Piece = Piece {
    color: Color::White,
    role: Role::Pawn,
};

fn range(min: u8, max: u8) -> PieceRange {
    PieceRange::new(min, max)
}

// -------------------------------------------- the basic searches

#[test]
fn any_material_matches_every_game_with_a_tested_ply() {
    let (_fx, mut db) = db_of(vec![game(""), game("e2e4"), game("e2e4 e7e5 g1f3")]);
    let (filter, _) = run_material(&mut db, &MaterialSearch::default());
    // The start position (ply 0) is not tested by default: the empty game has
    // no ply to test. The others match at ply 1, value 2.
    assert_eq!(values(&filter), vec![(2, 2), (3, 2)]);
}

#[test]
fn zero_material_matches_only_the_bare_kings() {
    let (_fx, mut db) = db_of(vec![
        game_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1", "e1e2"),
        game_fen("4k3/8/8/8/8/8/P7/4K3 w - - 0 1", "e1e2"),
    ]);
    let (filter, _) = run_material(&mut db, &MaterialSearch::zero());
    assert_eq!(values(&filter), vec![(1, 2)]);
}

#[test]
fn the_move_range_is_in_move_numbers_and_converted_to_plies() {
    // Black has 7 pawns from ply 3 on (1.e4 d5 2.exd5 Qxd5).
    let (_fx, mut db) = db_of(vec![game("e2e4 d7d5 e4d5 d8d5")]);
    let mut q = MaterialSearch::default();
    q.black.pawn = range(7, 7);
    let mut found = |moves: (u16, u16)| {
        q.moves = moves;
        values(&run_material(&mut db, &q).0)
    };
    assert_eq!(found((0, 999)), vec![(1, 4)]);
    // Move 2 starts at ply 3.
    assert_eq!(found((2, 999)), vec![(1, 4)]);
    // Move 3 starts at ply 5: the game has 4 plies.
    assert_eq!(found((3, 999)), vec![]);
    // Moves 1 to 1 cover plies 1 and 2 only.
    assert_eq!(found((1, 1)), vec![]);
    // Moves 2 to 2 cover plies 3 and 4.
    assert_eq!(found((2, 2)), vec![(1, 4)]);
}

#[test]
fn move_number_zero_also_tests_the_start_position() {
    let (_fx, mut db) = db_of(vec![game("e2e4")]);
    let mut q = MaterialSearch::default();
    q.white.pawn = range(8, 8);
    q.moves = (1, 999);
    // After 1.e4 White still has 8 pawns: ply 1, value 2.
    assert_eq!(values(&run_material(&mut db, &q).0), vec![(1, 2)]);
    q.moves = (0, 999);
    assert_eq!(values(&run_material(&mut db, &q).0), vec![(1, 1)]);
}

#[test]
fn match_for_at_least_needs_consecutive_plies_and_a_failure_restarts() {
    // The knight is on c3 at plies 1 and 2, back home at 3 and 4, on c3 at
    // plies 5, 6 and 7.
    let (_fx, mut db) = db_of(vec![game("b1c3 a7a6 c3b1 a6a5 b1c3 a5a4 d2d3")]);
    let mut q = MaterialSearch::default();
    q.patterns = vec![Pattern::on_square(
        Piece {
            color: Color::White,
            role: Role::Knight,
        },
        File::C,
        Rank::Third,
        true,
    )];
    let mut found = |plies: u16| {
        q.min_plies = plies;
        values(&run_material(&mut db, &q).0)
    };
    assert_eq!(found(1), vec![(1, 2)]);
    assert_eq!(found(2), vec![(1, 2)]);
    assert_eq!(found(3), vec![(1, 6)]);
    assert_eq!(found(4), vec![]);
    // 0 counts as 1.
    assert_eq!(found(0), vec![(1, 2)]);
}

#[test]
fn end_position_only_ignores_the_range_and_a_game_without_moves_never_matches() {
    let (_fx, mut db) = db_of(vec![
        game("e2e4 d7d5 e4d5"),
        game("e2e4 d7d5 e4d5 d8d5"),
        game(""),
        game("e2e4 d7d5"),
    ]);
    let mut q = MaterialSearch::default();
    q.black.pawn = range(7, 7);
    q.end_only = true;
    // Game 2 has 7 black pawns at the end too; game 4 has 8; game 3 has no
    // moves.
    assert_eq!(values(&run_material(&mut db, &q).0), vec![(1, 4), (2, 5)]);
    // The range does not matter in this mode.
    q.moves = (500, 600);
    q.min_plies = 50;
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1, 2]);
    // Even "any material" does not match the game without moves.
    let any = MaterialSearch {
        end_only: true,
        ..MaterialSearch::default()
    };
    assert_eq!(numbers(&run_material(&mut db, &any).0), vec![1, 2, 4]);
}

#[test]
fn a_first_ply_above_254_is_limited_to_255_and_does_not_wrap() {
    let mut moves = String::new();
    for _ in 0..70 {
        moves.push_str("g1f3 g8f6 f3g1 f6g8 ");
    }
    moves.push_str("e2e4 d7d5 e4d5");
    let (_fx, mut db) = db_of(vec![game(&moves)]);
    let mut q = MaterialSearch::default();
    q.black.pawn = range(7, 7);
    assert_eq!(values(&run_material(&mut db, &q).0), vec![(1, 255)]);
}

// ------------------------------------------------------------ bishops

#[test]
fn bishop_colours_filter_the_games_with_one_bishop_each() {
    let same = game_fen("4k3/8/8/8/8/8/8/2B1K1b1 w - - 0 1", "e1d2");
    let opposite = game_fen("4k3/8/8/8/8/8/8/2B1K2b w - - 0 1", "e1d2");
    let (_fx, mut db) = db_of(vec![same, opposite]);
    let mut q = MaterialSearch::default();
    let mut found = |b| {
        q.bishops = b;
        numbers(&run_material(&mut db, &q).0)
    };
    assert_eq!(found(BishopColors::Either), vec![1, 2]);
    assert_eq!(found(BishopColors::Same), vec![1]);
    assert_eq!(found(BishopColors::Opposite), vec![2]);
}

#[test]
fn a_failed_bishop_rule_fails_only_that_ply() {
    // Opposite-coloured bishops at ply 0 and 1, which fails "same colours"
    // at those plies; at ply 2 the white bishop is gone, the rule does not
    // apply and the position matches. Scid would give up the whole game at
    // ply 1 (D3220).
    let g = game_fen(
        "2r3bk/P7/8/8/8/8/8/2B1K3 w - - 0 1",
        "e1d2 c8c1 d2c1 h8h7 a7a8b",
    );
    let (_fx, mut db) = db_of(vec![g]);
    let mut q = MaterialSearch::default();
    q.bishops = BishopColors::Same;
    assert_eq!(values(&run_material(&mut db, &q).0), vec![(1, 3)]);
    // The promotion to a bishop on a light square makes the colours equal
    // again at ply 5; asking for 3 consecutive plies skips the run of plies
    // 2 to 4 only if one of them fails, which none does.
    q.bishops = BishopColors::Opposite;
    assert_eq!(values(&run_material(&mut db, &q).0), vec![(1, 2)]);
}

// --------------------------------------------------------- difference

#[test]
fn the_material_difference_uses_the_scid_values_and_inclusive_limits() {
    let cases = [
        ("4k3/8/8/8/8/8/P7/4K3 w - - 0 1", 1),
        ("4k3/8/8/8/8/8/N7/4K3 w - - 0 1", 3),
        ("4k3/8/8/8/8/8/B7/4K3 w - - 0 1", 3),
        ("4k3/8/8/8/8/8/R7/4K3 w - - 0 1", 5),
        ("4k3/8/8/8/8/8/Q7/4K3 w - - 0 1", 9),
        ("4k3/p7/8/8/8/8/Q7/4K3 w - - 0 1", 8),
    ];
    for (fen, value) in cases {
        let (_fx, mut db) = db_of(vec![game_fen(fen, "e1d1")]);
        let mut q = MaterialSearch::default();
        q.diff = Some((value, value));
        assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1], "{fen}");
        q.diff = Some((value + 1, value + 5));
        assert!(numbers(&run_material(&mut db, &q).0).is_empty(), "{fen}");
        q.diff = Some((value - 5, value - 1));
        assert!(numbers(&run_material(&mut db, &q).0).is_empty(), "{fen}");
    }
}

#[test]
fn a_reversed_difference_range_is_swapped() {
    let (_fx, mut db) = db_of(vec![game_fen("4k3/8/8/8/8/8/N7/4K3 w - - 0 1", "e1d1")]);
    let mut q = MaterialSearch::default();
    q.diff = Some((4, 2));
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
}

#[test]
fn no_difference_range_means_no_restriction() {
    // 41 points up: Scid's default of -40 to +40 would exclude it.
    let (_fx, mut db) = db_of(vec![game_fen("4k3/8/8/8/8/8/8/QQQQKQ2 w - - 0 1", "e1d2")]);
    let mut q = MaterialSearch::default();
    q.white.queen = range(0, 5);
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
    q.diff = Some((-40, 40));
    assert!(numbers(&run_material(&mut db, &q).0).is_empty());
}

// ------------------------------------------------------------ patterns

fn piece(code: &str) -> Piece {
    let color = if code.starts_with('w') {
        Color::White
    } else {
        Color::Black
    };
    Piece {
        color,
        role: Role::from_char(code.chars().nth(1).unwrap()).unwrap(),
    }
}

#[test]
fn patterns_test_a_square_a_file_or_a_rank_with_yes_and_no() {
    let (_fx, mut db) = db_of(vec![game_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1", "e1d1")]);
    let mut case = |p: Pattern| {
        let q = MaterialSearch {
            patterns: vec![p],
            moves: (0, 999),
            ..MaterialSearch::default()
        };
        !numbers(&run_material(&mut db, &q).0).is_empty()
    };
    let rook = piece("wr");
    assert!(case(Pattern::on_square(rook, File::A, Rank::First, true)));
    assert!(!case(Pattern::on_square(rook, File::A, Rank::First, false)));
    assert!(!case(Pattern::on_square(rook, File::B, Rank::First, true)));
    assert!(case(Pattern::on_square(rook, File::B, Rank::First, false)));
    assert!(case(Pattern::on_file(rook, File::A, true)));
    assert!(!case(Pattern::on_file(rook, File::B, true)));
    let on_rank = |rank, present| Pattern {
        piece: Some(rook),
        file: None,
        rank: Some(rank),
        present,
    };
    assert!(case(on_rank(Rank::First, true)));
    assert!(!case(on_rank(Rank::Second, true)));
    assert!(case(on_rank(Rank::Second, false)));
}

#[test]
fn a_pattern_without_a_piece_is_ignored_and_one_without_a_square_is_an_error() {
    let (_fx, mut db) = db_of(vec![game("e2e4")]);
    let mut q = MaterialSearch::default();
    q.patterns = vec![Pattern::default()];
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
    q.patterns = vec![Pattern {
        piece: Some(WP),
        ..Pattern::default()
    }];
    let mut filter = Filter::new(1);
    let before = filter.clone();
    let err = search_material(
        &mut db,
        &q,
        &mut filter,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(matches!(err, SearchError::InvalidPattern { .. }), "{err:?}");
    assert_eq!(filter, before);
    q.patterns = vec![Pattern::on_file(WP, File::A, true); 33];
    assert!(matches!(
        search_material(
            &mut db,
            &q,
            &mut filter,
            &mut NoProgress,
            &CancelToken::new()
        ),
        Err(SearchError::InvalidPattern { .. })
    ));
}

#[test]
fn pattern_text_is_read_like_the_scid_window_writes_it() {
    let p = Pattern::from_scid("wp", "d", "?", "Yes").unwrap();
    assert_eq!(p, Pattern::on_file(WP, File::D, true));
    let p = Pattern::from_scid("bk", "g", "8", "No").unwrap();
    assert_eq!(p.rank, Some(Rank::Eighth));
    assert!(!p.present);
    assert_eq!(
        Pattern::from_scid("?", "?", "?", "Yes").unwrap(),
        Pattern::default()
    );
    for bad in [
        ("xp", "a", "1", "Yes"),
        ("wx", "a", "1", "Yes"),
        ("wp", "i", "1", "Yes"),
        ("wp", "a", "9", "Yes"),
        ("wp", "a", "1", "Maybe"),
        ("w", "a", "1", "Yes"),
    ] {
        assert!(
            matches!(
                Pattern::from_scid(bad.0, bad.1, bad.2, bad.3),
                Err(SearchError::InvalidPattern { .. })
            ),
            "{bad:?}"
        );
    }
}

// ------------------------------------------------------- ignore colours

#[test]
fn ignoring_colours_swaps_counts_patterns_and_the_difference() {
    // Only Black has a queen: Black is better by 9.
    let (_fx, mut db) = db_of(vec![game_fen("3qk3/8/8/8/8/8/8/4K3 w - - 0 1", "e1e2")]);
    let mut q = MaterialSearch::zero();
    q.white.queen = range(1, 1);
    q.diff = Some((1, 40));
    assert!(numbers(&run_material(&mut db, &q).0).is_empty());
    q.ignore_colors = true;
    // The second pass asks for "Black with a queen and better by 1 to 40",
    // that is -40 to -1 seen from White: the game has -9.
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
}

#[test]
fn ignoring_colours_flips_the_ranks_of_the_patterns() {
    let (_fx, mut db) = db_of(vec![game_fen("4k3/p7/8/8/8/8/8/4K3 w - - 0 1", "e1e2")]);
    let mut q = MaterialSearch::default();
    q.patterns = vec![Pattern::on_square(WP, File::A, Rank::Second, true)];
    assert!(numbers(&run_material(&mut db, &q).0).is_empty());
    q.ignore_colors = true;
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
}

#[test]
fn the_normal_pass_wins_when_both_would_match() {
    // Both passes match; the value is the one of the first.
    let (_fx, mut db) = db_of(vec![game_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1", "e1e2 e8e7")]);
    let mut q = MaterialSearch::zero();
    q.ignore_colors = true;
    assert_eq!(values(&run_material(&mut db, &q).0), vec![(1, 2)]);
}

// ------------------------------------------------------- normalisation

#[test]
fn normalising_raises_maxima_and_bounds_the_minor_range_for_both_colours() {
    let mut q = MaterialSearch::default();
    q.white.queen = range(2, 1);
    q.black.queen = range(2, 1);
    q.white.bishop = range(1, 2);
    q.white.knight = range(1, 1);
    q.white.minor = range(0, 9);
    q.black.bishop = range(1, 2);
    q.black.knight = range(1, 1);
    q.black.minor = range(0, 9);
    let n = q.normalized();
    assert_eq!(n.white.queen, range(2, 2));
    assert_eq!(n.white.minor, range(2, 3));
    // Scid used max[BB] for Black's minimum (a typo); both colours alike now.
    assert_eq!(n.white, n.black);
    assert_eq!(n.normalized(), n);
}

#[test]
fn from_position_counts_everything_but_the_kings() {
    let pos = Position::new();
    let q = MaterialSearch::from_position(&pos);
    assert_eq!(q.white.pawn, range(8, 8));
    assert_eq!(q.white.queen, range(1, 1));
    assert_eq!(q.black.rook, range(2, 2));
    assert_eq!(q.white.minor, range(4, 4));
    assert_eq!(q.diff, None);
}

// ------------------------------------------------------------ presets

fn run_ending(key: &str, fens: &[&str]) -> Vec<u32> {
    let preset = material_presets()
        .into_iter()
        .find(|p| p.key == key)
        .unwrap();
    let mut q = MaterialSearch::default();
    preset.apply(&mut q);
    q.moves = (0, 999);
    let games = fens.iter().map(|f| game_fen(f, "")).collect();
    let (_fx, mut db) = db_of(games);
    numbers(&run_material(&mut db, &q).0)
}

#[test]
fn common_endings_find_the_endings_they_name() {
    assert_eq!(
        run_ending(
            "ending-rooks",
            &[
                "r3k3/pp3ppp/8/8/8/8/PP3PPP/R3K3 w - - 0 1",
                "r3k3/pp3ppp/8/8/8/8/PP3PPP/RN2K3 w - - 0 1",
                "4k3/8/8/8/8/8/8/R3K3 w - - 0 1",
            ]
        ),
        vec![1]
    );
    assert_eq!(
        run_ending(
            "ending-queen-pawn-vs-queen",
            &[
                "3qk3/8/8/8/8/8/4P3/3QK3 w - - 0 1",
                "3qk3/4p3/8/8/8/8/4P3/3QK3 w - - 0 1",
                "3qk3/8/8/8/8/8/8/3QK3 w - - 0 1",
            ]
        ),
        vec![1]
    );
    assert_eq!(
        run_ending(
            "ending-bishop-vs-knight",
            &[
                "4k3/pp3ppp/8/8/8/8/PP3PPP/2B1K3 w - - 0 1",
                "2n1k3/pp3ppp/8/8/8/8/PP3PPP/2B1K3 w - - 0 1",
            ]
        ),
        vec![2]
    );
}

#[test]
fn the_isolated_queens_pawn_pattern_finds_an_isolated_d_pawn() {
    let preset = pattern_presets()
        .into_iter()
        .find(|p| p.key == "pattern-white-iqp")
        .unwrap();
    let mut q = MaterialSearch::default();
    preset.apply(&mut q);
    let (_fx, mut db) = db_of(vec![
        game_fen("4k3/pp3ppp/8/8/3P4/8/PP3PPP/4K3 w - - 0 1", "e1d1"),
        game_fen("4k3/pp3ppp/8/8/3P4/8/PPP2PPP/4K3 w - - 0 1", "e1d1"),
        game_fen("4k3/pp3ppp/8/8/8/8/PP3PPP/4K3 w - - 0 1", "e1d1"),
    ]);
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
}

// -------------------------------------------------------- the index

#[test]
fn a_promotion_can_bring_back_a_piece_that_was_missing() {
    // A second queen only exists after the pawn promotes.
    let g = game_fen("6k1/P7/8/8/8/8/8/K6Q w - - 0 1", "a7a8q g8f7");
    let (_fx, mut db) = db_of(vec![g]);
    let mut q = MaterialSearch::default();
    q.white.queen = range(2, 2);
    assert_eq!(values(&run_material(&mut db, &q).0), vec![(1, 2)]);
}

#[test]
fn the_shortcuts_never_change_the_answer_for_searches_with_few_pieces() {
    let (_fx, mut db) = db_of(vec![
        game("e2e4 d7d5 e4d5 d8d5"),
        game("d2d4 d7d5"),
        game_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", "e2e4"),
    ]);
    let mut searches = Vec::new();
    let mut a = MaterialSearch::zero();
    a.white.pawn = range(0, 8);
    a.black.pawn = range(0, 8);
    searches.push(a);
    let mut b = MaterialSearch::default();
    b.patterns = vec![Pattern::on_file(WP, File::E, false)];
    searches.push(b.clone());
    b.ignore_colors = true;
    searches.push(b);
    let mut c = MaterialSearch::default();
    c.white.queen = range(0, 0);
    c.black.queen = range(0, 0);
    c.end_only = true;
    searches.push(c);
    for mut q in searches {
        let fast = run_material(&mut db, &q).0;
        q.shortcuts = false;
        let slow = run_material(&mut db, &q).0;
        assert_eq!(fast.as_bytes(), slow.as_bytes(), "{q:?}");
    }
}

#[test]
fn a_spoiled_index_matters_only_to_the_shortcuts_and_never_to_start_games() {
    // The final material of the index claims 15 pawns and 3 pieces of every
    // kind: a shortcut that trusted it would discard the game (D3207).
    let spoil = |fx: &base::Fixture, n: u32| {
        base::patch_entry(fx, n, |e| e[34..37].copy_from_slice(&[0xFF, 0xFF, 0xFF]));
    };
    let mut q = MaterialSearch::zero();
    q.white.pawn = range(0, 1);
    q.end_only = true;
    let standard = || game("e2e4");
    let start = || game_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", "e2e4");
    // A standard game: the shortcut uses the (wrong) index.
    let (fx, db) = db_of(vec![start()]);
    drop(db);
    spoil(&fx, 1);
    let mut db = fx.open();
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
    let (fx, db) = db_of(vec![standard()]);
    drop(db);
    spoil(&fx, 1);
    let mut db = fx.open();
    let mut q2 = MaterialSearch::default();
    q2.white.pawn = range(8, 8);
    q2.white.queen = range(1, 1);
    q2.white.rook = range(2, 2);
    q2.black = SideRanges::ANY;
    q2.end_only = true;
    // Only the queen's maximum is low enough for the shortcut (max <= 3 for
    // all pieces): the spoiled index discards the game, the naive scan keeps
    // it.
    assert!(numbers(&run_material(&mut db, &q2).0).is_empty());
    q2.shortcuts = false;
    assert_eq!(numbers(&run_material(&mut db, &q2).0), vec![1]);
}

// -------------------------------------------------------- properties

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// The filter equals the oracle on the decoded games, with and without
    /// the shortcuts, with one and with many threads.
    #[test]
    fn matches_the_oracle_with_and_without_shortcuts_on_any_thread_count(
        games in arb_games(10),
        which in any::<usize>(),
        at in any::<usize>(),
        seed in prop::collection::vec(any::<u8>(), 40),
    ) {
        let around = position_in(&games[which % games.len()], at);
        let mut q = material_around(&around, &seed);
        let expected: Vec<(u32, u8)> = games
            .iter()
            .enumerate()
            .filter_map(|(i, g)| oracle_material(g, &q).map(|v| (i as u32 + 1, v)))
            .collect();
        let (_fx, mut db) = db_of(games);
        let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap()
            .install(|| run_material(&mut db, &q).0);
        let many = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap()
            .install(|| run_material(&mut db, &q).0);
        q.shortcuts = false;
        let naive = run_material(&mut db, &q).0;
        prop_assert_eq!(values(&one), expected);
        prop_assert_eq!(one.as_bytes(), many.as_bytes());
        prop_assert_eq!(one.as_bytes(), naive.as_bytes());
    }
}

#[test]
fn a_reversed_or_oversized_move_range_is_an_error_and_changes_nothing() {
    let (_fx, mut db) = db_of(vec![game("e2e4 d7d5")]);
    for moves in [(5, 3), (1, 1000)] {
        let mut q = MaterialSearch::default();
        q.moves = moves;
        assert_eq!(
            q.validate(),
            Err(SearchError::InvalidMoveRange {
                min: moves.0,
                max: moves.1
            })
        );
        let mut filter = Filter::new(1);
        let result = search_material(
            &mut db,
            &q,
            &mut filter,
            &mut NoProgress,
            &CancelToken::new(),
        );
        assert!(matches!(result, Err(SearchError::InvalidMoveRange { .. })));
        assert_eq!(filter.count(), 1);
    }
    let mut ok = MaterialSearch::default();
    ok.moves = (0, 999);
    assert_eq!(ok.validate(), Ok(()));
}
