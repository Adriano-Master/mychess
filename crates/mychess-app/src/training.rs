//! Threat analysis, the shoot-out and the automove (spec M4-8, D490-D497
//! and D840 on).
//!
//! Three ways to use an engine on the game shown, none of them an analysis of
//! the board:
//!
//! - **threat**: trial mode plus a null move, so the engine analyses what the
//!   opponent would do if it were their turn (`setTrialMode`, `main.tcl`
//!   1882-1913);
//! - **shoot-out** ("Finish game"): the engine plays both sides until the game
//!   is over (`toggleFinishGame`, `analysis.tcl` 3228-3265);
//! - **automove** ("Training"): the engine answers every move of the user
//!   (`toggleAutomove`, `analysis.tcl` 4233-4327).
//!
//! This module holds the data: [`EngineMode`], [`ModeMessage`],
//! [`ModeNotice`], [`StopReason`], [`terminal_reason`] and the
//! [`TrainingHook`] seam. The wiring to `AppState` (events of the engine,
//! timers, the game) is in `state/training_flow.rs`.

mod hook;
mod message;
mod mode;

pub use hook::{TrainingHook, TrainingOutcome, run_hooks};
pub use message::{ModeKind, ModeMessage, ModeNotice};
pub use mode::{
    AUTOMOVE_RANGE, AUTOMOVE_RETRIES, AUTOMOVE_RETRY_DELAY, AutomoveState, DEFAULT_AUTOMOVE,
    EngineMode, ModeAvailability, SEARCH_STALL_GRACE, StopReason, terminal_reason,
};
