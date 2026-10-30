//! A serious game against an engine (spec M8-1, phase A: the core without
//! GUI).
//!
//! - `clock`: [`PlayClock`], the pure clock with an injected `Instant`
//!   (D1225-D1226), free moves and externally measured times;
//! - `rules`: [`PlayEnd`], how a game ends (mate, stalemate, repetition,
//!   50 moves, material, time, resignation);
//! - `coach`: [`CoachTracker`], the auxiliary engine that judges the moves of
//!   the player (M8-1d);
//! - `config`: [`PlayConfig`], [`TimeMode`], [`StartFrom`] and friends;
//! - `opponent`: [`Think`], the request to whoever plays the other side
//!   (D1244);
//! - `book`: [`PlayBook`], a Polyglot book that also is a
//!   [`BookProbe`](crate::BookProbe);
//! - `record`: tags and result of the game;
//! - `controller`: [`PlayController`], the pure state machine;
//! - `message`: [`PlayMessage`], [`PlayNotice`] and the view for the
//!   front-end; the glue with `AppState` is `state::play_flow`.

mod book;
mod clock;
mod coach;
mod config;
mod controller;
mod message;
mod opponent;
mod record;
pub(crate) mod rules;

pub(crate) use book::read_book;
pub use book::{PlayBook, load_play_book};
pub use clock::{MoveTime, PlayClock, SideBudget};
pub use coach::{
    BLUNDER_CP, COACH_DEPTH_RANGE, COACH_SECONDS_RANGE, CoachConfig, CoachGrade, CoachOutput,
    CoachSearch, CoachTracker, CoachVerdict, CoachView, DECIDED_CP, DEFAULT_COACH_DEPTH,
    DEFAULT_COACH_SECONDS, DEPTH_TOLERANCE, INACCURACY_CP, MISTAKE_CP, QUEUE_LIMIT,
};
pub use config::{
    BONUS_RANGE, BookChoice, BookSelection, ColorChoice, MINUTES_RANGE, MOVETIME_RANGE,
    NODES_RANGE, OpeningLine, PLAY_DEPTH_RANGE, PlayConfig, StartFrom, Strength, TimeMode,
};
pub use controller::{
    MoveRecord, PlayController, PlayError, PlayOutput, PlayPhase, PlayRemark, PlaySetup, Question,
    Turn,
};
pub use message::{BookFailure, PlayMessage, PlayNotice, PlayView};
pub use opponent::{OpponentInfo, Think, ThinkLimits};
pub use record::{EVENT, TACTICAL_EVENT, TagSetup, write_result, write_start_tags};
pub use rules::PlayEnd;
