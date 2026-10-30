//! Opening a database file in a slot (spec M3-4, D500, D501, D503).
//!
//! [`open_base`] is the blocking work behind [`Effect::OpenBase`](crate::Effect::OpenBase)
//! (`perform` runs it on `spawn_blocking`); `update` never calls it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use mychess_db::si4::Si4Database;
use mychess_db::{CancelToken, OpenOptions, PgnDatabase};

use super::error::BaseError;
use super::source::{BaseKind, BaseSource, SharedSource};
use crate::fsutil::{canonical_or_absolute, same_file};

/// Progress and cancellation of a long job in flight: opening a file, or
/// (slice 4c) a quick search, a sort or a copy of the list. For the jobs of
/// the list the numbers are games, not bytes.
///
/// The state keeps one clone and the effect another: the front-end reads
/// [`OpenControl::progress`] to draw a progress bar and calls
/// [`OpenControl::cancel`] (through
/// [`BasesMessage::CancelOpen`](super::BasesMessage::CancelOpen)) to stop
/// the load. Compared by identity.
///
/// The progress is a pair of atomics, not a message: the front-end must read
/// it from a periodic `Subscription` (for instance every 100 ms) while
/// [`AppState::base_opening`](crate::AppState::base_opening) is `Some`. Only the load of
/// a PGN file checks the cancellation; an SI4 database opens in one go and
/// ignores [`OpenControl::cancel`].
#[derive(Debug, Clone, Default)]
pub struct OpenControl {
    cancel: CancelToken,
    progress: Arc<ProgressCell>,
}

#[derive(Debug, Default)]
struct ProgressCell {
    done: AtomicU64,
    total: AtomicU64,
}

impl OpenControl {
    /// A control for a new open.
    pub fn new() -> OpenControl {
        OpenControl::default()
    }

    /// Asks the load to stop; it answers [`BaseError::Cancelled`].
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Whether [`OpenControl::cancel`] was called.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// The cancellation flag, for the operations of `mychess-db` that take a
    /// [`CancelToken`] (the ECO classification and the novelty search).
    pub(crate) fn token(&self) -> CancelToken {
        self.cancel.clone()
    }

    /// A progress sink for the operations of `mychess-db`; the front-end
    /// reads the numbers with [`OpenControl::progress`].
    pub(crate) fn reporter(&self) -> impl FnMut(u64, u64) + use<> {
        let cell = Arc::clone(&self.progress);
        move |done, total| {
            cell.done.store(done, Ordering::Relaxed);
            cell.total.store(total, Ordering::Relaxed);
        }
    }

    /// `(done, total)` in bytes of the file read so far (the total is 0
    /// until the loader reports it).
    pub fn progress(&self) -> (u64, u64) {
        (
            self.progress.done.load(Ordering::Relaxed),
            self.progress.total.load(Ordering::Relaxed),
        )
    }
}

impl PartialEq for OpenControl {
    fn eq(&self, other: &OpenControl) -> bool {
        Arc::ptr_eq(&self.progress, &other.progress)
    }
}

impl Eq for OpenControl {}

/// What identifies the state of the `.si4` file when it was read: its size
/// and modification time. A write by this program checks it first, so a file
/// changed behind the back of the slot is not written over stale numbers
/// (D573).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

impl FileStamp {
    /// The stamp of `path` now, if it can be read.
    pub fn of(path: &Path) -> Option<FileStamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some(FileStamp {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// A database that was opened, ready to take a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedBase {
    /// The canonical path of the file (for an SI4 database, of the `.si4`).
    pub path: PathBuf,
    /// The kind of database.
    pub kind: BaseKind,
    /// The database.
    pub source: SharedSource,
    /// Games of a PGN file that were skipped because they could not be
    /// read (0 for SI4).
    pub skipped: u64,
    /// The PGN file has text outside its games, which the database does not
    /// keep (so replacing the file would lose it).
    pub ignored_text: bool,
    /// The file cannot be written (a PGN, or an SI4 whose index file is
    /// read-only): flags cannot be changed (slice 4c, D560).
    pub read_only: bool,
    /// The state of the `.si4` file when it was read (SI4 only).
    pub stamp: Option<FileStamp>,
}

/// What an open request came to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenOutcome {
    /// The file is already open in a slot (same canonical path or same
    /// file); nothing was read. Carries the canonical path.
    AlreadyOpen(PathBuf),
    /// Every file slot is taken and the file is not open.
    NoFreeSlot,
    /// The database was loaded.
    Opened(OpenedBase),
}

/// How [`open_base`] treats a path, from its extension (compared without
/// regard to case): `.si3` needs an upgrade, `.si4`/`.sn4`/`.sg4` name an
/// SI4 database, anything else is read as PGN (Scid: unknown extension is
/// PGN).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    Si3,
    Si4,
    Pgn,
}

pub(crate) fn file_kind(path: &Path) -> FileKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("si3") => FileKind::Si3,
        Some("si4" | "sn4" | "sg4") => FileKind::Si4,
        _ => FileKind::Pgn,
    }
}

/// The file that identifies a database: the `.si4` for an SI4 database
/// (also when the path names its `.sn4` or `.sg4`), the file itself for
/// PGN.
fn identity_path(path: &Path, kind: FileKind) -> PathBuf {
    if kind == FileKind::Si4 {
        let si4 = path.with_extension("si4");
        if si4.exists() {
            return canonical_or_absolute(&si4);
        }
        let upper = path.with_extension("SI4");
        if upper.exists() {
            return canonical_or_absolute(&upper);
        }
    }
    canonical_or_absolute(path)
}

/// Opens `path`, unless it is one of the files in `open` (canonical paths
/// of the databases already open) or `allow_new` is false.
///
/// Blocking: reads the whole file for PGN.
///
/// # Errors
///
/// See [`BaseError`]. Cancelling through `control` gives
/// [`BaseError::Cancelled`].
pub(crate) fn open_base(
    path: &Path,
    open: &[PathBuf],
    allow_new: bool,
    control: &OpenControl,
) -> Result<OpenOutcome, BaseError> {
    let kind = file_kind(path);
    if kind == FileKind::Si3 {
        return Err(BaseError::Si3NeedsUpgrade);
    }
    let identity = identity_path(path, kind);
    if let Some(found) = open.iter().find(|p| same_file(&identity, p)) {
        return Ok(OpenOutcome::AlreadyOpen(found.clone()));
    }
    if !allow_new {
        return Ok(OpenOutcome::NoFreeSlot);
    }
    match kind {
        FileKind::Si3 => Err(BaseError::Si3NeedsUpgrade),
        FileKind::Si4 => {
            // The stamp is taken before the read: a change in between makes
            // the first write refuse (D573).
            let stamp = FileStamp::of(&identity);
            let db = Si4Database::open(path).map_err(|e| BaseError::from_si4(&e))?;
            // The writer opens the three files for writing: if any of them
            // cannot be written the database is read-only.
            let read_only = ["si4", "sn4", "sg4"].iter().any(|ext| {
                std::fs::metadata(identity.with_extension(ext))
                    .is_ok_and(|m| m.permissions().readonly())
            });
            Ok(OpenOutcome::Opened(OpenedBase {
                read_only,
                stamp,
                path: identity,
                kind: BaseKind::Si4,
                source: SharedSource::new(BaseSource::Si4(Box::new(db))),
                skipped: 0,
                ignored_text: false,
            }))
        }
        FileKind::Pgn => {
            let cell = Arc::clone(&control.progress);
            let mut sink = move |done: u64, total: u64| {
                cell.done.store(done, Ordering::Relaxed);
                cell.total.store(total, Ordering::Relaxed);
            };
            let (db, report) =
                PgnDatabase::open(path, &OpenOptions::default(), &mut sink, &control.cancel)
                    .map_err(|e| BaseError::from_pgn(&e))?;
            if mychess_db::GameSource::is_empty(&db) {
                return Err(BaseError::NoGames);
            }
            Ok(OpenOutcome::Opened(OpenedBase {
                path: db.path().to_path_buf(),
                kind: BaseKind::Pgn,
                source: SharedSource::new(BaseSource::Pgn(Box::new(db))),
                skipped: report.games_skipped_total,
                ignored_text: report.text_outside_games > 0,
                read_only: true,
                stamp: None,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const TWO: &str = "[White \"A\"]\n[Black \"B\"]\n\n1. e4 e5 *\n\n[White \"C\"]\n\n1. d4 *\n";

    #[test]
    fn extensions_choose_the_reader_without_regard_to_case() {
        assert_eq!(file_kind(Path::new("a/b.si3")), FileKind::Si3);
        assert_eq!(file_kind(Path::new("a/b.SI4")), FileKind::Si4);
        assert_eq!(file_kind(Path::new("a/b.sg4")), FileKind::Si4);
        assert_eq!(file_kind(Path::new("a/b.pgn")), FileKind::Pgn);
        assert_eq!(file_kind(Path::new("a/b.pgn.gz")), FileKind::Pgn);
        assert_eq!(file_kind(Path::new("a/b.txt")), FileKind::Pgn);
        assert_eq!(file_kind(Path::new("a/b")), FileKind::Pgn);
    }

    #[test]
    fn si3_asks_for_an_upgrade_without_touching_the_file() {
        let result = open_base(Path::new("nowhere/x.si3"), &[], true, &OpenControl::new());
        assert_eq!(result, Err(BaseError::Si3NeedsUpgrade));
    }

    #[test]
    fn a_pgn_opens_with_its_canonical_path_and_a_reused_file_is_not_read() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("two.pgn");
        fs::write(&path, TWO).expect("write");
        let control = OpenControl::new();
        let OpenOutcome::Opened(opened) = open_base(&path, &[], true, &control).expect("open")
        else {
            panic!("expected a new database");
        };
        assert_eq!(opened.kind, BaseKind::Pgn);
        assert_eq!(opened.path, canonical_or_absolute(&path));
        assert_eq!(mychess_db::GameSource::len(&*opened.source.lock()), 2);
        assert!(control.progress().1 > 0);

        let again = open_base(&path, std::slice::from_ref(&opened.path), true, &control);
        assert_eq!(again, Ok(OpenOutcome::AlreadyOpen(opened.path.clone())));
        let full = open_base(&dir.path().join("other.pgn"), &[], false, &control);
        assert_eq!(full, Ok(OpenOutcome::NoFreeSlot));
    }

    #[test]
    fn missing_empty_and_cancelled_files_are_errors() {
        let dir = tempfile::tempdir().expect("dir");
        let control = OpenControl::new();
        let missing = open_base(&dir.path().join("none.pgn"), &[], true, &control);
        assert!(matches!(
            missing,
            Err(BaseError::File(e)) if e.kind == crate::error::FileErrorKind::NotFound
        ));
        let empty = dir.path().join("empty.pgn");
        fs::write(&empty, "just text\n").expect("write");
        assert_eq!(
            open_base(&empty, &[], true, &control),
            Err(BaseError::NoGames)
        );
        let path = dir.path().join("two.pgn");
        fs::write(&path, TWO).expect("write");
        control.cancel();
        assert!(control.is_cancelled());
        assert_eq!(
            open_base(&path, &[], true, &control),
            Err(BaseError::Cancelled)
        );
    }
}
