//! The statistics and graphs of the filter, and the rating graph (spec
//! `docs/specs/m6-4-tabela-estatisticas-rastreador.md`, slice b, with the
//! controller of slice M6-4 "app", decisions D3370 to D3384 and D3410 to
//! D3429). Independent of `graphs/` (D3351).
//!
//! - `stats`: the table of the filter ([`StatsLineView`]) and its
//!   `[stats]` preferences;
//! - `histogram`: the graphs of the filter ([`GraphView`], [`YAxis`]) and
//!   their `[filter_graph]` preferences;
//! - `rating`: the rating graph ([`RatingGraphView`]) and its
//!   `[rating_graph]` preferences;
//! - `message`: [`ChartsMessage`], [`ChartsEffect`] and `perform`;
//! - `view`: [`StatsPanelView`] and [`RatingPanelView`].
//!
//! The flow (the panels follow the filter and the database, debounce,
//! answers) is in `state/charts_flow.rs`.

mod histogram;
mod message;
mod rating;
mod stats;
mod view;

use std::sync::Arc;

use mychess_db::{FilterGraphConfig, Histogram, RatingOptions, StatsConfig, StatsLine};

use crate::bases::{SharedSource, SlotId};
use crate::panel_calc::{FilterStamp, Refresh};

pub(crate) use histogram::graph_view;
pub use histogram::{
    FilterGraphKind, FilterGraphPrefs, GraphField, GraphPoint, GraphView, MAX_TICKS, StatsTab,
    YAxis,
};
pub(crate) use message::perform;
pub use message::{
    ChartsEffect, ChartsFailure, ChartsMessage, ChartsNotice, ChartsPart, DEBOUNCE_MS,
};
pub(crate) use rating::graph_view as rating_view;
pub use rating::{
    RATING_PALETTE, RatingAxes, RatingGraphView, RatingLineView, RatingPrefs, START_ELOS,
    START_YEARS,
};
pub use stats::{StatsLabel, StatsLineView, StatsOptionsView, StatsPrefs};
pub use view::{RatingPanelView, StatsPanelView};

/// What the table of the statistics was asked for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StatsKey {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    pub(crate) filter: FilterStamp,
    /// The year and the choices the configuration is made from (so that
    /// the key is compared without building the configuration, D3421).
    pub(crate) year: u16,
    pub(crate) prefs: StatsPrefs,
}

impl StatsKey {
    /// The configuration the engine takes.
    pub(crate) fn config(&self) -> StatsConfig {
        self.prefs.config(self.year)
    }
}

/// What the graph of the filter was asked for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GraphKey {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    pub(crate) filter: FilterStamp,
    pub(crate) kind: FilterGraphKind,
    pub(crate) config: FilterGraphConfig,
}

/// What the rating graph was asked for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RatingKey {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    /// Stamp of the filter; only the games when the filter does not count.
    pub(crate) filter: FilterStamp,
    pub(crate) players: Vec<String>,
    pub(crate) options: RatingOptions,
}

/// The graph of the filter on show, with the view converted once (D3425).
#[derive(Debug)]
pub(crate) struct GraphShown {
    pub(crate) key: GraphKey,
    pub(crate) histogram: Histogram,
    /// Whether `view` is the relative one.
    pub(crate) relative: bool,
    pub(crate) view: Arc<GraphView>,
}

/// The rating graph on show, with the view converted once (D3425).
#[derive(Debug)]
pub(crate) struct RatingShown {
    pub(crate) key: RatingKey,
    pub(crate) view: Arc<RatingGraphView>,
}

/// The state of the statistics panel and of the rating graph.
#[derive(Debug, Default)]
pub struct ChartsUi {
    /// The current year, once known.
    pub(crate) year: Option<u16>,
    pub(crate) year_asked: bool,
    pub(crate) stats_open: bool,
    pub(crate) stats: Refresh<StatsKey>,
    pub(crate) stats_shown: Option<(StatsKey, Vec<StatsLine>)>,
    pub(crate) graph: Refresh<GraphKey>,
    pub(crate) graph_shown: Option<GraphShown>,
    pub(crate) stats_issue: Option<ChartsNotice>,
    pub(crate) rating_open: bool,
    pub(crate) players: Vec<String>,
    pub(crate) rating: Refresh<RatingKey>,
    pub(crate) rating_shown: Option<RatingShown>,
    pub(crate) rating_issue: Option<ChartsNotice>,
}

impl ChartsUi {
    /// Whether the statistics panel is open.
    pub fn stats_is_open(&self) -> bool {
        self.stats_open
    }

    /// Whether the rating graph is open.
    pub fn rating_is_open(&self) -> bool {
        self.rating_open
    }

    /// The players of the rating graph, in order.
    pub fn players(&self) -> &[String] {
        &self.players
    }

    /// Whether any computation is in flight.
    pub fn is_running(&self) -> bool {
        self.stats.running() || self.graph.running() || self.rating.running()
    }

    /// Stops every computation (the program is ending).
    pub(crate) fn cancel(&mut self) {
        self.stats.cancel();
        self.graph.cancel();
        self.rating.cancel();
    }
}
