//! The opening tree panel (spec `docs/specs/m5-2-janela-da-arvore.md`,
//! sub-slice M5-2a): the controller side of the table of moves played from
//! the position of the game, with statistics, over the engine of
//! `mychess_db::tree`.
//!
//! - `prefs`: [`TreePrefs`] (`[tree]` in `preferences.toml`) and the
//!   front-end-facing [`TreeSort`] and [`TreeFastMode`];
//! - `message`: [`TreeMessage`], the [`TreeJob`] that [`Effect::TreeSearch`]
//!   carries and the [`TreeAnswer`] that comes back;
//! - `ui`: [`TreeUi`], the state of the panel (what is shown, the search in
//!   flight, the engine's [`TreeState`](mychess_db::tree::TreeState) of every
//!   slot);
//! - `view`: [`TreeView`], a ready-to-draw snapshot with numbers only (the
//!   front-end formats them with Fluent), and [`bar_segments`].
//!
//! The flow itself (refresh at the end of `update`, answers, adjust filter)
//! is in `state/tree_flow.rs`.
//!
//! [`Effect::TreeSearch`]: crate::Effect::TreeSearch

mod best;
mod cache;
mod mask;
mod message;
mod prefs;
mod ui;
mod view;

pub(crate) use best::BestSpec;
pub use best::{
    BEST_DEFAULT_MAX, BEST_MAX_CHOICES, BestColumn, BestMessage, BestOrder, BestResults, BestRow,
    BestUi, BestView,
};
pub use cache::{
    CacheAnswer, CacheBusy, CacheDiscard, CacheFill, CacheInfo, CacheIssue, CacheJob, CacheJobKind,
    CacheOutcome, CacheStatus, CacheView,
};
pub(crate) use cache::{
    CacheEntry, CacheUi, FileEpoch, RunningJob, SharedStc, file_guard,
    run_discard as run_cache_discard, run_job as run_cache_job, same_stc, try_lock,
};
pub(crate) use mask::perform_mask;
pub use mask::*;
pub(crate) use message::{LockingSource, run_search};
pub use message::{TreeAnswer, TreeIssue, TreeJob, TreeMessage};
pub use prefs::{
    CACHE_SIZE_RANGE, CACHE_SIZES, DEFAULT_CACHE_SIZE, TreeFastMode, TreePrefs, TreeSort,
};
pub(crate) use ui::TrainingWait as TrainingWaitInfo;
pub use ui::TreeUi;
pub(crate) use ui::{InFlight, Pending, Phase, RefreshKey, Shown, SlotTree};
pub use view::{
    BAR_WIDTH, BarSegments, TreeBody, TreeRowView, TreeTotalView, TreeView, bar_segments,
};
