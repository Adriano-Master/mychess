//! Helpers shared by the M2-6 layout contract tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use mychess_app::{
    LayoutNode, LoadedPreferences, PaneKind, Preferences, SplitAxis, load_preferences,
    render_preferences,
};

pub(crate) const ALL: [PaneKind; 8] = [
    PaneKind::Board,
    PaneKind::Info,
    PaneKind::Notation,
    PaneKind::Comment,
    PaneKind::GameList,
    PaneKind::Analysis,
    PaneKind::Graph,
    PaneKind::Tree,
];

pub(crate) fn pane(kind: PaneKind) -> LayoutNode {
    LayoutNode::pane(kind)
}

pub(crate) fn grp(tabs: &[PaneKind], active: usize) -> LayoutNode {
    LayoutNode::group(tabs.to_vec(), active)
}

pub(crate) fn h(ratio: f32, a: LayoutNode, b: LayoutNode) -> LayoutNode {
    LayoutNode::Split {
        axis: SplitAxis::Horizontal,
        ratio,
        a: Box::new(a),
        b: Box::new(b),
    }
}

pub(crate) fn v(ratio: f32, a: LayoutNode, b: LayoutNode) -> LayoutNode {
    LayoutNode::Split {
        axis: SplitAxis::Vertical,
        ratio,
        a: Box::new(a),
        b: Box::new(b),
    }
}

pub(crate) fn default_layout() -> LayoutNode {
    LayoutNode::default_layout()
}

/// Tabs and active index of the group holding `kind` (must be open).
pub(crate) fn tabs_of(layout: &LayoutNode, kind: PaneKind) -> (Vec<PaneKind>, usize) {
    let (tabs, active) = layout.group_of(kind).expect("panel is open");
    (tabs.to_vec(), active)
}

/// The preferences file text that holds `layout`.
pub(crate) fn render(layout: &LayoutNode) -> String {
    render_preferences(&Preferences {
        layout: Some(layout.clone()),
        ..Preferences::default()
    })
}

pub(crate) fn load(text: &str) -> LoadedPreferences {
    load_preferences(text)
}

/// Writes then reads a layout through the preferences file format.
pub(crate) fn round_trip(layout: &LayoutNode) -> Option<LayoutNode> {
    load(&render(layout)).prefs.layout
}

/// Reads a layout given as an inline TOML table (the text after `layout = `).
pub(crate) fn read_inline(table: &str) -> LoadedPreferences {
    load(&format!("layout = {table}\n"))
}

/// What the application would show: the layout read, or the default.
pub(crate) fn effective(table: &str) -> LayoutNode {
    read_inline(table)
        .prefs
        .layout
        .unwrap_or_else(default_layout)
}

pub(crate) fn same_panes(a: &LayoutNode, b: &LayoutNode) -> bool {
    let mut x = a
        .panes()
        .iter()
        .map(|k| format!("{k:?}"))
        .collect::<Vec<_>>();
    let mut y = b
        .panes()
        .iter()
        .map(|k| format!("{k:?}"))
        .collect::<Vec<_>>();
    x.sort();
    y.sort();
    x == y
}
