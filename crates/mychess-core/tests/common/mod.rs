//! Shared helpers for the integration tests (public API only).
#![allow(dead_code)]

use mychess_core::{Move, Position, Square};

/// Counts leaf nodes at `depth` using only `legal_moves` and `after`.
pub(crate) fn perft(pos: &Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = pos.legal_moves();
    if depth == 1 {
        return moves.len() as u64;
    }
    moves
        .into_iter()
        .map(|m| {
            perft(
                &pos.after(m).expect("generated move must be playable"),
                depth - 1,
            )
        })
        .sum()
}

pub(crate) fn fen(s: &str) -> Position {
    Position::from_fen(s).unwrap_or_else(|e| panic!("bad test FEN {s:?}: {e}"))
}

pub(crate) fn sq(s: &str) -> Square {
    s.parse().unwrap_or_else(|_| panic!("bad square {s}"))
}

/// Plays the choices (each reduced modulo the number of legal moves) from
/// `start`, returning every visited position, start included.
pub(crate) fn random_game(start: &Position, choices: &[u16]) -> Vec<(Position, Move)> {
    let mut pos = start.clone();
    let mut out = Vec::new();
    for c in choices {
        let moves = pos.legal_moves();
        if moves.is_empty() {
            break;
        }
        let m = moves[usize::from(*c) % moves.len()];
        out.push((pos.clone(), m));
        pos = pos.after(m).expect("legal move");
    }
    out
}

/// Number of entries in [`start_position`].
pub(crate) const START_COUNT: usize = 5;

/// Rich starting points for random games: the initial position, Kiwipete,
/// perft position 4, a promotion-heavy position and one with en passant.
pub(crate) fn start_position(index: usize) -> Position {
    const FENS: [&str; START_COUNT] = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "n1n5/PPPk4/8/8/8/8/4Kppp/5N1N b - - 0 1",
        "rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3",
    ];
    fen(FENS[index % START_COUNT])
}
