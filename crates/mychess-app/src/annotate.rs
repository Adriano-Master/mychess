//! Automatic annotation of a game (spec M4-6, phase A: one game).
//!
//! - [`AnnotateOptions`], [`Informant`]: what to annotate, with the Scid's
//!   defaults (D461, D474);
//! - [`decide`]: the pure rules of `addAnnotation` for one move, with
//!   [`score_to_nag`], [`format_score_text`] and [`append_tag`];
//! - `run`: the state machine that walks the game and asks the engine for one
//!   search per position (testable with synthetic engine events);
//! - [`BookProbe`]: the hook for opening books (milestone M5);
//! - [`AnnotateMessage`], [`AnnotateDialog`], [`AnnotateNotice`],
//!   [`AnnotateReport`]: the messages and the data the front-end shows.
//!
//! - `batch`: the disk step of the batch over a filter (phase B).

mod apply;
mod batch;
mod book;
mod decision;
mod exercise;
mod message;
mod options;
mod run;
mod score;

pub use apply::{ApplyStats, apply_actions};
pub use batch::{BatchSave, BatchStep, batch_step};
pub use book::{BookProbe, BookVerdict, classify, last_book_move_comment, out_of_book_comment};
pub use decision::{Action, Decision, Step, decide, skips_side};
pub use exercise::{
    DIFFICULTY_TIMES_MS, OBVIOUS_DEPTHS, difficulty, exercise_comment, exercise_move, is_obvious,
};
pub use message::{AnnotateCaps, AnnotateDialog, AnnotateMessage, AnnotateNotice, AnnotateReport};
pub use options::{
    AnnotateOptions, BLUNDER_RANGE, CUTOFF_RANGE, Control, DEFAULT_OPENING_MOVES, DEPTH_RANGE,
    INFORMANT_RANGE, Informant, MOVETIME_RANGE_MS, OPENING_MOVES_RANGE, ScoreFormat, Side,
    WithScore, WithVars,
};
pub use run::{AnnotationError, AnnotationProgress, Outcome};
pub(crate) use run::{AnnotationRun, CLEAR_HASH, RunOutput, RunSetup, is_annotation_tag, plan};
pub use score::{NO_MOVES_PAWNS, PositionEval, append_tag, format_score_text, score_to_nag};
