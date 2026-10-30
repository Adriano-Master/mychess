//! Messages and notices of the board (spec M8-10).

use mychess_core::GameResult;
use mychess_eboard::{EboardEvent, PortInfo};

use super::prefs::{EboardIssue, EboardPrefs, FilterChoice};

/// What the user asks of a connected board.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UserCommand {
    /// Ask the board for its position and take it as the start of a new
    /// game (D2730).
    Synchronise,
    /// Turn the board of the driver around (D2753).
    Rotate,
    /// Ask the driver to describe the hardware.
    Info,
    /// Choose which moves the driver reports.
    Filter(FilterChoice),
    /// A free line for the board or the driver (the console box).
    Raw(String),
}

/// An event for the board.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EboardMessage {
    /// Connect with these settings (kept in the preferences if usable).
    Connect(Box<EboardPrefs>),
    /// End the session (or the attempt).
    Disconnect,
    /// A request of the user to the connected board.
    Command(UserCommand),
    /// Empty the console.
    ClearConsole,
    /// List the serial ports again.
    RefreshPorts,
    /// Something the board said (from the hub).
    Event {
        /// The epoch of the connection that said it.
        epoch: u64,
        /// What it said.
        event: EboardEvent,
    },
    /// The session was opened and installed.
    Connected {
        /// The epoch of the attempt.
        epoch: u64,
    },
    /// The port or the driver could not be opened.
    ConnectFailed {
        /// The epoch of the attempt.
        epoch: u64,
        /// The message of the error.
        error: String,
    },
    /// The answer to [`EboardMessage::RefreshPorts`].
    PortsListed(Result<Vec<PortInfo>, String>),
    /// An effect that needs no follow-up was done.
    Ack,
}

/// A remark for the status bar (spec M8-10).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EboardNotice {
    /// The board is ready.
    Connected,
    /// The session ended.
    Disconnected,
    /// The settings cannot be used.
    BadSettings(EboardIssue),
    /// The port or the driver could not be opened.
    ConnectFailed(String),
    /// The board lost contact (unplugged, driver exited or timed out).
    ConnectionLost(String),
    /// A move of the board is not legal here; the text is the move.
    IllegalMove(String),
    /// A move of the board could not be played now (the game is busy).
    MoveNotApplied(String),
    /// The position of the board differs from the game.
    OutOfSync,
    /// The driver sent a position that is not a FEN.
    BadPosition,
    /// The board said the game ended; save the game.
    ResultRecorded(GameResult),
    /// The Novag did not answer.
    NoReply,
    /// The driver reported an error; the text is its message.
    DriverError(String),
}

impl EboardNotice {
    /// The Fluent key of the message.
    #[must_use]
    pub fn message_key(&self) -> &'static str {
        match self {
            EboardNotice::Connected => "eboard-notice-connected",
            EboardNotice::Disconnected => "eboard-notice-disconnected",
            EboardNotice::BadSettings(issue) => issue.message_key(),
            EboardNotice::ConnectFailed(_) => "eboard-notice-connect-failed",
            EboardNotice::ConnectionLost(_) => "eboard-notice-lost",
            EboardNotice::IllegalMove(_) => "eboard-notice-illegal-move",
            EboardNotice::MoveNotApplied(_) => "eboard-notice-move-not-applied",
            EboardNotice::OutOfSync => "eboard-notice-out-of-sync",
            EboardNotice::BadPosition => "eboard-notice-bad-position",
            EboardNotice::ResultRecorded(GameResult::White) => "eboard-notice-result-white",
            EboardNotice::ResultRecorded(GameResult::Black) => "eboard-notice-result-black",
            EboardNotice::ResultRecorded(_) => "eboard-notice-result-draw",
            EboardNotice::NoReply => "eboard-notice-no-reply",
            EboardNotice::DriverError(_) => "eboard-notice-driver-error",
        }
    }

    /// The text to put in the message (`$text`), when it has one.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            EboardNotice::ConnectFailed(t)
            | EboardNotice::ConnectionLost(t)
            | EboardNotice::IllegalMove(t)
            | EboardNotice::MoveNotApplied(t)
            | EboardNotice::DriverError(t) => Some(t),
            _ => None,
        }
    }
}
