//! Electronic chessboards (spec M8-10, D2700-D2759, ADR 0014): the Novag
//! Citrine on a serial port and a DGT board through an *input engine*.
//!
//! The front-end never sees `mychess-eboard`: it sends [`EboardMessage`]s,
//! reads [`EboardState`] and runs the [`Effect::Eboard`](crate::Effect)
//! effects through [`perform`](crate::perform). What the board says reaches
//! the front-end through [`EboardHub::events`], like the events of the
//! engines and of the online play.
//!
//! - `prefs`: [`EboardPrefs`] (`[eboard]` in `preferences.toml`) and the
//!   validation of the settings;
//! - `hub`: [`EboardHub`] (the running session) and [`EboardEffect`];
//! - `message`: [`EboardMessage`], [`EboardNotice`];
//! - `state`: [`EboardState`], what the dialog shows.
//!
//! The flow itself (applying the moves of the board, the position, the
//! results, the moves of the user towards the board) is in
//! `state/eboard_flow.rs`.

mod hub;
mod message;
mod prefs;
mod state;

pub(crate) use hub::perform;
pub use hub::{EboardEffect, EboardEvents, EboardHub};
pub use message::{EboardMessage, EboardNotice, UserCommand};
pub use prefs::{BoardChoice, EboardIssue, EboardPrefs, FilterChoice};
pub use state::{
    ConsoleLine, ConsoleNote, EboardPhase, EboardState, LastMove, MAX_CONSOLE_LINES, MoveOutcome,
};

pub use mychess_eboard::{Direction as ConsoleDirection, EboardEvent, PortInfo};
