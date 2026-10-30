//! The graph as the application keeps it: the cached view and the current
//! move bar (spec M4-7, D872).

use mychess_core::NodeId;

use super::line::GraphStyle;
use super::series::Invert;
use super::view::{GraphKind, GraphPrefs, ScoreGraphView};

/// What the cached view was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ViewKey {
    /// [`GameSession::changes`](crate::GameSession::changes): grows on every
    /// change of the game content, replacing the game included.
    pub(crate) changes: u64,
    /// Only the preferences that shape the series: `highlight_current` only
    /// moves the bar of the current move, so toggling it must not rebuild
    /// the view (nor make the front-end redraw its static layer).
    pub(crate) kind: GraphKind,
    pub(crate) invert: Invert,
    pub(crate) style: GraphStyle,
}

impl ViewKey {
    /// The key of the view of the game at `changes` with `prefs`.
    pub(crate) fn new(changes: u64, prefs: GraphPrefs) -> ViewKey {
        ViewKey {
            changes,
            kind: prefs.kind,
            invert: prefs.invert,
            style: prefs.style,
        }
    }
}

/// What the cached bar of the current move was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BarKey {
    pub(crate) changes: u64,
    pub(crate) cursor: NodeId,
    pub(crate) prefs: GraphPrefs,
}

/// The state of the graph panel. It is derived data: `update` rebuilds it
/// when the game, the cursor or the preferences change and the panel is
/// open, so `view` only reads it.
#[derive(Debug, Default)]
pub struct GraphState {
    /// A mirror of the layout of the front-end: the graph pane is open. A
    /// front-end with a layout (the GUI) is the source of truth and sets it
    /// with [`GraphMessage::SetOpen`](super::GraphMessage::SetOpen) every frame; `Toggle` only makes sense
    /// for a front-end without a layout.
    pub(crate) open: bool,
    pub(crate) view: Option<ScoreGraphView>,
    pub(crate) view_key: Option<ViewKey>,
    pub(crate) bar: Option<f32>,
    pub(crate) bar_key: Option<BarKey>,
    /// Counts the rebuilds of the view; the front-end clears its drawing
    /// caches when it changes.
    pub(crate) stamp: u64,
    /// How many times the series were computed (for tests of the cache).
    pub(crate) builds: u64,
}

impl GraphState {
    /// Whether the panel is open (the view is only kept while it is).
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The graph to draw: `None` while the panel is closed or before the
    /// first build.
    pub fn view(&self) -> Option<&ScoreGraphView> {
        self.view.as_ref()
    }

    /// The x coordinate of the bar of the current move, when it is to be
    /// highlighted.
    pub fn current_x(&self) -> Option<f32> {
        self.bar
    }

    /// A number that changes each time [`view`](GraphState::view) is rebuilt.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    /// Times the series were computed since the state was created.
    pub fn builds(&self) -> u64 {
        self.builds
    }

    pub(crate) fn forget(&mut self) {
        self.view = None;
        self.view_key = None;
        self.bar = None;
        self.bar_key = None;
    }
}
