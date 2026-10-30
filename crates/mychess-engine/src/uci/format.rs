//! The commands the interface writes to a UCI engine.

use std::fmt::Write as _;
use std::time::Duration;

use mychess_core::{CastlingMode, FenCastlingStyle, Position, Variant};

use crate::request::{SearchPosition, SearchRequest};

const STARTPOS_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// A `position` command and the position it describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PositionPlan {
    /// The command line.
    pub(crate) command: String,
    /// The position after all the moves.
    pub(crate) current: Position,
}

/// Builds `position startpos|fen ... [moves ...]` (spec m4-1, D406).
///
/// - the standard start position uses `startpos`; anything else `fen` (with
///   Shredder-FEN castling in Chess960);
/// - when the path has a null move the command carries the current FEN and no
///   moves, as the Scid does;
/// - Chess960 castling travels as "king takes rook".
///
/// Fails, with a description, when a move is illegal.
pub(crate) fn position_command(position: &SearchPosition) -> Result<PositionPlan, String> {
    let chess960 = position.start.variant() == Variant::Chess960;
    let style = if chess960 {
        FenCastlingStyle::Shredder
    } else {
        FenCastlingStyle::XFen
    };
    let mode = position.start.castling_mode();
    let mut current = position.start.clone();
    let mut texts = Vec::with_capacity(position.moves.len());
    for m in &position.moves {
        texts.push(m.to_uci(mode));
        current.play(*m).map_err(|e| e.to_string())?;
    }
    let command = if position.moves.iter().any(|m| m.is_null()) {
        format!("position fen {}", current.fen_with_style(style))
    } else {
        let head = if !chess960 && position.start.fen() == STARTPOS_FEN {
            "position startpos".to_owned()
        } else {
            format!("position fen {}", position.start.fen_with_style(style))
        };
        if texts.is_empty() {
            head
        } else {
            format!("{head} moves {}", texts.join(" "))
        }
    };
    Ok(PositionPlan { command, current })
}

fn millis(d: Duration) -> u128 {
    d.as_millis()
}

/// Builds the `go` command. `searchmoves` goes last; with no limit the
/// search is `infinite`.
pub(crate) fn go_command(request: &SearchRequest, mode: CastlingMode) -> String {
    let mut out = String::from("go");
    if request.ponder {
        out.push_str(" ponder");
    }
    let limits = &request.limits;
    let mut limited = false;
    if let Some(c) = &limits.clock {
        let _ = write!(
            out,
            " wtime {} btime {} winc {} binc {}",
            millis(c.white_time),
            millis(c.black_time),
            millis(c.white_increment),
            millis(c.black_increment)
        );
        if let Some(n) = c.moves_to_go {
            let _ = write!(out, " movestogo {n}");
        }
        limited = true;
    }
    if let Some(d) = limits.depth {
        let _ = write!(out, " depth {d}");
        limited = true;
    }
    if let Some(n) = limits.nodes {
        let _ = write!(out, " nodes {n}");
        limited = true;
    }
    if let Some(n) = limits.mate {
        let _ = write!(out, " mate {n}");
        limited = true;
    }
    if let Some(t) = limits.movetime {
        let _ = write!(out, " movetime {}", millis(t));
        limited = true;
    }
    if limits.infinite || !limited {
        out.push_str(" infinite");
    }
    if !request.search_moves.is_empty() {
        out.push_str(" searchmoves");
        for m in &request.search_moves {
            out.push(' ');
            out.push_str(&m.to_uci(mode));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{Clock, SearchLimits};

    fn mv(pos: &Position, text: &str) -> mychess_core::Move {
        pos.parse_uci(text).unwrap()
    }

    #[test]
    fn startpos_with_moves() {
        let start = Position::new();
        let e4 = mv(&start, "e2e4");
        let after = start.after(e4).unwrap();
        let e5 = mv(&after, "e7e5");
        let plan = position_command(&SearchPosition {
            start,
            moves: vec![e4, e5],
        })
        .unwrap();
        assert_eq!(plan.command, "position startpos moves e2e4 e7e5");
    }

    #[test]
    fn other_start_uses_fen_and_null_move_uses_current_fen() {
        let start = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
        let e4 = mv(&start, "e2e4");
        let plan = position_command(&SearchPosition {
            start: start.clone(),
            moves: vec![e4],
        })
        .unwrap();
        assert_eq!(
            plan.command,
            "position fen 4k3/8/8/8/8/8/4P3/4K3 w - - 0 1 moves e2e4"
        );
        let after = start.after(e4).unwrap();
        let plan = position_command(&SearchPosition {
            start,
            moves: vec![e4, mychess_core::Move::Null],
        })
        .unwrap();
        assert_eq!(plan.current.turn(), !after.turn());
        assert!(plan.command.starts_with("position fen ") && !plan.command.contains("moves"));
    }

    #[test]
    fn chess960_castling_is_king_takes_rook() {
        let start = Position::from_fen("4k3/8/8/8/8/8/8/1R1RK3 w D - 0 1").unwrap();
        assert_eq!(start.variant(), Variant::Chess960);
        let castle = mv(&start, "e1d1");
        let plan = position_command(&SearchPosition {
            start,
            moves: vec![castle],
        })
        .unwrap();
        assert!(plan.command.ends_with("moves e1d1"), "{}", plan.command);
        assert!(
            plan.command
                .starts_with("position fen 4k3/8/8/8/8/8/8/1R1RK3 w D"),
            "{}",
            plan.command
        );
    }

    #[test]
    fn illegal_move_is_an_error() {
        let start = Position::new();
        let other = Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
        let bad = mv(&other, "a1a8");
        assert!(
            position_command(&SearchPosition {
                start,
                moves: vec![bad]
            })
            .is_err()
        );
    }

    #[test]
    fn go_variants() {
        let pos = SearchPosition::new(Position::new());
        let mut req = SearchRequest::infinite(pos.clone());
        assert_eq!(go_command(&req, CastlingMode::Standard), "go infinite");
        req.limits = SearchLimits {
            depth: Some(8),
            movetime: Some(Duration::from_millis(200)),
            ..SearchLimits::default()
        };
        let e4 = mv(&pos.start, "e2e4");
        let d4 = mv(&pos.start, "d2d4");
        req.search_moves = vec![e4, d4];
        assert_eq!(
            go_command(&req, CastlingMode::Standard),
            "go depth 8 movetime 200 searchmoves e2e4 d2d4"
        );
        req.search_moves.clear();
        req.ponder = true;
        req.limits = SearchLimits {
            clock: Some(Clock {
                white_time: Duration::from_secs(60),
                black_time: Duration::from_secs(59),
                white_increment: Duration::from_secs(1),
                black_increment: Duration::ZERO,
                moves_to_go: Some(30),
            }),
            ..SearchLimits::default()
        };
        assert_eq!(
            go_command(&req, CastlingMode::Standard),
            "go ponder wtime 60000 btime 59000 winc 1000 binc 0 movestogo 30"
        );
    }
}
