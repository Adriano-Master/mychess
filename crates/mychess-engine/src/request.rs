//! What the application asks an engine to search.

use std::time::Duration;

use mychess_core::{Move, Position};

/// The position to search: a start position and the moves played from it.
///
/// The moves are the whole path from the start to the cursor, variations
/// included, so the engine sees repetitions (spec m4-1, D406).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPosition {
    /// The position the game started from (standard or Chess960).
    pub start: Position,
    /// The moves played from `start`. A null move is allowed; the position is
    /// then sent as a FEN without moves.
    pub moves: Vec<Move>,
}

impl SearchPosition {
    /// A position without moves.
    pub fn new(start: Position) -> SearchPosition {
        SearchPosition {
            start,
            moves: Vec::new(),
        }
    }
}

/// Clock state for a timed search (`go wtime ... btime ...`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Clock {
    /// Time left for White.
    pub white_time: Duration,
    /// Time left for Black.
    pub black_time: Duration,
    /// White's increment per move.
    pub white_increment: Duration,
    /// Black's increment per move.
    pub black_increment: Duration,
    /// Moves to the next time control.
    pub moves_to_go: Option<u32>,
}

/// How long or how deep to search. With no limit at all the search is
/// infinite (the Scid's analysis default, `go infinite`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchLimits {
    /// Search to this depth in plies.
    pub depth: Option<u32>,
    /// Search this many nodes.
    pub nodes: Option<u64>,
    /// Search this long.
    pub movetime: Option<Duration>,
    /// Look for a mate in this many moves.
    pub mate: Option<u32>,
    /// Search with a clock.
    pub clock: Option<Clock>,
    /// Search until stopped, in addition to any other limit.
    pub infinite: bool,
}

/// A request to search one position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    /// The position.
    pub position: SearchPosition,
    /// The limits.
    pub limits: SearchLimits,
    /// The moves the engine may choose among (`searchmoves`); empty means no
    /// restriction. The application computes "all moves but the excluded".
    pub search_moves: Vec<Move>,
    /// Start in pondering mode (`go ponder`).
    pub ponder: bool,
    /// An opaque label of the front-end, given back in
    /// [`EngineEvent::SearchTagged`](crate::EngineEvent::SearchTagged) when
    /// this very request starts. `0` means "no label" and emits nothing.
    /// A request that is replaced before it starts never reports its tag.
    pub tag: u64,
}

impl SearchRequest {
    /// An infinite search of `position`.
    pub fn infinite(position: SearchPosition) -> SearchRequest {
        SearchRequest {
            position,
            limits: SearchLimits::default(),
            search_moves: Vec::new(),
            ponder: false,
            tag: 0,
        }
    }
}
