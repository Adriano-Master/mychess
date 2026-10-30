//! The input engine ("dgtdrv") protocol as pure functions (spec m8-10,
//! D2715, D2722).
//!
//! Scid: `tcl/inputengine.tcl`, `readFromEngine`. The Tcl matches the
//! `info` messages with unanchored regular expressions in a fixed order;
//! this reader tests *contains* in the same order.

use mychess_core::{Color, GameResult};

use crate::{BoardMove, SendFilter};

/// Commands the app may send before the driver is ready.
pub const CMD_XBOARD: &str = "xboard";
/// Turns off the move announcements of the driver.
pub const CMD_ANNOUNCE: &str = "announce";
/// Asks for the position.
pub const CMD_GET_POSITION: &str = "getposition";
/// Asks for the clocks.
pub const CMD_GET_CLOCK: &str = "getclock";
/// Turns the board around.
pub const CMD_ROTATE: &str = "rotateboard";
/// Describes the hardware.
pub const CMD_SYSINFO: &str = "sysinfo";
/// First half of the goodbye.
pub const CMD_STOP: &str = "stop";
/// Second half of the goodbye.
pub const CMD_QUIT: &str = "quit";

/// The largest clock value read, in seconds.
const MAX_SECONDS: u32 = 99_999_999;

/// What a line from the driver means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverLine {
    /// `move e2e4`.
    Move(BoardMove),
    /// A `move` line whose move could not be read.
    UnreadableMove(String),
    /// `info string ERROR ...`.
    Error(String),
    /// `info string Chessboard found and initialised`.
    BoardFound,
    /// `info Engine mode : xboard`.
    XboardMode,
    /// `info string FEN ...`: the start-up announcement (the text is unused).
    StartupFen,
    /// `info FEN <fen>`.
    Fen(String),
    /// `info moving piece: <letter>`.
    MovingPiece(char),
    /// `!new game!`.
    NewGame,
    /// `!end game <result>!`.
    GameEnd(GameResult),
    /// `!enter setup mode!` (`true`) or `!end setup mode!` (`false`).
    SetupMode(bool),
    /// `!white to move!` or `!black to move!`.
    ToMove(Color),
    /// `No Clock detected`.
    NoClock,
    /// `Time White: ...` or `Time Black: ...`.
    Clock {
        /// Whose clock.
        color: Color,
        /// Seconds.
        seconds: u32,
    },
    /// `DGT Projects - This DGT board ...`; carries the text.
    BoardIdentified(String),
    /// Known but only logged: `!move now!`, `Wrong move performed:`.
    Noted,
    /// Anything else.
    Other,
}

/// Reads one line from the driver.
pub fn parse_line(line: &str) -> DriverLine {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("move ") {
        let rest = rest.trim();
        if rest.is_empty() {
            return DriverLine::Other;
        }
        return match BoardMove::parse(rest) {
            Ok(mv) => DriverLine::Move(mv),
            Err(_) => DriverLine::UnreadableMove(rest.chars().take(16).collect()),
        };
    }
    let Some(event) = line.strip_prefix("info ") else {
        return DriverLine::Other;
    };
    parse_event(event.trim())
}

const MARKS: [(&str, DriverLine); 9] = [
    ("!new game!", DriverLine::NewGame),
    ("!end game 1-0!", DriverLine::GameEnd(GameResult::White)),
    ("!end game 0-1!", DriverLine::GameEnd(GameResult::Black)),
    ("!end game 1/2-1/2!", DriverLine::GameEnd(GameResult::Draw)),
    ("!enter setup mode!", DriverLine::SetupMode(true)),
    ("!end setup mode!", DriverLine::SetupMode(false)),
    ("!white to move!", DriverLine::ToMove(Color::White)),
    ("!black to move!", DriverLine::ToMove(Color::Black)),
    ("No Clock detected", DriverLine::NoClock),
];

fn parse_event(event: &str) -> DriverLine {
    // "Engine mode    : xboard" has a run of blanks in the real driver.
    let squeezed = event.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(at) = event.find("string ERROR ") {
        let text = &event[at + "string ERROR ".len()..];
        return DriverLine::Error(text.trim().to_owned());
    }
    if event.contains("string Chessboard found and initialised") {
        return DriverLine::BoardFound;
    }
    if squeezed.contains("Engine mode : xboard") {
        return DriverLine::XboardMode;
    }
    if event.contains("string FEN ") {
        return DriverLine::StartupFen;
    }
    if let Some(at) = event.find("FEN ") {
        return DriverLine::Fen(event[at + 4..].trim().to_owned());
    }
    if let Some(at) = event.find("moving piece: ") {
        return match event[at + "moving piece: ".len()..].chars().next() {
            Some(letter) if letter.is_ascii_alphabetic() => DriverLine::MovingPiece(letter),
            _ => DriverLine::Other,
        };
    }
    for (mark, meaning) in MARKS {
        if event.contains(mark) {
            return meaning;
        }
    }
    if event.contains("!move now!") || event.contains("Wrong move performed:") {
        return DriverLine::Noted;
    }
    for (label, color) in [("Time White:", Color::White), ("Time Black:", Color::Black)] {
        if let Some(at) = event.find(label) {
            return match first_number(&event[at + label.len()..]) {
                Some(seconds) => DriverLine::Clock { color, seconds },
                None => DriverLine::Other,
            };
        }
    }
    if event.contains("DGT Projects - This DGT board") {
        return DriverLine::BoardIdentified(event.to_owned());
    }
    DriverLine::Other
}

/// The first run of digits in `text`, saturated at [`MAX_SECONDS`] (a
/// number too big to be a clock is cut to the limit, not refused).
fn first_number(text: &str) -> Option<u32> {
    let mut digits = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .map_while(|c| c.to_digit(10))
        .peekable();
    digits.peek()?;
    let value = digits.fold(0_u64, |n, d| {
        n.saturating_mul(10).saturating_add(u64::from(d))
    });
    Some(u32::try_from(value.min(u64::from(MAX_SECONDS))).unwrap_or(MAX_SECONDS))
}

/// The lines to send for a command of the application (D2720); empty when
/// the driver has nothing to do with it.
pub fn commands_for(command: &crate::EboardCommand) -> Vec<String> {
    use crate::EboardCommand as C;
    match command {
        C::Query => vec![CMD_GET_POSITION.to_owned(), CMD_GET_CLOCK.to_owned()],
        C::SendFilter(filter) => vec![SendFilter::command(*filter).to_owned()],
        C::Rotate => vec![CMD_ROTATE.to_owned()],
        C::SystemInfo => vec![CMD_SYSINFO.to_owned()],
        C::Raw(text) => sanitize_raw(text).into_iter().collect(),
        C::Move { .. } | C::Close => Vec::new(),
    }
}

/// The longest free line sent (D2720).
pub const MAX_RAW_BYTES: usize = 512;

/// Makes a free line safe to send: line breaks and NUL removed, trimmed, at
/// most [`MAX_RAW_BYTES`]; `None` when nothing is left.
pub fn sanitize_raw(text: &str) -> Option<String> {
    let clean: String = text
        .chars()
        .filter(|c| !matches!(c, '\n' | '\r' | '\0'))
        .collect();
    let clean = clean.trim().to_owned();
    let mut clean = clean;
    while clean.len() > MAX_RAW_BYTES {
        clean.pop();
    }
    (!clean.is_empty()).then_some(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_moves() {
        assert!(matches!(parse_line("move e2e4"), DriverLine::Move(_)));
        assert!(matches!(parse_line("move 0-0"), DriverLine::Move(_)));
        assert!(matches!(
            parse_line("move xx"),
            DriverLine::UnreadableMove(_)
        ));
        assert_eq!(parse_line("move "), DriverLine::Other);
        assert_eq!(parse_line("move"), DriverLine::Other);
    }

    #[test]
    fn follows_the_handshake() {
        assert_eq!(
            parse_line("info string Chessboard found and initialised on /dev/ttyUSB0"),
            DriverLine::BoardFound
        );
        assert_eq!(
            parse_line("info Engine mode    : xboard"),
            DriverLine::XboardMode
        );
        assert_eq!(
            parse_line("info string FEN rnbqkbnr/8 w"),
            DriverLine::StartupFen
        );
        assert_eq!(
            parse_line("info FEN 8/8/8/8/8/8/8/8 w - - 0 1"),
            DriverLine::Fen("8/8/8/8/8/8/8/8 w - - 0 1".to_owned())
        );
        assert_eq!(
            parse_line("info string ERROR no board on /dev/ttyUSB0"),
            DriverLine::Error("no board on /dev/ttyUSB0".to_owned())
        );
    }

    #[test]
    fn reads_the_markers() {
        assert_eq!(parse_line("info string !new game!"), DriverLine::NewGame);
        assert_eq!(
            parse_line("info !end game 1-0!"),
            DriverLine::GameEnd(GameResult::White)
        );
        assert_eq!(
            parse_line("info !end game 0-1!"),
            DriverLine::GameEnd(GameResult::Black)
        );
        assert_eq!(
            parse_line("info !end game 1/2-1/2!"),
            DriverLine::GameEnd(GameResult::Draw)
        );
        assert_eq!(
            parse_line("info !enter setup mode!"),
            DriverLine::SetupMode(true)
        );
        assert_eq!(
            parse_line("info !end setup mode!"),
            DriverLine::SetupMode(false)
        );
        assert_eq!(
            parse_line("info !black to move!"),
            DriverLine::ToMove(Color::Black)
        );
        assert_eq!(parse_line("info No Clock detected"), DriverLine::NoClock);
        assert_eq!(parse_line("info !move now!"), DriverLine::Noted);
        assert_eq!(
            parse_line("info moving piece: N"),
            DriverLine::MovingPiece('N')
        );
        assert_eq!(parse_line("info moving piece: "), DriverLine::Other);
        assert!(matches!(
            parse_line("info DGT Projects - This DGT board is a DGT XL"),
            DriverLine::BoardIdentified(_)
        ));
    }

    #[test]
    fn reads_clocks_and_refuses_nonsense() {
        assert_eq!(
            parse_line("info Time White: #3600"),
            DriverLine::Clock {
                color: Color::White,
                seconds: 3600
            }
        );
        assert_eq!(
            parse_line("info Time Black: 75"),
            DriverLine::Clock {
                color: Color::Black,
                seconds: 75
            }
        );
        assert_eq!(parse_line("info Time White: abc"), DriverLine::Other);
        // Too big for a clock: cut to the limit, not refused (item 12).
        for huge in ["99999999999", "99999999999999999999999999999"] {
            assert_eq!(
                parse_line(&format!("info Time White: {huge}")),
                DriverLine::Clock {
                    color: Color::White,
                    seconds: MAX_SECONDS
                }
            );
        }
    }

    #[test]
    fn unknown_and_hostile_lines_are_other() {
        for text in [
            "",
            "hello",
            "info",
            "info ",
            "info \u{1F600}\u{1F600}",
            "move \u{e9}\u{e9}\u{e9}",
            "xinfo FEN a",
        ] {
            drop(parse_line(text));
        }
        assert_eq!(parse_line("hello"), DriverLine::Other);
        assert_eq!(parse_line("info something new"), DriverLine::Other);
    }

    #[test]
    fn commands_are_built_from_the_app_commands() {
        use crate::EboardCommand as C;
        assert_eq!(commands_for(&C::Query), ["getposition", "getclock"]);
        assert_eq!(
            commands_for(&C::SendFilter(SendFilter::Black)),
            ["sendblack"]
        );
        assert_eq!(commands_for(&C::Rotate), ["rotateboard"]);
        assert_eq!(commands_for(&C::Raw("a\nb\r\0".into())), ["ab"]);
        assert_eq!(commands_for(&C::Raw("  \n".into())).len(), 0);
        assert_eq!(
            commands_for(&C::Move {
                uci: "e2e4".into(),
                capture: false
            })
            .len(),
            0
        );
    }

    #[test]
    fn raw_lines_are_bounded() {
        let long = "x".repeat(5000);
        assert_eq!(sanitize_raw(&long).map(|s| s.len()), Some(MAX_RAW_BYTES));
    }
}
