//! The search for tactics and patterns (spec `docs/specs/m6-2-taticas-e-padroes.md`,
//! slice M6-2c: the controller side over the engine of `mychess_db::pattern`).
//!
//! - `message`: [`PatternSearchMessage`], the edits of the tree
//!   ([`PatternEdit`]), the answers of the work and the
//!   [`PatternSearchNotice`] with its Fluent keys;
//! - `edit`: the pure edits of the tree of conditions and the kinds of
//!   condition the menu offers ([`ConditionKind`]);
//! - `describe`: the sentence of a card as a Fluent key plus arguments;
//! - `prefs`: the `[pattern_search]` table ([`PatternSearchPrefs`]);
//! - `view`: [`PatternSearchView`], a snapshot of the window;
//! - `work`: what [`perform`](crate::perform) runs on a blocking thread.
//!
//! The flow itself (tabs, start, confirmation, answer, the first game, the
//! occurrences, the user's patterns) is in `state/pattern_search_flow.rs`.
//! A search works on a copy of the filter of its slot and replaces it only
//! when it ends; a stop leaves the filter as it was (D328).

mod describe;
pub(crate) mod edit;
mod message;
mod prefs;
mod view;
mod work;

/// The pattern model and the presets, re-exported so that the front-end
/// does not depend on `mychess-db` (the interface edits the same `Pattern`
/// that the presets and the files produce, D3251).
pub mod model {
    pub use mychess_db::pattern::model::*;
    pub use mychess_db::pattern::presets::{ParamKey, Params};
    pub use mychess_db::pattern::{Highlight, HighlightKind, HighlightRole};
}

use std::path::PathBuf;

use mychess_db::PatternHit;
use mychess_db::pattern::model::{Pattern, Scope};
use mychess_db::pattern::presets::Params;

pub use describe::{Sentence, SentenceArg, describe, describe_region};
pub use edit::{ConditionGroup, ConditionKind, check_limits, group_kind, node};
pub use message::{
    EditRefusal, GroupKind, NodePath, ParamValue, PatternCommand, PatternDone, PatternEdit,
    PatternFailure, PatternIssue, PatternRunSummary, PatternSearchMessage, PatternSearchNotice,
    PatternTab, RegionSlot, SavedFailure, SavedOutcome, SavedPattern,
};
pub use prefs::{MAX_HOLD, MAX_PRESET_ID, MAX_SCOPE_MOVE, PatternSearchPrefs};
pub use view::{
    CostView, HitsView, NodeView, PatternRunningView, PatternSearchView, PresetEntry,
    param_label_key,
};

pub(crate) use work::{run_hits, run_saved, run_search};

use crate::bases::{OpenControl, SharedSource, SlotId};
use crate::error::RequestId;

/// The presets, built once: `mychess_db::pattern::presets::find` copies the
/// whole catalog on every call, and the window asks for it at each draw
/// (D3299 item 43).
pub(crate) fn preset_catalog() -> &'static [mychess_db::pattern::presets::Preset] {
    static CATALOG: std::sync::LazyLock<Vec<mychess_db::pattern::presets::Preset>> =
        std::sync::LazyLock::new(mychess_db::pattern::presets::all);
    &CATALOG
}

/// The preset called `id`, without copying the catalog.
pub(crate) fn find_preset(id: &str) -> Option<mychess_db::pattern::presets::Preset> {
    preset_catalog().iter().find(|p| p.id == id).copied()
}

/// A search in flight.
#[derive(Debug)]
pub(crate) struct Running {
    pub(crate) request: RequestId,
    /// The slot whose filter the search changes.
    pub(crate) slot: SlotId,
    /// The database searched; the answer is dropped if the slot no longer
    /// holds it.
    pub(crate) source: SharedSource,
    /// What the filter looked like when the search began.
    pub(crate) len: u32,
    pub(crate) count: u32,
    pub(crate) control: OpenControl,
    /// The pattern, kept to compute the occurrences of the games it found.
    pub(crate) pattern: Pattern,
}

/// A search that waits for the answer to "this may take a long time".
#[derive(Debug, Clone)]
pub(crate) struct Confirm {
    pub(crate) estimated: u64,
    pub(crate) games: u32,
}

/// The pattern of the last search that ended: the games of its filter show
/// their occurrences.
#[derive(Debug, Clone)]
pub(crate) struct Focus {
    pub(crate) slot: SlotId,
    pub(crate) pattern: Pattern,
    /// The first game of the search, to land on its first occurrence when
    /// the filter's ply is only approximate (plies past 253).
    pub(crate) first: Option<u32>,
}

/// The occurrences in the game of the editor.
#[derive(Debug, Clone)]
pub(crate) struct GameHits {
    pub(crate) hits: Vec<PatternHit>,
    pub(crate) current: usize,
    pub(crate) from_test: bool,
}

/// A computation of occurrences in flight.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HitsRequest {
    pub(crate) request: RequestId,
    pub(crate) key: Option<(SlotId, u32)>,
    pub(crate) from_test: bool,
    pub(crate) land_first: bool,
}

/// The operation on the saved patterns in flight, for the notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SavedOp {
    List,
    Save,
    Load(String),
    Delete(String),
    Import,
    Export,
}

/// Where the user's patterns live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirChoice {
    /// `patterns_dir()`.
    SystemDefault,
    /// This folder, or none (saving warns about it).
    Fixed(Option<PathBuf>),
}

/// The state of the window of the search for patterns.
#[derive(Debug)]
pub struct PatternUi {
    pub(crate) open: bool,
    pub(crate) tab: PatternTab,
    /// The id of the preset picked.
    pub(crate) preset: Option<String>,
    pub(crate) params: Params,
    /// The scope in use; `None` until the window is opened (then the
    /// preferences' scope).
    pub(crate) scope: Option<Scope>,
    /// The pattern of the "Custom" tab.
    pub(crate) draft: Pattern,
    pub(crate) selection: Option<NodePath>,
    pub(crate) reference: Option<SlotId>,
    pub(crate) running: Option<Running>,
    pub(crate) progress: (u64, u64),
    pub(crate) confirm: Option<Confirm>,
    pub(crate) summary: Option<PatternRunSummary>,
    pub(crate) issue: Option<PatternSearchNotice>,
    pub(crate) focus: Option<Focus>,
    /// The game the occurrences were last asked for.
    pub(crate) hits_key: Option<(SlotId, u32)>,
    pub(crate) hits: Option<GameHits>,
    pub(crate) hits_request: Option<HitsRequest>,
    /// The folder of the user's patterns.
    pub(crate) dir: DirChoice,
    pub(crate) saved: Vec<SavedPattern>,
    pub(crate) saved_request: Option<RequestId>,
    pub(crate) saved_listed: bool,
    pub(crate) saved_op: SavedOp,
}

impl Default for PatternUi {
    fn default() -> PatternUi {
        PatternUi {
            open: false,
            tab: PatternTab::Presets,
            preset: None,
            params: Params::default(),
            scope: None,
            draft: Pattern {
                name: String::new(),
                description: String::new(),
                scope: Scope::default(),
                root: edit::empty_tree(),
            },
            selection: None,
            reference: None,
            running: None,
            progress: (0, 0),
            confirm: None,
            summary: None,
            issue: None,
            focus: None,
            hits_key: None,
            hits: None,
            hits_request: None,
            dir: DirChoice::SystemDefault,
            saved: Vec::new(),
            saved_request: None,
            saved_listed: false,
            saved_op: SavedOp::List,
        }
    }
}

impl PatternUi {
    /// Whether the window is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The tab on show.
    pub fn tab(&self) -> PatternTab {
        self.tab
    }

    /// Whether a search is running.
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// How many games the last search that ended found, if any ended.
    pub fn last_matched(&self) -> Option<u32> {
        self.summary.as_ref().map(|s| s.matched)
    }

    /// Games done and games to do of the search in flight.
    pub fn progress(&self) -> Option<(u64, u64)> {
        self.running.as_ref().map(|r| {
            let live = r.control.progress();
            if live == (0, 0) { self.progress } else { live }
        })
    }

    /// Stops the search in flight, if any.
    pub(crate) fn cancel(&self) {
        if let Some(running) = &self.running {
            running.control.cancel();
        }
    }
}

/// The folder of the user's patterns: `<data dir>/patterns`, `None` when
/// the system has no data directory (spec D3277).
pub fn patterns_dir() -> Option<PathBuf> {
    crate::start::data_dir().map(|dir| dir.join("patterns"))
}
