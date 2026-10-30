//! The Novag Citrine protocol as pure functions (spec m8-10, D2711-D2714).
//!
//! Scid: `tcl/tools/novag.tcl`, `connect`, `addMove`, `send`, `recv`.

use std::time::Duration;

use mychess_core::Color;

use crate::BoardMove;

/// Serial speed (D2704).
pub const BAUD_RATE: u32 = 57_600;

/// What follows every line sent to the board (D2711): the Tcl `puts` of
/// `"msg\n\r"` writes `\n\r\n`. Only real hardware can confirm it (D2750).
pub const NOVAG_EOL: &str = "\n\r\n";

/// The pauses of the protocol (D2712). The defaults are Scid's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NovagTimings {
    /// Between the four start-up commands.
    pub init_gap: Duration,
    /// Between the two sends of a move.
    pub move_gap: Duration,
    /// The same, after a capture.
    pub capture_gap: Duration,
    /// How long silence is tolerated after `I` before warning (D2713).
    pub reply_warning: Duration,
}

impl Default for NovagTimings {
    fn default() -> NovagTimings {
        NovagTimings {
            init_gap: Duration::from_millis(200),
            move_gap: Duration::from_millis(200),
            capture_gap: Duration::from_millis(3000),
            reply_warning: Duration::from_secs(6),
        }
    }
}

/// The start-up commands, in order.
pub const INIT_COMMANDS: [&str; 4] = ["I", "N", "X ON", "U ON"];

/// The command that shows a move on the board: a promotion piece is set
/// off with `/` (`e7e8q` becomes `M e7e8/q`).
pub fn move_command(uci: &str) -> String {
    let uci = uci.trim();
    if uci.len() == 5 && uci.is_ascii() {
        format!("M {}/{}", &uci[..4], &uci[4..])
    } else {
        format!("M {uci}")
    }
}

/// Adds the terminator of the board to a line.
pub fn frame(line: &str) -> Vec<u8> {
    let mut bytes = line.as_bytes().to_vec();
    bytes.extend_from_slice(NOVAG_EOL.as_bytes());
    bytes
}

/// What a line from the board means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NovagLine {
    /// "New Game" (any case, anything after).
    NewGame,
    /// A move; `side` follows the comma rule (D2714).
    Move {
        /// The move.
        mv: BoardMove,
        /// Black when the number field ends with a comma.
        side: Color,
    },
    /// A move line whose move could not be read (kept for the console).
    UnreadableMove(String),
    /// "T": take back a move.
    TakeBack,
    /// Anything else.
    Other,
}

/// Reads one line from the board.
pub fn parse_line(line: &str) -> NovagLine {
    let line = line.trim();
    let starts_new_game = line
        .get(..8)
        .is_some_and(|head| head.eq_ignore_ascii_case("new game"));
    if starts_new_game {
        return NovagLine::NewGame;
    }
    let mut words = line.split_whitespace();
    match words.next() {
        Some("M") => {
            let number = words.next();
            let text = words.next();
            match (number, text) {
                (Some(number), Some(text)) => match BoardMove::parse(text) {
                    Ok(mv) => NovagLine::Move {
                        mv,
                        side: if number.ends_with(',') {
                            Color::Black
                        } else {
                            Color::White
                        },
                    },
                    Err(_) => NovagLine::UnreadableMove(text.chars().take(16).collect()),
                },
                _ => NovagLine::Other,
            }
        }
        Some("T") if words.next().is_none() => NovagLine::TakeBack,
        _ => NovagLine::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::Square;

    fn mv(from: &str, to: &str) -> BoardMove {
        BoardMove::Coordinate {
            from: from.parse::<Square>().unwrap(),
            to: to.parse::<Square>().unwrap(),
            promotion: None,
        }
    }

    #[test]
    fn move_command_marks_the_promotion() {
        assert_eq!(move_command("e2e4"), "M e2e4");
        assert_eq!(move_command("e7e8q"), "M e7e8/q");
        assert_eq!(move_command("  e7e8n "), "M e7e8/n");
    }

    #[test]
    fn frames_with_scids_terminator() {
        assert_eq!(frame("I"), b"I\n\r\n");
    }

    #[test]
    fn reads_new_game_in_any_case() {
        assert_eq!(parse_line("New Game"), NovagLine::NewGame);
        assert_eq!(parse_line("NEW GAME started"), NovagLine::NewGame);
        assert_eq!(parse_line("new"), NovagLine::Other);
    }

    #[test]
    fn the_comma_after_the_number_means_black() {
        assert_eq!(
            parse_line("M 1 e2-e4"),
            NovagLine::Move {
                mv: mv("e2", "e4"),
                side: Color::White
            }
        );
        assert_eq!(
            parse_line("M 1, e7-e5"),
            NovagLine::Move {
                mv: mv("e7", "e5"),
                side: Color::Black
            }
        );
    }

    #[test]
    fn reads_take_back_and_ignores_the_rest() {
        assert_eq!(parse_line("T"), NovagLine::TakeBack);
        assert_eq!(parse_line("T 3"), NovagLine::Other);
        assert_eq!(parse_line("M"), NovagLine::Other);
        assert_eq!(parse_line("M 1"), NovagLine::Other);
        assert_eq!(parse_line(""), NovagLine::Other);
        assert_eq!(parse_line("Novag Citrine v1.2"), NovagLine::Other);
        assert!(matches!(
            parse_line("M 1 zz-zz"),
            NovagLine::UnreadableMove(_)
        ));
    }

    #[test]
    fn hostile_text_never_panics() {
        for text in [
            "M \u{1F600} \u{1F600}",
            "ne\u{e9}w gam",
            "\u{0}",
            "M 1, \u{e9}2\u{e9}4",
        ] {
            drop(parse_line(text));
        }
    }
}
