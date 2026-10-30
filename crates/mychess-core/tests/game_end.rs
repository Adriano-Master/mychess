//! Game-ending rules and Zobrist hashing (spec rules 27-29, D4, D5).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    let_underscore_drop
)]
mod common;
use common::fen;
use mychess_core::{Color, GameEnd, Position, ZobristHash};
use rstest::rstest;

fn play_all(start: &Position, ucis: &[&str]) -> (Position, Vec<ZobristHash>) {
    let mut p = start.clone();
    let mut hist = Vec::new();
    for u in ucis {
        hist.push(p.zobrist());
        let m = p.parse_uci(u).unwrap_or_else(|e| panic!("{u}: {e}"));
        p.play(m).unwrap();
    }
    (p, hist)
}

#[test]
fn initial_position_is_not_over() {
    assert_eq!(Position::new().game_end(&[]), None);
}

#[test]
fn fools_mate_is_checkmate_won_by_black() {
    let (p, h) = play_all(&Position::new(), &["f2f3", "e7e5", "g2g4", "d8h4"]);
    assert!(p.is_checkmate() && p.is_check() && !p.is_stalemate());
    assert_eq!(
        p.game_end(&h),
        Some(GameEnd::Checkmate {
            winner: Color::Black
        })
    );
    assert!(p.legal_moves().is_empty());
}

#[test]
fn white_delivering_mate_has_white_as_winner() {
    let start = fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
    let (p, h) = play_all(&start, &["a1a8"]);
    assert_eq!(
        p.game_end(&h),
        Some(GameEnd::Checkmate {
            winner: Color::White
        })
    );
}

#[test]
fn stalemate_is_detected() {
    let p = fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    assert!(p.is_stalemate() && !p.is_check() && !p.is_checkmate());
    assert_eq!(p.game_end(&[]), Some(GameEnd::Stalemate));
}

#[test]
fn being_in_check_with_moves_is_not_over() {
    let p = fen("4k3/8/8/8/8/8/8/r3K3 w - - 0 1");
    assert!(p.is_check() && !p.is_checkmate());
    assert_eq!(p.game_end(&[]), None);
}

#[rstest]
#[case("4k3/8/8/8/8/8/8/4K3 w - - 0 1", true)] // K v K
#[case("4k3/8/8/8/8/8/8/3BK3 w - - 0 1", true)] // KB v K
#[case("4k3/8/8/8/8/8/8/3NK3 w - - 0 1", true)] // KN v K
#[case("4kb2/8/8/8/8/8/8/2B1K3 w - - 0 1", true)] // KB v KB same colour (both dark)
#[case("4kb2/8/8/8/8/8/8/3BK3 w - - 0 1", false)] // KB v KB opposite colours
#[case("4kn2/8/8/8/8/8/8/3NK3 w - - 0 1", false)] // KN v KN (D4)
#[case("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", false)] // KP v K
#[case("4k3/8/8/8/8/8/8/3RK3 w - - 0 1", false)] // KR v K
#[case("4k3/8/8/8/8/8/8/3QK3 w - - 0 1", false)] // KQ v K
#[case("4k3/8/8/8/8/8/8/2NNK3 w - - 0 1", false)] // KNN v K can still mate
#[case("4k3/8/8/8/8/8/8/2BBK3 w - - 0 1", false)] // KBB v K (opposite colours)
#[case("4k3/p7/8/8/8/8/8/4K3 w - - 0 1", false)] // K v KP
fn insufficient_material_table(#[case] f: &str, #[case] expected: bool) {
    let p = fen(f);
    assert_eq!(p.is_insufficient_material(), expected, "{f}");
    let end = p.game_end(&[]);
    if expected {
        assert_eq!(end, Some(GameEnd::InsufficientMaterial), "{f}");
    } else {
        assert!(
            !matches!(end, Some(GameEnd::InsufficientMaterial)),
            "{f}: {end:?}"
        );
    }
}

#[test]
fn many_bishops_all_on_one_colour_are_insufficient() {
    // White bishops c1 and a3 and Black bishop f8 all stand on dark squares.
    assert!(fen("4kb2/8/8/8/8/B7/8/2B1K3 w - - 0 1").is_insufficient_material());
    // Adding a light-squared bishop makes mate possible again.
    assert!(!fen("4kb2/8/8/8/8/B7/8/2BBK3 w - - 0 1").is_insufficient_material());
}

#[test]
fn fifty_move_rule_triggers_at_100_halfmoves() {
    let at99 = fen("4k3/8/8/8/8/8/8/R3K3 w - - 99 80");
    let at100 = fen("4k3/8/8/8/8/8/8/R3K3 w - - 100 80");
    let at150 = fen("4k3/8/8/8/8/8/8/R3K3 b - - 150 90");
    assert_eq!(at99.game_end(&[]), None);
    assert_eq!(at100.game_end(&[]), Some(GameEnd::FiftyMoves));
    assert_eq!(at150.game_end(&[]), Some(GameEnd::FiftyMoves));
}

#[test]
fn checkmate_takes_precedence_over_fifty_move_rule() {
    let p = fen("R5k1/5ppp/8/8/8/8/8/6K1 b - - 100 80");
    assert!(p.is_checkmate());
    assert_eq!(
        p.game_end(&[]),
        Some(GameEnd::Checkmate {
            winner: Color::White
        })
    );
}

#[test]
fn stalemate_takes_precedence_over_fifty_move_rule() {
    let p = fen("7k/5Q2/6K1/8/8/8/8/8 b - - 100 80");
    assert_eq!(p.game_end(&[]), Some(GameEnd::Stalemate));
}

#[test]
fn insufficient_material_takes_precedence_over_fifty_move_rule() {
    let p = fen("4k3/8/8/8/8/8/8/4K3 w - - 100 80");
    assert_eq!(p.game_end(&[]), Some(GameEnd::InsufficientMaterial));
}

const SHUFFLE: [&str; 4] = ["g1f3", "g8f6", "f3g1", "f6g8"];

#[test]
fn threefold_repetition_needs_third_occurrence() {
    let start = Position::new();
    let (p1, h1) = play_all(&start, &SHUFFLE);
    assert_eq!(p1.zobrist(), start.zobrist());
    assert_eq!(p1.game_end(&h1), None, "second occurrence is not enough");

    let twice: Vec<&str> = SHUFFLE.iter().chain(SHUFFLE.iter()).copied().collect();
    let (p2, h2) = play_all(&start, &twice);
    assert_eq!(p2.game_end(&h2), Some(GameEnd::ThreefoldRepetition));
}

#[test]
fn repetition_history_excludes_current_position() {
    // With the current position passed in the history it would count it twice.
    let start = Position::new();
    let (p1, h1) = play_all(&start, &SHUFFLE);
    let mut with_current = h1.clone();
    with_current.push(p1.zobrist());
    // Documented contract: history = earlier positions only, so the correct
    // call above yields None; this just pins that the API is not idempotent
    // to including the current one.
    assert_eq!(p1.game_end(&h1), None);
    assert_eq!(
        p1.game_end(&with_current),
        Some(GameEnd::ThreefoldRepetition)
    );
}

#[test]
fn repetition_does_not_need_to_be_consecutive() {
    let start = Position::new();
    let seq = [
        "g1f3", "g8f6", "f3g1", "f6g8", "b1c3", "b8c6", "c3b1", "c6b8", "g1f3", "g8f6", "f3g1",
        "f6g8",
    ];
    let (p, h) = play_all(&start, &seq);
    assert_eq!(p.game_end(&h), Some(GameEnd::ThreefoldRepetition));
}

#[test]
fn different_side_to_move_is_not_a_repetition() {
    let start = Position::new();
    let (p, h) = play_all(&start, &["g1f3", "g8f6", "f3g1"]);
    assert_eq!(p.game_end(&h), None);
}

#[test]
fn different_castling_rights_do_not_count_as_repetition() {
    // Rooks leave and return: same board and side to move, but the castling
    // rights are gone from the 4th ply on, so only plies 4 and 8 match.
    let start = fen("r3k3/8/8/8/8/8/8/R3K3 w Qq - 0 1");
    let cycle = ["a1b1", "a8b8", "b1a1", "b8a8"];
    let (p4, h4) = play_all(&start, &cycle);
    assert_ne!(
        p4.zobrist(),
        start.zobrist(),
        "rights differ: hashes must differ"
    );
    assert_eq!(p4.game_end(&h4), None);

    let twice: Vec<&str> = cycle.iter().chain(cycle.iter()).copied().collect();
    let (p8, h8) = play_all(&start, &twice);
    assert_eq!(
        p8.game_end(&h8),
        None,
        "start (with rights) must not count as occurrence"
    );

    let thrice: Vec<&str> = cycle
        .iter()
        .chain(cycle.iter())
        .chain(cycle.iter())
        .copied()
        .collect();
    let (p12, h12) = play_all(&start, &thrice);
    assert_eq!(p12.game_end(&h12), Some(GameEnd::ThreefoldRepetition));
}

#[test]
fn fifty_move_rule_takes_precedence_over_repetition() {
    let start = fen("4k3/8/8/8/8/8/8/R3K3 w - - 96 80");
    let seq: Vec<&str> = ["a1a2", "e8e7", "a2a1", "e7e8"]
        .into_iter()
        .cycle()
        .take(8)
        .collect();
    let (p, h) = play_all(&start, &seq);
    assert_eq!(p.halfmove_clock(), 104);
    assert_eq!(p.game_end(&h), Some(GameEnd::FiftyMoves));
}

#[test]
fn irreversible_move_resets_clock() {
    let (p, _) = play_all(&fen("4k3/8/8/8/8/8/P7/4K3 w - - 99 80"), &["a2a3"]);
    assert_eq!(p.halfmove_clock(), 0);
}

// ---------- Zobrist ----------

#[test]
fn zobrist_of_initial_position_is_stable() {
    assert_eq!(Position::new().zobrist().value(), 0x463b_9618_1691_fc9c);
    assert_eq!(u64::from(Position::new().zobrist()), 0x463b_9618_1691_fc9c);
    assert_eq!(Position::new().zobrist().to_string(), "463b96181691fc9c");
}

#[test]
fn zobrist_matches_polyglot_reference_values() {
    // Values published in the Polyglot book format description.
    let cases: [(&[&str], u64); 4] = [
        (&["e2e4"], 0x823c_9b50_fd11_4196),
        (&["e2e4", "d7d5"], 0x0756_b944_61c5_0fb0),
        (&["e2e4", "d7d5", "e4e5"], 0x662f_afb9_65db_29d4),
        (&["e2e4", "d7d5", "e4e5", "f7f5"], 0x22a4_8b5a_8e47_ff78),
    ];
    for (moves, expected) in cases {
        let (p, _) = play_all(&Position::new(), moves);
        assert_eq!(p.zobrist().value(), expected, "{moves:?}");
    }
}

#[test]
fn zobrist_is_equal_for_transpositions() {
    let (a, _) = play_all(&Position::new(), &["g1f3", "g8f6", "b1c3", "b8c6"]);
    let (b, _) = play_all(&Position::new(), &["b1c3", "b8c6", "g1f3", "g8f6"]);
    assert_eq!(a.zobrist(), b.zobrist());
    let (c, _) = play_all(&Position::new(), &["e2e4", "e7e5", "g1f3"]);
    let (d, _) = play_all(&Position::new(), &["g1f3", "e7e5", "e2e4"]);
    assert_eq!(c.zobrist(), d.zobrist());
}

#[test]
fn zobrist_ignores_clocks_and_depends_on_turn() {
    let a = fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
    let b = fen("4k3/8/8/8/8/8/8/4K3 w - - 37 55");
    let c = fen("4k3/8/8/8/8/8/8/4K3 b - - 0 1");
    assert_eq!(a.zobrist(), b.zobrist());
    assert_ne!(a.zobrist(), c.zobrist());
}

#[test]
fn zobrist_depends_on_castling_rights() {
    let a = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    let b = fen("r3k2r/8/8/8/8/8/8/R3K2R w Qkq - 0 1");
    assert_ne!(a.zobrist(), b.zobrist());
}

#[test]
fn zobrist_en_passant_counts_only_when_capture_is_possible() {
    // No adjacent pawn: with or without the e3 target the hash is the same.
    let (p, _) = play_all(&Position::new(), &["e2e4"]);
    let q = fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1");
    assert_eq!(p.zobrist(), q.zobrist());
}

#[test]
fn zobrist_from_fen_equals_from_play() {
    let (p, _) = play_all(&Position::new(), &["e2e4", "c7c5", "g1f3"]);
    let q = Position::from_fen(&p.fen()).unwrap();
    assert_eq!(p.zobrist(), q.zobrist());
}
