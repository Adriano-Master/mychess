//! The game report: classification of every move and the accuracy of each
//! side (spec M4-10a, D1420-D1432, D1520-D1529).
//!
//! This is the pure core. A report is computed from one engine evaluation per
//! position of the main line ([`PositionEval`](crate::PositionEval), with a
//! second line when the search used MultiPV 2); nothing here talks to an
//! engine, so it is tested with plain numbers. The run that fills the
//! evaluations (the whole game, cancellable) and the panel come in later
//! slices (M4-10b onwards).
//!
//! The formulas are the public win-probability and accuracy models, written
//! from their published descriptions (D1449). They are estimates and do not
//! claim to match any site.

mod classify;
mod config;
mod inputs;
mod review;
mod ui;
mod write;

pub use classify::{
    GameReport, Moment, MoveClass, MoveInput, MoveReport, ReportThresholds, SideFilter, SideReport,
    accuracy_of_loss, build_report, key_moments, win_percent,
};
pub use config::{LIMIT_MAX_TENTHS, ReportConfig, ReportDialog};
pub use inputs::{ReportInputs, inputs_from_game, inputs_from_game_with_book};
pub use review::{Attempt, HINT_AFTER, Review, ReviewStep};
pub use ui::{
    CLOCK_TICK_SECS, ClassMark, ReportClock, ReportData, ReportEffort, ReportMessage, ReportOrigin,
    ReportPrefs, ReportPreset, ReportUi, ReviewWalk, WALK_MAX_MS, WalkDirection, WalkPlan,
    eta_seconds, infer_user_side, marker_kind, walk_plan,
};
pub use write::{evals_from_game, write_actions, write_plan};
