//! The raw game tree: what a `.sg4` record holds, before it is converted to a
//! [`mychess_core::Game`] (`docs/formatos/si4.md`, section 9).
//!
//! Variations live in a flat arena of lines (`RawGame::lines`), referenced by
//! index, so neither building, walking nor dropping the tree recurses.

use mychess_core::{Nag, PartialDate, Role, Square};

/// A move as stored in the record: squares and promotion piece only. The
/// null move is a king "moving" to its own square.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawMove {
    /// Origin square.
    pub from: Square,
    /// Destination square (for castling, the king's destination).
    pub to: Square,
    /// The promotion piece, if any.
    pub promotion: Option<Role>,
}

impl RawMove {
    /// Whether this is the null move (`from == to`).
    pub fn is_null(self) -> bool {
        self.from == self.to
    }
}

/// One move of a line with its annotations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawNode {
    /// The move.
    pub mv: RawMove,
    /// The NAGs of the move, at most 7, in order, without NAG 0.
    pub nags: Vec<Nag>,
    /// The comment after the move, as raw bytes.
    pub comment: Option<Vec<u8>>,
    /// Alternatives to this move: indices into [`RawGame::lines`], in order.
    pub variations: Vec<usize>,
}

/// A sequence of moves: the main line or a variation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawLine {
    /// The comment before the first move (for the main line, the comment of
    /// the game itself), as raw bytes.
    pub start_comment: Option<Vec<u8>>,
    /// The moves of the line.
    pub moves: Vec<RawNode>,
}

/// A decoded `.sg4` record. Text is kept as raw bytes (decision D256).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawGame {
    /// Extra tags as `(name, value)` raw bytes, in order, without repeated
    /// names (a repeated tag replaces the value of the first one). Common-tag
    /// codes (241-250) are already expanded to their names.
    pub extra_tags: Vec<(Vec<u8>, Vec<u8>)>,
    /// The event date from a tag-255 item, if the record has one.
    pub event_date: Option<PartialDate>,
    /// The record flags byte (bit 0: own start position; bit 1: promotions;
    /// bit 2: under-promotions).
    pub flags: u8,
    /// The FEN of the start position, if the record has its own.
    pub start_fen: Option<Vec<u8>>,
    /// All lines; `lines[0]` is the main line.
    pub lines: Vec<RawLine>,
}

impl RawGame {
    /// The main line.
    pub fn mainline(&self) -> &[RawNode] {
        self.lines.first().map_or(&[], |l| l.moves.as_slice())
    }

    /// The total number of moves in all lines.
    pub fn move_count(&self) -> usize {
        self.lines.iter().map(|l| l.moves.len()).sum()
    }
}
