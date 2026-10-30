//! Commands sent to the server (spec m8-9, D2608).
//!
//! A [`Command`] is a single line of text that has been checked: no control
//! character (so no second command can ride along), at most
//! [`MAX_COMMAND_LEN`] bytes. The builders also check the handles and the
//! numbers they put in the line. The text of a command is never logged:
//! [`Command::echo`] is what a console may show (a `password` command is
//! masked).

use std::fmt::{self, Write as _};

/// Longest command line accepted.
pub const MAX_COMMAND_LEN: usize = 1024;

/// A refused command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CommandError {
    /// Empty text.
    #[error("empty command")]
    Empty,
    /// Longer than [`MAX_COMMAND_LEN`].
    #[error("command too long")]
    TooLong,
    /// A line break or another control character.
    #[error("control character in command")]
    ControlChar,
    /// A player handle that is not 2-17 letters or digits.
    #[error("invalid player handle")]
    BadHandle,
    /// A number out of its domain.
    #[error("number out of range")]
    OutOfRange,
}

/// Colour wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    /// The server decides.
    #[default]
    Any,
    /// White.
    White,
    /// Black.
    Black,
}

impl ColorChoice {
    fn word(self) -> Option<&'static str> {
        match self {
            ColorChoice::Any => None,
            ColorChoice::White => Some("white"),
            ColorChoice::Black => Some("black"),
        }
    }
}

/// Parameters of a seek (`seek`, D2612).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeekSpec {
    /// Minutes of each side (0-600).
    pub minutes: u32,
    /// Increment in seconds (0-600).
    pub increment: u32,
    /// Rated game.
    pub rated: bool,
    /// Colour wanted.
    pub color: ColorChoice,
    /// The advertiser confirms each match (`manual`) instead of `auto`.
    pub manual: bool,
    /// Rating range `lo-hi`, 0-9999.
    pub range: Option<(u32, u32)>,
}

/// A command line, checked.
#[derive(Clone, PartialEq, Eq)]
pub struct Command {
    line: String,
}

impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The text can hold a password: only the shape is printed.
        write!(f, "Command({} bytes)", self.line.len())
    }
}

fn valid_handle(name: &str) -> bool {
    (2..=17).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric())
}

impl Command {
    /// A command typed by the user (the console).
    ///
    /// # Errors
    /// [`CommandError`] for empty text, a control character or too long.
    pub fn raw(text: &str) -> Result<Command, CommandError> {
        let line = text.trim();
        if line.is_empty() {
            return Err(CommandError::Empty);
        }
        if line.len() > MAX_COMMAND_LEN {
            return Err(CommandError::TooLong);
        }
        if line.chars().any(char::is_control) {
            return Err(CommandError::ControlChar);
        }
        Ok(Command {
            line: line.to_owned(),
        })
    }

    fn built(line: String) -> Command {
        // Builders only put checked pieces together.
        Command { line }
    }

    /// The line to write (without the line end).
    #[must_use]
    pub fn line(&self) -> &str {
        &self.line
    }

    /// The text a console may echo: a command that carries a password shows
    /// its first word only.
    #[must_use]
    pub fn echo(&self) -> String {
        let first = self.line.split_whitespace().next().unwrap_or("");
        let lower = first.to_ascii_lowercase();
        if self.is_sensitive() {
            format!("{lower} ****")
        } else {
            self.line.clone()
        }
    }

    /// The command changes or sends a password.
    #[must_use]
    pub fn is_sensitive(&self) -> bool {
        let mut words = self.line.split_whitespace();
        let first = words.next().unwrap_or("").to_ascii_lowercase();
        // Abbreviations: `pass`, `passw`... of `password` (the server
        // accepts any prefix of at least four letters).
        (first.len() >= 4 && "password".starts_with(first.as_str()))
            || (first == "set"
                && words.next().is_some_and(|w| {
                    w.len() >= 4 && "password".starts_with(&w.to_ascii_lowercase())
                }))
    }

    /// `seek ...` (D2612).
    ///
    /// # Errors
    /// [`CommandError::OutOfRange`] for times or a range out of their domain.
    pub fn seek(spec: &SeekSpec) -> Result<Command, CommandError> {
        if spec.minutes > 600 || spec.increment > 600 {
            return Err(CommandError::OutOfRange);
        }
        let mut line = format!(
            "seek {} {} {}",
            spec.minutes,
            spec.increment,
            if spec.rated { "rated" } else { "unrated" }
        );
        if let Some(word) = spec.color.word() {
            line.push(' ');
            line.push_str(word);
        }
        if spec.manual {
            line.push_str(" manual");
        }
        if let Some((lo, hi)) = spec.range {
            if lo > hi || hi > 9999 {
                return Err(CommandError::OutOfRange);
            }
            write!(line, " {lo}-{hi}").ok();
        }
        Ok(Command::built(line))
    }

    /// `match <player> <t> <i> rated|unrated [white|black]`.
    ///
    /// # Errors
    /// [`CommandError::BadHandle`] or [`CommandError::OutOfRange`].
    pub fn challenge(
        player: &str,
        minutes: u32,
        increment: u32,
        rated: bool,
        color: ColorChoice,
    ) -> Result<Command, CommandError> {
        if !valid_handle(player) {
            return Err(CommandError::BadHandle);
        }
        if minutes > 600 || increment > 600 {
            return Err(CommandError::OutOfRange);
        }
        let mut line = format!(
            "match {player} {minutes} {increment} {}",
            if rated { "rated" } else { "unrated" }
        );
        if let Some(word) = color.word() {
            line.push(' ');
            line.push_str(word);
        }
        Ok(Command::built(line))
    }

    /// `unseek`: withdraws the seeks of the user.
    #[must_use]
    pub fn unseek() -> Command {
        Command::built("unseek".into())
    }

    /// `play <index>`: answers a seek of the graph.
    #[must_use]
    pub fn play_seek(index: u32) -> Command {
        Command::built(format!("play {index}"))
    }

    /// `accept` (the only offer) or `accept <player>`.
    ///
    /// # Errors
    /// [`CommandError::BadHandle`].
    pub fn accept(from: Option<&str>) -> Result<Command, CommandError> {
        Command::with_optional_handle("accept", from)
    }

    /// `decline` or `decline <player>`.
    ///
    /// # Errors
    /// [`CommandError::BadHandle`].
    pub fn decline(from: Option<&str>) -> Result<Command, CommandError> {
        Command::with_optional_handle("decline", from)
    }

    fn with_optional_handle(verb: &str, name: Option<&str>) -> Result<Command, CommandError> {
        match name {
            None => Ok(Command::built(verb.to_owned())),
            Some(n) if valid_handle(n) => Ok(Command::built(format!("{verb} {n}"))),
            Some(_) => Err(CommandError::BadHandle),
        }
    }

    /// A move in coordinates (`e2e4`, `e1g1`); a promotion goes with
    /// [`Command::promote`] sent before it (D2614).
    ///
    /// # Errors
    /// [`CommandError::OutOfRange`] when the text is not two squares.
    pub fn play_move(coords: &str) -> Result<Command, CommandError> {
        let lower = coords.to_ascii_lowercase();
        let b = lower.as_bytes();
        let sq = |f: u8, r: u8| (b'a'..=b'h').contains(&f) && (b'1'..=b'8').contains(&r);
        if b.len() < 4 || b.len() > 5 || !sq(b[0], b[1]) || !sq(b[2], b[3]) {
            return Err(CommandError::OutOfRange);
        }
        Ok(Command::built(lower[..4].to_owned()))
    }

    /// `promote q|r|b|n`: the piece a pawn becomes (D2614).
    ///
    /// # Errors
    /// [`CommandError::OutOfRange`] for any other letter.
    pub fn promote(piece: char) -> Result<Command, CommandError> {
        let c = piece.to_ascii_lowercase();
        if matches!(c, 'q' | 'r' | 'b' | 'n') {
            Ok(Command::built(format!("promote {c}")))
        } else {
            Err(CommandError::OutOfRange)
        }
    }

    /// `resign`.
    #[must_use]
    pub fn resign() -> Command {
        Command::built("resign".into())
    }

    /// `draw` (offers or accepts).
    #[must_use]
    pub fn draw() -> Command {
        Command::built("draw".into())
    }

    /// `abort`.
    #[must_use]
    pub fn abort() -> Command {
        Command::built("abort".into())
    }

    /// `takeback <plies>`.
    ///
    /// # Errors
    /// [`CommandError::OutOfRange`] outside 1-10.
    pub fn takeback(plies: u32) -> Result<Command, CommandError> {
        if (1..=10).contains(&plies) {
            Ok(Command::built(format!("takeback {plies}")))
        } else {
            Err(CommandError::OutOfRange)
        }
    }

    /// `observe <game number or player>`.
    #[must_use]
    pub fn observe_game(game: u32) -> Command {
        Command::built(format!("observe {game}"))
    }

    /// `observe <player>`.
    ///
    /// # Errors
    /// [`CommandError::BadHandle`].
    pub fn observe_player(player: &str) -> Result<Command, CommandError> {
        if valid_handle(player) {
            Ok(Command::built(format!("observe {player}")))
        } else {
            Err(CommandError::BadHandle)
        }
    }

    /// `unobserve <game>`.
    #[must_use]
    pub fn unobserve(game: u32) -> Command {
        Command::built(format!("unobserve {game}"))
    }

    /// `follow <player>`: observes the games of the player as they start.
    ///
    /// # Errors
    /// [`CommandError::BadHandle`].
    pub fn follow(player: &str) -> Result<Command, CommandError> {
        if valid_handle(player) {
            Ok(Command::built(format!("follow {player}")))
        } else {
            Err(CommandError::BadHandle)
        }
    }

    /// `unfollow` (the one followed).
    #[must_use]
    pub fn unfollow() -> Command {
        Command::built("unfollow".into())
    }

    /// `examine` (an empty board) or `examine <player> [n]`.
    ///
    /// # Errors
    /// [`CommandError::BadHandle`].
    pub fn examine(from: Option<&str>) -> Result<Command, CommandError> {
        Command::with_optional_handle("examine", from)
    }

    /// `unexamine`.
    #[must_use]
    pub fn unexamine() -> Command {
        Command::built("unexamine".into())
    }

    /// `forward <n>` in an examined game.
    #[must_use]
    pub fn forward(plies: u32) -> Command {
        Command::built(format!("forward {}", plies.clamp(1, 999)))
    }

    /// `backward <n>` in an examined game.
    #[must_use]
    pub fn backward(plies: u32) -> Command {
        Command::built(format!("backward {}", plies.clamp(1, 999)))
    }

    /// `tell <channel> <text>` is left to the console; this joins a
    /// channel (`+channel 24` follows the lectures, D2616).
    ///
    /// # Errors
    /// [`CommandError::OutOfRange`] outside 1-255.
    pub fn join_channel(channel: u32) -> Result<Command, CommandError> {
        if (1..=255).contains(&channel) {
            Ok(Command::built(format!("+channel {channel}")))
        } else {
            Err(CommandError::OutOfRange)
        }
    }

    /// `-channel <n>`.
    ///
    /// # Errors
    /// [`CommandError::OutOfRange`] outside 1-255.
    pub fn leave_channel(channel: u32) -> Result<Command, CommandError> {
        if (1..=255).contains(&channel) {
            Ok(Command::built(format!("-channel {channel}")))
        } else {
            Err(CommandError::OutOfRange)
        }
    }

    /// `ping` (D2610).
    #[must_use]
    pub fn ping() -> Command {
        Command::built("ping".into())
    }

    /// `refresh [game]`: asks for the board again.
    #[must_use]
    pub fn refresh(game: Option<u32>) -> Command {
        match game {
            Some(g) => Command::built(format!("refresh {g}")),
            None => Command::built("refresh".into()),
        }
    }

    /// `quit`.
    #[must_use]
    pub fn quit() -> Command {
        Command::built("quit".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_rejects_injection_and_size() {
        assert_eq!(Command::raw("   "), Err(CommandError::Empty));
        assert_eq!(Command::raw("who\nresign"), Err(CommandError::ControlChar));
        assert_eq!(Command::raw("a\u{1b}b"), Err(CommandError::ControlChar));
        let long = "x".repeat(MAX_COMMAND_LEN + 1);
        assert_eq!(Command::raw(&long), Err(CommandError::TooLong));
        assert_eq!(Command::raw("  who  ").unwrap().line(), "who");
    }

    #[test]
    fn builders_make_the_documented_lines() {
        let spec = SeekSpec {
            minutes: 5,
            increment: 3,
            rated: false,
            color: ColorChoice::Black,
            manual: true,
            range: Some((1200, 1800)),
        };
        assert_eq!(
            Command::seek(&spec).unwrap().line(),
            "seek 5 3 unrated black manual 1200-1800"
        );
        assert_eq!(
            Command::challenge("bob", 10, 0, true, ColorChoice::Any)
                .unwrap()
                .line(),
            "match bob 10 0 rated"
        );
        assert_eq!(Command::play_seek(16).line(), "play 16");
        assert_eq!(Command::accept(None).unwrap().line(), "accept");
        assert_eq!(Command::accept(Some("bob")).unwrap().line(), "accept bob");
        assert_eq!(Command::play_move("E2E4").unwrap().line(), "e2e4");
        assert_eq!(Command::promote('Q').unwrap().line(), "promote q");
        assert_eq!(Command::follow("bob").unwrap().line(), "follow bob");
        assert_eq!(Command::join_channel(24).unwrap().line(), "+channel 24");
        assert_eq!(Command::forward(0).line(), "forward 1");
    }

    #[test]
    fn builders_refuse_bad_arguments() {
        assert_eq!(
            Command::challenge("b o b", 1, 0, false, ColorChoice::Any),
            Err(CommandError::BadHandle)
        );
        assert_eq!(
            Command::challenge("bob\nresign", 1, 0, false, ColorChoice::Any),
            Err(CommandError::BadHandle)
        );
        assert_eq!(Command::accept(Some("x")), Err(CommandError::BadHandle));
        assert_eq!(Command::play_move("e2"), Err(CommandError::OutOfRange));
        assert_eq!(Command::play_move("i9a1"), Err(CommandError::OutOfRange));
        assert_eq!(Command::promote('k'), Err(CommandError::OutOfRange));
        assert_eq!(Command::takeback(0), Err(CommandError::OutOfRange));
        assert_eq!(Command::join_channel(0), Err(CommandError::OutOfRange));
        let mut spec = SeekSpec {
            minutes: 601,
            increment: 0,
            rated: true,
            color: ColorChoice::Any,
            manual: false,
            range: None,
        };
        assert_eq!(Command::seek(&spec), Err(CommandError::OutOfRange));
        spec.minutes = 5;
        spec.range = Some((1800, 1200));
        assert_eq!(Command::seek(&spec), Err(CommandError::OutOfRange));
    }

    #[test]
    fn passwords_never_show_in_echo_or_debug() {
        for text in ["password old new", "passw old new", "set password hunter2"] {
            let c = Command::raw(text).unwrap();
            assert!(c.is_sensitive(), "{text}");
            assert!(!c.echo().contains("hunter2") && !c.echo().contains("old"));
            assert!(!format!("{c:?}").contains("hunter2"));
        }
        let plain = Command::raw("pass").unwrap();
        assert!(plain.is_sensitive());
        assert!(!Command::raw("passport").unwrap().is_sensitive());
        assert_eq!(Command::raw("who").unwrap().echo(), "who");
    }
}
