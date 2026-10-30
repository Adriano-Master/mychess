//! Messages, effects and notices of the calculation exercise (spec M8-6,
//! D2310, D2319).

use std::path::PathBuf;
use std::time::Instant;

use super::config::CalvarConfig;
use super::engine::{EngineFailure, LinesJob, RootJob};
use super::eval::{LineProbes, RootLine};
use super::line::Assessment;
use super::machine::CalvarOrigin;
use super::store::{CalvarStore, LoadedStore};
use crate::bases::{BaseError, SharedSource, SlotId};
use crate::error::{FileError, RequestId};
use crate::shared::SharedGame;
use mychess_core::Position;

/// An event of the exercise.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CalvarMessage {
    /// Opens the configuration dialog.
    Open,
    /// Closes the dialog without starting (a session that is on goes on).
    CloseDialog,
    /// Starts an exercise (D2319). `now` is the time in seconds since the
    /// Unix epoch, which the front-end reads: `update` never reads the clock.
    Start {
        /// What the dialog chose.
        config: Box<CalvarConfig>,
        /// The wall clock, for the history (D2317).
        now: u64,
    },
    /// Moves typed by the user, in SAN or UCI, for the line in progress
    /// (D2302, D2334).
    EnterText(String),
    /// Takes back the last move of the line in progress.
    Undo,
    /// Ends the line in progress with the evaluation of the user (D2304).
    EndLine(Assessment),
    /// Removes a finished line.
    RemoveLine(usize),
    /// "Done": ask the engine to evaluate the lines (D2305).
    Done,
    /// Starts an exercise from game `number` of the list of `slot`, with the
    /// last configuration (M8-6d, D2368). `now` as in `Start`.
    StartFromGame {
        /// The database.
        slot: SlotId,
        /// The game, from 1.
        number: u32,
        /// The wall clock.
        now: u64,
    },
    /// Starts an exercise from the problem on the board of the tactics
    /// training (M8-6d, D2372).
    StartFromTactics {
        /// The wall clock.
        now: u64,
    },
    /// Opens entry `0`-based `index` of the history in read-only mode
    /// (M8-6d, D2369).
    Review(usize),
    /// Ends the exercise and puts the previous game back.
    Close,
    /// Ends the exercise and adds the lines to the game as variants (D2324).
    AddToGame,
    /// Clears the counters and the history (D2318).
    ResetStats,
    /// The one-second tick of the front-end (D2306).
    Tick(Instant),

    // ---- answers to effects ----
    /// The game asked by `StartFromGame` was read.
    GameRead {
        /// The request.
        request: RequestId,
        /// The game, or why not.
        result: Result<SharedGame, BaseError>,
    },
    /// `calvar.toml` was read.
    StoreLoaded(Box<LoadedStore>),
    /// `calvar.toml` was written.
    StoreSaved(Result<(), FileError>),
    /// The first stage answered.
    RootAnalyzed {
        /// The request.
        request: RequestId,
        /// The lines of the engine, or why not.
        result: Result<Vec<RootLine>, EngineFailure>,
    },
    /// The second stage answered.
    LinesAnalyzed {
        /// The request.
        request: RequestId,
        /// One entry per line of the user, or why not.
        result: Result<Vec<LineProbes>, EngineFailure>,
    },
}

/// Work for [`perform`](crate::perform): files and the engine (D2332).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CalvarEffect {
    /// Reads `calvar.toml`; answer [`CalvarMessage::StoreLoaded`].
    LoadStore {
        /// The file.
        path: PathBuf,
    },
    /// Writes `calvar.toml`; answer [`CalvarMessage::StoreSaved`].
    SaveStore {
        /// The file.
        path: PathBuf,
        /// What to write.
        store: Box<CalvarStore>,
    },
    /// Reads a game of a database; answer [`CalvarMessage::GameRead`].
    ReadGame {
        /// Correlates the answer.
        request: RequestId,
        /// The game, from 1.
        number: u32,
        /// The database.
        source: SharedSource,
    },
    /// The first stage; answer [`CalvarMessage::RootAnalyzed`].
    AnalyzeRoot {
        /// Correlates the answer.
        request: RequestId,
        /// The job.
        job: Box<RootJob>,
    },
    /// The second stage; answer [`CalvarMessage::LinesAnalyzed`].
    AnalyzeLines {
        /// Correlates the answer.
        request: RequestId,
        /// The job.
        job: Box<LinesJob>,
    },
}

/// A start that waits for the answer to "save the changes of the game?"
/// (D2320).
#[derive(Debug, Clone, PartialEq)]
pub struct CalvarStartRequest {
    /// What the dialog chose; `None` for a start that uses the last
    /// configuration, taken when the exercise begins (after `calvar.toml`
    /// was read, D2375).
    pub config: Option<CalvarConfig>,
    /// The wall clock.
    pub now: u64,
    /// A position that does not come from the dialog (D2368, D2372).
    pub root: Option<Box<Position>>,
    /// Where `root` came from.
    pub origin: Option<CalvarOrigin>,
    /// Entry of the history to review instead of starting (D2369).
    pub review: Option<usize>,
    /// The question about the game of the user was answered already (the
    /// tactics training had parked it, D2372).
    pub confirmed: bool,
}

impl CalvarStartRequest {
    /// A start with the last configuration, which is taken when it begins.
    pub fn with_saved_config(now: u64) -> CalvarStartRequest {
        CalvarStartRequest {
            config: None,
            ..CalvarStartRequest::new(CalvarConfig::default(), now)
        }
    }

    /// A start from the dialog.
    pub fn new(config: CalvarConfig, now: u64) -> CalvarStartRequest {
        CalvarStartRequest {
            config: Some(config),
            now,
            root: None,
            origin: None,
            review: None,
            confirmed: false,
        }
    }
}

/// A remark about the exercise, as data (the front-end translates it with
/// the key of [`CalvarNotice::message_key`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CalvarNotice {
    /// A game against an engine, a tournament, an annotation, an engine
    /// mode, the guided review or the tactics training is on (D2322).
    Busy,
    /// No UCI engine of the list can be used (D2307).
    NoEngine,
    /// The FEN of the dialog is not valid (D2301).
    BadFen,
    /// The position has no legal move (D2301).
    NoMoves,
    /// A typed move is not legal in the position being calculated; the text
    /// is in [`CalvarView::rejected`](super::CalvarView::rejected) (D2334).
    IllegalMove,
    /// The move of the board is not legal in the line being calculated:
    /// type it (D2303).
    UseKeyboard,
    /// The line in progress needs its evaluation before "Done" (D2305).
    LineOpen,
    /// Fewer lines than asked for (D2305).
    NeedMoreLines,
    /// There is no move in the line to end or take back.
    EmptyLine,
    /// The exercise has the most lines it can have (D2304).
    TooManyLines,
    /// The line has the most moves it can have (D2304).
    LineTooLong,
    /// The editor is locked while the exercise is on (D2323).
    Locked,
    /// A request took too long and was dropped (D2310).
    TimedOut,
    /// The engine did not answer (D2310).
    EngineFailed,
    /// The time ran out with no line to evaluate (D2306).
    TimeUp,
    /// `calvar.toml` could not be written.
    StoreNotSaved,
    /// `calvar.toml` could not be read and was set aside.
    StoreUnreadable,
    /// `calvar.toml` is from a newer version and will not be changed.
    StoreNewer,
    /// The position of the game changed: the lines were not added (D2324).
    CannotAdd,
    /// The game of the list could not be read (D2371).
    GameUnreadable,
    /// The tactics training has no problem on the board (D2372).
    NoProblem,
    /// The entry of the history has no saved lines to review (D2369).
    NoReview,
}

impl CalvarNotice {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            CalvarNotice::Busy => "calvar-notice-busy",
            CalvarNotice::NoEngine => "calvar-notice-no-engine",
            CalvarNotice::BadFen => "calvar-notice-bad-fen",
            CalvarNotice::NoMoves => "calvar-notice-no-moves",
            CalvarNotice::IllegalMove => "calvar-notice-illegal-move",
            CalvarNotice::UseKeyboard => "calvar-notice-use-keyboard",
            CalvarNotice::LineOpen => "calvar-notice-line-open",
            CalvarNotice::NeedMoreLines => "calvar-notice-need-more-lines",
            CalvarNotice::EmptyLine => "calvar-notice-empty-line",
            CalvarNotice::TooManyLines => "calvar-notice-too-many-lines",
            CalvarNotice::LineTooLong => "calvar-notice-line-too-long",
            CalvarNotice::Locked => "calvar-notice-locked",
            CalvarNotice::TimedOut => "calvar-notice-timed-out",
            CalvarNotice::EngineFailed => "calvar-notice-engine-failed",
            CalvarNotice::TimeUp => "calvar-notice-time-up",
            CalvarNotice::StoreNotSaved => "calvar-notice-store-not-saved",
            CalvarNotice::StoreUnreadable => "calvar-notice-store-unreadable",
            CalvarNotice::StoreNewer => "calvar-notice-store-newer",
            CalvarNotice::CannotAdd => "calvar-notice-cannot-add",
            CalvarNotice::GameUnreadable => "calvar-notice-game-unreadable",
            CalvarNotice::NoProblem => "calvar-notice-no-problem",
            CalvarNotice::NoReview => "calvar-notice-no-review",
        }
    }
}
