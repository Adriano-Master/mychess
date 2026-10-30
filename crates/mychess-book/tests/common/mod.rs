//! Shared helpers for the contract tests (not a test target by itself).
#![allow(dead_code)]

use mychess_book::{Book, Entry, encode_move, polyglot_key};
use mychess_core::{Move, Position};

/// The position reached from the start by the given UCI moves.
pub(crate) fn pos_after(ucis: &[&str]) -> Position {
    let mut pos = Position::new();
    for u in ucis {
        let m = pos.parse_uci(u).expect("legal uci move");
        pos.play(m).expect("play");
    }
    pos
}

/// The moves of a line from the standard start, parsed from UCI.
pub(crate) fn line(ucis: &[&str]) -> Vec<Move> {
    let mut pos = Position::new();
    ucis.iter()
        .map(|u| {
            let m = pos.parse_uci(u).expect("legal uci move");
            pos.play(m).expect("play");
            m
        })
        .collect()
}

pub(crate) fn mv(pos: &Position, uci: &str) -> Move {
    pos.parse_uci(uci).expect("legal uci move")
}

/// Polyglot code of a legal move given in UCI.
pub(crate) fn code(pos: &Position, uci: &str) -> u16 {
    encode_move(mv(pos, uci)).expect("encodable")
}

pub(crate) fn entry(key: u64, mv: u16, weight: u16) -> Entry {
    Entry {
        key,
        mv,
        weight,
        learn: 0,
    }
}

pub(crate) fn key_of(ucis: &[&str]) -> u64 {
    polyglot_key(&pos_after(ucis))
}

/// A book with the given (uci, weight) pairs for `pos`, in this order.
pub(crate) fn book_with(pos: &Position, moves: &[(&str, u16)]) -> Book {
    let key = polyglot_key(pos);
    Book::from_entries(
        moves
            .iter()
            .map(|(u, w)| entry(key, code(pos, u), *w))
            .collect(),
    )
}

/// `(key, mv, weight)` triples of a book, for compact assertions.
pub(crate) fn triples(book: &Book) -> Vec<(u64, u16, u16)> {
    book.entries()
        .iter()
        .map(|e| (e.key, e.mv, e.weight))
        .collect()
}

pub(crate) fn is_sorted_by_key(book: &Book) -> bool {
    book.entries().windows(2).all(|w| w[0].key <= w[1].key)
}

/// Deterministic pseudo-random walk of legal moves from the start.
/// Returns every position visited (including the start).
pub(crate) fn random_game(seeds: &[u16]) -> Vec<Position> {
    let mut pos = Position::new();
    let mut out = vec![pos.clone()];
    for s in seeds {
        let legal = pos.legal_moves();
        if legal.is_empty() {
            break;
        }
        let m = legal[usize::from(*s) % legal.len()];
        pos.play(m).expect("legal move plays");
        out.push(pos.clone());
    }
    out
}
