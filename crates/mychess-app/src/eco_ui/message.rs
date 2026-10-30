//! Messages, notices, jobs and outcomes of the ECO tools (spec M5-7b,
//! D1700-D1749).

use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use mychess_core::{Color, PartialDate};
use mychess_db::{ClassifyReport, EcoStatsTable, Filter};

use crate::bases::{BaseError, FileStamp, SharedSource, SlotId};
use crate::error::RequestId;
use crate::shared::{SharedEco, SharedGame};

/// The statistics table of a database, shared between the state and the
/// answer of the effect that built it; compared by identity.
#[derive(Debug, Clone)]
pub struct SharedStats(Arc<EcoStatsTable>);

impl SharedStats {
    /// Shares `table`.
    pub fn new(table: EcoStatsTable) -> SharedStats {
        SharedStats(Arc::new(table))
    }
}

impl Deref for SharedStats {
    type Target = EcoStatsTable;

    fn deref(&self) -> &EcoStatsTable {
        &self.0
    }
}

impl PartialEq for SharedStats {
    fn eq(&self, other: &SharedStats) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SharedStats {}

/// Which games the statistics of the browser count (D1183).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserScope {
    /// Every game of the database (as Scid does).
    #[default]
    WholeBase,
    /// Only the games of the filter of the slot (an extension).
    Filter,
}

/// Which games the "Classify games" dialog looks at (D1710).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClassifyChoice {
    /// Every game: the old codes are replaced, or erased when nothing
    /// matches ("All games (overwrite old ECO codes)").
    All,
    /// Only the games without a code ("Only games with no ECO code yet").
    #[default]
    NoCode,
    /// Only the games of the filter.
    Filter,
    /// The games played in the last year ("last year").
    LastYear,
    /// The games played in the last month.
    LastMonth,
    /// The games played on or after a date typed by the user.
    Since,
}

impl ClassifyChoice {
    /// Whether the choice needs today's date.
    pub fn needs_today(self) -> bool {
        matches!(self, ClassifyChoice::LastYear | ClassifyChoice::LastMonth)
    }
}

/// An event of the ECO tools.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EcoMessage {
    /// Opens the ECO browser on the current database. `glossary` is the
    /// list of `(from, to)` pairs that translate the names of the openings
    /// (empty: names as written in the file); the pairs apply from the last
    /// to the first (`translate_eco_name`).
    OpenBrowser {
        /// The translation pairs of the interface language.
        glossary: Vec<(String, String)>,
    },
    /// Closes the browser.
    CloseBrowser,
    /// Goes to a node of the ECO tree (`""`, `B`, `B9`, `B90`, `B90a`).
    BrowseTo(String),
    /// One level up (`B90` to `B9`).
    BrowseUp,
    /// The text of the search field changed.
    SetFind(String),
    /// Search the openings by the text of the field (empty: back to the
    /// summary of the node).
    SubmitFind,
    /// Counts all the games or only the filter.
    SetScope(BrowserScope),
    /// "Import this line": a new game with the moves of entry `index` of
    /// the list shown (asks about unsaved changes first).
    ImportLine(usize),
    /// Asks for an `.eco` file (becomes the preference and is loaded).
    LoadBookFile,

    /// Opens the "Classify games" dialog.
    OpenClassify,
    /// Closes it (cancels a run in flight).
    CloseClassify,
    /// Chooses the games to look at.
    SetClassifyChoice(ClassifyChoice),
    /// The text of the date of [`ClassifyChoice::Since`].
    SetClassifySince(String),
    /// Basic (`B12`) or extended (`B12j`) codes.
    SetClassifyExtended(bool),
    /// Counts the games that would change, writing nothing.
    ClassifyPreview,
    /// Classifies and writes the codes in the file.
    ClassifyRun,

    /// Opens the "Find novelty" dialog.
    OpenNovelty,
    /// Closes it (cancels a search in flight).
    CloseNovelty,
    /// Chooses the reference database (a slot, the Clipbase included).
    SetNoveltyReference(SlotId),
    /// Only games older than the current one count.
    SetNoveltyOlderOnly(bool),
    /// Starts the search.
    FindNovelty,

    /// "Identify opening": goes to the deepest position of the game that is
    /// in the ECO book and tells the code and the name.
    Identify,

    /// The statistics table of the browser was built (the answer to
    /// [`Effect::EcoStats`](crate::Effect::EcoStats)).
    StatsBuilt {
        /// The request.
        request: RequestId,
        /// The table.
        table: SharedStats,
    },
}

/// A remark of the ECO tools for the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EcoNotice {
    /// No `.eco` file is loaded (or it is empty).
    NoBook,
    /// "Identify opening" found the opening of the game.
    Opening {
        /// The extended code (`B90a2`).
        eco: String,
        /// The name as written in the file.
        name: String,
        /// The half-move reached.
        ply: u32,
    },
    /// "Identify opening" found nothing.
    NoOpening,
    /// The classification ended.
    Classified {
        /// Games processed.
        classified: u32,
        /// Games whose code changed (or would change, with `preview`).
        changed: u32,
        /// Entries written.
        written: u32,
        /// Nothing was written: it was only a count.
        preview: bool,
        /// The run was stopped before the end.
        stopped: bool,
    },
    /// The database cannot be written (a PGN file, a read-only base, the
    /// Clipbase).
    ReadOnly,
    /// A novelty was found.
    Novelty {
        /// The move number as printed.
        move_number: u32,
        /// The side that played the move.
        color: Color,
        /// The move in SAN.
        san: String,
    },
    /// Every position of the game is known to the reference database.
    NoNovelty,
    /// The novelty search was stopped.
    NoveltyStopped,
    /// The game starts from a position of its own (no novelty search).
    NoveltyNonStandard,
    /// The novelty search failed (a game of the base cannot be read).
    NoveltyFailed,
    /// The line of the entry could not be played.
    LineFailed,
}

impl EcoNotice {
    /// The Fluent key of the remark.
    pub fn message_key(&self) -> &'static str {
        match self {
            EcoNotice::NoBook => "eco-notice-no-book",
            EcoNotice::Opening { .. } => "eco-notice-opening",
            EcoNotice::NoOpening => "eco-notice-no-opening",
            EcoNotice::Classified {
                preview: true,
                stopped: false,
                ..
            } => "eco-notice-preview",
            EcoNotice::Classified {
                preview: false,
                stopped: false,
                ..
            } => "eco-notice-classified",
            EcoNotice::Classified { stopped: true, .. } => "eco-notice-classify-stopped",
            EcoNotice::ReadOnly => "eco-notice-read-only",
            EcoNotice::Novelty { .. } => "eco-notice-novelty",
            EcoNotice::NoNovelty => "eco-notice-no-novelty",
            EcoNotice::NoveltyStopped => "eco-notice-novelty-stopped",
            EcoNotice::NoveltyNonStandard => "eco-notice-novelty-non-standard",
            EcoNotice::NoveltyFailed => "eco-notice-novelty-failed",
            EcoNotice::LineFailed => "eco-notice-line-failed",
        }
    }
}

/// The games a classification looks at, owned (the job outlives the state
/// that made it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifyScope {
    /// Every game.
    All,
    /// The games without a code.
    NoCode,
    /// The games of this filter (a copy of the one of the slot).
    Filter(Box<Filter>),
    /// The games played on or after the date (unknown dates are skipped).
    Since(PartialDate),
}

/// What [`Effect::EcoClassify`](crate::Effect::EcoClassify) does.
#[derive(Debug, Clone, PartialEq)]
pub struct EcoClassifyJob {
    pub(crate) source: SharedSource,
    pub(crate) path: Option<PathBuf>,
    /// Write the codes (else only count).
    pub(crate) write: bool,
    pub(crate) stamp: Option<FileStamp>,
    pub(crate) expected_total: u32,
    pub(crate) book: SharedEco,
    pub(crate) scope: ClassifyScope,
    pub(crate) extended: bool,
}

/// What [`Effect::EcoNovelty`](crate::Effect::EcoNovelty) does.
#[derive(Debug, Clone, PartialEq)]
pub struct EcoNoveltyJob {
    pub(crate) source: SharedSource,
    /// The `.si4` file, read through a reader of its own so the database
    /// stays free while the search goes on.
    pub(crate) path: Option<PathBuf>,
    pub(crate) game: SharedGame,
    pub(crate) book: Option<SharedEco>,
    pub(crate) older_only: bool,
    pub(crate) current_game: Option<u32>,
}

/// How a classification ended.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifyOutcome {
    /// The database opened again after a write; `None` when nothing was
    /// written or it could not be opened (`error` says why).
    pub source: Option<SharedSource>,
    /// The state of the file after the write.
    pub stamp: Option<FileStamp>,
    /// The counters (partial after a failure or a cancel).
    pub report: ClassifyReport,
    /// Why the run stopped early, if it did: what was written stays.
    pub error: Option<BaseError>,
    /// Whether codes were written (else it was a count).
    pub write: bool,
}

/// How a novelty search ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NoveltyOutcome {
    /// The novel move.
    Found {
        /// Half-moves played up to and including the move.
        ply: u32,
        /// The move in SAN.
        san: String,
        /// The move number as printed.
        move_number: u32,
        /// The side that played it.
        color: Color,
    },
    /// Every position occurs in the reference games.
    NoNovelty,
    /// The game starts from a position of its own.
    NonStandardStart,
    /// The search was stopped.
    Interrupted,
    /// A position of the game cannot be matched.
    Unsupported,
    /// A reference game cannot be read.
    Unreadable {
        /// Its number.
        game: u32,
    },
}
