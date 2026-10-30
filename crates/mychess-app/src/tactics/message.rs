//! Messages, effects and notices of the tactics training (spec M8-4,
//! D1928-D1936).

use std::path::PathBuf;
use std::time::Instant;

use super::config::TacticsConfig;
use super::judge::{JudgeJob, Verdict};
use super::opinion::{OpinionJob, OpinionVerdict};
use super::source::{FindOutcome, FindQuery, OpenedSource, SourceError, SourceHandle};
use super::store::{LoadedStore, TacticsStore};
use crate::error::{FileError, RequestId};

/// An event of the tactics training.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TacticsMessage {
    /// Opens the configuration dialog.
    Open,
    /// Closes the dialog without starting (a session that is on goes on).
    CloseDialog,
    /// Starts a session (D1928). `seed` decides the random order (D1915):
    /// the front-end draws it, `update` never reads the clock.
    Start {
        /// What the dialog chose.
        config: Box<TacticsConfig>,
        /// The seed of the random order.
        seed: u64,
    },
    /// The hint button (D1919).
    Hint,
    /// "Show the solution" (D1920).
    ShowSolution,
    /// "Next problem": skips the problem on the board or goes on after it
    /// ended (D1922).
    Next,
    /// "Try again" after a failure.
    Retry,
    /// Ends the session and puts the previous game back.
    Close,
    /// Clears the score of the mode of the configuration and the solved
    /// problems of the source it names (D1927).
    ResetStats(Box<TacticsConfig>),
    /// The one-second tick of the front-end (D1925).
    Tick(Instant),

    // ---- answers to effects ----
    /// `tactics.toml` was read.
    StoreLoaded(Box<LoadedStore>),
    /// `tactics.toml` was written.
    StoreSaved(Result<(), FileError>),
    /// A PGN file was opened as a source.
    Opened {
        /// The request.
        request: RequestId,
        /// The source, or why not.
        result: Result<Box<OpenedSource>, SourceError>,
    },
    /// The search for the next problem answered.
    Found {
        /// The request.
        request: RequestId,
        /// What it found.
        outcome: Box<FindOutcome>,
    },
    /// A move was judged.
    Judged {
        /// The request.
        request: RequestId,
        /// The judgement.
        verdict: Box<Verdict>,
    },
    /// The engine gave its second opinion on a move (D2131).
    Opinion {
        /// The request.
        request: RequestId,
        /// What the engine thinks.
        verdict: Box<OpinionVerdict>,
    },
    /// "Continue against the engine" (D2140): the problem is over and the
    /// game goes on from its position, against the engine.
    Continue,
}

/// Work for [`perform`](crate::perform): reading and writing files and the
/// solver (D1936).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TacticsEffect {
    /// Reads `tactics.toml`; answer [`TacticsMessage::StoreLoaded`].
    LoadStore {
        /// The file.
        path: PathBuf,
    },
    /// Writes `tactics.toml`; answer [`TacticsMessage::StoreSaved`].
    SaveStore {
        /// The file.
        path: PathBuf,
        /// What to write.
        store: Box<TacticsStore>,
    },
    /// Reads a PGN file as a source; answer [`TacticsMessage::Opened`].
    OpenFile {
        /// Correlates the answer.
        request: RequestId,
        /// The file.
        path: PathBuf,
    },
    /// Looks for the next problem; answer [`TacticsMessage::Found`].
    Find {
        /// Correlates the answer.
        request: RequestId,
        /// The games.
        source: SourceHandle,
        /// What to look for.
        query: Box<FindQuery>,
    },
    /// Asks an engine whether a move is as good as the one of the game;
    /// answer [`TacticsMessage::Opinion`].
    Opinion {
        /// Correlates the answer.
        request: RequestId,
        /// The question.
        job: Box<OpinionJob>,
    },
    /// Judges a move with the solver; answer [`TacticsMessage::Judged`].
    Judge {
        /// Correlates the answer.
        request: RequestId,
        /// The move and its rule.
        job: Box<JudgeJob>,
    },
}

/// A start that waits for the answer to "save the changes of the game?"
/// (D2145).
#[derive(Debug, Clone, PartialEq)]
pub struct TacticsStartRequest {
    /// What the dialog chose.
    pub config: TacticsConfig,
    /// The seed of the random order.
    pub seed: u64,
}

/// A remark about the training, as data (the front-end translates it with
/// the key of [`TacticsNotice::message_key`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TacticsNotice {
    /// A game against an engine, a tournament, an annotation, an engine
    /// mode or the guided review is on (D1931).
    Busy,
    /// The game in the editor has unsaved changes (D1929).
    UnsavedGame,
    /// No PGN file was chosen, or the current base is not usable.
    NoSource,
    /// The file could not be read.
    SourceUnreadable,
    /// The source has no game.
    NoGames,
    /// The database is in the old SI3 format (D1988).
    SourceNeedsUpgrade,
    /// The database files are damaged or of another version (D1988).
    SourceCorrupt,
    /// The database has an unfinished transaction (D1988).
    SourceUnfinished,
    /// The editor is locked while a problem is on (D1930).
    Locked,
    /// A request took too long and was dropped (D1934).
    TimedOut,
    /// The solver could not judge the move (D1923).
    Inconclusive,
    /// There is no hint for this position.
    NoHint,
    /// `tactics.toml` could not be written.
    StoreNotSaved,
    /// `tactics.toml` could not be read and was set aside.
    StoreUnreadable,
    /// `tactics.toml` is from a newer version of the program and will not be
    /// changed (D1954).
    StoreNewer,
    /// No engine of the list can be asked (D2131, D2140).
    NoEngine,
    /// No UCI engine of the list can play the game that "Continue" starts
    /// (D2162).
    NoEngineToPlay,
    /// "Continue" would let go of the game set aside, which has unsaved
    /// changes (D2140).
    ParkedUnsaved,
    /// The engine did not answer: the move was judged by the game alone
    /// (D2133).
    OpinionUnavailable,
    /// The database of the session was closed or changed, so the session
    /// ended (D1953).
    SourceChanged,
}

impl TacticsNotice {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            TacticsNotice::Busy => "tactics-notice-busy",
            TacticsNotice::UnsavedGame => "tactics-notice-unsaved-game",
            TacticsNotice::NoSource => "tactics-notice-no-source",
            TacticsNotice::SourceUnreadable => "tactics-notice-source-unreadable",
            TacticsNotice::NoGames => "tactics-notice-no-games",
            TacticsNotice::SourceNeedsUpgrade => "tactics-notice-source-upgrade",
            TacticsNotice::SourceCorrupt => "tactics-notice-source-corrupt",
            TacticsNotice::SourceUnfinished => "tactics-notice-source-unfinished",
            TacticsNotice::Locked => "tactics-notice-locked",
            TacticsNotice::TimedOut => "tactics-notice-timed-out",
            TacticsNotice::Inconclusive => "tactics-notice-inconclusive",
            TacticsNotice::NoHint => "tactics-notice-no-hint",
            TacticsNotice::StoreNotSaved => "tactics-notice-store-not-saved",
            TacticsNotice::StoreUnreadable => "tactics-notice-store-unreadable",
            TacticsNotice::NoEngine => "tactics-notice-no-engine",
            TacticsNotice::NoEngineToPlay => "tactics-notice-no-engine-to-play",
            TacticsNotice::ParkedUnsaved => "tactics-notice-parked-unsaved",
            TacticsNotice::OpinionUnavailable => "tactics-notice-opinion-unavailable",
            TacticsNotice::StoreNewer => "tactics-notice-store-newer",
            TacticsNotice::SourceChanged => "tactics-notice-source-changed",
        }
    }
}
