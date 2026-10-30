//! Messages and notices of the engine modes (spec M4-8).

use std::time::Duration;

use super::mode::StopReason;
use crate::engines::RunId;

/// The three modes, to say which one a notice is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeKind {
    /// Threat analysis.
    Threat,
    /// "Finish game".
    ShootOut,
    /// "Training".
    Automove,
}

/// An event of the engine modes: the buttons of a section and the timer.
/// `RunId` names the execution (the section).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ModeMessage {
    /// The "Threat" button: enters trial mode with a null move, or leaves it.
    ToggleThreat(RunId),
    /// The "Finish game" button: starts or stops the shoot-out.
    ToggleFinishGame(RunId),
    /// The "Training" button: starts or stops the automove.
    ToggleAutomove(RunId),
    /// Sets the thinking time of the automove (answer of the dialog of
    /// `setAutomoveTime`, `analysis.tcl` 4233-4284). Kept clamped to
    /// 0.1 s - 300 s. Takes effect at the next move of the engine.
    SetAutomoveTime(RunId, Duration),
    /// Stops the shoot-out or the automove, whichever is on (the `Esc` key).
    StopAll,
    /// The timer of [`Effect::ScheduleModeTick`](crate::Effect) fired.
    ModeTick {
        /// The execution that asked for it.
        run: RunId,
        /// The generation of the cycle that asked; an older one is ignored.
        generation: u64,
    },
}

/// A remark about the modes, as data (the front-end translates it with the
/// key of [`ModeNotice::message_key`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModeNotice {
    /// The threat was refused: the side to move is in check
    /// ("Null Move while in Check is not allowed", `main.tcl` 1326-1328).
    NullMoveInCheck,
    /// Another mode, the autoplay or an annotation is in the way; the mode
    /// asked for was refused, or the other one was switched off (D495).
    Conflict,
    /// The user's trial mode is on: shoot-out and automove need the real
    /// game.
    TrialMode,
    /// The user tried to edit the game while the shoot-out plays it (D858).
    ShootOutBusy,
    /// A shoot-out or an automove ended. [`StopReason::UserStopped`] is not
    /// reported.
    Ended {
        /// Which mode.
        kind: ModeKind,
        /// Why.
        reason: StopReason,
    },
}

impl ModeNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            ModeNotice::NullMoveInCheck => "status-notice-null-move-in-check",
            ModeNotice::Conflict => "analysis-mode-conflict",
            ModeNotice::TrialMode => "analysis-mode-trial",
            ModeNotice::ShootOutBusy => "analysis-mode-shootout-busy",
            ModeNotice::Ended { reason, .. } => reason.message_key(),
        }
    }
}
