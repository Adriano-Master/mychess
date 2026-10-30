//! The arrangement of the panels: conversions between the application's
//! [`LayoutNode`] (plain data, spec D135, M2-6 D1000) and iced's `pane_grid`.
//!
//! The types themselves live in `mychess-app`; this module adds what only a
//! front-end can do. `SplitAxis::Horizontal` places the parts side by side,
//! which is iced's `pane_grid::Axis::Vertical` (a vertical split line). The
//! grid's pane is a [`Group`]: the tabs of one leaf of the tree.

use iced::widget::pane_grid::{self, Configuration};
pub use mychess_app::{LayoutNode, PaneKind, SplitAxis};

/// The state of one pane of the grid: a group of tabs (spec M2-6, D1009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The panels of the group, in tab order.
    pub tabs: Vec<PaneKind>,
    /// Index of the selected tab.
    pub active: usize,
}

impl Group {
    /// The panel shown by the group (the first when `active` is out of
    /// range; the board for an empty group, which a valid layout never has).
    pub fn active_kind(&self) -> PaneKind {
        self.tabs
            .get(self.active)
            .or_else(|| self.tabs.first())
            .copied()
            .unwrap_or(PaneKind::Board)
    }

    /// Whether the group has the panel as a tab.
    pub fn contains(&self, kind: PaneKind) -> bool {
        self.tabs.contains(&kind)
    }
}

/// The layout to use for a saved one: invalid layouts become the default.
pub fn or_default(saved: Option<&LayoutNode>) -> LayoutNode {
    saved
        .filter(|l| l.is_valid())
        .cloned()
        .unwrap_or_else(LayoutNode::default_layout)
}

/// The `pane_grid` configuration of the tree.
pub fn to_configuration(layout: &LayoutNode) -> Configuration<Group> {
    match layout {
        LayoutNode::Group { tabs, active } => Configuration::Pane(Group {
            tabs: tabs.clone(),
            active: *active,
        }),
        LayoutNode::Split { axis, ratio, a, b } => Configuration::Split {
            axis: match axis {
                SplitAxis::Horizontal => pane_grid::Axis::Vertical,
                SplitAxis::Vertical => pane_grid::Axis::Horizontal,
            },
            ratio: *ratio,
            a: Box::new(to_configuration(a)),
            b: Box::new(to_configuration(b)),
        },
    }
}

/// Reads the tree back from a `pane_grid` state.
pub fn from_state(state: &pane_grid::State<Group>) -> Option<LayoutNode> {
    from_node(state.layout(), state)
}

fn from_node(node: &pane_grid::Node, state: &pane_grid::State<Group>) -> Option<LayoutNode> {
    match node {
        pane_grid::Node::Pane(id) => state
            .get(*id)
            .map(|group| LayoutNode::group(group.tabs.clone(), group.active)),
        pane_grid::Node::Split {
            axis, ratio, a, b, ..
        } => Some(LayoutNode::Split {
            axis: match axis {
                pane_grid::Axis::Vertical => SplitAxis::Horizontal,
                pane_grid::Axis::Horizontal => SplitAxis::Vertical,
            },
            ratio: *ratio,
            a: Box::new(from_node(a, state)?),
            b: Box::new(from_node(b, state)?),
        }),
    }
}

/// The groups of the grid in layout order (left to right, top to bottom).
pub fn groups_in_order(state: &pane_grid::State<Group>) -> Vec<(pane_grid::Pane, &Group)> {
    fn walk<'a>(
        node: &pane_grid::Node,
        state: &'a pane_grid::State<Group>,
        out: &mut Vec<(pane_grid::Pane, &'a Group)>,
    ) {
        match node {
            pane_grid::Node::Pane(id) => {
                if let Some(group) = state.get(*id) {
                    out.push((*id, group));
                }
            }
            pane_grid::Node::Split { a, b, .. } => {
                walk(a, state, out);
                walk(b, state, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(state.layout(), state, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::unnecessary_box_returns)]
    fn pane(kind: PaneKind) -> Box<LayoutNode> {
        Box::new(LayoutNode::pane(kind))
    }

    fn roundtrip(layout: &LayoutNode) -> LayoutNode {
        let state = pane_grid::State::with_configuration(to_configuration(layout));
        from_state(&state).expect("a layout")
    }

    #[test]
    fn the_default_layout_round_trips_with_its_tabs() {
        let layout = LayoutNode::default_layout();
        assert!(layout.is_valid());
        assert_eq!(roundtrip(&layout), layout);
        let state = pane_grid::State::with_configuration(to_configuration(&layout));
        let groups = groups_in_order(&state);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].1.active_kind(), PaneKind::Board);
        assert_eq!(
            groups[1].1.tabs,
            [PaneKind::Notation, PaneKind::Info, PaneKind::Comment]
        );
    }

    #[test]
    fn grouped_layouts_keep_ratios_and_active_tabs() {
        let layout = LayoutNode::default_layout()
            .open(PaneKind::GameList)
            .open(PaneKind::Analysis)
            .select(PaneKind::Comment);
        assert_eq!(roundtrip(&layout), layout);
    }

    #[test]
    fn horizontal_axis_maps_to_a_vertical_split_line() {
        let state =
            pane_grid::State::with_configuration(to_configuration(&LayoutNode::default_layout()));
        assert!(matches!(
            state.layout(),
            pane_grid::Node::Split {
                axis: pane_grid::Axis::Vertical,
                ..
            }
        ));
    }

    #[test]
    fn an_out_of_range_active_tab_shows_the_first() {
        let group = Group {
            tabs: vec![PaneKind::Info, PaneKind::Comment],
            active: 9,
        };
        assert_eq!(group.active_kind(), PaneKind::Info);
        assert!(group.contains(PaneKind::Comment));
    }

    #[test]
    fn invalid_layouts_fall_back_to_the_default() {
        let no_board = LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            a: pane(PaneKind::Info),
            b: pane(PaneKind::Notation),
        };
        let repeated = LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            a: pane(PaneKind::Board),
            b: pane(PaneKind::Board),
        };
        let bad_ratio = LayoutNode::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.95,
            a: pane(PaneKind::Board),
            b: pane(PaneKind::Info),
        };
        let nan_ratio = LayoutNode::Split {
            axis: SplitAxis::Vertical,
            ratio: f32::NAN,
            a: pane(PaneKind::Board),
            b: pane(PaneKind::Info),
        };
        for bad in [no_board, repeated, bad_ratio, nan_ratio] {
            assert!(!bad.is_valid());
            assert_eq!(or_default(Some(&bad)), LayoutNode::default_layout());
        }
        assert_eq!(or_default(None), LayoutNode::default_layout());
    }
}
