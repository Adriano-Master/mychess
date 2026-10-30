//! The state of the tree panel (spec M5-2, D961).

use std::sync::Arc;

use mychess_core::Position;
use mychess_db::Filter;
use mychess_db::progress::CancelToken;
use mychess_db::tree::{FastMode, Sort, TreeResult, TreeState};

use crate::bases::{SharedSource, SlotId};

use super::best::{BestKey, BestUi};
use super::message::TreeIssue;

/// What a refresh was started for. Two equal keys mean the same tree, so no
/// new search (D963). The short display is not part of it: it only changes
/// columns.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RefreshKey {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    /// The position of the cursor (squares and side are what the engine
    /// matches; the whole FEN is a cheap superset).
    pub(crate) fen: String,
    /// The ply of the cursor; only the fast modes look at it.
    pub(crate) ply: Option<u32>,
    pub(crate) sort: Sort,
    pub(crate) fast: FastMode,
    pub(crate) adjust: bool,
    /// Training hides the moves (D1621).
    pub(crate) hide_moves: bool,
    /// The best games asked for (D1602).
    pub(crate) best: Option<BestKey>,
}

/// Which pass of a mode a search is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// The only pass.
    Single,
    /// The fast pass of "fast then slow"; the slow one follows.
    FastFirst,
}

/// The search in flight (at most one, D976).
#[derive(Debug, Clone)]
pub(crate) struct InFlight {
    pub(crate) generation: u64,
    pub(crate) slot: SlotId,
    pub(crate) epoch: u64,
    pub(crate) cancel: CancelToken,
    pub(crate) phase: Phase,
    pub(crate) key: RefreshKey,
    /// What the search was asked about; the second pass of "fast then
    /// slow" repeats it even if the cursor moved meanwhile.
    pub(crate) position: Position,
    pub(crate) ply: u32,
    /// The filter of the slot the job was sent with (D990).
    pub(crate) filter: Filter,
}

/// What must be done to the engine state of a slot when it comes back from
/// a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pending {
    Nothing,
    /// End the tree: drop its filters (and give the slot its old filter
    /// back if it was adjusted).
    Clean,
    /// Make the current filter of the slot the filter "adjust" starts from.
    Commit,
}

/// The engine's state for one slot, valid for one database and one size.
#[derive(Debug)]
pub(crate) struct SlotTree {
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    /// Distinguishes this entry from an earlier one of the same slot, so a
    /// late answer cannot restore a state of another database.
    pub(crate) epoch: u64,
    /// `None` while the state is out in a search.
    pub(crate) state: Option<TreeState>,
    /// The filter of the slot is the adjusted one: closing the tree must
    /// give the old one back.
    pub(crate) adjusted: bool,
    /// The filter of the slot before the first adjust of this run of the
    /// tree: the one closing the tree (or a failed search) gives back (D986).
    pub(crate) original: Option<Filter>,
    /// The filter the tree last wrote to the slot; if the slot has another
    /// one, the user changed it and it is the new base (D979).
    pub(crate) applied: Option<Filter>,
    /// A restore that waits for a running list job (D987).
    pub(crate) restore: Option<Filter>,
    pub(crate) pending: Pending,
}

/// The tree on screen.
#[derive(Debug, Clone)]
pub(crate) struct Shown {
    pub(crate) result: Arc<TreeResult>,
    /// The FEN the rows were searched for.
    pub(crate) fen: String,
}

/// The state of the tree panel. Derived from the game and the databases by
/// `update`; the front-end reads it through
/// [`AppState::tree_view`](crate::AppState::tree_view) and sends
/// [`TreeMessage`](super::TreeMessage)s.
#[derive(Debug)]
pub struct TreeUi {
    pub(crate) open: bool,
    pub(crate) autorefresh: bool,
    /// Grows whenever a new search is wanted or the current one stops
    /// mattering; an answer with another value is dropped (D962).
    pub(crate) generation: u64,
    pub(crate) next_epoch: u64,
    pub(crate) in_flight: Option<InFlight>,
    /// A newer search waits for the one in flight to return.
    pub(crate) queued: bool,
    pub(crate) last_key: Option<RefreshKey>,
    pub(crate) slots: Vec<Option<SlotTree>>,
    pub(crate) shown: Option<Shown>,
    pub(crate) issue: Option<TreeIssue>,
    pub(crate) unavailable: bool,
    /// The one retry after `FilterSize` was used for this key.
    pub(crate) retried: bool,
    /// Searches started (for tests and diagnostics).
    pub(crate) started: u64,
    /// The best games section (D1600).
    pub(crate) best: BestUi,
    /// The database the tree is locked on (D1630).
    pub(crate) locked: Option<(SlotId, SharedSource)>,
    /// Training is on (D1620).
    pub(crate) training: bool,
    /// The training waits for the tree of this position to answer (D1623).
    pub(crate) training_owns_trial: bool,
    pub(crate) training_wait: Option<TrainingWait>,
    /// The tree is playing its own answer (D1622).
    pub(crate) answering: bool,
    /// The state of the generator of the training (D1624).
    pub(crate) rng: u64,
    /// The file of "open base as tree" waits for its slot (D1640).
    pub(crate) open_as_tree: Option<std::path::PathBuf>,
    /// The application asks the front-end to show the panel (D1641).
    pub(crate) show_request: bool,
    /// The caches of the trees of the slots (spec M5-3b).
    pub(crate) cache: super::CacheUi,
    /// The tree masks (spec M5-4b).
    pub(crate) mask: super::MaskUi,
}

/// What the training waits for after a move of the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrainingWait {
    pub(crate) fen: String,
    pub(crate) cursor: mychess_core::NodeId,
}

impl Default for TreeUi {
    fn default() -> TreeUi {
        TreeUi {
            open: false,
            autorefresh: true,
            generation: 0,
            next_epoch: 0,
            in_flight: None,
            queued: false,
            last_key: None,
            slots: (0..SlotId::COUNT).map(|_| None).collect(),
            shown: None,
            issue: None,
            unavailable: false,
            retried: false,
            started: 0,
            best: BestUi::default(),
            locked: None,
            training: false,
            training_owns_trial: false,
            training_wait: None,
            answering: false,
            rng: 0,
            open_as_tree: None,
            show_request: false,
            cache: super::CacheUi::default(),
            mask: super::MaskUi::default(),
        }
    }
}

impl TreeUi {
    /// Whether the panel is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the table follows the game ("Refresh").
    pub fn autorefresh(&self) -> bool {
        self.autorefresh
    }

    /// Whether the training is on.
    pub fn training(&self) -> bool {
        self.training
    }

    /// Whether the tree is locked on a database.
    pub fn locked(&self) -> bool {
        self.locked.is_some()
    }

    /// The best games section.
    pub fn best(&self) -> &BestUi {
        &self.best
    }

    /// Fixes the state of the generator of the training (for tests).
    pub fn seed_training(&mut self, seed: u64) {
        self.rng = seed.max(1);
    }

    /// The next number of the generator; seeded from the clock on first use.
    pub(crate) fn next_random(&mut self) -> u32 {
        if self.rng == 0 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| {
                    u64::try_from(d.as_nanos() & u128::from(u64::MAX)).unwrap_or(1)
                });
            self.rng = nanos.max(1);
        }
        // xorshift64*; the high half is the better one.
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        u32::try_from(x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32).unwrap_or(0)
    }

    /// Whether a search is running or waiting to start.
    pub fn searching(&self) -> bool {
        self.in_flight.is_some() || self.queued
    }

    /// The current generation (grows with every search wanted).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// How many searches were started since the state was created.
    pub fn searches_started(&self) -> u64 {
        self.started
    }

    pub(crate) fn fresh_epoch(&mut self) -> u64 {
        self.next_epoch = self.next_epoch.wrapping_add(1);
        self.next_epoch
    }

    pub(crate) fn entry_mut(&mut self, slot: SlotId) -> Option<&mut SlotTree> {
        self.slots.get_mut(slot.index()).and_then(Option::as_mut)
    }

    /// Cancels the search in flight, if any, and makes whatever comes back
    /// stale.
    pub(crate) fn cancel_search(&mut self) {
        if let Some(flight) = &self.in_flight {
            flight.cancel.cancel();
        }
        self.generation = self.generation.wrapping_add(1);
        self.queued = false;
    }
}
