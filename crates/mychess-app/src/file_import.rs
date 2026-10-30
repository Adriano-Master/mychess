//! Import of whole PGN files into a database (spec M3-8, D1970-D1989; Scid:
//! `tcl/tools/import.tcl`, `importPgnFile`/`doPgnFileImport`).
//!
//! The flow (messages, the answer of the job, hooks of `update`) is in
//! `state/file_import_flow.rs`; the blocking work is [`work`]. The import is
//! a job of the game list (`ListJobKind::ImportPgn`), so it shares the
//! progress, the cancellation, the "one job at a time" rule and the quit
//! handling of the other list jobs.

mod work;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use mychess_pgn::read::Severity;

use crate::bases::{BaseError, FileStamp, SharedSource};
use crate::error::RequestId;

pub(crate) use work::import_run;

/// The most messages of the report that are kept (D1983); the others are
/// only counted.
pub const MAX_REPORT_LINES: usize = 30;

/// Events of the import of PGN files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileImportMessage {
    /// "Import PGN file...": asks for the files.
    Open,
    /// Closes the window of the report (while the job runs only
    /// `ListMessage::CancelJob` acts).
    Close,
}

/// What the job has done so far, read by the front-end while it runs (the
/// bytes are in the `OpenControl` of the job). Compared by identity.
#[derive(Debug, Clone, Default)]
pub struct ImportProgress {
    inner: Arc<ProgressCells>,
}

#[derive(Debug, Default)]
struct ProgressCells {
    file: AtomicUsize,
    games: AtomicU64,
}

impl ImportProgress {
    /// The file being read (from 0).
    pub fn file(&self) -> usize {
        self.inner.file.load(Ordering::Relaxed)
    }

    /// Games stored from the files already finished.
    pub fn games(&self) -> u64 {
        self.inner.games.load(Ordering::Relaxed)
    }

    pub(crate) fn set(&self, file: usize, games: u64) {
        self.inner.file.store(file, Ordering::Relaxed);
        self.inner.games.store(games, Ordering::Relaxed);
    }
}

impl PartialEq for ImportProgress {
    fn eq(&self, other: &ImportProgress) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for ImportProgress {}

/// Where the games go (D1972, D1974).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileImportTarget {
    /// The `.si4` of the current slot, as the slot read it.
    Existing {
        /// The `.si4` file.
        path: PathBuf,
        /// Games the slot knows; the file must still have as many.
        expected_total: u32,
        /// The state of the file when the slot read it.
        stamp: Option<FileStamp>,
    },
    /// A database to create (it must not exist).
    New {
        /// The `.si4` file to create.
        path: PathBuf,
    },
}

impl FileImportTarget {
    /// The `.si4` file.
    pub fn path(&self) -> &Path {
        match self {
            FileImportTarget::Existing { path, .. } | FileImportTarget::New { path } => path,
        }
    }
}

/// What [`Effect::ImportPgnFiles`](crate::Effect::ImportPgnFiles) does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileImportJob {
    /// The database.
    pub target: FileImportTarget,
    /// The PGN files, in the order to import.
    pub files: Vec<PathBuf>,
    /// Where the job tells how far it is.
    pub progress: ImportProgress,
}

/// One remark of the report: where and what (D1983).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportLine {
    /// The file (index into the list of the job).
    pub file: usize,
    /// The game in the file (1-based); 0 when it does not apply.
    pub game: u64,
    /// The line of the game in the file; 0 when unknown.
    pub line: u64,
    /// How serious it is.
    pub severity: Severity,
    /// The Fluent key of the text.
    pub key: &'static str,
}

/// The totals of an import and its first remarks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileImportReport {
    /// Files that were opened and read (to the end or until a stop).
    pub files_read: usize,
    /// Games found in the files, the skipped ones included.
    pub games_read: u64,
    /// Games stored.
    pub games_added: u32,
    /// Games found but not stored.
    pub games_skipped: u64,
    /// Warnings and repairs.
    pub warnings: u64,
    /// The first remarks, in reading order (at most [`MAX_REPORT_LINES`]).
    pub lines: Vec<ImportLine>,
    /// Remarks that did not fit in `lines`.
    pub lines_dropped: u64,
    /// Files that could not be opened (index in the job, why).
    pub file_errors: Vec<(usize, BaseError)>,
    /// The user stopped the import; what was read is stored (D1978).
    pub interrupted: bool,
}

impl FileImportReport {
    /// Whether anything besides the totals is worth showing.
    pub fn has_problems(&self) -> bool {
        self.games_skipped > 0
            || self.warnings > 0
            || !self.lines.is_empty()
            || !self.file_errors.is_empty()
    }
}

/// What the job did to the files (the answer of the effect).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileImportOutcome {
    /// The database read again; `None` if it could not be (or was removed).
    pub source: Option<SharedSource>,
    /// The state of the file after the import.
    pub stamp: Option<FileStamp>,
    /// The `.si4` file.
    pub path: PathBuf,
    /// The database was created by the job.
    pub created: bool,
    /// The database was created and removed again, because no game was
    /// stored (D1975).
    pub removed: bool,
    /// The totals.
    pub report: FileImportReport,
    /// Why the job ended before the end of the files, if it did (a write
    /// failure or a limit of the format); a stop by the user is not one.
    pub error: Option<BaseError>,
}

/// The state of the window while the job runs (D1985).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileImportWindow {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) target: PathBuf,
    pub(crate) new_base: bool,
    pub(crate) progress: ImportProgress,
    pub(crate) result: Option<FileImportResult>,
}

/// How an import ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileImportResult {
    /// The totals and remarks.
    pub report: FileImportReport,
    /// Why it ended early, if it did.
    pub error: Option<BaseError>,
    /// The new database was opened in a slot.
    pub opened: bool,
    /// The new database exists but was not opened (D1975).
    pub kept: bool,
}

/// A request for a file dialog of the import in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingFileImportPick {
    pub(crate) request: RequestId,
    /// `None`: the PGN files; `Some`: the name of the new database, with
    /// these files chosen.
    pub(crate) files: Option<Vec<PathBuf>>,
}

/// The state of the import in [`AppState`](crate::AppState).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileImportUi {
    pub(crate) pending: Option<PendingFileImportPick>,
    pub(crate) window: Option<FileImportWindow>,
}

impl FileImportUi {
    /// Whether the window is open.
    pub fn is_open(&self) -> bool {
        self.window.is_some()
    }
}

/// What the window draws.
#[derive(Debug, Clone)]
pub struct FileImportView<'a> {
    /// The PGN files.
    pub files: &'a [PathBuf],
    /// The database, existing or new.
    pub target: &'a Path,
    /// The database is new (it is being created).
    pub new_base: bool,
    /// The job is running.
    pub running: bool,
    /// The file being read (from 0).
    pub file: usize,
    /// Games stored from the files already finished.
    pub games: u64,
    /// Bytes read and bytes to read, from the progress of the job.
    pub bytes: (u64, u64),
    /// How it ended; `None` while it runs.
    pub result: Option<&'a FileImportResult>,
}

/// What the status bar says (D1984).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileImportNotice {
    /// The games were stored.
    Imported {
        /// How many.
        count: u32,
    },
    /// The user stopped it after `count` games were stored.
    Interrupted {
        /// How many.
        count: u32,
    },
    /// It ended early with an error after `count` games were stored.
    Failed {
        /// How many.
        count: u32,
        /// Why.
        error: BaseError,
    },
    /// The file or files had no game.
    NothingImported,
    /// The files were not read at all.
    NotStarted(BaseError),
}

impl FileImportNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            FileImportNotice::Imported { .. } => "file-import-notice-imported",
            FileImportNotice::Interrupted { .. } => "file-import-notice-interrupted",
            FileImportNotice::Failed { .. } => "file-import-notice-failed",
            FileImportNotice::NothingImported => "file-import-notice-nothing",
            FileImportNotice::NotStarted(error) => error.message_key(),
        }
    }
}

/// The name proposed for a new database: the name of `pgn` without `.gz` and
/// `.pgn`, in the folder of the file (D1974).
pub fn suggest_base_path(pgn: &Path) -> PathBuf {
    let mut name = pgn
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    for extension in [".gz", ".pgn"] {
        if name.len() > extension.len()
            && name
                .get(name.len() - extension.len()..)
                .is_some_and(|tail| tail.eq_ignore_ascii_case(extension))
            && let Some(stem) = name.get(..name.len() - extension.len())
        {
            name = stem.to_owned();
        }
    }
    if name.is_empty() {
        "games".clone_into(&mut name);
    }
    name.push_str(".si4");
    pgn.parent()
        .map_or_else(|| PathBuf::from(&name), |dir| dir.join(&name))
}

/// `path` with the extension of the database (`.si4`) added when missing;
/// `.sn4` and `.sg4` become `.si4` (D1974).
pub fn ensure_base_extension(path: PathBuf) -> PathBuf {
    let known = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["si4", "sn4", "sg4"]
            .iter()
            .any(|k| k.eq_ignore_ascii_case(e))
    });
    if known {
        path.with_extension("si4")
    } else {
        let mut name = path.into_os_string();
        name.push(".si4");
        PathBuf::from(name)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn suggests_the_name_without_pgn_and_gz() {
        assert_eq!(
            suggest_base_path(Path::new("dir/Games.PGN.gz")),
            Path::new("dir").join("Games.si4")
        );
        assert_eq!(suggest_base_path(Path::new("a.pgn")), Path::new("a.si4"));
        assert_eq!(
            suggest_base_path(Path::new("x.txt")),
            Path::new("x.txt.si4")
        );
    }

    #[test]
    fn ensures_the_si4_extension() {
        assert_eq!(ensure_base_extension("a.b".into()), Path::new("a.b.si4"));
        assert_eq!(ensure_base_extension("a".into()), Path::new("a.si4"));
        assert_eq!(ensure_base_extension("a.SI4".into()), Path::new("a.si4"));
        assert_eq!(ensure_base_extension("a.sg4".into()), Path::new("a.si4"));
    }

    #[test]
    fn progress_is_compared_by_identity() {
        let a = ImportProgress::default();
        assert_eq!(a, a.clone());
        assert_ne!(a, ImportProgress::default());
        a.set(2, 7);
        assert_eq!((a.file(), a.games()), (2, 7));
    }
}
