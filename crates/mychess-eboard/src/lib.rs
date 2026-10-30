//! Electronic chessboards for MyChess (milestone M8-10,
//! `docs/specs/m8-10-tabuleiros-eletronicos.md`, ADR 0014).
//!
//! Two kinds of board, as in the Scid vs PC:
//!
//! - the **Novag Citrine** talks over a serial port ([`novag`], 57600 8N1);
//! - a **DGT** (or any board with a driver) is reached through an *input
//!   engine*, a child process that tells the moves, the position and the
//!   clocks line by line ([`input`]).
//!
//! The protocols are pure functions ([`novag::parse_line`],
//! [`input::parse_line`]), a move on the board becomes a legal move with
//! [`BoardMove::resolve`], and a session ([`connect`], [`start_novag`],
//! [`start_driver`]) is a task that owns the device and speaks to the
//! application through [`Eboard`] (commands) and [`EboardEvents`] (events).
//! Nothing blocks the caller, every wait has a deadline and nothing the
//! board or the driver says can panic.
//!
//! With the `testing` feature, `testing` offers a fake serial port and a
//! fake driver, and the `mychess-fake-inputengine` binary speaks the driver
//! protocol over stdin/stdout. There is no hardware in the tests.
//!
//! Dependency rule: may depend only on `mychess-core`.

mod board_move;
mod error;
mod event;
pub mod input;
mod line;
pub mod novag;
mod platform;
mod serial;
mod session;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use board_move::{BoardMove, BoardMoveError, placement_matches};
pub use error::EboardError;
pub use event::{
    BoardKind, Direction, EboardCommand, EboardEvent, Note, Reason, SendFilter, Warning,
};
pub use line::{Line, LineSplitter, MAX_LINE_BYTES};
pub use platform::default_port;
pub use serial::{DeviceIo, PortInfo, READ_POLL, list_ports, open_serial, validate_port};
pub use session::{
    DriverProcess, Eboard, EboardEvents, NovagTimings, Timeouts, connect, start_driver, start_novag,
};

/// Capacity of the queue of events (D2707): a slow consumer slows the
/// reading, it does not grow the memory.
pub(crate) const EVENT_QUEUE: usize = 256;

/// How many lines of a device are read per second; the rest is dropped
/// with a note (D2759). A real board says a few lines per second.
pub(crate) const MAX_LINES_PER_SECOND: u32 = 200;
