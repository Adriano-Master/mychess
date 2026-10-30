//! The analysis window (spec M4-4, D437-D450): what is shown for each
//! execution of an engine and the pure rules behind its buttons.
//!
//! - [`AnalysisView`], [`PvLine`], [`HistoryLine`], [`SearchStats`],
//!   [`LockedPosition`]: the state of one section (D438); the merge of the
//!   engine's reports into it is [`AnalysisView::apply_info`]'s rules.
//! - [`format_score`], [`format_pv`], [`format_comment`], [`short_nodes`]:
//!   the text of the lines (D439, D441).
//! - [`parse_exclusion`]: the "exclude moves" text (D445).
//! - [`AnalysisMessage`], [`AnalysisNotice`]: the messages and remarks.
//!
//! The wiring to `AppState` (events of the engines, the effects) is in
//! `state/analysis_flow.rs`; editing the game with an engine line is in
//! [`GameSession`](crate::GameSession) (D440).

mod exclude;
mod format;
mod message;
mod view;

pub use exclude::{ExcludeError, parse_exclusion, search_moves_without};
pub use format::{
    NO_MOVES_MATE_CP, NumberStyle, comment_score, format_comment, format_nodes, format_pv,
    format_pv_limited, format_score, format_seconds, format_wdl, nodes_per_second,
    permille_percent, short_nodes, wdl_percent,
};
pub use message::{AllLinesMode, AnalysisMessage, AnalysisNotice};
pub use view::{
    AnalysisView, Display, FormatOptions, HistoryLine, LockedPosition, MAX_HISTORY, PvLine,
    SearchStats, Terminal,
};
pub(crate) use view::{Binding, position_of};

use crate::engines::{EngineId, RunId};

/// An engine that is being restarted (low priority switch, D446): the new
/// execution inherits the settings of the section.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Restart {
    pub(crate) engine: EngineId,
    pub(crate) analyzing: bool,
    pub(crate) low_priority: bool,
    pub(crate) view: Box<AnalysisView>,
}

/// The views of the executions, in the order they started.
#[derive(Debug, Default)]
pub struct AnalysisViews {
    views: Vec<AnalysisView>,
    pub(crate) restarts: Vec<(RunId, Restart)>,
}

impl AnalysisViews {
    /// The views.
    pub fn as_slice(&self) -> &[AnalysisView] {
        &self.views
    }

    /// The view of an execution.
    pub fn get(&self, run: RunId) -> Option<&AnalysisView> {
        self.views.iter().find(|v| v.run == run)
    }

    pub(crate) fn get_mut(&mut self, run: RunId) -> Option<&mut AnalysisView> {
        self.views.iter_mut().find(|v| v.run == run)
    }

    pub(crate) fn push(&mut self, view: AnalysisView) {
        self.views.push(view);
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(RunId) -> bool) {
        self.views.retain(|v| keep(v.run));
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut AnalysisView> {
        self.views.iter_mut()
    }
}
