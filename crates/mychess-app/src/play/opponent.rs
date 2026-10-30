//! The opponent of the player, seen from the controller (spec M8-1, D1244).
//!
//! The controller does not know engines, processes or the network. It asks
//! the opponent to think with a [`Think`] request and is told the answer
//! with `PlayController::opponent_moved`. Today the only opponent is a
//! local UCI engine (the glue in `state::play_flow` turns a [`Think`] into
//! a [`SearchRequest`]); the tournaments (M8-2) and Lichess (M8-3) will
//! speak the same two messages. The clock and the end-of-game rules stay in
//! the controller, so they are shared.

use std::time::Duration;

use mychess_core::{Color, Move, Position};
use mychess_engine::{Clock, SearchLimits, SearchPosition, SearchRequest};

use super::config::TimeMode;

/// Who the opponent is, for the tags and the player strips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpponentInfo {
    /// The name written in the `White` or `Black` tag.
    pub name: String,
    /// The rating shown with the name, if known.
    pub rating: Option<u16>,
}

/// How long or how deep the opponent may think.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkLimits {
    /// With the clock of the game (`go wtime ... btime ... winc ... binc`).
    Clock(Clock),
    /// `go depth`.
    Depth(u32),
    /// `go nodes`.
    Nodes(u64),
    /// `go movetime`.
    MoveTime(Duration),
}

/// A request to the opponent: "play in this position".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Think {
    /// The position the game started from.
    pub start: Position,
    /// The moves played since, so the opponent sees the repetitions.
    pub moves: Vec<Move>,
    /// The side the opponent plays.
    pub side: Color,
    /// The limits.
    pub limits: ThinkLimits,
}

impl Think {
    /// The UCI search for this request (D1238). The order of the `position`
    /// and `go` commands and the wait for `readyok` between a changed
    /// option and the `position` are the engine session's business
    /// (`mychess-engine`), which queues the search behind every pending
    /// `isready`.
    pub fn to_request(&self, tag: u64) -> SearchRequest {
        let mut request = SearchRequest::infinite(SearchPosition {
            start: self.start.clone(),
            moves: self.moves.clone(),
        });
        let mut limits = SearchLimits::default();
        match self.limits {
            ThinkLimits::Clock(clock) => limits.clock = Some(clock),
            ThinkLimits::Depth(depth) => limits.depth = Some(depth),
            ThinkLimits::Nodes(nodes) => limits.nodes = Some(nodes),
            ThinkLimits::MoveTime(time) => limits.movetime = Some(time),
        }
        request.limits = limits;
        request.tag = tag;
        request
    }
}

/// The limits for `mode`, with `clock` already read at the moment the
/// opponent starts to think.
pub(crate) fn limits_for(mode: TimeMode, clock: Clock) -> ThinkLimits {
    match mode.normalized() {
        TimeMode::TimeBonus { .. } => ThinkLimits::Clock(clock),
        TimeMode::Depth(n) => ThinkLimits::Depth(n),
        TimeMode::Nodes(n) => ThinkLimits::Nodes(u64::from(n)),
        TimeMode::MoveTime(n) => ThinkLimits::MoveTime(Duration::from_secs(u64::from(n))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn think(limits: ThinkLimits) -> Think {
        Think {
            start: Position::new(),
            moves: Vec::new(),
            side: Color::White,
            limits,
        }
    }

    #[test]
    fn each_mode_makes_its_own_go() {
        let clock = Clock {
            white_time: Duration::from_secs(300),
            black_time: Duration::from_secs(290),
            white_increment: Duration::from_secs(3),
            black_increment: Duration::from_secs(2),
            moves_to_go: None,
        };
        let bonus = TimeMode::TimeBonus {
            white_minutes: 5,
            white_bonus: 3,
            black_minutes: 5,
            black_bonus: 2,
        };
        let r = think(limits_for(bonus, clock)).to_request(9);
        assert_eq!(r.limits.clock, Some(clock));
        assert_eq!(
            (r.limits.depth, r.limits.nodes, r.limits.movetime),
            (None, None, None)
        );
        assert_eq!(r.tag, 9);

        let r = think(limits_for(TimeMode::Depth(3), clock)).to_request(1);
        assert_eq!(r.limits.depth, Some(3));
        assert!(r.limits.clock.is_none());

        let r = think(limits_for(TimeMode::Nodes(1000), clock)).to_request(1);
        assert_eq!(r.limits.nodes, Some(1000));

        let r = think(limits_for(TimeMode::MoveTime(6), clock)).to_request(1);
        assert_eq!(r.limits.movetime, Some(Duration::from_millis(6000)));
        assert!(!r.limits.infinite);
    }

    #[test]
    fn the_request_carries_the_whole_game() {
        let start = Position::new();
        let e4 = start.parse_uci("e2e4").expect("uci");
        let t = Think {
            moves: vec![e4],
            side: Color::Black,
            ..think(ThinkLimits::Depth(2))
        };
        let r = t.to_request(1);
        assert_eq!(r.position.moves, [e4]);
        assert_eq!(r.position.start, start);
    }
}
