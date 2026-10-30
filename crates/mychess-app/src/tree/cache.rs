//! The cache of the opening trees: state, messages' payloads and the job
//! that saves, fills or describes it (spec M5-3b, D1760-D1789).
//!
//! One [`StcCache`] lives per slot, behind a mutex shared with the jobs. A
//! tree search only *tries* the lock ([`try_lock`]): if a fill holds it, the
//! search runs without the cache instead of waiting. The jobs of this module
//! (save, fill, info) lock it for their whole run on a blocking thread, so
//! `update` and `view` never wait for it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError, TryLockError};

use mychess_core::Game;
use mychess_db::Filter;
use mychess_db::GameSource;
use mychess_db::tree::{
    PrimeError, PrimeReport, STANDARD_LINES, StcCache, StcError, TreeState, prime_with_filter,
    prime_with_game, prime_with_lines, stc_path,
};

use crate::bases::{OpenControl, SharedSource, SlotId};
use crate::shared::SharedEco;

use super::message::LockingSource;

/// The cache of one slot, shared with the jobs.
pub(crate) type SharedStc = Arc<Mutex<StcCache>>;

/// Locks the cache, waiting for it; a poisoned lock is recovered (the cache
/// holds only counters and filters, any state is coherent).
pub(crate) fn lock(cache: &SharedStc) -> MutexGuard<'_, StcCache> {
    cache.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Locks the cache if nobody holds it.
pub(crate) fn try_lock(cache: &SharedStc) -> Option<MutexGuard<'_, StcCache>> {
    match cache.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

/// What a fill goes through (Scid: `::tree::primeWithBase`,
/// `primeWithGame` and `prime`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheFill {
    /// Every position of every game of the filter of the database.
    Base,
    /// Every position of the game in the editor, variations included.
    Game,
    /// The 110 standard opening lines.
    StandardLines,
}

/// What a cache job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheJobKind {
    /// Writes the cache to its `.stc` file.
    Save,
    /// Fills the cache.
    Fill(CacheFill),
    /// Describes the cache.
    Info,
}

/// What the "cache information" shows (Scid: `sc_tree cacheinfo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheInfo {
    /// Trees in the main cache.
    pub used: usize,
    /// Capacity of the main cache.
    pub capacity: usize,
    /// Trees in the memory-only backup cache.
    pub backup_used: usize,
    /// Capacity of the backup cache.
    pub backup_capacity: usize,
    /// The total of the smallest tree held, `None` when empty.
    pub lowest_total: Option<u32>,
    /// Size of the `.stc` file, `None` if it does not exist.
    pub file_bytes: Option<u64>,
}

/// Why a cache job did nothing or stopped. The front-end translates from
/// the variant.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CacheIssue {
    /// The database has no cache file (a PGN file or the Clipbase).
    NotScidBase,
    /// Another cache job is running.
    Busy,
    /// The cache file of the old content is still being removed.
    Discarding,
    /// There is no database to work on.
    NoBase,
    /// The cache was changed (the database changed) while the job ran.
    Stale,
    /// The job was cancelled; what was done stays in the cache.
    Cancelled,
    /// A line of the standard list is not legal (cannot happen).
    IllegalLine,
    /// The file could not be written or read; the text is for display.
    File(String),
    /// A search of the fill failed; the text is for display.
    Search(String),
    /// Something that should not happen (the thread failed).
    Internal,
}

/// What a finished job did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CacheOutcome {
    /// The file was written.
    Saved {
        /// Its size in bytes (0 for a cache without a file).
        bytes: u64,
        /// Trees left out (size limit or sums over 32 bits).
        skipped: usize,
    },
    /// The fill ended.
    Filled(PrimeReport),
    /// The description.
    Info(CacheInfo),
}

/// The line the panel shows about the cache; the front-end translates.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CacheStatus {
    /// Nothing to say.
    #[default]
    None,
    /// The cache file was written.
    Saved {
        /// Its size in bytes.
        bytes: u64,
        /// Trees left out.
        skipped: usize,
    },
    /// The fill ended.
    Filled {
        /// Searches made.
        searches: u32,
        /// Positions that were already in the cache.
        hits: u32,
    },
    /// The description of the cache.
    Info(CacheInfo),
    /// A job did not finish.
    Failed(CacheIssue),
}

/// A job in progress, for the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheBusy {
    /// What it does.
    pub kind: CacheJobKind,
    /// Items done.
    pub done: u64,
    /// Items to do; 0 when not known.
    pub total: u64,
}

/// The cache part of [`TreeView`](super::TreeView).
#[derive(Debug, Clone, PartialEq)]
pub struct CacheView {
    /// The configured capacity.
    pub size: usize,
    /// Save when the panel closes and when the program ends.
    pub autosave: bool,
    /// The job running, if any.
    pub busy: Option<CacheBusy>,
    /// The last result.
    pub status: CacheStatus,
    /// The database of the tree has a cache file (an `.si4`).
    pub has_file: bool,
}

impl Default for CacheView {
    fn default() -> CacheView {
        CacheView {
            size: super::prefs::DEFAULT_CACHE_SIZE,
            autosave: false,
            busy: None,
            status: CacheStatus::None,
            has_file: false,
        }
    }
}

/// The cache of one slot and what is known about it.
#[derive(Debug)]
pub(crate) struct CacheEntry {
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    /// The file of the database the cache belongs to (any kind); another
    /// one in the slot means another database.
    pub(crate) identity: Option<PathBuf>,
    /// Where the `.stc` file of the database is: only for an `.si4`.
    pub(crate) file: Option<PathBuf>,
    pub(crate) cache: SharedStc,
    /// Trees were stored since the last save.
    pub(crate) dirty: bool,
    /// The capacity last given to the cache.
    pub(crate) size: usize,
}

/// A user's job in flight.
#[derive(Debug)]
pub(crate) struct RunningJob {
    pub(crate) id: u64,
    pub(crate) slot: SlotId,
    pub(crate) kind: CacheJobKind,
    pub(crate) control: OpenControl,
}

/// The state of the caches of the trees.
#[derive(Debug)]
pub(crate) struct CacheUi {
    pub(crate) entries: Vec<Option<CacheEntry>>,
    /// The job the user asked for (at most one).
    pub(crate) job: Option<RunningJob>,
    pub(crate) status: CacheStatus,
    pub(crate) next_job: u64,
    /// Saves started by the program (close of the panel, closing a base,
    /// quit) that have not answered.
    pub(crate) autosaves: u32,
    /// The program is on its way out: autosaves were started. Irreversible:
    /// `finish_quit` is the point of no return of the quit (D1777).
    pub(crate) quit_started: bool,
    /// A deadline was asked for the saves of the quit (D1776); the number
    /// is the generation the message must carry (D1780).
    pub(crate) deadline_armed: bool,
    pub(crate) deadline_generation: u64,
    /// `finish_quit` was called by the cache: it is not called twice
    /// (D1782).
    pub(crate) quit_finished: bool,
    /// Files whose cache is being discarded: until the discard ends no new
    /// cache may read them (D1772).
    pub(crate) discarding: Vec<PathBuf>,
}

impl Default for CacheUi {
    fn default() -> CacheUi {
        CacheUi {
            entries: (0..SlotId::COUNT).map(|_| None).collect(),
            job: None,
            status: CacheStatus::None,
            next_job: 0,
            autosaves: 0,
            quit_started: false,
            deadline_armed: false,
            deadline_generation: 0,
            quit_finished: false,
            discarding: Vec::new(),
        }
    }
}

impl CacheUi {
    pub(crate) fn entry_mut(&mut self, slot: SlotId) -> Option<&mut CacheEntry> {
        self.entries.get_mut(slot.index()).and_then(Option::as_mut)
    }

    pub(crate) fn entry(&self, slot: SlotId) -> Option<&CacheEntry> {
        self.entries.get(slot.index()).and_then(Option::as_ref)
    }
}

/// Everything a cache job needs, owned (D1766). Built by `update`, run by
/// [`perform`](crate::perform).
#[derive(Debug, Clone)]
pub struct CacheJob {
    pub(crate) id: u64,
    pub(crate) slot: SlotId,
    pub(crate) kind: CacheJobKind,
    /// Started by the program, not by the user.
    pub(crate) autosave: bool,
    pub(crate) cache: SharedStc,
    /// The state of the file at the time the job was made (D1779).
    pub(crate) epoch: Option<FileEpoch>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) source: SharedSource,
    pub(crate) filter: Filter,
    pub(crate) game: Option<Box<Game>>,
    pub(crate) eco: Option<SharedEco>,
    pub(crate) control: OpenControl,
}

impl CacheJob {
    /// What the job does.
    pub fn kind(&self) -> CacheJobKind {
        self.kind
    }

    /// Asks the job to stop (a save cannot be interrupted).
    pub fn cancel(&self) {
        self.control.cancel();
    }
}

impl PartialEq for CacheJob {
    fn eq(&self, other: &CacheJob) -> bool {
        self.id == other.id && self.slot == other.slot && self.kind == other.kind
    }
}

/// The answer to a [`CacheJob`].
#[derive(Debug, Clone)]
pub struct CacheAnswer {
    pub(crate) id: u64,
    pub(crate) slot: SlotId,
    pub(crate) autosave: bool,
    /// The cache the job worked on: an answer for a cache that is no longer
    /// the one of the slot is ignored (D1773).
    pub(crate) cache: SharedStc,
    pub(crate) result: Result<CacheOutcome, CacheIssue>,
}

impl PartialEq for CacheAnswer {
    fn eq(&self, other: &CacheAnswer) -> bool {
        self.id == other.id
            && self.slot == other.slot
            && self.autosave == other.autosave
            && self.result == other.result
    }
}

impl CacheAnswer {
    /// The answer for a thread that failed.
    pub(crate) fn internal_failure(
        id: u64,
        slot: SlotId,
        autosave: bool,
        cache: SharedStc,
    ) -> CacheAnswer {
        CacheAnswer {
            id,
            slot,
            autosave,
            cache,
            result: Err(CacheIssue::Internal),
        }
    }
}

/// What serialises the writers and the remover of one `.stc` file, whatever
/// cache they work for (spec M5-3b, D1779).
#[derive(Debug, Default)]
pub(crate) struct FileGuard {
    mutex: Mutex<()>,
    /// Grows each time the content of the file stops being valid.
    epoch: AtomicU64,
}

impl FileGuard {
    pub(crate) fn lock(&self) -> MutexGuard<'_, ()> {
        self.mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The content of the file is no longer valid: the saves made before
    /// this call must not write.
    pub(crate) fn invalidate(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }
}

static FILE_GUARDS: LazyLock<Mutex<HashMap<PathBuf, Arc<FileGuard>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The guard of the `.stc` file of the database at `base`.
pub(crate) fn file_guard(base: &std::path::Path) -> Arc<FileGuard> {
    let mut map = FILE_GUARDS.lock().unwrap_or_else(PoisonError::into_inner);
    map.entry(stc_path(base)).or_default().clone()
}

/// The guard of a file and its epoch when a job was made.
#[derive(Debug, Clone)]
pub(crate) struct FileEpoch {
    pub(crate) guard: Arc<FileGuard>,
    expected: u64,
}

impl FileEpoch {
    /// Now.
    pub(crate) fn of(base: &std::path::Path) -> FileEpoch {
        let guard = file_guard(base);
        let expected = guard.epoch.load(Ordering::SeqCst);
        FileEpoch { guard, expected }
    }

    fn is_stale(&self) -> bool {
        self.guard.epoch.load(Ordering::SeqCst) != self.expected
    }
}

/// Whether two paths of a database name the same `.stc` file.
pub(crate) fn same_stc(a: &std::path::Path, b: &std::path::Path) -> bool {
    stc_path(a) == stc_path(b)
}

/// Empties a cache and removes its file once nobody holds the cache
/// (spec M5-3b, D1772). Built by `update`, run by [`perform`](crate::perform).
#[derive(Debug, Clone)]
pub struct CacheDiscard {
    pub(crate) cache: SharedStc,
    pub(crate) file: PathBuf,
}

impl PartialEq for CacheDiscard {
    fn eq(&self, other: &CacheDiscard) -> bool {
        Arc::ptr_eq(&self.cache, &other.cache) && self.file == other.file
    }
}

/// Waits for the lock of the cache (a save or fill in flight ends first),
/// empties it and removes the `.stc` file. Returns the file.
pub(crate) fn run_discard(discard: CacheDiscard) -> PathBuf {
    let CacheDiscard { cache, file } = discard;
    let mut cache = lock(&cache);
    cache.clear();
    // Any save of this file, of whatever cache, is over or has not written
    // (D1779).
    let guard = file_guard(&file);
    let _file = guard.lock();
    if let Err(error) = mychess_db::tree::remove_stc(&file) {
        tracing::warn!(%error, "the tree cache file could not be removed");
    }
    file
}

fn file_issue(error: &StcError) -> CacheIssue {
    CacheIssue::File(error.to_string())
}

fn prime_issue(error: &PrimeError) -> CacheIssue {
    match error {
        PrimeError::Cancelled => CacheIssue::Cancelled,
        PrimeError::NotScidBase => CacheIssue::NotScidBase,
        PrimeError::IllegalMove { .. } => CacheIssue::IllegalLine,
        other => CacheIssue::Search(other.to_string()),
    }
}

/// Runs a job on a blocking thread.
pub(crate) fn run_job(job: CacheJob) -> CacheAnswer {
    let (id, slot, autosave, cache) = (job.id, job.slot, job.autosave, job.cache.clone());
    let result = run(job);
    if let Err(issue) = &result
        && !matches!(issue, CacheIssue::Cancelled)
    {
        tracing::warn!(?issue, "tree cache job failed");
    }
    CacheAnswer {
        id,
        slot,
        autosave,
        cache,
        result,
    }
}

fn run(job: CacheJob) -> Result<CacheOutcome, CacheIssue> {
    let CacheJob {
        kind,
        cache,
        epoch,
        path,
        source,
        filter,
        game,
        eco,
        control,
        ..
    } = job;
    let mut cache = lock(&cache);
    match kind {
        CacheJobKind::Info => {
            let (used, capacity) = cache.info();
            let (backup_used, backup_capacity) = cache.backup_info();
            Ok(CacheOutcome::Info(CacheInfo {
                used,
                capacity,
                backup_used,
                backup_capacity,
                lowest_total: cache.main_lowest().map(|(total, _)| total),
                file_bytes: path
                    .as_deref()
                    .and_then(|p| std::fs::metadata(stc_path(p)).ok())
                    .map(|m| m.len()),
            }))
        }
        CacheJobKind::Save => {
            if path.is_none() {
                return Err(CacheIssue::NotScidBase);
            }

            // A cache never used in this session has not read its file;
            // writing would replace it by an empty one (D1056).
            if let Err(error) = cache.read() {
                // A missing or invalid file: the cache is saved over it.
                tracing::debug!(%error, "the cache file was not read before saving");
            }
            // The file is taken for the write: a discard of the file waits
            // for it, and a discard asked since the job was made stops it
            // (D1772, D1779).
            let _file = epoch.as_ref().map(|e| e.guard.lock());
            if epoch.as_ref().is_some_and(FileEpoch::is_stale) {
                return Err(CacheIssue::Stale);
            }
            let bytes = cache.write(eco.as_deref()).map_err(|e| file_issue(&e))?;
            Ok(CacheOutcome::Saved {
                bytes,
                skipped: cache.last_write_skipped(),
            })
        }
        CacheJobKind::Fill(fill) => {
            let mut locking = LockingSource::new(source);
            if !GameSource::is_scid_database(&locking) {
                return Err(CacheIssue::NotScidBase);
            }
            let mut state = TreeState::default();
            let token = control.token();
            let mut progress = control.reporter();
            let report = match fill {
                CacheFill::Base => prime_with_filter(
                    &mut locking,
                    &mut state,
                    &mut *cache,
                    &filter,
                    &token,
                    &mut progress,
                ),
                CacheFill::Game => {
                    let game = game.ok_or(CacheIssue::Internal)?;
                    prime_with_game(
                        &mut locking,
                        &mut state,
                        &mut *cache,
                        &game,
                        &token,
                        &mut progress,
                    )
                }
                CacheFill::StandardLines => prime_with_lines(
                    &mut locking,
                    &mut state,
                    &mut *cache,
                    STANDARD_LINES,
                    &token,
                    &mut progress,
                ),
            };
            report
                .map(CacheOutcome::Filled)
                .map_err(|e| prime_issue(&e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_discard_of_a_file_waits_for_the_save_that_holds_it() {
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempfile::tempdir().expect("dir");
        let base = dir.path().join("g.si4");
        let stc = stc_path(&base);
        std::fs::write(&stc, b"old trees").expect("write");
        // A save of another cache is writing this file.
        let guard = file_guard(&base);
        let writing = guard.lock();
        let (sender, receiver) = mpsc::channel();
        let discard = CacheDiscard {
            cache: Arc::new(Mutex::new(StcCache::new(None))),
            file: base.clone(),
        };
        let worker = std::thread::spawn(move || {
            let file = run_discard(discard);
            sender.send(file).expect("send");
        });
        assert!(
            receiver.recv_timeout(Duration::from_millis(200)).is_err(),
            "the discard ran while the file was being written"
        );
        assert!(stc.exists());
        drop(writing);
        let file = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("done");
        assert_eq!(file, base);
        worker.join().expect("thread");
        assert!(!stc.exists());
    }

    #[test]
    fn try_lock_gives_up_while_the_cache_is_held() {
        let cache: SharedStc = Arc::new(Mutex::new(StcCache::new(None)));
        let held = lock(&cache);
        assert!(try_lock(&cache).is_none());
        drop(held);
        assert!(try_lock(&cache).is_some());
    }

    #[test]
    fn a_poisoned_lock_is_recovered() {
        let cache: SharedStc = Arc::new(Mutex::new(StcCache::new(None)));
        let other = Arc::clone(&cache);
        drop(
            std::thread::spawn(move || {
                let _guard = other.lock();
                panic!("poison the lock");
            })
            .join(),
        );
        assert!(try_lock(&cache).is_some());
        assert_eq!(lock(&cache).info().1, 1000);
    }
}
