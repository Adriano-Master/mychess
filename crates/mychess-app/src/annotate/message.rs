//! Messages, dialog state and remarks of the annotation (spec M4-6).

use std::time::Duration;

use super::batch::BatchStep;
use super::options::{AnnotateOptions, Control, Informant};
use super::run::{AnnotationError, Outcome};
use crate::bases::BaseError;
use crate::engines::RunId;

/// An event of the annotation: the dialog and the buttons.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AnnotateMessage {
    /// "Annotate game...": opens the "Configure annotation" dialog; with a
    /// run in progress it cancels it instead (`initAnnotation`, 896-899).
    Open,
    /// Closes the dialog without starting.
    Close,
    /// Replaces the options shown by the dialog.
    SetOptions(AnnotateOptions),
    /// Replaces the informant thresholds ("Configure informant values").
    SetInformant(Informant),
    /// Chooses the engine of the run among those running (`None`: the
    /// first running one, or the engine of F2 if none runs, D480).
    SetEngine(Option<RunId>),
    /// Starts the run with the options of the dialog (or the last ones when
    /// no dialog is open).
    Start,
    /// Stops the run after the search in progress; the edits stay (D469).
    Cancel,
    /// The timer of [`Effect::ScheduleAnnotateTick`](crate::Effect) fired:
    /// the watchdog looks whether the search in flight advanced (D895).
    Tick {
        /// The execution of the run.
        run: RunId,
        /// The generation of the timer; an older one is ignored.
        generation: u64,
    },
    /// A step of the batch finished (answer to
    /// [`Effect::AnnotateBatchStep`](crate::Effect)).
    BatchStep(Result<BatchStep, BaseError>),
}

/// The "Configure annotation" dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotateDialog {
    /// The options being edited.
    pub options: AnnotateOptions,
    /// The informant thresholds being edited.
    pub informant: Informant,
    /// The engine chosen, `None` for the default (D480).
    pub engine: Option<RunId>,
    /// Moves that would be annotated from the cursor with these options
    /// (the dialog shows the estimated time, rule 22).
    pub moves: u32,
    /// Whether the book option can be used. Always `false` until M5.
    pub book_available: bool,
}

impl AnnotateDialog {
    /// The estimated duration of the run under a time limit (`moves` times
    /// the time per move); `None` under a depth limit.
    pub fn estimated_duration(&self) -> Option<Duration> {
        match self.options.control {
            Control::Time => self.options.movetime.checked_mul(self.moves.max(1)),
            Control::Depth => None,
        }
    }
}

/// What the engine of a run can do (the dialog disables the rest, D477).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotateCaps {
    /// A depth limit can be used (UCI).
    pub depth: bool,
    /// Missed mates can be looked for (UCI).
    pub missed_mates: bool,
    /// Exercises can be marked (UCI with MultiPV).
    pub exercises: bool,
}

/// How a run ended, for the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotateReport {
    /// How it ended.
    pub outcome: Outcome,
    /// Moves annotated.
    pub plies: u32,
    /// Moves to annotate.
    pub total: u32,
    /// Variations added.
    pub variations: u32,
    /// Exercises marked.
    pub exercises: u32,
}

/// A remark about the annotation for the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnnotateNotice {
    /// A run is in progress.
    AlreadyRunning,
    /// No engine is running and none is configured for F2.
    NoEngine,
    /// The game is in trial mode: leave it first.
    TrialMode,
    /// There is no move to annotate from the cursor.
    NothingToAnnotate,
    /// The game cannot be edited while the annotation runs.
    Busy,
    /// The run finished.
    Finished {
        /// Moves annotated.
        plies: u32,
        /// Variations added.
        variations: u32,
    },
    /// The user cancelled.
    Cancelled {
        /// Moves annotated.
        plies: u32,
    },
    /// The game was replaced or altered during the run.
    GameChanged {
        /// Moves annotated.
        plies: u32,
    },
    /// The engine ended during the run.
    EngineFailed {
        /// Moves annotated.
        plies: u32,
    },
    /// The engine answered without a score.
    NoScore {
        /// Moves annotated.
        plies: u32,
    },
    /// A game report stopped before the end (cancelled, engine failure).
    ReportStopped {
        /// Moves analysed.
        plies: u32,
        /// Moves of the main line.
        total: u32,
    },
    /// The report was written into the game.
    ReportWritten {
        /// Moves with a verdict.
        moves: u32,
        /// Variations added.
        variations: u32,
    },
    /// The game has no usable `[%eval]` comments for a report.
    ReportNoEvals,
    /// A report went on with another engine than the one that began it
    /// (the first one is gone, D2190).
    ReportContinuedOtherEngine,
    /// A move or an edit was refused because the review of the report is
    /// waiting for the better move.
    ReviewMoveIgnored,
    /// The report could not be written into the game.
    ReportWriteFailed,
    /// A cancelled run kept the earlier, longer report.
    ReportKept {
        /// Moves of the report that was kept.
        kept: u32,
        /// Moves the cancelled run had analysed.
        plies: u32,
    },
    /// The batch needs an editable `.si4` database in the current slot.
    BatchUnavailable,
    /// The current game has unsaved changes the batch would discard.
    BatchDirty,
    /// The batch annotated every game of the filter.
    BatchFinished {
        /// Games annotated and saved.
        games: u32,
        /// Games without a move to annotate, left as they were.
        skipped: u32,
    },
    /// The batch stopped before the end (cancelled, engine failure).
    BatchStopped {
        /// Games annotated and saved.
        games: u32,
        /// Games of the filter.
        total: u32,
    },
}

impl AnnotateNotice {
    /// The remark that ends a run with `report`.
    pub fn of_report(report: &AnnotateReport) -> AnnotateNotice {
        let plies = report.plies;
        match report.outcome {
            Outcome::Finished => AnnotateNotice::Finished {
                plies,
                variations: report.variations,
            },
            Outcome::Cancelled => AnnotateNotice::Cancelled { plies },
            Outcome::GameChanged => AnnotateNotice::GameChanged { plies },
            Outcome::Failed(AnnotationError::NoScore) => AnnotateNotice::NoScore { plies },
            Outcome::Failed(AnnotationError::NothingToAnnotate) => {
                AnnotateNotice::NothingToAnnotate
            }
            Outcome::Failed(_) => AnnotateNotice::EngineFailed { plies },
        }
    }

    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            AnnotateNotice::AlreadyRunning => "annotate-notice-already-running",
            AnnotateNotice::NoEngine => "annotate-notice-no-engine",
            AnnotateNotice::TrialMode => "annotate-notice-trial-mode",
            AnnotateNotice::NothingToAnnotate => "annotate-notice-nothing-to-annotate",
            AnnotateNotice::Busy => "annotate-notice-busy",
            AnnotateNotice::Finished { .. } => "annotate-notice-finished",
            AnnotateNotice::Cancelled { .. } => "annotate-notice-cancelled",
            AnnotateNotice::GameChanged { .. } => "annotate-notice-game-changed",
            AnnotateNotice::EngineFailed { .. } => "annotate-notice-engine-failed",
            AnnotateNotice::NoScore { .. } => "annotate-notice-no-score",
            AnnotateNotice::ReportStopped { .. } => "annotate-notice-report-stopped",
            AnnotateNotice::ReportWritten { .. } => "annotate-notice-report-written",
            AnnotateNotice::ReportNoEvals => "annotate-notice-report-no-evals",
            AnnotateNotice::ReportContinuedOtherEngine => {
                "annotate-notice-report-continued-other-engine"
            }
            AnnotateNotice::ReviewMoveIgnored => "annotate-notice-review-move-ignored",
            AnnotateNotice::ReportWriteFailed => "annotate-notice-report-write-failed",
            AnnotateNotice::ReportKept { .. } => "annotate-notice-report-kept",
            AnnotateNotice::BatchUnavailable => "annotate-notice-batch-unavailable",
            AnnotateNotice::BatchDirty => "annotate-notice-batch-dirty",
            AnnotateNotice::BatchFinished { .. } => "annotate-notice-batch-finished",
            AnnotateNotice::BatchStopped { .. } => "annotate-notice-batch-stopped",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_estimate_is_only_given_under_a_time_limit() {
        let mut dialog = AnnotateDialog {
            options: AnnotateOptions::default(),
            informant: Informant::default(),
            engine: None,
            moves: 40,
            book_available: false,
        };
        assert_eq!(dialog.estimated_duration(), None);
        dialog.options.control = Control::Time;
        dialog.options.movetime = Duration::from_secs(5);
        assert_eq!(dialog.estimated_duration(), Some(Duration::from_secs(200)));
    }

    #[test]
    fn every_outcome_has_a_notice_with_its_own_key() {
        let report = |outcome| AnnotateReport {
            outcome,
            plies: 7,
            total: 40,
            variations: 2,
            exercises: 0,
        };
        let finished = AnnotateNotice::of_report(&report(Outcome::Finished));
        assert_eq!(
            finished,
            AnnotateNotice::Finished {
                plies: 7,
                variations: 2
            }
        );
        let keys: Vec<&str> = [
            Outcome::Finished,
            Outcome::Cancelled,
            Outcome::GameChanged,
            Outcome::Failed(AnnotationError::EngineFailed),
            Outcome::Failed(AnnotationError::NoScore),
        ]
        .into_iter()
        .map(|o| AnnotateNotice::of_report(&report(o)).message_key())
        .collect();
        let mut unique = keys.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), keys.len());
    }
}
