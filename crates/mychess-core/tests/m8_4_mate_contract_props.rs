//! Property tests of the forced-mate solver against brute force (spec M8-4).

use mychess_core::Position;
use mychess_core::mate::{
    DefenceResult, MateLimits, MateResult, best_defence, find_mate, principal_line,
};
use proptest::prelude::*;

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

fn fen_of(pieces: &[(u8, char)], white_to_move: bool) -> Option<String> {
    let mut grid = [' '; 64];
    for (sq, c) in pieces {
        if grid[usize::from(*sq)] != ' ' {
            return None;
        }
        grid[usize::from(*sq)] = *c;
    }
    let mut out = String::new();
    for rank in (0..8usize).rev() {
        let mut empty = 0;
        for file in 0..8usize {
            let c = grid[rank * 8 + file];
            if c == ' ' {
                empty += 1;
            } else {
                if empty > 0 {
                    out.push_str(&empty.to_string());
                    empty = 0;
                }
                out.push(c);
            }
        }
        if empty > 0 {
            out.push_str(&empty.to_string());
        }
        if rank > 0 {
            out.push('/');
        }
    }
    out.push_str(if white_to_move {
        " w - - 0 1"
    } else {
        " b - - 0 1"
    });
    Some(out)
}

fn extra() -> impl Strategy<Value = (u8, char)> {
    (
        0u8..64,
        prop::sample::select(vec!['Q', 'R', 'B', 'N', 'P', 'q', 'r', 'b', 'n', 'p']),
    )
}

fn position() -> impl Strategy<Value = Position> {
    (
        0u8..64,
        0u8..64,
        prop::collection::vec(extra(), 1..=3),
        any::<bool>(),
    )
        .prop_filter_map("illegal position", |(wk, bk, more, white)| {
            let mut pieces = vec![(wk, 'K'), (bk, 'k')];
            pieces.extend(more);
            let fen = fen_of(&pieces, white)?;
            let p = Position::from_fen(&fen).ok()?;
            (!p.legal_moves().is_empty()).then_some(p)
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(150))]

    #[test]
    fn solver_agrees_with_brute_force_and_the_line_is_playable(p in position()) {
        for n in 1..=2u8 {
            let r = find_mate(&p, MateLimits { max_moves: n, max_nodes: 50_000_000 });
            let expected = brute(&p, n);
            match r {
                MateResult::Mate { moves, first } => {
                    prop_assert!(expected, "solver claims mate in {} for {}", moves, p.fen());
                    prop_assert!(moves <= n);
                    prop_assert!(p.is_legal(first));
                    if moves > 1 { prop_assert!(!brute(&p, moves - 1)); }
                    let line = principal_line(&p, n, 50_000_000).expect("line");
                    prop_assert_eq!(line.len(), 2 * usize::from(moves) - 1);
                    let mut cur = p.clone();
                    for m in &line {
                        prop_assert!(cur.is_legal(*m));
                        cur.play(*m).expect("legal");
                    }
                    prop_assert!(cur.is_checkmate());
                }
                MateResult::NoMate => prop_assert!(!expected, "missed a mate in {} for {}", n, p.fen()),
                other => prop_assert!(false, "unexpected {:?}", other),
            }
        }
    }

    #[test]
    fn small_budgets_never_contradict_the_full_answer(p in position(), nodes in 0u64..400) {
        let full = find_mate(&p, MateLimits { max_moves: 2, max_nodes: 50_000_000 });
        let small = find_mate(&p, MateLimits { max_moves: 2, max_nodes: nodes });
        prop_assert!(small == full || small == MateResult::Unknown
            || matches!((small, full), (MateResult::Mate { moves: a, .. }, MateResult::Mate { moves: b, .. }) if a == b),
            "{:?} vs {:?} for {}", small, full, p.fen());
    }

    #[test]
    fn best_defence_agrees_with_brute_force(p in position()) {
        // Judge the position as if its side to move were the defender.
        let d = best_defence(&p, 2, 50_000_000);
        let all_lose = |n: u8| !p.legal_moves().is_empty() && p.legal_moves().into_iter()
            .all(|m| brute(&p.after(m).expect("legal"), n));
        match d {
            DefenceResult::Forced { moves, reply } => {
                prop_assert!(all_lose(moves), "{}", p.fen());
                prop_assert!(p.is_legal(reply));
            }
            DefenceResult::Escapes => {
                prop_assert!(!all_lose(1) && !all_lose(2), "{}", p.fen());
            }
            DefenceResult::Mated => prop_assert!(p.is_checkmate()),
            other => prop_assert!(false, "unexpected {:?}", other),
        }
    }
}
