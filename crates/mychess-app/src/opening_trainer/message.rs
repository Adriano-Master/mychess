//! Messages, effects and notices of the opening trainer (spec M8-5,
//! D2228-D2237).

use std::path::PathBuf;
use std::time::Instant;

use super::build::{BuildError, BuildJob, BuildOutput};
use super::config::OpeningConfig;
use super::store::{LoadedStore, OpeningStore};
use crate::error::{FileError, RequestId};

/// An event of the opening trainer.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OpeningMessage {
    /// Opens the configuration dialog.
    Open,
    /// Closes the dialog without starting (a session that is on goes on).
    CloseDialog,
    /// Starts a session (D2228). `seed` draws the lines and `today` (days
    /// since 1970-01-01, counted on the UTC clock) dates the repetitions:
    /// the front-end reads the clock, `update` never does (D2214, D2223).
    Start {
        /// What the dialog chose.
        config: Box<OpeningConfig>,
        /// The seed of the draw.
        seed: u64,
        /// The day, for the spaced repetition.
        today: u32,
    },
    /// The hint button (D2217).
    Hint,
    /// "Next line" (D2219).
    Next,
    /// "Repeat the line" (D2219).
    Retry,
    /// Shows or hides the report of the repertoire (D2224).
    Report,
    /// Ends the session and puts the previous game back.
    Close,
    /// Clears the score of the repertoire of the configuration (D2226).
    ResetStats(Box<OpeningConfig>),
    /// The one-second tick of the front-end (the *watchdog*, D2231).
    Tick(Instant),

    // ---- answers to effects ----
    /// `opening_trainer.toml` was read.
    StoreLoaded(Box<LoadedStore>),
    /// `opening_trainer.toml` was written.
    StoreSaved(Result<(), FileError>),
    /// The repertoire was built.
    Built {
        /// The request.
        request: RequestId,
        /// The repertoire, or why not.
        result: Result<Box<BuildOutput>, BuildError>,
    },
}

/// Work for [`perform`](crate::perform): files and the build (D2235).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OpeningEffect {
    /// Reads `opening_trainer.toml`; answer [`OpeningMessage::StoreLoaded`].
    LoadStore {
        /// The file.
        path: PathBuf,
    },
    /// Writes `opening_trainer.toml`; answer [`OpeningMessage::StoreSaved`].
    SaveStore {
        /// The file.
        path: PathBuf,
        /// What to write.
        store: Box<OpeningStore>,
    },
    /// Builds the repertoire; answer [`OpeningMessage::Built`].
    Build {
        /// Correlates the answer.
        request: RequestId,
        /// What to build.
        job: Box<BuildJob>,
    },
}

/// A start that waits for the answer to "save the changes of the game?"
/// (D2229).
#[derive(Debug, Clone, PartialEq)]
pub struct OpeningStartRequest {
    /// What the dialog chose.
    pub config: OpeningConfig,
    /// The seed of the draw.
    pub seed: u64,
    /// The day.
    pub today: u32,
}

/// A remark about the trainer, as data (the front-end translates it with
/// the key of [`OpeningNotice::message_key`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpeningNotice {
    /// Another mode owns the board or the engines (D2232).
    Busy,
    /// No open database has the type of the side asked for (D2203).
    NoRepertoire,
    /// No file was chosen, or there is no current base.
    NoSource,
    /// The file could not be read.
    SourceUnreadable,
    /// The database is in the old SI3 format.
    SourceNeedsUpgrade,
    /// The database files are damaged or of another version.
    SourceCorrupt,
    /// The database has an unfinished transaction.
    SourceUnfinished,
    /// The book could not be read.
    BookUnreadable,
    /// The mask could not be read.
    MaskUnreadable,
    /// The repertoire has no move for the side to train (D2212).
    EmptyRepertoire,
    /// Only one of the two colours has moves; the session uses it (D2212).
    OneSideEmpty,
    /// The repertoire was cut by a limit (D2210).
    Truncated,
    /// The editor is locked while the user is to move (D2230).
    Locked,
    /// A request took too long and was dropped (D2231).
    TimedOut,
    /// There is no hint for this position.
    NoHint,
    /// The move is in the repertoire but marked dubious (D2216).
    Dubious,
    /// The move is not in the repertoire (D2216).
    OutOfRepertoire,
    /// `opening_trainer.toml` could not be written.
    StoreNotSaved,
    /// `opening_trainer.toml` could not be read and was set aside.
    StoreUnreadable,
    /// `opening_trainer.toml` is from a newer version and will not be
    /// changed (D2225).
    StoreNewer,
    /// The database of the session was closed or changed before the
    /// repertoire was ready (D2237).
    SourceChanged,
    /// The game that was in the editor before the training could not be put
    /// back: the editor held another game with unsaved changes and no slot
    /// could keep it (D2256).
    GameNotRestored,
}

impl OpeningNotice {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            OpeningNotice::Busy => "opening-notice-busy",
            OpeningNotice::NoRepertoire => "opening-notice-no-repertoire",
            OpeningNotice::NoSource => "opening-notice-no-source",
            OpeningNotice::SourceUnreadable => "opening-notice-source-unreadable",
            OpeningNotice::SourceNeedsUpgrade => "opening-notice-source-upgrade",
            OpeningNotice::SourceCorrupt => "opening-notice-source-corrupt",
            OpeningNotice::SourceUnfinished => "opening-notice-source-unfinished",
            OpeningNotice::BookUnreadable => "opening-notice-book-unreadable",
            OpeningNotice::MaskUnreadable => "opening-notice-mask-unreadable",
            OpeningNotice::EmptyRepertoire => "opening-notice-empty-repertoire",
            OpeningNotice::OneSideEmpty => "opening-notice-one-side-empty",
            OpeningNotice::Truncated => "opening-notice-truncated",
            OpeningNotice::Locked => "opening-notice-locked",
            OpeningNotice::TimedOut => "opening-notice-timed-out",
            OpeningNotice::NoHint => "opening-notice-no-hint",
            OpeningNotice::Dubious => "opening-notice-dubious",
            OpeningNotice::OutOfRepertoire => "opening-notice-out-of-repertoire",
            OpeningNotice::StoreNotSaved => "opening-notice-store-not-saved",
            OpeningNotice::StoreUnreadable => "opening-notice-store-unreadable",
            OpeningNotice::StoreNewer => "opening-notice-store-newer",
            OpeningNotice::SourceChanged => "opening-notice-source-changed",
            OpeningNotice::GameNotRestored => "opening-notice-game-not-restored",
        }
    }
}
