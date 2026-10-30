//! The messages of the tournament (spec M8-2): what the front-end sends and
//! what the tasks answer. The glue with `AppState` is
//! `state::tournament_flow`.

use std::path::PathBuf;
use std::time::Instant;

use mychess_core::{GameResult, PartialDate};

use super::config::{ConfigError, TournamentConfig};
use super::driver::{PrepareFailure, SearchOutcome, SideReady};
use super::export::{ExportFormat, ExportLabels, ExportTable};
use super::openings::OpeningSuite;
use super::resources::SystemResources;
use super::runner::Decision;
use super::store::{ResumableTournament, SavedTournament, StoreError};
use crate::bases::{BaseError, Reloaded};
use crate::play::PlayBook;

/// What a failed read of a saved tournament tells (the error of the store
/// without the operating system's error, so a message can be compared).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreFailure {
    /// The file could not be read.
    Io,
    /// The file is not a valid tournament.
    Invalid,
    /// The file is from another version.
    Version,
    /// The file is too large.
    TooLarge,
}

impl From<&StoreError> for StoreFailure {
    fn from(error: &StoreError) -> StoreFailure {
        match error {
            StoreError::Io { .. } => StoreFailure::Io,
            StoreError::Invalid(_) | StoreError::Config(_) => StoreFailure::Invalid,
            StoreError::UnsupportedVersion(_) => StoreFailure::Version,
            StoreError::TooLarge => StoreFailure::TooLarge,
        }
    }
}

/// Why the book, the suite or the directory could not be prepared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadFailure {
    /// The configuration refers to a file that is not there.
    Config(ConfigError),
    /// The suite of openings is empty or unreadable.
    Suite,
    /// The directory of the tournaments cannot be used.
    Directory,
}

/// What `perform` prepared before the tournament can start.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The book, if the tournament uses one and it could be read.
    pub book: Option<PlayBook>,
    /// The book was asked for and could not be read.
    pub book_unreadable: bool,
    /// The suite of openings.
    pub suite: Option<OpeningSuite>,
    /// The base of the file names.
    pub slug: String,
    /// The directory of the files of the tournament.
    pub dir: PathBuf,
    /// Today.
    pub today: PartialDate,
    /// The seed of the draws (the one of the configuration, or a new one).
    pub seed: u64,
    /// The local tables, when the tablebase rule is on and a table could be
    /// opened (D2538).
    pub tablebase: Option<super::tablebase::TournamentTablebase>,
}

/// What writing a game to the `.si4` file of the destination slot did
/// (D2519).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotWrite {
    /// The game is in the file and the database was opened again.
    Reopened(Reloaded),
    /// The game is in the file but the database could not be opened again:
    /// the slot shows the old file and must be closed and opened.
    WrittenNotReopened(BaseError),
    /// Nothing was written.
    NotWritten(BaseError),
}

/// An event of the tournament.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TournamentMessage {
    /// Start a tournament (D1289: validated first).
    Start {
        /// The configuration.
        config: Box<TournamentConfig>,
        /// When.
        at: Instant,
    },
    /// Resume a saved tournament (D1285). `replacements` give an engine of
    /// the list (by identity) to each participant (by index) whose engine
    /// is gone.
    Resume {
        /// The state file.
        path: PathBuf,
        /// Replacement engines.
        replacements: Vec<(usize, u64)>,
        /// When.
        at: Instant,
    },
    /// Looks for the tournaments that can be resumed.
    ListResumable,
    /// The answer to [`TournamentMessage::ListResumable`].
    Resumable(Vec<ResumableTournament>),
    /// The book, the suite and the directory are ready.
    Loaded {
        /// The request.
        request: u64,
        /// What was prepared.
        result: Box<Result<Loaded, LoadFailure>>,
        /// When.
        at: Instant,
    },
    /// A saved tournament was read.
    StateLoaded {
        /// The request.
        request: u64,
        /// The state.
        result: Box<Result<SavedTournament, StoreFailure>>,
        /// When.
        at: Instant,
    },
    /// The engines of a game are ready.
    Prepared {
        /// The epoch of the request.
        epoch: u64,
        /// The result.
        result: Result<[SideReady; 2], PrepareFailure>,
        /// When.
        at: Instant,
    },
    /// An engine answered a search.
    Searched {
        /// The epoch of the request.
        epoch: u64,
        /// The answer.
        outcome: SearchOutcome,
        /// When.
        at: Instant,
    },
    /// A game was appended to the PGN of the tournament.
    GameSaved {
        /// The number of the game.
        number: u32,
        /// Whether it worked.
        ok: bool,
    },
    /// The state was written.
    StateSaved {
        /// Whether it worked.
        ok: bool,
    },
    /// A command to the engines was carried out.
    PoolDone,
    /// Ask for the cores and the free memory of the machine (D2502).
    ProbeResources,
    /// The answer to [`TournamentMessage::ProbeResources`].
    Resources(SystemResources),
    /// A game was added to the `.si4` file of the destination slot, or not
    /// (D2519).
    SlotAdded {
        /// What happened to the file of the slot.
        result: Box<SlotWrite>,
    },
    /// Show the game of this lane in the live tab (D2508).
    Watch(usize),
    /// Pause after the move in progress (D1284).
    Pause,
    /// Go on after a pause or a stop.
    Continue(Instant),
    /// Stop when the game ends (D1284).
    StopAfterGame(Instant),
    /// End the tournament (D1284); the front-end asks for confirmation.
    Abort(Instant),
    /// Decide the game in progress (D1275).
    Adjudicate(GameResult, Instant),
    /// Drop the game in progress without result (D1286).
    SkipCurrent(Instant),
    /// Skip a game that waits (D1286).
    SkipPending(u32),
    /// Play a game again (D1286).
    Redo(u32),
    /// Move a game that waits to the front of the queue (D1286).
    MoveToFront(u32),
    /// The answer to the question about a participant that failed too often
    /// (D1269).
    Decide(Decision, Instant),
    /// Open the PGN of the tournament as a database (or in the chooser), the
    /// "Open PGN as database" button of the summary (M8-2b, D1363).
    OpenPgn,
    /// Leave the tournament view (a tournament in progress is kept).
    Close,
    /// Write a table to a file (D1287).
    Export {
        /// The table.
        table: ExportTable,
        /// The format.
        format: ExportFormat,
        /// The column headers, translated.
        labels: Box<ExportLabels>,
        /// The file.
        path: PathBuf,
    },
    /// The table was written (or not).
    Exported {
        /// Whether it worked.
        ok: bool,
    },
}
