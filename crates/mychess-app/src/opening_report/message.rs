//! Messages, effects, answers and notices of the opening report (spec M7-2,
//! slice b, decisions D3600-D3650).

use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use mychess_core::{Move, PartialDate, Position};
use mychess_db::PlayerStats;
use mychess_db::report::{OpeningReport, ReportLabels, ReportOptions};

use super::favorites::{Favorites, LoadedFavorites};
use super::prefs::{OpeningReportPrefs, ReportFormat, ReportPart};
use crate::bases::{OpenControl, SharedSource};
use crate::charts::StatsPrefs;
use crate::error::{FileError, RequestId};
use crate::shared::SharedEco;
use crate::spelling_ui::Shared;

/// A finished report, shared between the state, the messages and the
/// effects (compared by identity).
#[derive(Debug, Clone)]
pub struct SharedReport(Arc<OpeningReport>);

impl SharedReport {
    pub(crate) fn new(report: OpeningReport) -> SharedReport {
        SharedReport(Arc::new(report))
    }
}

impl Deref for SharedReport {
    type Target = OpeningReport;

    fn deref(&self) -> &OpeningReport {
        &self.0
    }
}

impl PartialEq for SharedReport {
    fn eq(&self, other: &SharedReport) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SharedReport {}

/// A folder in the temporary directory, removed when the last handle goes
/// (D3615: the HTML of "View HTML").
#[derive(Debug, Clone)]
pub struct SharedTemp(Arc<tempfile::TempDir>);

impl SharedTemp {
    pub(crate) fn new(dir: tempfile::TempDir) -> SharedTemp {
        SharedTemp(Arc::new(dir))
    }

    /// The folder.
    pub fn path(&self) -> &std::path::Path {
        self.0.path()
    }
}

impl PartialEq for SharedTemp {
    fn eq(&self, other: &SharedTemp) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SharedTemp {}

/// What a click on the text of the report selects (D3622); the games of the
/// selection become the filter, each with the ply of the position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportSelector {
    /// Every game of the report (the number of games).
    All,
    /// The games that show positional theme `n` (0 to 9).
    Theme(usize),
    /// The games of footnote `n` of the theory table.
    Note(u32),
    /// The games that reach the position by this move order.
    MoveOrder(u32),
    /// The games that end in endgame class `n` (0 to 7).
    Endgame(usize),
}

impl ReportSelector {
    pub(crate) fn engine(self) -> mychess_db::report::Selector {
        use mychess_db::report::Selector;
        match self {
            ReportSelector::All => Selector::All,
            ReportSelector::Theme(n) => Selector::Theme(n),
            ReportSelector::Note(n) => Selector::Note(n),
            ReportSelector::MoveOrder(id) => Selector::MoveOrder(id),
            ReportSelector::Endgame(n) => Selector::Endgame(n),
        }
    }
}

/// An edit of the favorites (D3618).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FavoriteMessage {
    /// Adds the position of the board under `name`.
    Add {
        /// The name (trimmed; not empty, not used).
        name: String,
    },
    /// Renames the favorite at `index`.
    Rename {
        /// Its place in the list.
        index: usize,
        /// The new name.
        name: String,
    },
    /// Removes the favorite at `index`.
    Delete(usize),
    /// Moves the favorite at `index` one place up.
    MoveUp(usize),
    /// Moves the favorite at `index` one place down.
    MoveDown(usize),
    /// Reads the favorites of a Scid `reports.dat` (D3619).
    Import(PathBuf),
    /// "Generate reports": one file per favorite in `dir`, for the
    /// database in use (D3620).
    GenerateAll {
        /// The folder.
        dir: PathBuf,
        /// The part of the report written.
        part: ReportPart,
        /// The format of the files.
        format: ReportFormat,
    },
    /// Stops the generation of the reports after the one in progress.
    StopBatch,
}

/// An event of the opening report.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OpeningReportMessage {
    /// "Opening Report" of the Tools menu: opens the window and makes the
    /// report of the position of the board.
    Generate,
    /// Makes the report of favorite `index` (D3650: the game is not
    /// touched).
    OpenFavorite(usize),
    /// "Update": makes the report of the same position again, over the
    /// database in use now.
    Refresh,
    /// "Stop" of the progress window; the window opens with what was done.
    Stop,
    /// Closes the window and stops the job in flight.
    Close,
    /// "Exclude": leaves a move of the position out of the theory table
    /// (`None` is "none") and makes the report again.
    SetExclude(Option<String>),
    /// The words of the documents, already translated.
    SetLabels(Box<ReportLabels>),
    /// The options changed ("OK" of the options dialog); the report is not
    /// made again until [`OpeningReportMessage::Refresh`].
    SetPrefs(Box<OpeningReportPrefs>),
    /// "Defaults" of the options dialog.
    ResetPrefs,
    /// A click on an element of the text: the filter of the database of the
    /// report becomes the games of the selection (D3601).
    Select(ReportSelector),
    /// A click on a game reference: loads game `n` at the position.
    LoadGame(u32),
    /// "Merge games": the best 50 games into the game of the editor (D3614).
    MergeGames,
    /// Writes the report to `path` (the extension of the format is added
    /// unless the name has one; D3616).
    Save {
        /// The file chosen in the native dialog.
        path: PathBuf,
        /// The part of the report.
        part: ReportPart,
        /// The format.
        format: ReportFormat,
    },
    /// "View HTML": writes the report to a temporary file and opens it in
    /// the browser (D3615).
    ViewHtml,
    /// An edit of the favorites.
    Favorite(FavoriteMessage),

    /// The answer of [`OpeningReportEffect::Compute`].
    Computed {
        /// The request it answers.
        request: RequestId,
        /// The report, or why there is none.
        outcome: Box<ComputeOutcome>,
    },
    /// The answer of [`OpeningReportEffect::Save`].
    Saved {
        /// The request it answers.
        request: RequestId,
        /// The file written, or why not.
        result: Result<PathBuf, SaveFailure>,
    },
    /// The answer of [`OpeningReportEffect::WriteHtml`].
    HtmlWritten {
        /// The request it answers.
        request: RequestId,
        /// The file, or `None` when it could not be written.
        result: Option<(PathBuf, SharedTemp)>,
    },
    /// The answer of [`OpeningReportEffect::OpenFile`].
    FileOpened {
        /// Whether the system took the file.
        ok: bool,
    },
    /// The answer of [`OpeningReportEffect::LoadFavorites`].
    FavoritesLoaded {
        /// The request it answers.
        request: RequestId,
        /// The favorites, or why not.
        result: Result<LoadedFavorites, FileError>,
    },
    /// The answer of [`OpeningReportEffect::SaveFavorites`].
    FavoritesSaved {
        /// The request it answers.
        request: RequestId,
        /// Whether the file was written.
        ok: bool,
    },
    /// The answer of [`OpeningReportEffect::ReadReportsDat`].
    ReportsDatRead {
        /// The request it answers.
        request: RequestId,
        /// The favorites found and how many entries were ignored.
        result: Result<ReportsDat, FileError>,
    },
    /// The answer of [`OpeningReportEffect::BatchItem`].
    BatchItem {
        /// The request it answers.
        request: RequestId,
        /// The file written, or why not.
        result: Result<PathBuf, BatchFailure>,
    },
}

/// What a `reports.dat` held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportsDat {
    /// `(name, moves)` of each favorite, in file order.
    pub favorites: Vec<(String, String)>,
    /// Commands and entries that were not understood.
    pub ignored: u32,
}

/// Why a report could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReportFailure {
    /// The position does not fit the piece lists of the database (more
    /// than 16 pieces of a side).
    UnsupportedPosition,
    /// More distinct moves than the tree holds.
    TooManyMoves,
    /// The database could not be read.
    Read,
    /// The line to the position is not legal.
    BadLine,
    /// Something that should not happen (the thread failed).
    Internal,
}

/// What the generation came to.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ComputeOutcome {
    /// The report, complete or interrupted (`core.interrupted`).
    Ready {
        /// The report.
        report: SharedReport,
        /// The date of the generation.
        generated: PartialDate,
    },
    /// The search of the games was stopped: the window opens with the text
    /// "Search interrupted" and no report (spec section 1.2).
    SearchInterrupted,
    /// The report could not be made.
    Failed(ReportFailure),
}

/// Why a report was not written.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SaveFailure {
    /// The file is one of the open databases (D2013).
    ProtectedFile,
    /// The file exists and its replacement was not confirmed (only the
    /// player report raises it, D4211).
    NeedsConfirm(PathBuf),
    /// The file could not be written.
    File(FileError),
    /// The thread failed.
    Internal,
}

/// Why a report of the batch was not written.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchFailure {
    /// Stopped by the user.
    Stopped,
    /// The report could not be made.
    Report(ReportFailure),
    /// The file could not be written.
    Save(SaveFailure),
}

/// The line of moves that leads to the reference position of a report.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    /// Where the line starts; `None` is the standard start position.
    pub start: Option<Position>,
    /// The moves.
    pub stem: Vec<Move>,
}

/// Everything the generation of a report needs, owned.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputeJob {
    pub(crate) request: RequestId,
    pub(crate) source: SharedSource,
    pub(crate) reference: Reference,
    pub(crate) options: ReportOptions,
    pub(crate) stats: StatsPrefs,
    pub(crate) eco: Option<SharedEco>,
    pub(crate) player_stats: Option<Shared<PlayerStats>>,
    pub(crate) today: Option<PartialDate>,
    /// Progress and cancellation of phase 1 (the search of the games).
    pub(crate) search: OpenControl,
    /// Progress and cancellation of phase 2 (the generation).
    pub(crate) generate: OpenControl,
}

/// The data a document needs besides the report.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderJob {
    pub(crate) report: SharedReport,
    pub(crate) options: ReportOptions,
    pub(crate) labels: Box<ReportLabels>,
    pub(crate) part: ReportPart,
    pub(crate) format: ReportFormat,
    pub(crate) database_name: String,
    pub(crate) generated: PartialDate,
}

/// Writes a report to a file.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveJob {
    pub(crate) request: RequestId,
    pub(crate) path: PathBuf,
    pub(crate) render: RenderJob,
    /// The files of the open databases, which are never overwritten.
    pub(crate) protected: Vec<PathBuf>,
}

/// Makes the report of one favorite and writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchJob {
    pub(crate) request: RequestId,
    pub(crate) path: PathBuf,
    pub(crate) compute: ComputeJob,
    pub(crate) part: ReportPart,
    pub(crate) format: ReportFormat,
    pub(crate) labels: Box<ReportLabels>,
    pub(crate) database_name: String,
    pub(crate) protected: Vec<PathBuf>,
}

/// What `perform` does for the opening report.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OpeningReportEffect {
    /// Searches the games and makes the report on a blocking thread;
    /// answers with [`OpeningReportMessage::Computed`].
    Compute(Box<ComputeJob>),
    /// Renders a document and writes it atomically; answers with
    /// [`OpeningReportMessage::Saved`].
    Save(Box<SaveJob>),
    /// Writes `html` to a temporary file (in `dir`, or in a new temporary
    /// folder); answers with [`OpeningReportMessage::HtmlWritten`].
    WriteHtml {
        /// Correlates the answer.
        request: RequestId,
        /// The document.
        html: Box<RenderJob>,
        /// The folder of an earlier view, reused.
        dir: Option<SharedTemp>,
    },
    /// Opens a file with the program of the system; answers with
    /// [`OpeningReportMessage::FileOpened`].
    OpenFile {
        /// The file.
        path: PathBuf,
    },
    /// Reads the favorites file; answers with
    /// [`OpeningReportMessage::FavoritesLoaded`].
    LoadFavorites {
        /// Correlates the answer.
        request: RequestId,
        /// The file.
        path: PathBuf,
    },
    /// Writes the favorites file atomically; answers with
    /// [`OpeningReportMessage::FavoritesSaved`].
    SaveFavorites {
        /// Correlates the answer.
        request: RequestId,
        /// The file.
        path: PathBuf,
        /// The list.
        favorites: Favorites,
        /// Copy the file to `<file>.bak` first (it holds what the list
        /// does not); when the copy fails nothing is written.
        backup: bool,
    },
    /// Reads a Scid `reports.dat` as data; answers with
    /// [`OpeningReportMessage::ReportsDatRead`].
    ReadReportsDat {
        /// Correlates the answer.
        request: RequestId,
        /// The file.
        path: PathBuf,
    },
    /// Makes and writes the report of a favorite; answers with
    /// [`OpeningReportMessage::BatchItem`].
    BatchItem(Box<BatchJob>),
}

/// A remark about the opening report. The front-end translates from the
/// variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpeningReportNotice {
    /// The database of the position is being reloaded (`oprep-err-unavailable`).
    Unavailable,
    /// There is no report on the window (`oprep-err-no-report`).
    NoReport,
    /// The report no longer matches the database (`oprep-stale`).
    Stale,
    /// The filter of the database is being rewritten by a job
    /// (`oprep-err-busy`).
    Busy,
    /// The report could not be made (`oprep-err-failed`).
    Failed,
    /// The position has too many pieces for the database
    /// (`oprep-err-position`).
    UnsupportedPosition,
    /// The line of the report is not legal (`oprep-err-line`).
    BadLine,
    /// The report was interrupted (`oprep-partial`).
    Interrupted,
    /// The filter was set (`oprep-filter-set`).
    FilterSet,
    /// The report has no game to merge (`oprep-err-merge-empty`).
    NothingToMerge,
    /// The report was written (`oprep-saved`).
    Saved,
    /// The report was not written (`oprep-err-save`).
    SaveFailed,
    /// The file is one of the open databases (`oprep-err-save-base`).
    SaveProtected,
    /// The browser could not be started (`oprep-err-view`).
    ViewFailed,
    /// The favorites are being read (`oprep-fav-loading`).
    FavoritesLoading,
    /// The name of the favorite is empty (`oprep-fav-err-empty`).
    FavoriteNameEmpty,
    /// The name of the favorite is used (`oprep-fav-err-taken`).
    FavoriteNameTaken,
    /// The name or the moves are too long, or there are too many
    /// favorites (`oprep-fav-err-limit`).
    FavoriteLimit,
    /// The favorite does not exist (`oprep-fav-err-missing`).
    FavoriteMissing,
    /// The game starts from another position (`oprep-fav-err-start`).
    FavoriteStart,
    /// The moves of the favorite are not legal (`oprep-fav-err-moves`).
    FavoriteMoves,
    /// The favorites could not be written (`oprep-fav-err-save`).
    FavoritesSaveFailed,
    /// The favorites file could not be read (`oprep-fav-err-load`).
    FavoritesLoadFailed,
    /// The favorites file was damaged and was kept aside
    /// (`oprep-fav-err-corrupt`).
    FavoritesCorrupt,
    /// Favorites were imported (`oprep-fav-imported`).
    FavoritesImported {
        /// How many were added.
        added: u32,
        /// How many were left out (repeated, empty, or not understood).
        skipped: u32,
    },
    /// The file could not be read (`oprep-fav-err-import`).
    ImportFailed,
    /// There is no favorite to generate (`oprep-fav-none`).
    NoFavorites,
    /// The reports of the favorites were written (`oprep-fav-generated`).
    BatchDone {
        /// How many files were written.
        written: u32,
        /// How many reports were not written.
        failed: u32,
    },
}

impl OpeningReportNotice {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            OpeningReportNotice::Unavailable => "oprep-err-unavailable",
            OpeningReportNotice::NoReport => "oprep-err-no-report",
            OpeningReportNotice::Stale => "oprep-stale",
            OpeningReportNotice::Busy => "oprep-err-busy",
            OpeningReportNotice::Failed => "oprep-err-failed",
            OpeningReportNotice::UnsupportedPosition => "oprep-err-position",
            OpeningReportNotice::BadLine => "oprep-err-line",
            OpeningReportNotice::Interrupted => "oprep-partial",
            OpeningReportNotice::FilterSet => "oprep-filter-set",
            OpeningReportNotice::NothingToMerge => "oprep-err-merge-empty",
            OpeningReportNotice::Saved => "oprep-saved",
            OpeningReportNotice::SaveFailed => "oprep-err-save",
            OpeningReportNotice::SaveProtected => "oprep-err-save-base",
            OpeningReportNotice::ViewFailed => "oprep-err-view",
            OpeningReportNotice::FavoritesLoading => "oprep-fav-loading",
            OpeningReportNotice::FavoriteNameEmpty => "oprep-fav-err-empty",
            OpeningReportNotice::FavoriteNameTaken => "oprep-fav-err-taken",
            OpeningReportNotice::FavoriteLimit => "oprep-fav-err-limit",
            OpeningReportNotice::FavoriteMissing => "oprep-fav-err-missing",
            OpeningReportNotice::FavoriteStart => "oprep-fav-err-start",
            OpeningReportNotice::FavoriteMoves => "oprep-fav-err-moves",
            OpeningReportNotice::FavoritesSaveFailed => "oprep-fav-err-save",
            OpeningReportNotice::FavoritesLoadFailed => "oprep-fav-err-load",
            OpeningReportNotice::FavoritesCorrupt => "oprep-fav-err-corrupt",
            OpeningReportNotice::FavoritesImported { .. } => "oprep-fav-imported",
            OpeningReportNotice::ImportFailed => "oprep-fav-err-import",
            OpeningReportNotice::NoFavorites => "oprep-fav-none",
            OpeningReportNotice::BatchDone { .. } => "oprep-fav-generated",
        }
    }
}
