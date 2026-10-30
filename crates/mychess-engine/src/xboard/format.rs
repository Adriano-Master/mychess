//! The commands the interface writes to an XBoard engine (spec m4-2,
//! D418-D419): position set-up and limits.

use std::time::Duration;

use mychess_core::{CastlingMode, Color, Move, Position, Variant};

use crate::request::{Clock, SearchLimits, SearchPosition};

const STARTPOS_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// The search time without a limit (Scid: `st 120000`, `analysis.tcl:4207`).
pub(crate) const DEFAULT_SECONDS: u64 = 120_000;
/// The search depth without a limit (Scid: `sd 50`).
pub(crate) const DEFAULT_DEPTH: u32 = 50;

/// A position as the engine needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    /// The moves in coordinate notation, from the start position.
    pub(crate) texts: Vec<String>,
    /// The moves themselves.
    pub(crate) moves: Vec<Move>,
    /// The position after all the moves: the root of the search.
    pub(crate) current: Position,
    /// Whether the start is the standard position (moves can be replayed).
    pub(crate) standard_start: bool,
    /// Whether the path has a null move.
    pub(crate) has_null: bool,
}

/// Replays the moves; fails, with a description, when one is illegal.
pub(crate) fn plan(position: &SearchPosition) -> Result<Plan, String> {
    let mode = position.start.castling_mode();
    let mut current = position.start.clone();
    let mut texts = Vec::with_capacity(position.moves.len());
    for m in &position.moves {
        texts.push(m.to_uci(mode));
        current.play(*m).map_err(|e| e.to_string())?;
    }
    Ok(Plan {
        texts,
        moves: position.moves.clone(),
        current,
        standard_start: is_standard_start(&position.start),
        has_null: position.moves.iter().any(|m| m.is_null()),
    })
}

fn is_standard_start(start: &Position) -> bool {
    start.variant() == Variant::Standard
        && start.castling_mode() == CastlingMode::Standard
        && start.fen() == STARTPOS_FEN
}

/// The longest move list replayed to an engine without `setboard`. Every
/// move is one line in the write queue (1024 lines): a longer game would
/// overflow it and end the session, so it is refused instead.
pub(crate) const MAX_REPLAY_MOVES: usize = 800;

/// Why an engine without `setboard` cannot take this position, if so: it
/// would need a start position other than the standard one, a null move, or
/// more moves than can be replayed.
pub(crate) fn unsupported_start(position: &SearchPosition) -> Option<&'static str> {
    if !is_standard_start(&position.start) {
        Some("non-standard start position")
    } else if position.moves.iter().any(|m| m.is_null()) {
        Some("null move in the move list")
    } else if position.moves.len() > MAX_REPLAY_MOVES {
        Some("game too long to replay to an engine without setboard")
    } else {
        None
    }
}

/// `st` and `sd` for a search that runs to a limit (Scid: `4207-4210`).
/// `st` is in whole seconds, rounded up, at least 1.
pub(crate) fn limit_commands(limits: &SearchLimits) -> [String; 2] {
    let seconds = limits.movetime.map_or(DEFAULT_SECONDS, |t| {
        let ms = t.as_millis();
        u64::try_from(ms.div_ceil(1000)).unwrap_or(u64::MAX).max(1)
    });
    let depth = limits.depth.unwrap_or(DEFAULT_DEPTH).max(1);
    [format!("st {seconds}"), format!("sd {depth}")]
}

/// `level`, `time` and `otim` for a search with a clock (spec M8-2c, D2522).
///
/// `level <mps> <base> <inc>` gives the engine its own remaining time as the
/// base (`m:ss`) and the increment in seconds (decimals when it is not a
/// whole number); `time` and `otim` are the remaining time of the engine and
/// of its opponent in centiseconds (at least 1). `side` is the side the
/// engine plays.
pub(crate) fn clock_commands(clock: &Clock, side: Color) -> [String; 3] {
    let (mine, theirs, increment) = match side {
        Color::White => (clock.white_time, clock.black_time, clock.white_increment),
        Color::Black => (clock.black_time, clock.white_time, clock.black_increment),
    };
    let base_secs = mine.as_secs().max(1);
    let inc_ms = increment.as_millis();
    let inc = if inc_ms % 1000 == 0 {
        (inc_ms / 1000).to_string()
    } else {
        let text = format!("{}.{:03}", inc_ms / 1000, inc_ms % 1000);
        text.trim_end_matches('0').to_owned()
    };
    let centis = |d: Duration| (d.as_millis() / 10).max(1);
    [
        format!(
            "level {} {}:{:02} {inc}",
            clock.moves_to_go.unwrap_or(0),
            base_secs / 60,
            base_secs % 60
        ),
        format!("time {}", centis(mine)),
        format!("otim {}", centis(theirs)),
    ]
}

/// A move for the engine: `usermove <m>` when it asked for it.
pub(crate) fn move_command(text: &str, usermove: bool) -> String {
    if usermove {
        format!("usermove {text}")
    } else {
        text.to_owned()
    }
}

/// `white` or `black`: the side to move (Scid: `analysis.tcl:4219`).
pub(crate) fn color_command(position: &Position) -> &'static str {
    match position.turn() {
        Color::White => "white",
        Color::Black => "black",
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn after(moves: &[&str]) -> SearchPosition {
        let start = Position::new();
        let mut pos = start.clone();
        let mut list = Vec::new();
        for text in moves {
            let m = pos.parse_uci(text).unwrap();
            pos.play(m).unwrap();
            list.push(m);
        }
        SearchPosition { start, moves: list }
    }

    #[test]
    fn plan_replays_the_moves() {
        let plan = plan(&after(&["e2e4", "e7e5", "g1f3"])).unwrap();
        assert_eq!(plan.texts, ["e2e4", "e7e5", "g1f3"]);
        assert!(plan.standard_start && !plan.has_null);
        assert_eq!(plan.current.turn(), Color::Black);
    }

    #[test]
    fn plan_rejects_an_illegal_path() {
        let other = Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
        let bad = other.parse_uci("a1a8").unwrap();
        let p = SearchPosition {
            start: Position::new(),
            moves: vec![bad],
        };
        assert!(plan(&p).is_err());
    }

    #[test]
    fn nonstandard_starts_and_null_moves_are_flagged() {
        assert_eq!(unsupported_start(&after(&["e2e4"])), None);
        let odd = SearchPosition::new(Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap());
        assert_eq!(unsupported_start(&odd), Some("non-standard start position"));
        let mut null = after(&["e2e4"]);
        null.moves.push(Move::Null);
        assert_eq!(unsupported_start(&null), Some("null move in the move list"));
    }

    #[test]
    fn limits_map_to_st_and_sd() {
        assert_eq!(
            limit_commands(&SearchLimits::default()),
            ["st 120000", "sd 50"]
        );
        let l = SearchLimits {
            movetime: Some(Duration::from_millis(1500)),
            depth: Some(12),
            ..SearchLimits::default()
        };
        assert_eq!(limit_commands(&l), ["st 2", "sd 12"]);
        let tiny = SearchLimits {
            movetime: Some(Duration::from_millis(1)),
            ..SearchLimits::default()
        };
        assert_eq!(limit_commands(&tiny)[0], "st 1");
        let zero = SearchLimits {
            movetime: Some(Duration::ZERO),
            depth: Some(0),
            ..SearchLimits::default()
        };
        assert_eq!(limit_commands(&zero), ["st 1", "sd 1"]);
    }

    #[test]
    fn clock_commands_use_the_clock_of_the_side_to_move() {
        let clock = Clock {
            white_time: Duration::from_millis(61_500),
            black_time: Duration::from_secs(30),
            white_increment: Duration::from_millis(1000),
            black_increment: Duration::from_millis(500),
            moves_to_go: None,
        };
        assert_eq!(
            clock_commands(&clock, Color::White),
            ["level 0 1:01 1", "time 6150", "otim 3000"]
        );
        assert_eq!(
            clock_commands(&clock, Color::Black),
            ["level 0 0:30 0.5", "time 3000", "otim 6150"]
        );
    }

    #[test]
    fn clock_commands_never_send_zero() {
        let clock = Clock {
            white_time: Duration::ZERO,
            black_time: Duration::from_millis(3),
            white_increment: Duration::from_millis(1250),
            moves_to_go: Some(40),
            ..Clock::default()
        };
        assert_eq!(
            clock_commands(&clock, Color::White),
            ["level 40 0:01 1.25", "time 1", "otim 1"]
        );
    }

    #[test]
    fn move_and_color_commands() {
        assert_eq!(move_command("e2e4", false), "e2e4");
        assert_eq!(move_command("e2e4", true), "usermove e2e4");
        assert_eq!(color_command(&Position::new()), "white");
    }
}
