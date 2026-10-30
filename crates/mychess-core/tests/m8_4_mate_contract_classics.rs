//! Contract tests of the forced-mate solver (spec M8-4, D1906-D1909) on
//! classic problems with a known length and first move, on "no mate"
//! answers, on hostile inputs and on determinism and budgets.
//!
//! The positions are classic patterns written down by us ("classic
//! position"); their lengths come from chess literature, not from the solver.

use mychess_core::mate::{
    DefenceResult, MateLimits, MateResult, best_defence, find_mate, principal_line,
};
use mychess_core::{Move, Position};
use rstest::rstest;

const BIG: u64 = 20_000_000;

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).expect("test fen")
}

fn lim(max_moves: u8) -> MateLimits {
    MateLimits {
        max_moves,
        max_nodes: BIG,
    }
}

/// Plays `line` from `start`, checking legality, and returns the end.
fn play_all(start: &Position, line: &[Move]) -> Position {
    let mut cur = start.clone();
    for (i, m) in line.iter().enumerate() {
        assert!(cur.is_legal(*m), "move {i} is illegal in {}", cur.fen());
        cur.play(*m).expect("legal");
    }
    cur
}

// ---- classic problems: exact length and first move -----------------------

/// (name, FEN, mate length, accepted first moves; empty = any move that mates)
#[rstest]
// Back-rank mate.
#[case("back_rank", "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", 1, &["a1a8"])]
// Legal's mate, final position of the trap: 7.Nd5#.
#[case("legal", "rn1q1bnr/ppp1kB1p/3p2p1/4N3/4P3/2N5/PPPP1PPP/R1BbK2R w KQ - 0 7", 1, &["c3d5"])]
// Smothered mate with the knight.
#[case("smothered", "6rk/6pp/8/6N1/8/8/8/6K1 w - - 0 1", 1, &["g5f7"])]
// Fool's mate, Black to move.
#[case("fools", "rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq - 0 2", 1, &["d8h4"])]
// Scholar's mate.
#[case("scholars", "r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4", 1, &["h5f7"])]
// Anastasia's mate pattern: knight on e7 and rook on the h-file.
#[case("anastasia", "8/4N1pk/8/8/8/8/8/K2R4 w - - 0 1", 1, &["d1h1"])]
// Corner queen mate supported by the king.
#[case("queen_corner", "k7/8/1K6/8/8/8/8/6Q1 w - - 0 1", 1, &["g1g8"])]
// Knight promotion is the only mate (queen promotion is not even check).
#[case("underpromotion", "rb6/kpP5/p7/8/8/8/8/7K w - - 0 1", 1, &["c7c8n"])]
// Two rooks, ladder: 1.Ra7 Kg8 2.Rb8#.
#[case("ladder", "7k/8/8/8/8/8/R7/1R4K1 w - - 0 1", 2, &[])]
// Morphy's Opera Game finish: Qb8+ Nxb8 Rd8#.
#[case("morphy_opera", "4kb1r/p2n1ppp/4q3/4p1B1/4P3/1Q6/PPP2PPP/2KR4 w k - 1 1", 2, &["b3b8"])]
fn classics_have_the_known_length_and_first_move(
    #[case] name: &str,
    #[case] fen: &str,
    #[case] n: u8,
    #[case] firsts: &[&str],
) {
    let p = pos(fen);
    let MateResult::Mate { moves, first } = find_mate(&p, lim(5)) else {
        panic!("{name}: a mate in {n} exists");
    };
    assert_eq!(moves, n, "{name}: length");
    if firsts.is_empty() {
        assert!(p.is_legal(first), "{name}: first move legal");
    } else {
        assert!(
            firsts.contains(&p.uci(first).as_str()),
            "{name}: got {}",
            p.uci(first)
        );
    }
    if n == 1 {
        assert!(p.after(first).expect("legal").is_checkmate(), "{name}");
    }
    // Asking for exactly the right N works; the line is playable and mates.
    assert!(matches!(find_mate(&p, lim(n)), MateResult::Mate { moves, .. } if moves == n));
    let line = principal_line(&p, n, BIG).expect("line");
    assert_eq!(line.len(), 2 * usize::from(n) - 1, "{name}: line length");
    assert!(play_all(&p, &line).is_checkmate(), "{name}: ends in mate");
}

// ---- no mate: N+1 asked as N, stalemate, nothing to play ------------------

#[rstest]
// Mate in 2 asked with one move.
#[case("7k/8/8/8/8/8/R7/1R4K1 w - - 0 1", 1)]
// Mate in 2 (Opera) asked with 1.
#[case("4kb1r/p2n1ppp/4q3/4p1B1/4P3/1Q6/PPP2PPP/2KR4 w k - 1 1", 1)]
// Start position.
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 3)]
// K+R vs K in the open: no mate in 2.
#[case("8/8/8/4k3/8/8/8/R3K3 w - - 0 1", 2)]
// Black to move, stalemated: nothing to play.
#[case("k7/2Q5/1K6/8/8/8/8/8 b - - 0 1", 3)]
// Black to move, checkmated.
#[case("R5k1/5ppp/8/8/8/8/8/6K1 b - - 0 1", 3)]
// Bare kings and lone minor pieces.
#[case("8/8/8/4k3/8/8/8/4K3 w - - 0 1", 3)]
#[case("8/8/8/4k3/8/8/8/3NK3 w - - 0 1", 3)]
fn no_mate_within_the_limit(#[case] fen: &str, #[case] n: u8) {
    assert_eq!(find_mate(&pos(fen), lim(n)), MateResult::NoMate, "{fen}");
    assert_eq!(principal_line(&pos(fen), n, BIG), None, "{fen}");
}

#[test]
fn a_move_that_only_stalemates_is_not_a_mate() {
    // K+Q vs K cornered: Qf7 / Qg6 stalemate, but a mate exists; the answer
    // must be a real mate and never one of the stalemating moves.
    let p = pos("7k/8/6K1/8/8/8/8/5Q2 w - - 0 1");
    let MateResult::Mate { first, .. } = find_mate(&p, lim(2)) else {
        panic!("mate expected");
    };
    let after = p.after(first).expect("legal");
    assert!(after.is_checkmate());
    assert!(!after.is_stalemate());
}

#[test]
fn a_position_where_every_winning_try_stalemates_has_no_mate_in_one() {
    // White king and pawn only: nothing mates in one.
    let p = pos("k7/P7/K7/8/8/8/8/8 w - - 0 1");
    assert_eq!(find_mate(&p, lim(1)), MateResult::NoMate);
}

#[test]
fn terminal_positions_do_not_panic_for_any_entry_point() {
    for fen in [
        "R5k1/5ppp/8/8/8/8/8/6K1 b - - 0 1", // checkmated
        "k7/2Q5/1K6/8/8/8/8/8 b - - 0 1",    // stalemated
    ] {
        let p = pos(fen);
        assert_eq!(find_mate(&p, lim(8)), MateResult::NoMate);
        assert_eq!(principal_line(&p, 8, BIG), None);
        let d = best_defence(&p, 8, BIG);
        if p.is_checkmate() {
            assert_eq!(d, DefenceResult::Mated);
        } else {
            assert_eq!(d, DefenceResult::Escapes);
        }
    }
}

// ---- defences a solver could forget ---------------------------------------

#[test]
fn en_passant_is_a_defence() {
    // Black king on g5 is in check from the pawn that just moved f2-f4;
    // ...exf3 e.p. is legal and resolves the check.
    let p = pos("8/8/8/6k1/4pP2/8/8/K7 b - f3 0 1");
    assert!(
        p.legal_moves().iter().any(|m| p.uci(*m) == "e4f3"),
        "en passant must be legal"
    );
    // With the en passant capture the defender is never mated at once.
    assert_ne!(best_defence(&p, 1, BIG), DefenceResult::Mated);
}

#[test]
fn underpromotion_to_a_knight_is_found_among_the_attacker_moves() {
    let p = pos("rb6/kpP5/p7/8/8/8/8/7K w - - 0 1");
    let result = find_mate(&p, lim(1));
    let MateResult::Mate { first, .. } = result else {
        panic!("mate expected, got {result:?}");
    };
    assert_eq!(p.uci(first), "c7c8n");
}

#[test]
fn castling_defence_is_considered() {
    // White threatens Qd8+ style ideas; Black can castle. Only the contract
    // that the answer is a valid, internally consistent one is checked here:
    // the principal line (if any) is legal and ends in mate.
    let p = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    let r = find_mate(&p, lim(2));
    if let MateResult::Mate { moves, first } = r {
        assert!(p.is_legal(first));
        let line = principal_line(&p, moves, BIG).expect("line");
        assert!(play_all(&p, &line).is_checkmate());
    }
}

#[test]
fn double_check_pinned_and_kingless_defender_positions_agree_with_brute_force() {
    for fen in [
        // double check: only a king move
        "4k3/8/8/1B6/8/4R3/8/4K3 b - - 0 1",
        // pinned defender
        "4k3/4r3/8/8/8/8/4R3/4K3 w - - 0 1",
        // defender king has no moves but is not in check
        "7k/5Q2/8/8/8/8/8/K7 b - - 0 1",
    ] {
        let p = pos(fen);
        for n in 1..=2 {
            let found = match find_mate(&p, lim(n)) {
                MateResult::Mate { moves, .. } => Some(moves),
                MateResult::NoMate => None,
                MateResult::Unknown => panic!("unexpected Unknown"),
                _ => panic!("unexpected variant"),
            };
            assert_eq!(found.is_some(), brute(&p, n), "{fen} n={n}");
        }
    }
}

// ---- determinism and many mates --------------------------------------------

#[test]
fn many_mates_give_the_same_first_move_every_time() {
    let p = pos("7k/8/5K2/8/8/8/8/QQ6 w - - 0 1");
    let a = find_mate(&p, lim(2));
    let b = find_mate(&p, lim(2));
    assert_eq!(a, b);
    let MateResult::Mate { moves, first } = a else {
        panic!("mate expected");
    };
    assert_eq!(moves, 1);
    assert!(p.after(first).expect("legal").is_checkmate());
    assert_eq!(principal_line(&p, 2, BIG), principal_line(&p, 2, BIG));
}

#[test]
fn best_defence_is_deterministic_and_legal() {
    let p = pos("7k/8/8/8/8/8/R7/1R4K1 w - - 0 1");
    let after = p.after(p.parse_uci("a2a7").expect("uci")).expect("legal");
    let a = best_defence(&after, 2, BIG);
    assert_eq!(a, best_defence(&after, 2, BIG));
    match a {
        DefenceResult::Forced { moves, reply } => {
            assert_eq!(moves, 1);
            assert!(after.is_legal(reply));
        }
        other => panic!("expected a forced mate, got {other:?}"),
    }
}

// ---- budgets: Unknown, never a false NoMate ---------------------------------

#[test]
fn a_small_budget_never_turns_a_known_mate_into_no_mate() {
    let known = [
        ("7k/8/8/8/8/8/R7/1R4K1 w - - 0 1", 2u8),
        ("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", 1),
        ("4kb1r/p2n1ppp/4q3/4p1B1/4P3/1Q6/PPP2PPP/2KR4 w k - 1 1", 2),
    ];
    for (fen, n) in known {
        let p = pos(fen);
        for nodes in [
            0u64, 1, 2, 3, 5, 8, 13, 21, 50, 100, 300, 1000, 5000, 30_000,
        ] {
            match find_mate(
                &p,
                MateLimits {
                    max_moves: 4,
                    max_nodes: nodes,
                },
            ) {
                MateResult::Mate { moves, first } => {
                    assert_eq!(moves, n, "{fen} with {nodes} nodes");
                    assert!(p.is_legal(first));
                }
                MateResult::Unknown => {}
                other => panic!("{fen} with {nodes} nodes gave {other:?}"),
            }
        }
    }
}

#[test]
fn a_small_budget_never_invents_a_mate_in_a_drawn_position() {
    let p = pos("8/8/8/4k3/8/8/8/R3K3 w - - 0 1");
    for nodes in [0u64, 1, 10, 100, 1000, 10_000] {
        let r = find_mate(
            &p,
            MateLimits {
                max_moves: 2,
                max_nodes: nodes,
            },
        );
        assert!(
            matches!(r, MateResult::NoMate | MateResult::Unknown),
            "{r:?}"
        );
    }
}

#[test]
fn principal_line_and_best_defence_respect_tiny_budgets() {
    let p = pos("7k/8/8/8/8/8/R7/1R4K1 w - - 0 1");
    assert_eq!(principal_line(&p, 3, 0), None);
    assert_eq!(best_defence(&p, 3, 0), DefenceResult::Unknown);
}

// ---- hostile FENs ----------------------------------------------------------

#[rstest]
#[case("8/8/8/8/8/8/8/8 w - - 0 1")] // no kings
#[case("4k3/8/8/8/8/8/8/8 w - - 0 1")] // no white king
#[case("4k2k/8/8/8/8/8/8/4K3 w - - 0 1")] // two black kings
#[case("4k3/8/8/8/8/8/8/3KK3 w - - 0 1")] // two white kings
#[case("P3k3/8/8/8/8/8/8/4K3 w - - 0 1")] // pawn on the 8th rank
#[case("4k3/8/8/8/8/8/8/p3K3 w - - 0 1")] // pawn on the 1st rank
#[case("4k3/8/8/8/8/8/4R3/4K3 w - - 0 1")] // black is in check, white to move
#[case("4k3/8/8/8/8/8/8/4K2r w - - 0 1")] // white king in check, ok
#[case("4k3/4K3/8/8/8/8/8/8 w - - 0 1")] // adjacent kings
#[case("")]
#[case("not a fen")]
fn hostile_fens_never_panic(#[case] fen: &str) {
    if let Ok(p) = Position::from_fen(fen) {
        let _ = find_mate(&p, lim(3));
        drop(principal_line(&p, 3, 100_000));
        let _ = best_defence(&p, 3, 100_000);
    }
}

#[test]
fn chess960_positions_are_solved_like_any_other() {
    // Every one of the 960 starts has no mate in 2 (nobody can mate that
    // fast from the initial array).
    for n in [0u16, 1, 518, 959] {
        let p = Position::chess960_start(n).expect("valid index");
        assert_eq!(find_mate(&p, lim(2)), MateResult::NoMate, "#{n}");
    }
}

#[test]
fn chess960_castling_does_not_break_the_search() {
    let p = Position::from_fen_with(
        "1r2k1rb/8/8/8/8/8/8/1R2K1RB w GBgb - 0 1",
        mychess_core::FenMode::Chess960,
    );
    if let Ok(p) = p {
        let r = find_mate(&p, lim(2));
        if let MateResult::Mate { moves, first } = r {
            assert!(p.is_legal(first));
            let line = principal_line(&p, moves, BIG).expect("line");
            assert!(play_all(&p, &line).is_checkmate());
        }
    }
}

// ---- brute-force helper (independent of the solver) --------------------------

fn brute(p: &Position, n: u8) -> bool {
    if n == 0 {
        return false;
    }
    p.legal_moves().into_iter().any(|m| {
        let next = p.after(m).expect("legal");
        if next.is_checkmate() {
            return true;
        }
        if n == 1 || next.is_stalemate() {
            return false;
        }
        next.legal_moves()
            .into_iter()
            .all(|r| brute(&next.after(r).expect("legal"), n - 1))
    })
}
