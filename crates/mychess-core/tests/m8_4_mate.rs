//! Contract tests of the forced-mate search (spec M8-4, D1906-D1909): the
//! answers are checked against a naive, independent oracle and against the
//! rules of the game, not against the search's own internals.

use mychess_core::mate::{
    DefenceResult, MateLimits, MateResult, best_defence, find_mate, principal_line,
};
use mychess_core::{Move, Position};

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).expect("test fen")
}

/// Naive oracle: does the side to move force mate in at most `n` moves?
fn oracle_attack(p: &Position, n: u8) -> bool {
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
            .all(|r| oracle_attack(&next.after(r).expect("legal"), n - 1))
    })
}

fn oracle_distance(p: &Position, max: u8) -> Option<u8> {
    (1..=max).find(|n| oracle_attack(p, *n))
}

fn limits(max_moves: u8) -> MateLimits {
    MateLimits {
        max_moves,
        max_nodes: 20_000_000,
    }
}

fn distance(p: &Position, max: u8) -> Option<u8> {
    match find_mate(p, limits(max)) {
        MateResult::Mate { moves, .. } => Some(moves),
        MateResult::NoMate => None,
        other => panic!("unexpected {other:?} for {}", p.fen()),
    }
}

/// Compares the search with the oracle on `board`; returns the distance
/// found, or `None` when the position was skipped or has no mate within 3.
fn compare(board: &[(&str, char)]) -> (bool, Option<u8>) {
    let Some(fen) = build_fen(board) else {
        return (false, None);
    };
    // White to move with Black in check is not a legal position.
    let Ok(p) = Position::from_fen(&fen) else {
        return (false, None);
    };
    if p.legal_moves().is_empty() {
        return (false, None);
    }
    let found = distance(&p, 3);
    assert_eq!(
        found,
        oracle_distance(&p, 3),
        "mate distance differs for {fen}"
    );
    (true, found)
}

#[test]
fn agrees_with_the_naive_oracle_on_king_and_queen_endings() {
    let mut checked = 0;
    for queen in ["a1", "b2", "d4", "g7", "h3", "e8", "f5", "c8", "g2", "a6"] {
        for black_king in ["a8", "h8", "h1", "e4", "c6", "g5", "h5", "a4", "f8", "h6"] {
            let (ok, _) = compare(&[("c3", 'K'), (queen, 'Q'), (black_king, 'k')]);
            checked += usize::from(ok);
        }
    }
    assert!(checked >= 40, "only {checked} positions were comparable");
}

#[test]
fn agrees_with_the_naive_oracle_on_two_rook_endings_and_finds_a_mate_in_three() {
    let mut checked = 0;
    let mut threes = 0;
    for (r1, r2) in [
        ("b1", "h2"),
        ("a1", "h2"),
        ("g1", "h2"),
        ("e1", "d2"),
        ("a2", "b3"),
    ] {
        for black_king in ["a8", "h8", "e5", "d7", "g6", "b6", "h4"] {
            let (ok, found) = compare(&[("c4", 'K'), (r1, 'R'), (r2, 'R'), (black_king, 'k')]);
            checked += usize::from(ok);
            threes += usize::from(found == Some(3));
        }
    }
    assert!(checked >= 20, "only {checked} positions were comparable");
    assert!(threes >= 1, "no mate in three among {checked} positions");
}

/// A FEN with White to move from `(square, piece)` pairs, or `None` when two
/// pieces share a square or the kings touch.
fn build_fen(pieces: &[(&str, char)]) -> Option<String> {
    let mut grid = [[' '; 8]; 8];
    for (square, piece) in pieces {
        let bytes = square.as_bytes();
        let file = usize::from(bytes[0] - b'a');
        let rank = usize::from(bytes[1] - b'1');
        if grid[rank][file] != ' ' {
            return None;
        }
        grid[rank][file] = *piece;
    }
    let find = |c: char| {
        (0..8)
            .flat_map(|r| (0..8).map(move |f| (r, f)))
            .find(|&(r, f)| grid[r][f] == c)
    };
    let (wr, wf) = find('K')?;
    let (br, bf) = find('k')?;
    if wr.abs_diff(br) <= 1 && wf.abs_diff(bf) <= 1 {
        return None;
    }
    let mut out = String::new();
    for rank in (0..8).rev() {
        let mut empty = 0;
        for c in grid[rank] {
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
    out.push_str(" w - - 0 1");
    Some(out)
}

#[test]
fn the_principal_line_is_a_legal_sequence_ending_in_checkmate() {
    for fen in [
        "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1",
        "kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1",
        "7k/8/6K1/8/8/8/8/5Q2 w - - 0 1",
    ] {
        let p = pos(fen);
        let MateResult::Mate { moves, .. } = find_mate(&p, limits(3)) else {
            panic!("no mate in {fen}");
        };
        let line = principal_line(&p, 3, 5_000_000).expect("line");
        assert_eq!(line.len(), 2 * usize::from(moves) - 1, "{fen}");
        let mut cur = p;
        for (i, m) in line.iter().enumerate() {
            assert!(cur.is_legal(*m), "move {i} of {fen} is illegal");
            cur.play(*m).expect("legal");
        }
        assert!(cur.is_checkmate(), "{fen} does not end in mate");
    }
}

#[test]
fn the_first_move_forces_the_distance_the_search_announced() {
    let p = pos("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1");
    let MateResult::Mate { moves, first } = find_mate(&p, limits(3)) else {
        panic!("mate expected");
    };
    let after = p.after(first).expect("legal");
    match best_defence(&after, 3, 5_000_000) {
        DefenceResult::Forced { moves: left, .. } => assert_eq!(left, moves - 1),
        other => panic!("expected a forced mate after the first move, got {other:?}"),
    }
}

#[test]
fn a_wrong_first_move_lets_the_defender_escape() {
    let p = pos("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1");
    // 1.Ra2 does not force mate in two.
    let wrong = p.parse_uci("a1a2").expect("uci");
    let after = p.after(wrong).expect("legal");
    assert_eq!(best_defence(&after, 1, 5_000_000), DefenceResult::Escapes);
}

#[test]
fn the_same_input_gives_the_same_answer() {
    let p = pos("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1");
    let a: Vec<Move> = principal_line(&p, 3, 5_000_000).expect("line");
    let b: Vec<Move> = principal_line(&p, 3, 5_000_000).expect("line");
    assert_eq!(a, b);
}

#[test]
fn a_budget_of_zero_nodes_is_unknown_not_a_panic() {
    let p = pos("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
    let result = find_mate(
        &p,
        MateLimits {
            max_moves: 3,
            max_nodes: 0,
        },
    );
    assert_eq!(result, MateResult::Unknown);
    assert_eq!(best_defence(&p, 3, 0), DefenceResult::Unknown);
}

#[test]
fn zero_moves_asked_means_no_mate() {
    let p = pos("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
    assert_eq!(find_mate(&p, limits(0)), MateResult::NoMate);
}
