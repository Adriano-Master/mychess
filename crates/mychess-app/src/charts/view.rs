//! The statistics panel and the rating graph as the front-end draws them
//! (spec M6-4, slice b, D3350): plain data, no `mychess-db` types.

use std::sync::Arc;

use super::histogram::{FilterGraphPrefs, GraphView, StatsTab};
use super::message::ChartsNotice;
use super::rating::{RatingGraphView, RatingPrefs};
use super::stats::{StatsLineView, StatsOptionsView};

/// The statistics panel with its three tabs.
#[derive(Debug, Clone, PartialEq)]
pub struct StatsPanelView {
    /// The panel is open.
    pub open: bool,
    /// The tab on show.
    pub tab: StatsTab,
    /// The lines of the table (empty until the first computation ends).
    pub lines: Vec<StatsLineView>,
    /// The "Options" menu of the table; `None` until the current year is
    /// known.
    pub options: Option<StatsOptionsView>,
    /// The graph of the tab on show (`None` on the table tab or while it is
    /// being computed).
    pub graph: Option<Arc<GraphView>>,
    /// The configuration of the graphs.
    pub graph_prefs: FilterGraphPrefs,
    /// What is on show is not for the filter now: a computation is under way.
    pub stale: bool,
    /// `(done, total)` games of the computation in flight.
    pub progress: Option<(u64, u64)>,
    /// Games in the filter of the current database.
    pub filter_games: u32,
    /// Games in the current database.
    pub base_games: u32,
    /// There is no database to count in.
    pub unavailable: bool,
    /// The last failure.
    pub issue: Option<ChartsNotice>,
}

/// The rating graph.
#[derive(Debug, Clone, PartialEq)]
pub struct RatingPanelView {
    /// The graph is open.
    pub open: bool,
    /// The players, in the order of their lines.
    pub players: Vec<String>,
    /// More players fit (at most nine).
    pub can_add: bool,
    /// The lines and the axes (`None` until the first computation ends).
    pub graph: Option<Arc<RatingGraphView>>,
    /// The options.
    pub prefs: RatingPrefs,
    /// What is on show is not for the players and the options now.
    pub stale: bool,
    /// `(done, total)` games of the computation in flight.
    pub progress: Option<(u64, u64)>,
    /// There is no database to read.
    pub unavailable: bool,
    /// The last failure or remark.
    pub issue: Option<ChartsNotice>,
}
