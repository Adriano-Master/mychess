//! The crosstable panel: the table of the tournament of the game on screen
//! (spec `docs/specs/m6-4-tabela-estatisticas-rastreador.md`, slice a, with
//! the controller of slice M6-4 "app", decisions D3350 to D3369 and D3410 to
//! D3429).
//!
//! The table itself is computed by `mychess_db::crosstable`; this module is
//! the controller around it:
//!
//! - `message`: [`CrosstableMessage`], [`CrosstableEffect`] (the `perform`
//!   of the computation, the debounce, the export and the rendering for the
//!   clipboard), the notices and the mirrors of the labels;
//! - `prefs`: [`CrosstablePrefs`] (`[crosstable]`) and the mirrors of the
//!   format and the order;
//! - `view`: [`CrosstableView`], the ready-to-draw snapshot, which does not
//!   name `mychess-db`.
//!
//! The flow (the panel follows the game on screen, the debounce, the
//! answers, "Set filter", the export) is in `state/crosstable_flow.rs`.

mod message;
mod prefs;
mod view;

use crate::bases::{SharedSource, SlotId};
use crate::panel_calc::Refresh;

pub(crate) use message::perform;
pub use message::{
    CrossLabels, CrossWarning, CrosstableDone, CrosstableEffect, CrosstableFailure,
    CrosstableMessage, CrosstableNotice, CrosstableSpec, CrosstableToken, DEBOUNCE_MS,
    ExportFormat, TagsRef,
};
pub(crate) use prefs::is_hex_color;
pub use prefs::{CrossFlag, CrossFormat, CrossSort, CrosstablePrefs, DEFAULT_ROW_COLOR};
pub(crate) use view::table_view;
pub use view::{
    ClashView, CrossCellView, CrossColumnsView, CrossMatchView, CrossMode, CrossOutcome,
    CrossPerformance, CrossRoundView, CrossRowView, CrossTableView, CrosstableBody, CrosstableView,
};

/// What a computation was asked for; a different key means a new one.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CrossKey {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    /// [`BaseSlot::data_rev`](crate::bases::BaseSlot::data_rev): an edit
    /// that keeps the total still invalidates the table (D3428).
    pub(crate) data_rev: u64,
    pub(crate) spec: CrosstableSpec,
}

/// The table on show and what it was computed from.
#[derive(Debug)]
pub(crate) struct Shown {
    pub(crate) key: CrossKey,
    pub(crate) done: CrosstableDone,
    /// The table as the front-end draws it, converted once (D3425).
    pub(crate) view: std::sync::Arc<CrossTableView>,
    /// The game of the editor the `view` marks.
    pub(crate) current: Option<u32>,
}

/// The state of the crosstable panel.
#[derive(Debug, Default)]
pub struct CrosstableUi {
    pub(crate) open: bool,
    pub(crate) refresh: Refresh<CrossKey>,
    pub(crate) shown: Option<Shown>,
    /// Why there is no table, when the last computation had none.
    pub(crate) empty: Option<CrosstableNotice>,
    /// The tournament of this game, whatever the game on screen is.
    pub(crate) pinned: Option<(SlotId, u32)>,
    pub(crate) labels: CrossLabels,
    pub(crate) notice: Option<CrosstableNotice>,
}

impl CrosstableUi {
    /// Whether the panel is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether a table is being computed.
    pub fn is_running(&self) -> bool {
        self.refresh.running()
    }

    /// Stops the computation in flight (the program is ending).
    pub(crate) fn cancel(&mut self) {
        self.refresh.cancel();
    }

    /// The user cancelled the computation: no table is left in flight, and
    /// a panel without a table says so (D3422).
    pub(crate) fn user_cancel(&mut self) {
        if self.refresh.running() || self.refresh.delay.is_some() {
            self.refresh.user_cancel();
            if self.shown.is_none() {
                self.empty = Some(CrosstableNotice::Cancelled);
            }
        }
    }
}
