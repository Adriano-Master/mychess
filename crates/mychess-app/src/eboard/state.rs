//! What the dialog of the board shows (spec M8-10, D2725).

use std::collections::VecDeque;

use mychess_core::Color;
use mychess_eboard::{Direction, PortInfo};

use super::hub::EboardHub;
use super::message::EboardNotice;
use super::prefs::{BoardChoice, FilterChoice};

/// How many console lines are kept (D2719).
pub const MAX_CONSOLE_LINES: usize = 200;

/// Where the connection is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EboardPhase {
    /// Not connected.
    #[default]
    Idle,
    /// Opening the port or starting the driver, or waiting for the board.
    Connecting,
    /// Ready.
    Connected,
}

/// A remark of the program in the console: the front-end translates it
/// (the traffic with the board is shown as it came, never translated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConsoleNote {
    /// The session ended by the user.
    Disconnected,
    /// The board entered the set-up mode.
    SetupMode,
    /// A move of the board is not legal; the text is the move.
    IllegalMove,
    /// A legal move of the board could not be played now; the text is the
    /// move.
    MoveNotApplied,
    /// A line the session could not queue; the text is the line.
    Refused,
    /// A move of the board that could not be read; the text is the move.
    UnreadableMove,
    /// A command for input engines went to a Novag.
    InputEngineOnly,
    /// What the driver wrote to its error output; the text is that.
    DriverStderr,
    /// Lines dropped because the device said too many; the text is the
    /// count (the Fluent message takes it as the number `$count`).
    LinesOmitted,
}

impl ConsoleNote {
    /// The Fluent key of the message (the argument `$text` is the text of
    /// the line; `LinesOmitted` takes it as the number `$count`).
    #[must_use]
    pub fn message_key(self) -> &'static str {
        match self {
            ConsoleNote::Disconnected => "eboard-console-disconnected",
            ConsoleNote::SetupMode => "eboard-console-setup-mode",
            ConsoleNote::IllegalMove => "eboard-console-illegal-move",
            ConsoleNote::MoveNotApplied => "eboard-console-move-not-applied",
            ConsoleNote::Refused => "eboard-console-refused",
            ConsoleNote::UnreadableMove => "eboard-console-unreadable-move",
            ConsoleNote::InputEngineOnly => "eboard-console-input-engine-only",
            ConsoleNote::DriverStderr => "eboard-console-driver-stderr",
            ConsoleNote::LinesOmitted => "eboard-console-lines-omitted",
        }
    }
}

/// A line of the console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    /// Which way it went.
    pub direction: Direction,
    /// The text: the line of the traffic, or the argument of the note.
    pub text: String,
    /// The remark, when the line is one of the program's own (the text is
    /// then its argument).
    pub note: Option<ConsoleNote>,
}

/// What became of the last move of the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveOutcome {
    /// Played in the game (green).
    Played,
    /// Not a legal move in the position (red).
    Illegal,
    /// Legal, but the game could not take it now: online or engine game,
    /// training, a question open... (neutral).
    NotApplied,
}

/// The last move the board made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastMove {
    /// The move as the board said it.
    pub text: String,
    /// What became of it.
    pub outcome: MoveOutcome,
}

impl LastMove {
    /// Whether the move was played.
    #[must_use]
    pub fn accepted(&self) -> bool {
        self.outcome == MoveOutcome::Played
    }
}

/// The state of the board, shown by the dialog.
#[derive(Debug, Default)]
pub struct EboardState {
    pub(crate) hub: EboardHub,
    pub(crate) epoch: u64,
    pub(crate) phase: EboardPhase,
    pub(crate) connected: BoardChoice,
    pub(crate) console: VecDeque<ConsoleLine>,
    pub(crate) last_move: Option<LastMove>,
    pub(crate) white_clock: Option<u32>,
    pub(crate) black_clock: Option<u32>,
    pub(crate) to_move: Option<Color>,
    pub(crate) in_sync: Option<bool>,
    pub(crate) board_name: Option<String>,
    pub(crate) ports: Vec<PortInfo>,
    pub(crate) ports_error: Option<String>,
    pub(crate) issue: Option<EboardNotice>,
    pub(crate) filter: FilterChoice,
    /// The next position of the driver is the start of a new game (D2730).
    pub(crate) awaiting_start: bool,
    /// A move of the board is being played: it must not go back to the
    /// board (D2733).
    pub(crate) suppress_echo: bool,
}

impl EboardState {
    /// Where the connection is.
    #[must_use]
    pub fn phase(&self) -> EboardPhase {
        self.phase
    }

    /// The kind of board connected (or being connected).
    #[must_use]
    pub fn connected_kind(&self) -> BoardChoice {
        if self.phase == EboardPhase::Idle {
            BoardChoice::None
        } else {
            self.connected
        }
    }

    /// Whether the session is ready.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.phase == EboardPhase::Connected
    }

    /// Whether the board is connected or being connected.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.phase != EboardPhase::Idle
    }

    /// The console, oldest line first.
    pub fn console(&self) -> impl Iterator<Item = &ConsoleLine> {
        self.console.iter()
    }

    /// The last move of the board.
    #[must_use]
    pub fn last_move(&self) -> Option<&LastMove> {
        self.last_move.as_ref()
    }

    /// The clock of a side, in seconds, when the board has one.
    #[must_use]
    pub fn clock(&self, color: Color) -> Option<u32> {
        match color {
            Color::White => self.white_clock,
            Color::Black => self.black_clock,
        }
    }

    /// Whose turn the board's clock says.
    #[must_use]
    pub fn to_move(&self) -> Option<Color> {
        self.to_move
    }

    /// Whether the board and the game agree (`None`: not known yet).
    #[must_use]
    pub fn in_sync(&self) -> Option<bool> {
        self.in_sync
    }

    /// The text the driver used to identify a board (DGT).
    #[must_use]
    pub fn board_name(&self) -> Option<&str> {
        self.board_name.as_deref()
    }

    /// The serial ports found by the last listing.
    #[must_use]
    pub fn ports(&self) -> &[PortInfo] {
        &self.ports
    }

    /// Why the last listing failed.
    #[must_use]
    pub fn ports_error(&self) -> Option<&str> {
        self.ports_error.as_deref()
    }

    /// The last problem to show in the dialog (failed connection, driver
    /// error, lost contact).
    #[must_use]
    pub fn issue(&self) -> Option<&EboardNotice> {
        self.issue.as_ref()
    }

    /// The filter of the driver.
    #[must_use]
    pub fn filter(&self) -> FilterChoice {
        self.filter
    }

    /// The hub whose events the front-end subscribes to.
    #[must_use]
    pub fn hub(&self) -> &EboardHub {
        &self.hub
    }

    /// The epoch of the current connection: events that carry another one
    /// are dropped.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    #[cfg(test)]
    pub(crate) fn epoch_for_tests(&self) -> u64 {
        self.epoch
    }

    pub(crate) fn log(&mut self, direction: Direction, text: String) {
        self.push_line(ConsoleLine {
            direction,
            text,
            note: None,
        });
    }

    pub(crate) fn note(&mut self, note: ConsoleNote, text: String) {
        self.push_line(ConsoleLine {
            direction: Direction::Note,
            text,
            note: Some(note),
        });
    }

    fn push_line(&mut self, line: ConsoleLine) {
        if self.console.len() == MAX_CONSOLE_LINES {
            self.console.pop_front();
        }
        self.console.push_back(line);
    }

    pub(crate) fn reset_session(&mut self) {
        self.console.clear();
        self.last_move = None;
        self.white_clock = None;
        self.black_clock = None;
        self.to_move = None;
        self.in_sync = None;
        self.board_name = None;
        self.awaiting_start = false;
        self.suppress_echo = false;
        self.issue = None;
    }
}
