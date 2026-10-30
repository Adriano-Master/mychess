//! The vocabulary between a board session and the application: what to
//! connect to, what to tell the board and what the board says (spec m8-10,
//! D2707, D2708, D2720).

use std::path::PathBuf;

use mychess_core::{Color, GameResult};

use crate::BoardMove;

/// The board to talk to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardKind {
    /// A Novag Citrine (or compatible) on a serial port.
    Novag {
        /// The port name, for example `COM3` or `/dev/ttyUSB0`.
        port: String,
    },
    /// A driver program speaking the dgtdrv "input engine" protocol.
    InputEngine {
        /// The driver program.
        command: PathBuf,
        /// The port the driver is told to use (first argument).
        port: String,
        /// Extra arguments, split on white space (for DGT: `la`).
        params: String,
    },
}

/// Which moves the driver reports (`sendboth`, `sendwhite`, `sendblack`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SendFilter {
    /// Moves of both sides.
    #[default]
    Both,
    /// Only White's moves.
    White,
    /// Only Black's moves.
    Black,
}

impl SendFilter {
    /// The command the driver understands.
    pub const fn command(self) -> &'static str {
        match self {
            SendFilter::Both => "sendboth",
            SendFilter::White => "sendwhite",
            SendFilter::Black => "sendblack",
        }
    }
}

/// Something to tell the board (or its driver).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EboardCommand {
    /// Show a move the user made on the screen (Novag only; a driver ignores
    /// it). `uci` is the move as `e2e4` or `e7e8q`.
    Move {
        /// The move.
        uci: String,
        /// The move captured a piece (the board needs more time).
        capture: bool,
    },
    /// Ask the driver for the position and the clocks.
    Query,
    /// Choose which moves the driver reports.
    SendFilter(SendFilter),
    /// Turn the board of the driver around.
    Rotate,
    /// Ask the driver to describe the hardware.
    SystemInfo,
    /// A free line (the console box).
    Raw(String),
    /// End the session.
    Close,
}

/// Which way a console line went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// From the board or driver to us.
    In,
    /// From us to the board or driver.
    Out,
    /// A note of the session itself.
    Note,
}

/// Something worth telling the user that does not end the session.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Warning {
    /// The Novag said nothing after the first command (D2713).
    NoReply,
    /// A command that needs a ready driver came too early (D2720).
    NotReady,
    /// The command queue is full or the line was refused.
    Refused(String),
}

/// A remark of the session itself for the console. The front-end writes the
/// text (and translates it); the session sends only the data (D2719).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Note {
    /// A line announced a move that could not be read; the text is the
    /// move as it came.
    UnreadableMove(String),
    /// A command for input engines reached a Novag session.
    InputEngineOnly,
    /// The last lines the driver wrote to its error output before it ended.
    DriverStderr(String),
    /// A flood of lines: this many were dropped without being read
    /// (D2759).
    LinesOmitted(u32),
}

/// Why a session ended (D2708).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reason {
    /// The user asked (or the handle was dropped).
    Requested,
    /// The serial port failed (unplugged, I/O error).
    DeviceLost(String),
    /// The driver process ended by itself.
    DriverExited,
    /// The driver did not get ready in time (D2716).
    HandshakeTimeout,
    /// The driver reported a fatal error (D2718).
    DriverError(String),
}

/// Something the board said. The last event of a session is always
/// [`EboardEvent::Disconnected`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EboardEvent {
    /// The board is ready for use.
    Ready,
    /// A line for the console.
    Log {
        /// Which way it went.
        direction: Direction,
        /// The text; bounded by the line limit.
        text: String,
    },
    /// A warning.
    Warning(Warning),
    /// A remark of the session for the console.
    Note(Note),
    /// A new game started on the board.
    NewGame,
    /// A move made on the board. `side` is the color the board says moved
    /// (Novag only: a comma after the number means Black).
    Move {
        /// The move.
        mv: BoardMove,
        /// The side that moved, when the board tells.
        side: Option<Color>,
    },
    /// The board asks to take back one move (Novag).
    TakeBack,
    /// The position on the board, as the driver sent it (not validated).
    Fen(String),
    /// The driver announced its start-up position: the next position it
    /// sends is a fresh start (D2730).
    StartupFen,
    /// A piece was picked up (FEN letter).
    MovingPiece(char),
    /// Whose turn the clock says.
    ToMove(Color),
    /// The board has no clock.
    NoClock,
    /// The time left on a clock, in seconds.
    Clock {
        /// Whose clock.
        color: Color,
        /// Seconds left.
        seconds: u32,
    },
    /// The game ended on the board.
    GameEnd(GameResult),
    /// The board entered (`true`) or left (`false`) the set-up mode.
    SetupMode(bool),
    /// The driver identified a board (text of the line).
    BoardIdentified(String),
    /// The driver reported a fatal error.
    DriverError(String),
    /// The session is over.
    Disconnected(Reason),
}
