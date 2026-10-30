//! Messages, the search job and its answer (spec M5-2, D962).

use std::cell::OnceCell;

use mychess_core::{Game, Position};
use mychess_db::progress::{CancelToken, NoProgress};
use mychess_db::si4::{Header, IndexEntry, NameBase, RawGame, Result as DbResult};
use mychess_db::tree::{
    FastMode, NoCache, Sort, TreeError, TreeOptions, TreeQuery, TreeResult, TreeState, search,
};
use mychess_db::{Filter, GameSource};

use crate::bases::{SharedSource, SlotId};
use crate::shared::SharedEco;

use super::best::{BestRow, BestSpec};
use super::prefs::{TreeFastMode, TreeSort};

/// An event of the tree panel.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TreeMessage {
    /// Opens the panel if closed, closes it if open.
    Toggle,
    /// Opens the panel (refreshing turns on and a search is started).
    Open,
    /// Closes the panel: cancels the search and, if "adjust filter" was on,
    /// gives the slot its previous filter back.
    Close,
    /// Chooses the order of the rows; searches again.
    SetSort(TreeSort),
    /// Chooses the search mode; searches again.
    SetFastMode(TreeFastMode),
    /// Short display (hides rating, performance, year and ECO). Only the
    /// columns change; no search.
    SetShort(bool),
    /// Turns "Refresh" (automatic refresh) off or on.
    ToggleAutoRefresh,
    /// "Adjust filter" off or on.
    SetAdjust(bool),
    /// A click on row `index` of [`TreeView::rows`](super::TreeView::rows).
    RowClicked(usize),
    /// An event of the best games section (D1600).
    Best(super::BestMessage),
    /// Training on or off (D1620).
    SetTraining(bool),
    /// Locks the tree on the current database, or unlocks it (D1630).
    SetLocked(bool),
    /// Asks for a file and opens it as a locked tree (D1640).
    OpenBaseAsTree,
    /// Writes the cache of the trees to its `.stc` file (D1762).
    SaveCache,
    /// Fills the cache of the trees (D1763).
    FillCache(super::CacheFill),
    /// Describes the cache of the trees (D1764).
    CacheInfo,
    /// Stops the fill that runs.
    CancelCacheJob,
    /// Hides the last line about the cache.
    DismissCacheStatus,
    /// Sets the capacity of the cache, within
    /// [`CACHE_SIZE_RANGE`](super::CACHE_SIZE_RANGE) (D1760).
    SetCacheSize(usize),
    /// Turns the saving of the cache when the panel closes on or off
    /// (D1761).
    SetCacheAutosave(bool),
    /// A cache job ended (the answer to
    /// [`Effect::TreeCache`](crate::Effect::TreeCache)).
    CacheFinished(Box<super::CacheAnswer>),
    /// A cache was emptied and its file removed (the answer to
    /// [`Effect::TreeCacheDiscard`](crate::Effect::TreeCacheDiscard)).
    CacheDiscarded(std::path::PathBuf),
    /// The time the quit waits for the cache is over; carries the
    /// generation of the timer (D1780).
    CacheDeadline(u64),
    /// A search ended (the answer to
    /// [`Effect::TreeSearch`](crate::Effect::TreeSearch)).
    Finished(Box<TreeAnswer>),
    /// An event of the masks (spec M5-4, D1860).
    Mask(super::MaskMessage),
}

/// Why the table shows no tree (spec M5-2, D972). The front-end translates
/// from the variant.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TreeIssue {
    /// The search was cancelled. Never shown.
    Cancelled,
    /// More than 60 distinct moves in the position.
    TooManyMoves,
    /// The position does not fit the database's piece lists.
    UnsupportedPosition,
    /// The state of the engine did not match the database (retried once).
    FilterSize,
    /// The database could not be read; the text is for display as is.
    Read(String),
    /// Something that should not happen (the search thread failed).
    Internal,
}

impl From<TreeError> for TreeIssue {
    fn from(error: TreeError) -> TreeIssue {
        match error {
            TreeError::Cancelled => TreeIssue::Cancelled,
            TreeError::TooManyMoves => TreeIssue::TooManyMoves,
            TreeError::UnsupportedPosition => TreeIssue::UnsupportedPosition,
            TreeError::FilterSize { .. } => TreeIssue::FilterSize,
            TreeError::Source(error) => TreeIssue::Read(error.to_string()),
            _ => TreeIssue::Internal,
        }
    }
}

/// Everything a tree search needs, owned, so it can run off the interface
/// thread. Built by `update`; run by [`perform`](crate::perform).
///
/// The engine's [`TreeState`] of the slot **moves** into the job and comes
/// back in the [`TreeAnswer`] (D976).
#[derive(Debug, Clone)]
pub struct TreeJob {
    pub(crate) generation: u64,
    pub(crate) slot: SlotId,
    pub(crate) epoch: u64,
    pub(crate) source: SharedSource,
    pub(crate) state: TreeState,
    pub(crate) position: Position,
    pub(crate) ply: u32,
    pub(crate) filter: Filter,
    pub(crate) sort: Sort,
    pub(crate) fast: FastMode,
    pub(crate) adjust: bool,
    pub(crate) hide_moves: bool,
    pub(crate) best: Option<BestSpec>,
    pub(crate) eco: Option<SharedEco>,
    pub(crate) cancel: CancelToken,
    /// The cache of the slot, used if nobody holds it (D1765).
    pub(crate) cache: Option<super::cache::SharedStc>,
}

impl TreeJob {
    /// The generation the job was started with.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The slot it searches.
    pub fn slot(&self) -> SlotId {
        self.slot
    }

    /// Asks the search to stop; it answers `Cancelled`.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

impl PartialEq for TreeJob {
    fn eq(&self, other: &TreeJob) -> bool {
        self.generation == other.generation && self.slot == other.slot && self.epoch == other.epoch
    }
}

/// The answer to a [`TreeJob`]: the result and the engine state it used.
#[derive(Debug, Clone)]
pub struct TreeAnswer {
    pub(crate) generation: u64,
    pub(crate) slot: SlotId,
    pub(crate) epoch: u64,
    pub(crate) state: TreeState,
    pub(crate) result: Result<Box<TreeResult>, TreeIssue>,
    /// The best games, when the job was asked for them.
    pub(crate) best: Option<Vec<BestRow>>,
    /// The search used the cache (it may have stored a tree).
    pub(crate) cache_used: Option<super::cache::SharedStc>,
}

impl TreeAnswer {
    /// The generation of the search that produced it.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

impl TreeAnswer {
    /// The answer for a search thread that failed: the state is lost.
    pub(crate) fn internal_failure(generation: u64, slot: SlotId, epoch: u64) -> TreeAnswer {
        TreeAnswer {
            generation,
            slot,
            epoch,
            state: TreeState::default(),
            result: Err(TreeIssue::Internal),
            best: None,
            cache_used: None,
        }
    }
}

impl PartialEq for TreeAnswer {
    fn eq(&self, other: &TreeAnswer) -> bool {
        self.generation == other.generation
            && self.slot == other.slot
            && self.epoch == other.epoch
            && self.result == other.result
    }
}

/// A [`GameSource`] that takes the lock of the shared database **per call**
/// (spec M5-2, D976/D986), so a long scan never starves the code that needs
/// the database (list, sort, text search, imports) and a cancel is seen
/// between games, not after the scan.
///
/// The trait returns references for `header`, `names` and `record_bytes`,
/// which cannot outlive a guard: the header and the names are cloned on the
/// first use (the scan itself never asks for them), the record bytes are
/// copied into a reused buffer.
pub(crate) struct LockingSource {
    source: SharedSource,
    header: OnceCell<Header>,
    names: OnceCell<NameBase>,
    buffer: Vec<u8>,
}

impl LockingSource {
    pub(crate) fn shared(&self) -> SharedSource {
        self.source.clone()
    }

    pub(crate) fn new(source: SharedSource) -> LockingSource {
        LockingSource {
            source,
            header: OnceCell::new(),
            names: OnceCell::new(),
            buffer: Vec::new(),
        }
    }
}

impl GameSource for LockingSource {
    fn is_scid_database(&self) -> bool {
        GameSource::is_scid_database(&*self.source.read())
    }

    fn header(&self) -> &Header {
        self.header
            .get_or_init(|| GameSource::header(&*self.source.read()).clone())
    }

    fn len(&self) -> u32 {
        GameSource::len(&*self.source.read())
    }

    fn entry(&self, n: u32) -> DbResult<IndexEntry> {
        GameSource::entry(&*self.source.read(), n)
    }

    fn names(&self) -> &NameBase {
        self.names
            .get_or_init(|| GameSource::names(&*self.source.read()).clone())
    }

    fn record_bytes(&mut self, n: u32) -> DbResult<&[u8]> {
        let mut guard = self.source.lock();
        let bytes = GameSource::record_bytes(&mut *guard, n)?;
        self.buffer.clear();
        self.buffer.extend_from_slice(bytes);
        drop(guard);
        Ok(&self.buffer)
    }

    fn raw_game(&mut self, n: u32) -> DbResult<RawGame> {
        GameSource::raw_game(&mut *self.source.lock(), n)
    }

    fn game(&mut self, n: u32) -> DbResult<Game> {
        GameSource::game(&mut *self.source.lock(), n)
    }
}

/// Runs a job (on a blocking thread). The database is locked per call, never
/// for the whole scan (D976, D986).
pub(crate) fn run_search(job: TreeJob) -> TreeAnswer {
    let TreeJob {
        generation,
        slot,
        epoch,
        source,
        mut state,
        position,
        ply,
        filter,
        sort,
        fast,
        adjust,
        hide_moves,
        best,
        eco,
        cancel,
        cache,
    } = job;
    let options = TreeOptions {
        sort,
        hide_moves,
        adjust,
        fast,
        eco_book: eco.as_deref(),
    };
    let query = TreeQuery {
        position: &position,
        ply,
        main_filter: &filter,
    };
    let mut locking = LockingSource::new(source);
    // A fill holds the lock for a long time: the search does not wait for
    // it, it runs without the cache (D1765).
    let mut guard = cache.as_ref().and_then(super::cache::try_lock);
    let cache_used = guard.is_some().then(|| cache.clone()).flatten();
    let mut none = NoCache;
    let cache_ref: &mut dyn mychess_db::tree::TreeCache = match guard.as_deref_mut() {
        Some(cache) => cache,
        None => &mut none,
    };
    let result = search(
        &mut locking,
        &query,
        &options,
        &mut state,
        cache_ref,
        &cancel,
        &mut NoProgress,
    );
    if let Some(mut guard) = guard.take()
        && let Some(error) = guard.take_load_error()
    {
        tracing::warn!(%error, "the tree cache file was not used");
    }
    // The list of best games is skipped when the search was cancelled while
    // it ran and in the fast pass of `FastThenSlow`, whose answer is replaced
    // by the slow pass at once (D1743).
    let first_pass = matches!(fast, FastMode::FastThenSlow);
    let best = match (&result, best) {
        (Ok(_), Some(spec)) if !cancel.is_cancelled() && !first_pass => {
            Some(super::best::compute(&locking.shared(), &state, spec))
        }
        _ => None,
    };
    let result = match result {
        Ok(result) => Ok(Box::new(result)),
        Err(error) => {
            if !matches!(error, TreeError::Cancelled) {
                tracing::warn!(%error, "opening tree search failed");
            }
            Err(TreeIssue::from(error))
        }
    };
    TreeAnswer {
        generation,
        slot,
        epoch,
        state,
        result,
        best,
        cache_used,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bases::{BaseSource, Clipbase};

    #[test]
    fn the_locking_source_holds_no_lock_between_calls() {
        let clip = Clipbase::new(100).expect("clipbase");
        let shared = SharedSource::new(BaseSource::Clipbase(Box::new(clip)));
        let mut locking = LockingSource::new(shared.clone());
        assert_eq!(locking.len(), 0);
        assert!(locking.entry(1).is_err());
        assert!(locking.record_bytes(1).is_err());
        // Names and header are cloned once; the lock is free right after.
        let _ = locking.names();
        let _ = locking.header();
        assert!(shared.try_read().is_some());
        drop(shared.lock());
    }

    #[test]
    fn the_locking_source_forwards_is_scid_database() {
        let clip = Clipbase::new(100).expect("clipbase");
        let shared = SharedSource::new(BaseSource::Clipbase(Box::new(clip)));
        assert!(!GameSource::is_scid_database(&LockingSource::new(shared)));
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().join("t");
        drop(mychess_db::si4::Si4Writer::create(&base, 1).expect("create"));
        let si4 = mychess_db::si4::Si4Database::open(&base).expect("open");
        let shared = SharedSource::new(BaseSource::Si4(Box::new(si4)));
        assert!(GameSource::is_scid_database(&LockingSource::new(shared)));
    }
}
