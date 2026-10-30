//! Messages, effects, notices, jobs and outcomes of the spelling file, the
//! name checker and "Add Elo" (spec M6-3, slice b).

use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use mychess_db::si4::{NameChange, NameChangeReport};
use mychess_db::spelling::elo::{EloPlan, EloReport};
use mychess_db::spelling::photo::PhotoLibrary;
use mychess_db::spelling::{SpellingError, SpellingSet};
use mychess_db::{Filter, PlayerStats};

use crate::bases::{BaseError, FileStamp, OpenControl, SharedSource, SlotId};
use crate::error::{FileError, FileErrorKind, RequestId};
use crate::game_info::NameKind;

/// A value built off the interface thread and shared with the state; two
/// values are equal when they are the same allocation (the messages and
/// effects must be `Clone + PartialEq`).
#[derive(Debug)]
pub struct Shared<T>(Arc<T>);

impl<T> Shared<T> {
    /// Shares `value`.
    pub fn new(value: T) -> Shared<T> {
        Shared(Arc::new(value))
    }

    /// The shared value.
    pub fn arc(&self) -> &Arc<T> {
        &self.0
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Shared<T> {
        Shared(Arc::clone(&self.0))
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> PartialEq for Shared<T> {
    fn eq(&self, other: &Shared<T>) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> Eq for Shared<T> {}

/// The spelling file read, shared by every window.
pub type SharedSpelling = Shared<SpellingSet>;
/// The photos found.
pub type SharedPhotos = Shared<PhotoLibrary>;

/// The kind of name of the checker as `mychess-db` names it.
pub(crate) fn db_kind(kind: NameKind) -> mychess_db::si4::NameKind {
    match kind {
        NameKind::Event => mychess_db::si4::NameKind::Event,
        NameKind::Site => mychess_db::si4::NameKind::Site,
        NameKind::Round => mychess_db::si4::NameKind::Round,
        _ => mychess_db::si4::NameKind::Player,
    }
}

/// Why a spelling file could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpellingLoadError {
    /// The file could not be opened or read.
    File(FileErrorKind),
    /// The path is not a file.
    NotAFile,
    /// The file (decompressed) is above the limit of 256 MiB.
    TooLarge,
    /// The gzip data is damaged.
    Gzip,
    /// A section header opens a quote and does not close it.
    UnclosedQuote {
        /// The line, from 1.
        line: u64,
    },
    /// The reading was cancelled.
    Cancelled,
    /// A task that cannot fail failed.
    Internal,
}

impl SpellingLoadError {
    pub(crate) fn from_db(error: &SpellingError) -> SpellingLoadError {
        match error {
            SpellingError::Io { source, path } => {
                SpellingLoadError::File(FileError::from_io(source, path.clone()).kind)
            }
            SpellingError::NotAFile { .. } => SpellingLoadError::NotAFile,
            SpellingError::TooLarge { .. } => SpellingLoadError::TooLarge,
            SpellingError::Gzip(_) => SpellingLoadError::Gzip,
            SpellingError::UnclosedQuote { line } => {
                SpellingLoadError::UnclosedQuote { line: *line }
            }
            SpellingError::Cancelled => SpellingLoadError::Cancelled,
            _ => SpellingLoadError::Internal,
        }
    }

    /// The Fluent key of the reason (the message `spell-read-error` takes it
    /// as `$reason`).
    pub fn reason_key(&self) -> &'static str {
        match self {
            SpellingLoadError::File(kind) => kind.message_key(),
            SpellingLoadError::NotAFile => "spell-error-not-a-file",
            SpellingLoadError::TooLarge => "spell-error-too-large",
            SpellingLoadError::Gzip => "spell-error-gzip",
            SpellingLoadError::UnclosedQuote { .. } => "spell-error-unclosed-quote",
            SpellingLoadError::Cancelled => "spell-error-cancelled",
            SpellingLoadError::Internal => "base-error-internal",
        }
    }
}

/// What the reading of the file found (the counts of correct names, rule 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpellingCounts {
    /// Correct player names.
    pub players: u32,
    /// Correct event names.
    pub events: u32,
    /// Correct site names.
    pub sites: u32,
    /// Correct round names.
    pub rounds: u32,
    /// Lines too long to be read.
    pub skipped_lines: u32,
    /// The player section has `%Elo` lines.
    pub has_elo: bool,
}

/// A spelling file read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSpelling {
    /// The checkers.
    pub set: SharedSpelling,
    /// The counts.
    pub counts: SpellingCounts,
    /// The file.
    pub path: PathBuf,
}

/// The photos found in the folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhotoCounts {
    /// Photos in the library.
    pub photos: u32,
    /// Files that could not be used.
    pub errors: u32,
}

/// One correction offered by the checker, as the window edits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellRow {
    /// The name in the database.
    pub old: String,
    /// The correct name; the user may edit it.
    pub new: String,
    /// The games that have the old name.
    pub games: u32,
    /// More than one correct name matched.
    pub ambiguous: bool,
    /// The birth of the player, when the file knows it.
    pub born: mychess_core::PartialDate,
    /// The death of the player.
    pub died: mychess_core::PartialDate,
    /// The row will be applied.
    pub selected: bool,
}

/// The answer of a scan of the names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanDone {
    /// The corrections, in the order of the name base.
    pub rows: Vec<SpellRow>,
    /// The scan was stopped; the list is partial.
    pub interrupted: bool,
}

/// An "Add Elo" plan, waiting for the confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedElo {
    /// The counts of what would change.
    pub report: EloReport,
    /// The writes.
    pub plan: Shared<EloPlan>,
}

/// What the scan job needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanJob {
    /// The database (read).
    pub source: SharedSource,
    /// The spelling file.
    pub set: SharedSpelling,
    /// The kind of name.
    pub kind: NameKind,
    /// Also names that are a surname only.
    pub surnames: bool,
    /// Also the ambiguous ones.
    pub ambiguous: bool,
}

/// What the plan job needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanJob {
    /// The database (read).
    pub source: SharedSource,
    /// The spelling file.
    pub set: SharedSpelling,
    /// A copy of the filter of the slot, when only the filter is wanted.
    pub filter: Option<Box<Filter>>,
    /// Replace the ratings that are there.
    pub overwrite: bool,
    /// Only the games of the filter.
    pub only_filter: bool,
}

/// What the writing of the corrected names needs (a list job; spec rule 56).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamesJob {
    /// The database of the slot.
    pub source: SharedSource,
    /// Its file; `None` for the Clipbase, changed in memory.
    pub path: Option<PathBuf>,
    /// The state of the file when the slot read it.
    pub stamp: Option<FileStamp>,
    /// The games the slot had.
    pub expected_total: u32,
    /// The kind of name.
    pub kind: NameKind,
    /// The corrections.
    pub changes: Vec<NameChange>,
}

/// What the writing of the ratings needs (a list job).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EloJob {
    /// The database of the slot.
    pub source: SharedSource,
    /// Its file; `None` for the Clipbase.
    pub path: Option<PathBuf>,
    /// The state of the file when the slot read it.
    pub stamp: Option<FileStamp>,
    /// The games the slot had.
    pub expected_total: u32,
    /// The plan.
    pub plan: Shared<EloPlan>,
}

/// How a write ended; the database is read again whatever happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOutcome<R> {
    /// What was done.
    pub report: R,
    /// The database read again (`.si4` only).
    pub source: Option<SharedSource>,
    /// The state of the file after the write.
    pub stamp: Option<FileStamp>,
    /// Why it stopped early, if it did.
    pub error: Option<BaseError>,
    /// The change was made in memory (the Clipbase).
    pub in_memory: bool,
}

/// The answer of the writing of names.
pub type NamesOutcome = WriteOutcome<NameChangeReport>;
/// The answer of the writing of ratings.
pub type EloOutcome = WriteOutcome<EloReport>;

/// An event of the spelling file, the name checker and "Add Elo".
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SpellingMessage {
    /// Loads the spelling file of the preferences, or the default one, when
    /// the preference asks for it. The front-end sends it once, after the
    /// preferences were loaded; a missing default file is silent.
    LoadAtStart,
    /// Asks for the file with the dialog of the host (`Options > Load
    /// Spellcheck File`).
    PickFile,
    /// Loads this file; it becomes the preference.
    Load(PathBuf),
    /// Reads the loaded file again.
    Reload,
    /// Forgets the spelling file.
    Unload,
    /// Stops the reading in flight.
    CancelLoad,
    /// The answer of [`SpellingEffect::Load`].
    Loaded {
        /// Correlates the answer.
        request: RequestId,
        /// The file, or why not.
        result: Box<Result<LoadedSpelling, SpellingLoadError>>,
    },
    /// The answer of [`SpellingEffect::Photos`].
    PhotosLoaded {
        /// Correlates the answer.
        request: RequestId,
        /// The library.
        library: SharedPhotos,
        /// The counts.
        counts: PhotoCounts,
    },
    /// The answer of [`SpellingEffect::Stats`].
    StatsBuilt {
        /// Correlates the answer.
        request: RequestId,
        /// The statistics, `None` when stopped.
        stats: Option<Shared<PlayerStats>>,
    },

    /// Opens the name checker window for a kind of name and scans.
    OpenCheck(NameKind),
    /// Closes it (stops the scan).
    CloseCheck,
    /// Scans again.
    Rescan,
    /// "Surnames": scans again.
    SetSurnames(bool),
    /// "Ambiguous": scans again.
    SetAmbiguous(bool),
    /// Stops the scan.
    CancelScan,
    /// Ticks or unticks a row.
    ToggleRow(usize),
    /// Ticks or unticks every row that is not ambiguous.
    SelectAll(bool),
    /// Edits the new name of a row.
    EditNew(usize, String),
    /// The text of the search box of the list.
    SetFind(String),
    /// Copies the list as the text of the Scid.
    CopyText,
    /// "Make corrections": asks for the confirmation (not in the Clipbase).
    Apply,
    /// The answer to the confirmation.
    ConfirmApply(bool),
    /// The answer of [`SpellingEffect::Scan`].
    Scanned {
        /// Correlates the answer.
        request: RequestId,
        /// The corrections.
        result: Box<ScanDone>,
    },
    /// The answer of [`SpellingEffect::ApplyNames`].
    Applied {
        /// Correlates the answer.
        request: RequestId,
        /// What happened.
        result: Box<Result<NamesOutcome, BaseError>>,
    },

    /// Opens the "Add Elo ratings" dialog.
    OpenAddElo,
    /// Closes it (stops what runs).
    CloseAddElo,
    /// "Overwrite existing ratings?".
    SetEloOverwrite(bool),
    /// "Add ratings to": the filter (`true`) or all the games.
    SetEloOnlyFilter(bool),
    /// Works out what would change.
    PlanElo,
    /// Writes the plan.
    ConfirmElo,
    /// The answer of [`SpellingEffect::PlanElo`].
    Planned {
        /// Correlates the answer.
        request: RequestId,
        /// The plan, or why not.
        result: Box<Result<PlannedElo, SpellingNotice>>,
    },
    /// The answer of [`SpellingEffect::ApplyElo`].
    EloApplied {
        /// Correlates the answer.
        request: RequestId,
        /// What happened.
        result: Box<Result<EloOutcome, BaseError>>,
    },
    /// Stops the plan or the write in flight.
    CancelJob,
}

/// What `perform` does for the spelling file and the name checker.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SpellingEffect {
    /// Reads the spelling file; answers [`SpellingMessage::Loaded`].
    Load {
        /// Correlates the answer.
        request: RequestId,
        /// The file; `None` = `spelling.ssp` or `spelling.ssp.gz` of the
        /// data directory.
        path: Option<PathBuf>,
        /// The data directory (`None` = the system's).
        data_dir: Option<PathBuf>,
        /// A missing file is not an error (start-up).
        quiet: bool,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Looks for the photos; answers [`SpellingMessage::PhotosLoaded`].
    Photos {
        /// Correlates the answer.
        request: RequestId,
        /// The extra folder of the preferences.
        extra: Option<PathBuf>,
        /// The data directory (`None` = the system's).
        data_dir: Option<PathBuf>,
        /// Cancellation.
        control: OpenControl,
    },
    /// Builds the highest Elo of every player with the estimates of the
    /// spelling file; answers [`SpellingMessage::StatsBuilt`].
    Stats {
        /// Correlates the answer.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// The spelling file.
        set: SharedSpelling,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Looks for corrections; answers [`SpellingMessage::Scanned`].
    Scan {
        /// Correlates the answer.
        request: RequestId,
        /// What to scan.
        job: Box<ScanJob>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Works out "Add Elo"; answers [`SpellingMessage::Planned`].
    PlanElo {
        /// Correlates the answer.
        request: RequestId,
        /// What to plan.
        job: Box<PlanJob>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Writes the names; answers [`SpellingMessage::Applied`].
    ApplyNames {
        /// Correlates the answer.
        request: RequestId,
        /// What to write.
        job: Box<NamesJob>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Writes the ratings; answers [`SpellingMessage::EloApplied`].
    ApplyElo {
        /// Correlates the answer.
        request: RequestId,
        /// What to write.
        job: Box<EloJob>,
        /// Progress and cancellation.
        control: OpenControl,
    },
}

/// A remark about the spelling file, the checker or "Add Elo". Fluent keys
/// in [`SpellingNotice::message_key`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpellingNotice {
    /// The file was read (`spell-loaded`).
    Loaded(SpellingCounts),
    /// The file could not be read (`spell-read-error`, `$reason` from
    /// [`SpellingLoadError::reason_key`]).
    LoadFailed(SpellingLoadError),
    /// There is no spelling file (`spell-not-loaded`).
    NotLoaded,
    /// The file has no Elo ratings (`spell-no-elo`).
    NoEloData,
    /// The database cannot be written (`base-error-read-only`).
    ReadOnly,
    /// The database changed since the list was made (`base-error-stale`).
    Stale,
    /// There is no database to work on (`spell-no-base`).
    NoBase,
    /// No row is ticked (`spell-none-valid`).
    NoneSelected,
    /// The scan was stopped (`spell-interrupted`).
    ScanInterrupted,
    /// Names were corrected (`spell-done`).
    Applied {
        /// The kind of name.
        kind: NameKind,
        /// Names mapped.
        names: u32,
        /// Name changes in the games.
        times: u32,
        /// Player sides left alone because of the dates
        /// (`spell-done-skipped`).
        skipped_by_date: u32,
        /// Old names that were not in the database (`spell-not-found`).
        not_found: u32,
        /// The run was stopped (`spell-interrupted`).
        interrupted: bool,
    },
    /// Ratings were added (`spell-elo-done`).
    EloAdded {
        /// Ratings written.
        ratings: u32,
        /// Games changed.
        games: u32,
    },
    /// Nothing to add (`spell-elo-nothing`).
    EloNothing,
    /// The names of the game on screen were changed in the database but the
    /// game could not be read again (it is being edited, or a game, a
    /// training or the trial mode owns the board): its tags still hold the
    /// old names (`spell-game-not-refreshed`).
    GameNotRefreshed,
    /// A write failed.
    Failed(BaseError),
}

impl SpellingNotice {
    /// The Fluent key of the message. `Applied` also uses
    /// `spell-done-skipped` and `spell-not-found` when those counts are not
    /// zero, and `spell-interrupted` when the run was stopped.
    pub fn message_key(&self) -> &'static str {
        match self {
            SpellingNotice::Loaded(_) => "spell-loaded",
            SpellingNotice::LoadFailed(_) => "spell-read-error",
            SpellingNotice::NotLoaded => "spell-not-loaded",
            SpellingNotice::NoEloData => "spell-no-elo",
            SpellingNotice::ReadOnly => "base-error-read-only",
            SpellingNotice::Stale => "base-error-stale",
            SpellingNotice::NoBase => "spell-no-base",
            SpellingNotice::NoneSelected => "spell-none-valid",
            SpellingNotice::ScanInterrupted => "spell-interrupted",
            SpellingNotice::Applied { .. } => "spell-done",
            SpellingNotice::EloAdded { .. } => "spell-elo-done",
            SpellingNotice::EloNothing => "spell-elo-nothing",
            SpellingNotice::GameNotRefreshed => "spell-game-not-refreshed",
            SpellingNotice::Failed(error) => error.message_key(),
        }
    }
}

/// The slot an operation of the checker is aimed at, with what identifies
/// its database at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    /// `SpellingUi::version` when the target was taken: a database changed
    /// in place (the Clipbase) keeps its source but not its version.
    pub(crate) version: u64,
}
