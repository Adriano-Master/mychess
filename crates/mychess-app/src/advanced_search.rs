//! The advanced searches: by position, by material and patterns, and by a
//! sequence of moves (spec `docs/specs/m6-1-buscas-avancadas.md`, slice
//! M6-1b: the controller side over the engines of `mychess_db::search`).
//!
//! - `message`: [`AdvancedSearchMessage`], the answers of the searches and the
//!   [`AdvancedSearchNotice`] with its Fluent keys;
//! - `prefs`: the options the front-end can name ([`SearchOp`],
//!   [`BoardKind`]...) and the `[advanced_search]` table
//!   ([`AdvancedSearchPrefs`]);
//! - `view`: [`AdvancedSearchView`], a snapshot of the window without
//!   `mychess-db` types;
//! - `work`: what [`perform`](crate::perform) runs on a blocking thread.
//!
//! The flow itself (forms, start, answers, the first game, saved searches)
//! is in `state/advanced_search_flow.rs`. Each search works on a copy of the
//! filter of the slot it is aimed at and replaces it only when it ends; a
//! stop leaves the filter as it was (D3202).

mod message;
mod prefs;
mod view;
mod work;

use std::collections::VecDeque;
use std::path::PathBuf;

use mychess_db::{MaterialSearch, Pattern};

pub use message::{
    AdvancedDone, AdvancedKind, AdvancedSearchMessage, AdvancedSearchNotice, MAX_MATERIAL_DIFF,
    MAX_PIECE_COUNT, MaterialPiece, PATTERN_ROWS, SavedCommand, SavedEntry, SavedFailure,
    SavedOutcome, SearchFailure, SearchSummary,
};
pub use prefs::{
    AdvancedSearchPrefs, BishopsKind, BoardKind, CheckKind, MAX_MIN_PLIES, MAX_MOVE_NUMBER,
    SearchOp, SideKind,
};
pub use view::{
    AdvancedSearchView, DatabaseChoice, MaterialView, MovesView, PatternView, PositionView,
    PresetView, RangeView, RunningView, SideView,
};

pub(crate) use view::databases as database_choices;
pub(crate) use work::{run_saved, run_search};

use crate::bases::{OpenControl, SharedSource, SlotId};
use crate::error::RequestId;

/// A search in flight.
#[derive(Debug)]
pub(crate) struct Running {
    pub(crate) request: RequestId,
    pub(crate) kind: AdvancedKind,
    /// The slot whose filter the search changes.
    pub(crate) slot: SlotId,
    /// The database searched; the answer is dropped if the slot no longer
    /// holds it.
    pub(crate) source: SharedSource,
    /// What the filter looked like when the search began: its size and its
    /// count, to notice a change under the search.
    pub(crate) len: u32,
    pub(crate) count: u32,
    pub(crate) control: OpenControl,
}

/// The state of the window of the advanced searches.
#[derive(Debug)]
pub struct AdvancedUi {
    pub(crate) open: bool,
    pub(crate) tab: AdvancedKind,
    /// The text of the FEN field of the position tab, as typed (it may be
    /// empty). Only used while [`fen_mode`](AdvancedUi::fen_mode) is on.
    pub(crate) fen: String,
    /// Whether the position tab searches the FEN field (`true`) or the
    /// position on the board (`false`). Kept apart from the text so that an
    /// emptied field stays on show (D3392).
    pub(crate) fen_mode: bool,
    /// The ranges, the difference and the patterns of the material tab (the
    /// other options live in the preferences).
    pub(crate) material: MaterialSearch,
    pub(crate) moves_text: String,
    /// "Search in database" of each tab.
    pub(crate) references: [Option<SlotId>; 3],
    pub(crate) running: Option<Running>,
    pub(crate) progress: (u64, u64),
    pub(crate) summary: Option<SearchSummary>,
    pub(crate) issue: Option<AdvancedSearchNotice>,
    pub(crate) saved: Vec<SavedEntry>,
    /// The saved-search operation in flight; one at a time (D3390).
    pub(crate) saved_op: Option<SavedPending>,
    /// The operations asked while one was in flight, in order (short).
    pub(crate) saved_queue: VecDeque<(PathBuf, SavedCommand)>,
    /// The list of saved searches was asked for at least once.
    pub(crate) saved_listed: bool,
}

/// The most saved-search operations kept waiting behind the one in flight.
pub(crate) const SAVED_QUEUE_MAX: usize = 8;

/// The saved-search operation in flight and what its answer needs.
#[derive(Debug)]
pub(crate) struct SavedPending {
    pub(crate) request: RequestId,
    /// The operation reads a `.sso` file (a failure is told as an import
    /// failure).
    pub(crate) import: bool,
    /// The name asked, for the notice that says it was not found.
    pub(crate) name: Option<String>,
}

impl Default for AdvancedUi {
    fn default() -> AdvancedUi {
        let material = MaterialSearch {
            patterns: vec![Pattern::default(); PATTERN_ROWS],
            ..MaterialSearch::default()
        };
        AdvancedUi {
            open: false,
            tab: AdvancedKind::Position,
            fen: String::new(),
            fen_mode: false,
            material,
            moves_text: String::new(),
            references: [None; 3],
            running: None,
            progress: (0, 0),
            summary: None,
            issue: None,
            saved: Vec::new(),
            saved_op: None,
            saved_queue: VecDeque::new(),
            saved_listed: false,
        }
    }
}

impl AdvancedUi {
    pub(crate) fn reference(&self, kind: AdvancedKind) -> Option<SlotId> {
        self.references.get(kind.index()).copied().flatten()
    }

    /// Whether the window is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The tab on show.
    pub fn tab(&self) -> AdvancedKind {
        self.tab
    }

    /// Whether a search is running.
    pub fn is_running(&self) -> bool {
        self.running.is_some()
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
