//! The opening trainer: drill a repertoire against the machine (spec M8-5,
//! D2200-D2248).
//!
//! The user plays one side of an opening repertoire and the machine answers
//! with the moves of the repertoire, drawn by how often they are played. The
//! repertoire comes from games (a database of the type "Openings for ...",
//! the filter of the current database, a PGN file), from a Polyglot book or
//! from a tree mask (`.stm`) whose moves are marked "to train". No engine is
//! involved.
//!
//! - `config`: what the dialog edits;
//! - `repertoire`: the map from positions to moves and the draw of a line;
//! - `build`: reading games, a book or a mask into a repertoire;
//! - `stats` and `store`: the score, the spaced repetition and
//!   `opening_trainer.toml`;
//! - `machine`: the pure state machine ([`OpeningTrainer`]);
//! - `message` and `effect`: [`OpeningMessage`] and the effects;
//! - `flow`: the glue with [`AppState`](crate::AppState).

mod build;
mod config;
mod effect;
mod flow;
mod machine;
mod message;
mod repertoire;
mod stats;
mod store;

pub use build::{
    BuildError, BuildJob, BuildOutput, BuildSource, GamesSource, MAX_GAMES, RepFileKind, build,
};
pub use config::{DEFAULT_DEPTH, DEPTH_RANGE, OpeningConfig, RepSourceKind, TrainSide};
pub(crate) use effect::perform;
pub(crate) use flow::OpeningFlow;
pub use flow::{TYPE_OPENINGS_BLACK, TYPE_OPENINGS_EITHER, TYPE_OPENINGS_WHITE, pick_typed};
pub use machine::{
    Candidate, LineEnd, MAX_TICK_MS, OpeningPhase, OpeningTrainer, OpeningView, Out, WATCHDOG_MS,
};
pub use message::{OpeningEffect, OpeningMessage, OpeningNotice, OpeningStartRequest};
pub use repertoire::{
    MAX_CANDIDATES, MAX_POSITIONS, RepBuilder, RepMove, RepNode, RepSet, Repertoire, Rng,
    WALK_SAMPLES, WALK_SAMPLES_NO_REPEAT, Walk, has_user_move, pick_weighted, sample_walk,
};
pub use stats::{
    BOX_INTERVALS, LEARNED_BOX, NEED_DUE, NEED_UNSEEN, PosStat, ReportView, SessionStats, TOP_BOX,
    need_of,
};
pub use store::{
    LoadedStore, MAX_POSITIONS_PER_REP, MAX_REPS, OpeningStore, RepRecord, STORE_FILE,
    STORE_VERSION, load_file, parse_store, render_store, save_file, store_path,
};
