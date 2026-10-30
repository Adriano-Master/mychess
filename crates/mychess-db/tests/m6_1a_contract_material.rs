//! Contract tests of the search by material and patterns, written from the
//! spec `docs/specs/m6-1-buscas-avancadas.md` (comportamento 12 a 24, regras
//! 17 to 24 of "Material e padrões", D3215 to D3224), over the PGN fixture
//! `fixtures/m6_1a_contract_material.pgn` and games built in code.
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
    clippy::similar_names,
    dead_code
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m6_1a_contract_support.rs"]
mod cs;
#[path = "common/m6_1a_support.rs"]
mod support;

use mychess_core::{Color, File, Piece, Position, Rank, Role};
use mychess_db::{
    BishopColors, CancelToken, Filter, FilterOp, GameSource, MaterialSearch, NoProgress, Pattern,
    PieceRange, SearchError, SideRanges, material_presets, pattern_presets, search_material,
};
use pretty_assertions::assert_eq;
use support::{db_of, game, game_fen, numbers, pgn_fixture, run_material, values};

fn fixture() -> mychess_db::PgnDatabase {
    let db = pgn_fixture("m6_1a_contract_material.pgn");
    assert_eq!(db.len(), 12, "fixture must load");
    db
}

fn piece(color: Color, role: Role) -> Piece {
    Piece { color, role }
}

const WN: Piece = Piece {
    color: Color::White,
    role: Role::Knight,
};
const WQ: Piece = Piece {
    color: Color::White,
    role: Role::Queen,
};
const WP: Piece = Piece {
    color: Color::White,
    role: Role::Pawn,
};

fn r(min: u8, max: u8) -> PieceRange {
    PieceRange::new(min, max)
}

fn with_pattern(p: Pattern) -> MaterialSearch {
    MaterialSearch {
        patterns: vec![p],
        ..MaterialSearch::default()
    }
}

// ----------------------------------------------------- the default search

#[test]
fn any_material_matches_each_game_with_a_tested_ply_and_the_value_is_two() {
    let mut db = fixture();
    let (f, report) = run_material(&mut db, &MaterialSearch::default());
    // The empty game (8) has no ply 1; ply 0 is not tested by default.
    assert_eq!(numbers(&f), vec![1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12]);
    assert!(f.iter_games().all(|n| f.get(n) == 2));
    assert_eq!(report.start_count, 12);
    // A first move number of 0 tests the start position: now the empty game
    // matches too, with value 1.
    let q = MaterialSearch {
        moves: (0, 999),
        ..MaterialSearch::default()
    };
    let (f, _) = run_material(&mut db, &q);
    assert_eq!(f.count(), 12);
    assert!(f.iter_games().all(|n| f.get(n) == 1));
}

#[test]
fn zero_matches_only_two_bare_kings() {
    let (_fx, mut db) = db_of(vec![
        game_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1", "e1d1 e8d8"),
        game_fen("4k3/8/8/8/8/8/P7/4K3 w - - 0 1", "e1d1 e8d8"),
        // The pawn promotes into a rook and is captured: kings only at the end.
        game_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a7a8r e8d7 a8a1 d7e6"),
    ]);
    let (f, _) = run_material(&mut db, &MaterialSearch::zero());
    assert_eq!(numbers(&f), vec![1]);
    // End position only: kings only is never true in the third game.
    let q = MaterialSearch {
        end_only: true,
        ..MaterialSearch::zero()
    };
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
}

// ------------------------------------------------------------- ranges

#[test]
fn per_piece_ranges_are_inclusive_and_minors_add_bishops_and_knights() {
    let (_fx, mut db) = db_of(vec![
        game_fen("4k3/8/8/8/8/8/8/1NB1K3 w - - 0 1", "e1f1"),
        game_fen("4k3/8/8/8/8/8/8/1NBNK3 w - - 0 1", "e1f1"),
        game_fen("4k3/8/8/8/8/8/8/1N2K3 w - - 0 1", "e1f1"),
    ]);
    let mut q = MaterialSearch::default();
    q.white.knight = r(1, 2);
    q.white.bishop = r(0, 2);
    q.white.minor = r(2, 2);
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
    q.white.minor = r(3, 4);
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![2]);
    // Raising the minimum of the minors above the sum of the maxima of
    // bishops and knights cannot match anything.
    q.white.minor = r(1, 1);
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![3]);
}

#[test]
fn consistency_rules_are_the_same_for_both_colours() {
    let mut q = MaterialSearch::default();
    // max below min is raised; minor min follows the sum of the minima;
    // minor max is cut to the sum of the maxima.
    for side in [&mut q.white, &mut q.black] {
        side.queen = r(3, 1);
        side.bishop = r(1, 1);
        side.knight = r(1, 1);
        side.minor = r(0, 9);
    }
    let n = q.normalized();
    assert_eq!(n.white, n.black);
    assert_eq!(n.white.queen, r(3, 3));
    assert_eq!(n.white.minor, r(2, 2));
    // The Scid bug (black): a minor minimum above the sum must not be lowered.
    let mut q = MaterialSearch::default();
    q.black.bishop = r(0, 2);
    q.black.knight = r(0, 2);
    q.black.minor = r(3, 4);
    q.white = q.black;
    let n = q.normalized();
    assert_eq!(n.black.minor, r(3, 4));
    assert_eq!(n.white.minor, r(3, 4));
}

#[test]
fn the_user_may_cap_the_minors_below_the_sum_of_the_maxima() {
    let (_fx, mut db) = db_of(vec![
        game_fen("4k3/8/8/8/8/8/8/1NB1K3 w - - 0 1", "e1f1"),
        game_fen("4k3/8/8/8/8/8/8/2B1K3 w - - 0 1", "e1f1"),
    ]);
    let mut q = MaterialSearch::default();
    q.white.minor = r(0, 1);
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![2]);
}

#[test]
fn current_board_counts_everything_but_the_kings() {
    let pos =
        Position::from_fen("r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4")
            .unwrap();
    let q = MaterialSearch::from_position(&pos);
    assert_eq!(q.white.pawn, r(8, 8));
    assert_eq!(q.white.bishop, r(2, 2));
    assert_eq!(q.white.knight, r(2, 2));
    assert_eq!(q.white.minor, r(4, 4));
    assert_eq!(q.black.pawn, r(8, 8));
    assert_eq!(q.black.rook, r(2, 2));
    assert_eq!(q.black.queen, r(1, 1));
    assert_eq!(q.diff, None);
}

// -------------------------------------------------------------- bishops

#[test]
fn bishop_colours_apply_only_with_one_bishop_each_and_fail_only_that_ply() {
    let mut db = fixture();
    let mut q = MaterialSearch {
        moves: (0, 999),
        ..MaterialSearch::default()
    };
    q.white.bishop = r(0, 1);
    q.black.bishop = r(0, 1);
    q.bishops = BishopColors::Same;
    let (f, _) = run_material(&mut db, &q);
    // 3: same colour at once; 5: same colour at ply 0.
    assert_eq!((f.get(3), f.get(5)), (1, 1));
    assert_eq!(f.get(4), 0, "opposite colours all game");
    q.bishops = BishopColors::Opposite;
    let (f, _) = run_material(&mut db, &q);
    assert_eq!(f.get(3), 0);
    assert_eq!(f.get(4), 1);
    // Game 5: ply 0 fails, but ply 1 (Black has no bishop left) passes: the
    // game is not lost for good (Scid would return false for the game).
    assert_eq!(f.get(5), 2);
    // Without the colour rule a game of the fixture without bishops is not
    // touched by it (rule not applicable).
    assert_eq!(f.get(1), 1);
}

// ----------------------------------------------------------- difference

#[test]
fn ignoring_colours_swaps_the_counts_and_the_sign_of_the_difference() {
    let mut db = fixture();
    let mut q = MaterialSearch::default();
    q.white.queen = r(1, 1);
    q.black.queen = r(0, 0);
    q.diff = Some((1, 40));
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![2]);
    q.ignore_colors = true;
    // Game 11 has the queen on Black's side: it is the image of the search.
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![2, 11]);
    // "Black must be better" with White's queen asked: nothing, in either
    // pass (queen on the wrong side with the wrong sign).
    q.diff = Some((-40, -1));
    assert_eq!(numbers(&run_material(&mut db, &q).0), Vec::<u32>::new());
}

#[test]
fn the_difference_values_pieces_with_the_scid_scale() {
    // P=1 N=B=3 R=5 Q=9, kings 0; limits are inclusive.
    let cases = [
        ("4k3/8/8/8/8/8/P7/4K3 w - - 0 1", 1),
        ("4k3/8/8/8/8/8/N7/4K3 w - - 0 1", 3),
        ("4k3/8/8/8/8/8/B7/4K3 w - - 0 1", 3),
        ("4k3/8/8/8/8/8/R7/4K3 w - - 0 1", 5),
        ("4k3/8/8/8/8/8/Q7/4K3 w - - 0 1", 9),
        ("4k3/p7/8/8/8/8/Q7/4K3 w - - 0 1", 8),
        ("4k3/8/8/8/8/8/n7/R3K2R w - - 0 1", 7),
        ("4k3/8/8/8/8/8/8/4K3 w - - 0 1", 0),
    ];
    let wide = SideRanges {
        queen: r(0, 9),
        rook: r(0, 9),
        bishop: r(0, 9),
        knight: r(0, 9),
        minor: r(0, 9),
        pawn: r(0, 9),
    };
    for (fen, value) in cases {
        let (_fx, mut db) = db_of(vec![game_fen(fen, "e1d1")]);
        let mut q = MaterialSearch {
            white: wide,
            black: wide,
            ..MaterialSearch::default()
        };
        q.diff = Some((value, value));
        assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1], "{fen}");
        q.diff = Some((value + 1, value + 2));
        assert!(numbers(&run_material(&mut db, &q).0).is_empty(), "{fen}");
        q.diff = Some((value - 2, value - 1));
        assert!(numbers(&run_material(&mut db, &q).0).is_empty(), "{fen}");
    }
}

// ---------------------------------------------------- move range, runs

#[test]
fn the_range_is_in_move_numbers_converted_to_plies() {
    let mut db = fixture();
    // White queen on h5 from ply 3 to ply 6 (game 6).
    let p = with_pattern(Pattern::on_square(WQ, File::H, Rank::Fifth, true));
    let mut at = |moves: (u16, u16)| {
        let q = MaterialSearch { moves, ..p.clone() };
        run_material(&mut db, &q).0.get(6)
    };
    assert_eq!(at((1, 999)), 4);
    assert_eq!(at((1, 1)), 0, "plies 1 and 2 only");
    assert_eq!(at((2, 2)), 4, "plies 3 and 4");
    assert_eq!(at((3, 3)), 6, "plies 5 and 6");
    assert_eq!(at((4, 4)), 0, "plies 7 and 8");
    // The queen is on d1 at plies 0 to 2.
    let d1 = with_pattern(Pattern::on_square(WQ, File::D, Rank::First, true));
    let mut at = |moves: (u16, u16)| {
        let q = MaterialSearch {
            moves,
            ..d1.clone()
        };
        run_material(&mut db, &q).0.get(6)
    };
    assert_eq!(at((0, 999)), 1);
    assert_eq!(at((1, 999)), 2);
    assert_eq!(at((2, 999)), 0);
}

#[test]
fn at_least_n_plies_in_a_row_and_the_value_is_the_start_of_the_run() {
    let mut db = fixture();
    // Game 7: the knight is on f3 at plies 1-2 and 5-8 and not at 3-4 or 9.
    let p = with_pattern(Pattern::on_square(WN, File::F, Rank::Third, true));
    let value = |db: &mut mychess_db::PgnDatabase, min_plies: u16, moves: (u16, u16)| {
        let q = MaterialSearch {
            min_plies,
            moves,
            ..p.clone()
        };
        run_material(db, &q).0.get(7)
    };
    assert_eq!(value(&mut db, 0, (1, 999)), 2, "0 counts as 1");
    assert_eq!(value(&mut db, 1, (1, 999)), 2);
    assert_eq!(value(&mut db, 2, (1, 999)), 2);
    assert_eq!(value(&mut db, 3, (1, 999)), 6, "the first run is too short");
    assert_eq!(value(&mut db, 4, (1, 999)), 6);
    assert_eq!(value(&mut db, 5, (1, 999)), 0);
    // The range cuts the runs: from ply 5 on, runs of 2 start at 5.
    assert_eq!(value(&mut db, 2, (3, 999)), 6);
    // A run cannot cross the last tested ply (move 2 = ply 4).
    assert_eq!(value(&mut db, 3, (1, 2)), 0);
}

#[test]
fn end_position_only_ignores_range_and_length_and_never_matches_an_empty_game() {
    let mut db = fixture();
    let p = with_pattern(Pattern::on_square(WN, File::F, Rank::Third, true));
    let q = MaterialSearch {
        end_only: true,
        ..p.clone()
    };
    let (f, _) = run_material(&mut db, &q);
    // Game 7 ends without the knight on f3; game 12 has it at the end.
    assert_eq!(f.get(7), 0);
    assert_eq!(f.get(12), 3, "the last ply is 2, plus one");
    let q = MaterialSearch {
        end_only: true,
        moves: (500, 600),
        min_plies: 99,
        ..p
    };
    assert_eq!(run_material(&mut db, &q).0.get(12), 3);
    let q = MaterialSearch {
        end_only: true,
        moves: (0, 999),
        ..MaterialSearch::default()
    };
    let (f, _) = run_material(&mut db, &q);
    assert_eq!(f.get(8), 0, "a game without moves");
    assert_eq!(f.get(9), 2);
}

#[test]
fn a_first_ply_after_254_is_limited_to_255_and_never_wraps() {
    let mut q = MaterialSearch::default();
    q.black.pawn = r(7, 7);
    for (n, expected) in [(248, 252), (250, 254), (252, 255), (300, 255), (600, 255)] {
        let moves = format!("{} e2e4 d7d5 e4d5", cs::shuffle(n));
        let (_fx, mut db) = db_of(vec![game(&moves)]);
        // exd5 is ply n + 3, so the value would be n + 4.
        assert_eq!(
            values(&run_material(&mut db, &q).0),
            vec![(1, expected)],
            "n = {n}"
        );
    }
}

// ------------------------------------------------------------- patterns

#[test]
fn up_to_ten_patterns_must_all_hold_in_any_order() {
    use Color::{Black as B, White as W};
    let mut db = fixture();
    let on = |c, role, f, rk, yes| Pattern::on_square(piece(c, role), f, rk, yes);
    let mut patterns = vec![
        on(W, Role::Rook, File::A, Rank::Third, true),
        on(W, Role::King, File::D, Rank::Second, true),
        on(W, Role::Pawn, File::E, Rank::Second, true),
        on(B, Role::Rook, File::H, Rank::Seventh, true),
        on(B, Role::King, File::D, Rank::Seventh, true),
        on(B, Role::Pawn, File::E, Rank::Seventh, true),
        Pattern::on_file(piece(W, Role::Rook), File::B, false),
        Pattern::on_file(piece(B, Role::King), File::E, false),
        Pattern::on_file(piece(W, Role::Pawn), File::D, false),
        Pattern::on_file(piece(B, Role::Queen), File::A, false),
    ];
    let mut q = MaterialSearch {
        patterns: patterns.clone(),
        end_only: true,
        ..MaterialSearch::default()
    };
    assert_eq!(values(&run_material(&mut db, &q).0), vec![(1, 5)]);
    patterns.reverse();
    q.patterns = patterns.clone();
    assert_eq!(values(&run_material(&mut db, &q).0), vec![(1, 5)]);
    patterns[3].present = !patterns[3].present;
    q.patterns = patterns;
    assert!(numbers(&run_material(&mut db, &q).0).is_empty());
}

#[test]
fn yes_and_no_on_a_file_and_on_a_rank() {
    let mut db = fixture();
    let mut q = |p: Pattern| {
        numbers(
            &run_material(
                &mut db,
                &MaterialSearch {
                    end_only: true,
                    ..with_pattern(p)
                },
            )
            .0,
        )
    };
    // Game 12 (ends with a bishop on g5 and a pawn on h6): an end-position
    // pattern about the d-pawns.
    let wp = piece(Color::White, Role::Pawn);
    let d_yes = q(Pattern::on_file(wp, File::D, true));
    assert!(d_yes.contains(&12) && !d_yes.contains(&2));
    let d_no = q(Pattern::on_file(wp, File::D, false));
    assert!(!d_no.contains(&12) && d_no.contains(&2));
    let rank_two = Pattern {
        piece: Some(wp),
        file: None,
        rank: Some(Rank::Second),
        present: true,
    };
    let on_rank = q(rank_two);
    assert!(on_rank.contains(&12) && on_rank.contains(&1) && !on_rank.contains(&2));
}

#[test]
fn a_pattern_without_a_piece_is_ignored_one_without_square_is_an_error_and_the_cap_is_32() {
    let (_fx, mut db) = db_of(vec![game("e2e4")]);
    let ignored = Pattern {
        piece: None,
        file: Some(File::A),
        rank: None,
        present: false,
    };
    let q = MaterialSearch {
        patterns: vec![ignored],
        ..MaterialSearch::default()
    };
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
    let mut filter = Filter::new(1);
    let before = filter.clone();
    let q = with_pattern(Pattern {
        piece: Some(WP),
        file: None,
        rank: None,
        present: false,
    });
    let err = search_material(
        &mut db,
        &q,
        &mut filter,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(matches!(err, SearchError::InvalidPattern { .. }));
    assert_eq!(filter, before);
    let ok = Pattern::on_file(WP, File::E, true);
    let q = MaterialSearch {
        patterns: vec![ok; 32],
        ..MaterialSearch::default()
    };
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
    let q = MaterialSearch {
        patterns: vec![ok; 33],
        ..MaterialSearch::default()
    };
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
fn ignoring_colours_flips_the_rank_but_not_the_file() {
    let (_fx, mut db) = db_of(vec![
        game_fen("4k3/p7/8/8/8/8/8/4K3 w - - 0 1", "e1d1"),
        game_fen("4k3/7p/8/8/8/8/8/4K3 w - - 0 1", "e1d1"),
    ]);
    // "White pawn on a2" ... flipped: black pawn on a7.
    let mut q = with_pattern(Pattern::on_square(WP, File::A, Rank::Second, true));
    q.white.pawn = r(0, 8);
    assert!(numbers(&run_material(&mut db, &q).0).is_empty());
    q.ignore_colors = true;
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1]);
}

// ------------------------------------------------------------- presets

fn ending(key: &str) -> mychess_db::EndingPreset {
    material_presets()
        .into_iter()
        .find(|p| p.key == key)
        .unwrap()
}

fn pattern_preset(key: &str) -> mychess_db::PatternPreset {
    pattern_presets()
        .into_iter()
        .find(|p| p.key == key)
        .unwrap()
}

#[test]
fn there_are_thirteen_endings_and_fourteen_patterns_with_distinct_keys() {
    let e = material_presets();
    let p = pattern_presets();
    assert_eq!((e.len(), p.len()), (13, 14));
    let mut keys: Vec<_> = e
        .iter()
        .map(|x| x.key)
        .chain(p.iter().map(|x| x.key))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), 27);
}

#[test]
fn endings_find_the_endings_they_name() {
    let games = vec![
        game_fen("4k3/4p3/8/8/8/8/8/R3K3 w - - 0 1", "e1d1 e8d8"), // 1 rook v pawn
        game_fen("4k2r/8/8/8/8/8/4P3/R3K3 w - - 0 1", "e1d1 e8d8"), // 2 rook+pawn v rook
        game_fen("q3k3/8/8/8/8/8/8/3QK3 w - - 0 1", "e1f1 e8f8"),  // 3 queens
        game_fen("4k3/8/8/8/8/8/8/2B1K1n1 w - - 0 1", "e1d1 e8d8"), // 4 bishop v knight
        game_fen("4k2r/8/8/8/8/8/P7/R3K3 w - - 0 1", "a1b1 e8d8"), // 5 passed a-pawn, plain
        game_fen("r3k3/p7/8/8/8/8/8/4K2R w - - 0 1", "h1g1 e8d8"), // 6 passed a-pawn, colours swapped
        game_fen("4k1r1/8/8/8/8/8/8/R2RK3 w - - 0 1", "d1d2 e8f8"), // 7 two rooks v one
    ];
    let (_fx, mut db) = db_of(games);
    let run = |db: &mut _, key: &str| {
        let mut q = MaterialSearch::default();
        ending(key).apply(&mut q);
        numbers(&run_material(db, &q).0)
    };
    assert_eq!(run(&mut db, "ending-rook-vs-pawns"), vec![1]);
    assert_eq!(run(&mut db, "ending-rook-pawn-vs-rook"), vec![2, 5]); // 5 is also R+P v R
    assert_eq!(run(&mut db, "ending-queens"), vec![3]);
    assert_eq!(run(&mut db, "ending-bishop-vs-knight"), vec![4]);
    // Passed a-pawn with colours ignored finds both orientations.
    assert_eq!(run(&mut db, "ending-rooks-passed-a"), vec![5, 6]);
    let mut q = MaterialSearch::default();
    ending("ending-rooks-passed-a").apply(&mut q);
    q.ignore_colors = false;
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![5]);
}

#[test]
fn applying_an_ending_keeps_the_other_options_and_a_pattern_only_raises_minimums() {
    let mut q = MaterialSearch::default();
    q.moves = (3, 30);
    q.min_plies = 4;
    q.bishops = BishopColors::Opposite;
    q.op = FilterOp::Or;
    ending("ending-knights").apply(&mut q);
    assert_eq!(
        (q.moves, q.min_plies, q.bishops, q.op),
        ((3, 30), 4, BishopColors::Opposite, FilterOp::Or)
    );
    // Counts of "White IQP": white pawns minimum raised to at least 1 from
    // what the window already had (0..8).
    let mut q = MaterialSearch::default();
    q.white.pawn = r(3, 6);
    pattern_preset("pattern-white-iqp").apply(&mut q);
    assert_eq!(q.white.pawn, r(3, 6), "an already higher minimum stays");
    let mut q = MaterialSearch::default();
    pattern_preset("pattern-white-iqp").apply(&mut q);
    assert_eq!(q.white.pawn, r(1, 8));
    assert_eq!(q.patterns.len(), 3);
}

#[test]
fn common_patterns_find_an_isolated_queens_pawn() {
    let mut db = fixture();
    let mut q = MaterialSearch::default();
    pattern_preset("pattern-white-iqp").apply(&mut q);
    // Game 12: white pawns a2 b2 d4 f2 g2 h2; the scholar's mate game has
    // c and e pawns beside d.
    assert_eq!(numbers(&run_material(&mut db, &q).0), vec![12]);
    let mut q = MaterialSearch::default();
    pattern_preset("pattern-black-iqp").apply(&mut q);
    assert!(numbers(&run_material(&mut db, &q).0).is_empty());
    // Kc1/Kg8 patterns carry no count changes.
    let p = pattern_preset("pattern-kc1-kg8");
    assert!(p.counts.is_empty());
    assert_eq!(p.patterns.len(), 2);
}

#[test]
fn the_pattern_presets_set_the_counts_of_the_spec() {
    // (key, white pawns, black pawns) after applying on the window default
    // (0..8 pawns): "if lower" minimums and the explicit ranges of the table.
    let table = [
        ("pattern-white-iqp", r(1, 8), r(0, 8)),
        ("pattern-white-iqp-break-e6", r(1, 8), r(1, 8)),
        ("pattern-white-iqp-break-c6", r(1, 8), r(1, 8)),
        ("pattern-black-iqp", r(0, 8), r(1, 8)),
        ("pattern-white-black-iqp", r(1, 8), r(1, 8)),
        ("pattern-couple-c3-d4", r(4, 6), r(4, 6)),
        ("pattern-hanging-c5-d5", r(4, 6), r(4, 6)),
        ("pattern-maroczy", r(2, 7), r(1, 7)),
        ("pattern-rook-sac-c3", r(0, 8), r(0, 8)),
        ("pattern-kc1-kg8", r(0, 8), r(0, 8)),
        ("pattern-kg1-kc8", r(0, 8), r(0, 8)),
        ("pattern-light-fian", r(0, 8), r(0, 8)),
        ("pattern-dark-fian", r(0, 8), r(0, 8)),
        ("pattern-four-fian", r(0, 8), r(0, 8)),
    ];
    for (key, wp, bp) in table {
        let mut q = MaterialSearch::default();
        pattern_preset(key).apply(&mut q);
        assert_eq!((q.white.pawn, q.black.pawn), (wp, bp), "{key}");
    }
    let mut q = MaterialSearch::default();
    pattern_preset("pattern-rook-sac-c3").apply(&mut q);
    assert_eq!((q.white.rook, q.black.rook), (r(2, 2), r(2, 2)));
    let mut q = MaterialSearch::default();
    pattern_preset("pattern-four-fian").apply(&mut q);
    assert_eq!((q.white.bishop.min, q.black.bishop.min), (2, 2));
}

// ----------------------------------------------- index shortcuts, START

#[test]
fn an_own_start_game_survives_a_lying_index_and_the_no_pattern_without_rank() {
    let start = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
    let games = || {
        vec![
            game_fen(start, "e2e4 e8d7 e4e5 d7e6"),
            game_fen(start, "e1d1 e8d8"),
        ]
    };
    // A "no pawn on the a file" pattern is the one that made Scid discard
    // every START game (hpSig_Final of an empty list).
    let mut q = with_pattern(Pattern::on_file(
        piece(Color::Black, Role::Pawn),
        File::A,
        false,
    ));
    q.white.queen = r(0, 0);
    for lie in [cs::spoil_index as fn(&base::Fixture, u32), cs::empty_index] {
        let (fx, db) = db_of(games());
        drop(db);
        lie(&fx, 1);
        lie(&fx, 2);
        let mut db = fx.open();
        assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1, 2]);
        q.shortcuts = false;
        assert_eq!(numbers(&run_material(&mut db, &q).0), vec![1, 2]);
        q.shortcuts = true;
    }
}

#[test]
fn shortcuts_never_change_the_answer_over_the_fixture() {
    let mut db = fixture();
    let mut searches = vec![MaterialSearch::default(), MaterialSearch::zero()];
    let mut q = MaterialSearch::default();
    q.white.queen = r(0, 0);
    q.black.queen = r(0, 0);
    searches.push(q.clone());
    q.patterns = vec![Pattern::on_file(WP, File::E, false)];
    searches.push(q.clone());
    q.patterns = vec![
        Pattern::on_file(WP, File::D, false),
        Pattern::on_file(piece(Color::Black, Role::Pawn), File::A, false),
    ];
    q.ignore_colors = true;
    searches.push(q.clone());
    q.end_only = true;
    searches.push(q);
    for mut q in searches {
        let fast = run_material(&mut db, &q).0;
        q.shortcuts = false;
        let slow = run_material(&mut db, &q).0;
        assert_eq!(fast.as_bytes(), slow.as_bytes());
    }
}

// ---------------------------------------- filter operation and scanning

#[test]
fn and_or_reset_and_a_non_match_is_zero_even_in_or() {
    let mut db = fixture();
    let mut q = MaterialSearch::default();
    q.white.queen = r(1, 1);
    q.black.queen = r(0, 0);
    let mut seed = Filter::new(12);
    seed.fill(0);
    seed.set(2, 77); // matches
    seed.set(1, 66); // does not
    let go = |op, db: &mut mychess_db::PgnDatabase| {
        let mut f = seed.clone();
        let q = MaterialSearch { op, ..q.clone() };
        let rep = search_material(db, &q, &mut f, &mut NoProgress, &CancelToken::new()).unwrap();
        (values(&f), rep.start_count)
    };
    assert_eq!(go(FilterOp::And, &mut db), (vec![(2, 2)], 2));
    // OR: games in the filter are not examined (value kept); others enter.
    assert_eq!(go(FilterOp::Or, &mut db), (vec![(1, 66), (2, 77)], 12));
    assert_eq!(go(FilterOp::Reset, &mut db).0, vec![(2, 2)]);
}

#[test]
fn cancel_keeps_the_filter_and_progress_is_monotonic() {
    let (_fx, mut db) = cs::crowd(5000, &["e2e4 e7e5", "d2d4 d7d5 c2c4"]);
    let mut filter = Filter::new(db.len());
    filter.fill(0);
    filter.set(9, 12);
    let before = filter.as_bytes().to_vec();
    let token = CancelToken::new();
    let mut sink = cs::Recorder::cancelling(&token);
    let q = MaterialSearch {
        op: FilterOp::Or,
        ..MaterialSearch::default()
    };
    let rep = search_material(&mut db, &q, &mut filter, &mut sink, &token).unwrap();
    assert!(rep.interrupted);
    assert_eq!(filter.as_bytes(), &before[..]);
    let mut sink = cs::Recorder::recording();
    let rep = search_material(&mut db, &q, &mut filter, &mut sink, &CancelToken::new()).unwrap();
    assert!(!rep.interrupted);
    assert!(sink.calls.windows(2).all(|w| w[0].0 <= w[1].0));
    assert_eq!(*sink.calls.last().unwrap(), (5000, 5000));
}

#[test]
fn damaged_records_do_not_match_and_are_listed() {
    let (fx, db) = db_of(vec![game("e2e4"), game("e2e4"), game("e2e4")]);
    drop(db);
    base::zero_length(&fx, 1);
    base::break_offset(&fx, 3);
    let mut db = fx.open();
    let (f, rep) = run_material(&mut db, &MaterialSearch::default());
    assert_eq!(numbers(&f), vec![2]);
    assert_eq!(rep.unreadable, vec![3]);
}

#[test]
fn one_thread_many_threads_and_no_shortcuts_give_the_same_bytes() {
    let templates = [
        "e2e4 e7e5 g1f3 b8c6",
        "d2d4 d7d5 c2c4 d5c4",
        "",
        "e2e4 d7d5 e4d5 d8d5",
    ];
    let (_fx, mut db) = cs::crowd(mychess_db::BLOCK_GAMES * 2 + 11, &templates);
    let mut q = MaterialSearch::default();
    q.black.pawn = r(7, 7);
    q.ignore_colors = true;
    let one = cs::with_threads(1, || run_material(&mut db, &q).0);
    let many = cs::with_threads(4, || run_material(&mut db, &q).0);
    q.shortcuts = false;
    let slow = cs::with_threads(3, || run_material(&mut db, &q).0);
    assert_eq!(one.as_bytes(), many.as_bytes());
    assert_eq!(one.as_bytes(), slow.as_bytes());
    // Only the fourth template captures a pawn: value ply 3 + 1.
    assert_eq!(one.get(4), 4);
    assert_eq!(one.get(1), 0);
}
