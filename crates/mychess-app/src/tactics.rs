//! Tactics training: "mate in N" and "find the best move" (spec M8-4,
//! D1900-D1946).
//!
//! Problems come from a PGN file or from the filter of the current database
//! of the user (nothing is bundled): a game is a problem when it has the
//! `****D` marker of the automatic annotation or starts from a position
//! other than the standard one. A solver of forced mates
//! ([`mychess_core::mate`]) judges the moves of "mate in N"; the line of the
//! game judges "the best move". No engine is involved.
//!
//! - `config`: what the dialog edits;
//! - `problem`: a problem and how it is found in a game;
//! - `source`: opening a source and looking for the next problem;
//! - `judge`: judging a move;
//! - `stats` and `store`: the score and `tactics.toml`;
//! - `machine`: the pure state machine ([`Tactics`]);
//! - `message` and `effect`: [`TacticsMessage`] and the effects;
//! - `flow`: the glue with [`AppState`](crate::AppState).

mod config;
mod effect;
mod flow;
mod judge;
mod machine;
mod message;
mod opinion;
mod problem;
mod scan;
mod source;
mod stats;
mod store;

pub use config::{
    ATTEMPTS_RANGE, DIFFICULTY_RANGE, MATE_RANGE, MAX_FILTER_ELO, SourceKind, TIME_LIMIT_RANGE,
    TacticsConfig, TacticsMode, TacticsOrder,
};
pub(crate) use effect::perform;
pub(crate) use flow::{Parked, TacticsFlow};
pub use judge::{
    JUDGE_NODES, JudgeJob, JudgeRule, LONGER_MARGIN, Verdict, Wrong, judge, quick_verdict,
};
pub use machine::{
    Feedback, Hint, MAX_TICK_MS, Out, Tactics, TacticsPhase, TacticsView, WATCHDOG_MS,
};
pub use message::{TacticsEffect, TacticsMessage, TacticsNotice, TacticsStartRequest};
pub use opinion::{
    OPINION_MS, OPINION_TOLERANCE_CP, OpinionEngine, OpinionJob, OpinionVerdict,
    ask as ask_opinion, decide,
};
pub use problem::{
    MARKER, MAX_PROBLEMS_PER_GAME, MAX_SOLUTION_PLIES, Problem, extract_problems, problem_id,
};
pub use scan::{MAX_SCAN_CANDIDATES, MateHint, SCAN_NODES, mate_hint, scan_plies};
pub use source::{
    CLASSIFY_NODES, CLASSIFY_PER_REQUEST, Cancel, FindOutcome, FindQuery, Found, GAMES_PER_REQUEST,
    LINE_NODES, OpenedSource, SCAN_LINE_NODES, SourceError, SourceHandle, SourceStamp,
    coprime_step, find_next, open_file, source_key, walk_index,
};
pub use stats::{FailReason, ModeStats, ProblemOutcome, TacticsStats};
pub use store::{
    LoadedStore, MAX_SOLVED_PER_SOURCE, MAX_SOURCES, STORE_FILE, STORE_VERSION, TacticsStore,
    load_file, parse_store, render_store, save_file, store_path,
};
