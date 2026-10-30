//! Pure operations on a [`LayoutNode`] (spec M2-6 D1003-D1007, D1012).
//!
//! Every operation returns a valid layout, or a copy of the input when it
//! does not apply (unknown panel, the board, a result deeper than allowed).
//! None of them panics.

use super::{LayoutNode, PaneKind, SplitAxis};

/// Split ratio of a panel opened beside the board (D1004).
const BESIDE_BOARD_RATIO: f32 = 0.60;
/// Split ratio of a tab detached from its group (D1006).
const DETACH_RATIO: f32 = 0.50;
/// Ratios of the other panels, placed around the whole arrangement.
const GAMES_RATIO: f32 = 0.58;
const ANALYSIS_RATIO: f32 = 0.64;
const GRAPH_RATIO: f32 = 0.70;
const TREE_RATIO: f32 = 0.56;
const TOURNAMENT_RATIO: f32 = 0.45;
const REPORT_RATIO: f32 = 0.5;
const BOOK_RATIO: f32 = 0.5;
const TABLEBASE_RATIO: f32 = 0.6;
/// Share of the arrangement kept by the panels of spec M6-4 (crosstable,
/// statistics, rating graph, piece tracker) when they open to the right.
const TOOL_PANE_RATIO: f32 = 0.55;

/// The panels that share the notation group (D1004).
const NOTATION_FAMILY: [PaneKind; 3] = [PaneKind::Notation, PaneKind::Info, PaneKind::Comment];

impl LayoutNode {
    /// The groups, left to right: their tabs and the active index.
    pub fn groups(&self) -> Vec<(&[PaneKind], usize)> {
        let mut out = Vec::new();
        self.collect_groups(&mut out);
        out
    }

    fn collect_groups<'a>(&'a self, out: &mut Vec<(&'a [PaneKind], usize)>) {
        match self {
            LayoutNode::Group { tabs, active } => out.push((tabs, *active)),
            LayoutNode::Split { a, b, .. } => {
                a.collect_groups(out);
                b.collect_groups(out);
            }
        }
    }

    /// The group that holds `kind`: its tabs and the active index.
    pub fn group_of(&self, kind: PaneKind) -> Option<(&[PaneKind], usize)> {
        self.groups()
            .into_iter()
            .find(|(tabs, _)| tabs.contains(&kind))
    }

    /// Whether the panel is open (a tab of some group, active or not).
    pub fn contains(&self, kind: PaneKind) -> bool {
        self.group_of(kind).is_some()
    }

    /// The panel shown by each group (its active tab), left to right.
    pub fn visible_panes(&self) -> Vec<PaneKind> {
        self.groups()
            .into_iter()
            .filter_map(|(tabs, active)| tabs.get(active).copied())
            .collect()
    }

    /// Merges the group of `source` into the group of `target` (D1005): its
    /// tabs go to the end and its active tab becomes the active one. The
    /// source group disappears from its place. No effect for the same group,
    /// an unknown panel, or the board.
    #[must_use]
    pub fn merge(&self, source: PaneKind, target: PaneKind) -> LayoutNode {
        let (Some((src_tabs, src_active)), Some((dst_tabs, _))) =
            (self.group_of(source), self.group_of(target))
        else {
            return self.clone();
        };
        if src_tabs.contains(&target)
            || src_tabs.contains(&PaneKind::Board)
            || dst_tabs.contains(&PaneKind::Board)
        {
            return self.clone();
        }
        let (src_tabs, src_active) = (src_tabs.to_vec(), src_active);
        let Some(rest) = self.without_group(source) else {
            return self.clone();
        };
        let merged = rest.edit_group(target, &|tabs, active| {
            *active = tabs.len() + src_active;
            tabs.extend(src_tabs.iter().copied());
        });
        self.checked(merged)
    }

    /// Takes the tab `kind` out of its group into a new split to the right
    /// of it, ratio 0.50 (D1006). No effect when it is alone in its group.
    #[must_use]
    pub fn detach(&self, kind: PaneKind) -> LayoutNode {
        match self.group_of(kind) {
            Some((tabs, _)) if tabs.len() >= 2 => {}
            _ => return self.clone(),
        }
        self.checked(self.detach_in(kind))
    }

    fn detach_in(&self, kind: PaneKind) -> LayoutNode {
        match self {
            LayoutNode::Group { tabs, active } if tabs.contains(&kind) && tabs.len() >= 2 => {
                let (rest, active) = remove_tab(tabs, *active, kind);
                LayoutNode::Split {
                    axis: SplitAxis::Horizontal,
                    ratio: DETACH_RATIO,
                    a: Box::new(LayoutNode::group(rest, active)),
                    b: Box::new(LayoutNode::pane(kind)),
                }
            }
            LayoutNode::Group { .. } => self.clone(),
            LayoutNode::Split { axis, ratio, a, b } => LayoutNode::Split {
                axis: *axis,
                ratio: *ratio,
                a: Box::new(a.detach_in(kind)),
                b: Box::new(b.detach_in(kind)),
            },
        }
    }

    /// Closes the panel (D1003, D1012): the tab leaves its group; an empty
    /// group is removed and its sibling takes the parent's place. The board
    /// cannot be closed.
    #[must_use]
    pub fn close(&self, kind: PaneKind) -> LayoutNode {
        if kind == PaneKind::Board || !self.contains(kind) {
            return self.clone();
        }
        match self.without_tab(kind) {
            Some(next) => self.checked(next),
            None => self.clone(),
        }
    }

    /// Makes `kind` the active tab of its group (D1007).
    #[must_use]
    pub fn select(&self, kind: PaneKind) -> LayoutNode {
        let Some((tabs, _)) = self.group_of(kind) else {
            return self.clone();
        };
        let Some(index) = tabs.iter().position(|k| *k == kind) else {
            return self.clone();
        };
        self.checked(self.edit_group(kind, &|_, active| *active = index))
    }

    /// Opens the panel and selects it (D1004, D1007). Already open: only
    /// selected. `Notation`, `Info` and `Comment` become a tab of the first
    /// group that holds one of the other two, or else a split to the right
    /// of the board (ratio 0.60). The other panels go around the whole
    /// arrangement, as the front-end always did.
    #[must_use]
    pub fn open(&self, kind: PaneKind) -> LayoutNode {
        if self.contains(kind) {
            return self.select(kind);
        }
        let next = match kind {
            PaneKind::Board => return self.clone(),
            PaneKind::Notation | PaneKind::Info | PaneKind::Comment => {
                let anchor = self
                    .groups()
                    .into_iter()
                    .find(|(tabs, _)| {
                        tabs.iter()
                            .any(|t| *t != kind && NOTATION_FAMILY.contains(t))
                    })
                    .and_then(|(tabs, _)| tabs.first().copied());
                match anchor {
                    Some(anchor) => self.edit_group(anchor, &|tabs, active| {
                        tabs.push(kind);
                        *active = tabs.len() - 1;
                    }),
                    None => self.map_leaves(&|leaf| match leaf {
                        LayoutNode::Group { tabs, .. } if tabs.as_slice() == [PaneKind::Board] => {
                            LayoutNode::Split {
                                axis: SplitAxis::Horizontal,
                                ratio: BESIDE_BOARD_RATIO,
                                a: Box::new(leaf.clone()),
                                b: Box::new(LayoutNode::pane(kind)),
                            }
                        }
                        other => other.clone(),
                    }),
                }
            }
            PaneKind::GameList => self.wrapped(SplitAxis::Vertical, GAMES_RATIO, kind),
            PaneKind::Analysis => self.wrapped(SplitAxis::Horizontal, ANALYSIS_RATIO, kind),
            PaneKind::Graph => self.wrapped(SplitAxis::Vertical, GRAPH_RATIO, kind),
            PaneKind::Tree => self.wrapped(SplitAxis::Horizontal, TREE_RATIO, kind),
            PaneKind::Tournament => self.wrapped(SplitAxis::Horizontal, TOURNAMENT_RATIO, kind),
            PaneKind::Report => self.wrapped(SplitAxis::Horizontal, REPORT_RATIO, kind),
            PaneKind::Book => self.wrapped(SplitAxis::Horizontal, BOOK_RATIO, kind),
            PaneKind::Tablebase => self.wrapped(SplitAxis::Horizontal, TABLEBASE_RATIO, kind),
            PaneKind::Crosstable
            | PaneKind::FilterStats
            | PaneKind::RatingGraph
            | PaneKind::PieceTracker
            | PaneKind::Epd => self.wrapped(SplitAxis::Horizontal, TOOL_PANE_RATIO, kind),
        };
        self.checked(next)
    }

    fn wrapped(&self, axis: SplitAxis, ratio: f32, kind: PaneKind) -> LayoutNode {
        LayoutNode::Split {
            axis,
            ratio,
            a: Box::new(self.clone()),
            b: Box::new(LayoutNode::pane(kind)),
        }
    }

    /// `next` when it is valid, else a copy of `self`.
    fn checked(&self, next: LayoutNode) -> LayoutNode {
        if next.is_valid() { next } else { self.clone() }
    }

    /// Applies `f` to the tabs and the active index of the group holding
    /// `anchor`.
    fn edit_group(
        &self,
        anchor: PaneKind,
        f: &impl Fn(&mut Vec<PaneKind>, &mut usize),
    ) -> LayoutNode {
        match self {
            LayoutNode::Group { tabs, active } if tabs.contains(&anchor) => {
                let (mut tabs, mut active) = (tabs.clone(), *active);
                f(&mut tabs, &mut active);
                LayoutNode::Group { tabs, active }
            }
            LayoutNode::Group { .. } => self.clone(),
            LayoutNode::Split { axis, ratio, a, b } => LayoutNode::Split {
                axis: *axis,
                ratio: *ratio,
                a: Box::new(a.edit_group(anchor, f)),
                b: Box::new(b.edit_group(anchor, f)),
            },
        }
    }

    /// The tree without the tab `kind`; `None` when nothing is left.
    fn without_tab(&self, kind: PaneKind) -> Option<LayoutNode> {
        match self {
            LayoutNode::Group { tabs, active } if tabs.contains(&kind) => {
                let (rest, active) = remove_tab(tabs, *active, kind);
                (!rest.is_empty()).then(|| LayoutNode::group(rest, active))
            }
            LayoutNode::Group { .. } => Some(self.clone()),
            LayoutNode::Split { axis, ratio, a, b } => {
                collapse(*axis, *ratio, a.without_tab(kind), b.without_tab(kind))
            }
        }
    }

    /// The tree without the whole group that holds `kind`.
    fn without_group(&self, kind: PaneKind) -> Option<LayoutNode> {
        match self {
            LayoutNode::Group { tabs, .. } if tabs.contains(&kind) => None,
            LayoutNode::Group { .. } => Some(self.clone()),
            LayoutNode::Split { axis, ratio, a, b } => {
                collapse(*axis, *ratio, a.without_group(kind), b.without_group(kind))
            }
        }
    }
}

/// Rebuilds a split after its parts were pruned: a lost side leaves the
/// other one in the parent's place.
fn collapse(
    axis: SplitAxis,
    ratio: f32,
    a: Option<LayoutNode>,
    b: Option<LayoutNode>,
) -> Option<LayoutNode> {
    match (a, b) {
        (Some(a), Some(b)) => Some(LayoutNode::Split {
            axis,
            ratio,
            a: Box::new(a),
            b: Box::new(b),
        }),
        (only, None) | (None, only) => only,
    }
}

/// `tabs` without `kind`, and the active index adjusted: the selection
/// stays on the same tab, or moves to the neighbour when it was removed.
fn remove_tab(tabs: &[PaneKind], active: usize, kind: PaneKind) -> (Vec<PaneKind>, usize) {
    let Some(index) = tabs.iter().position(|k| *k == kind) else {
        return (tabs.to_vec(), active);
    };
    let rest: Vec<PaneKind> = tabs.iter().copied().filter(|k| *k != kind).collect();
    let active = if index < active { active - 1 } else { active };
    let active = active.min(rest.len().saturating_sub(1));
    (rest, active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use PaneKind::{Analysis, Board, Comment, GameList, Info, Notation, Tree};

    fn default() -> LayoutNode {
        LayoutNode::default_layout()
    }

    fn tabs_of(layout: &LayoutNode, kind: PaneKind) -> (Vec<PaneKind>, usize) {
        let (tabs, active) = layout.group_of(kind).expect("open");
        (tabs.to_vec(), active)
    }

    #[test]
    fn close_removes_a_tab_and_keeps_the_selection() {
        let closed = default().select(Info).close(Comment);
        assert_eq!(tabs_of(&closed, Info), (vec![Notation, Info], 1));
        let closed = default().select(Info).close(Notation);
        assert_eq!(tabs_of(&closed, Info), (vec![Info, Comment], 0));
        assert!(closed.is_valid());
    }

    #[test]
    fn closing_the_last_tab_gives_the_space_to_the_sibling() {
        let layout = default().close(Info).close(Comment).close(Notation);
        assert_eq!(layout, LayoutNode::pane(Board));
        assert_eq!(layout.close(Board), layout);
        assert_eq!(layout.close(Tree), layout);
    }

    #[test]
    fn merge_appends_tabs_and_activates_the_dragged_one() {
        let layout = default().open(GameList).merge(GameList, Notation);
        assert!(layout.is_valid());
        assert_eq!(
            tabs_of(&layout, Notation),
            (vec![Notation, Info, Comment, GameList], 3)
        );
        assert_eq!(layout, default().open(GameList).merge(GameList, Info));
        // The vertical split of the game list is gone.
        assert_eq!(layout.groups().len(), 2);
    }

    #[test]
    fn merge_ignores_the_same_group_the_board_and_unknown_panels() {
        let layout = default();
        assert_eq!(layout.merge(Info, Comment), layout);
        assert_eq!(layout.merge(Board, Notation), layout);
        assert_eq!(layout.merge(Notation, Board), layout);
        assert_eq!(layout.merge(Tree, Notation), layout);
        assert_eq!(layout.merge(Notation, Tree), layout);
    }

    #[test]
    fn detach_splits_a_tab_to_the_right_of_its_group() {
        let layout = default().select(Comment).detach(Comment);
        assert!(layout.is_valid());
        assert_eq!(tabs_of(&layout, Notation), (vec![Notation, Info], 1));
        assert_eq!(tabs_of(&layout, Comment), (vec![Comment], 0));
        assert_eq!(layout.detach(Comment), layout);
        assert_eq!(default().detach(Board), default());
    }

    #[test]
    fn select_changes_only_the_active_tab() {
        let layout = default().select(Comment);
        assert_eq!(tabs_of(&layout, Info), (vec![Notation, Info, Comment], 2));
        assert_eq!(default().select(Tree), default());
    }

    #[test]
    fn open_joins_the_notation_group_or_splits_beside_the_board() {
        let layout = default().close(Info);
        let reopened = layout.open(Info);
        assert_eq!(tabs_of(&reopened, Info), (vec![Notation, Comment, Info], 2));
        // Nothing of the family is open: beside the board.
        let alone = default().close(Info).close(Comment).close(Notation);
        let opened = alone.open(Comment);
        assert!(opened.is_valid());
        assert_eq!(opened.panes(), [Board, Comment]);
        assert!(matches!(
            opened,
            LayoutNode::Split { ratio, axis: SplitAxis::Horizontal, .. } if (ratio - 0.6).abs() < 1e-6
        ));
        // Already open: only selects.
        assert_eq!(default().open(Comment), default().select(Comment));
        // The board stays as it is.
        assert_eq!(alone.open(Board), alone);
    }

    #[test]
    fn open_wraps_the_other_panels_around_the_arrangement() {
        for kind in [GameList, Analysis, Tree, PaneKind::Graph] {
            let layout = default().open(kind);
            assert!(layout.is_valid(), "{kind:?}");
            assert_eq!(layout.panes().last(), Some(&kind));
            assert_eq!(layout.close(kind), default());
        }
    }

    #[test]
    fn operations_never_break_the_depth_limit() {
        let mut layout = default();
        for _ in 0..20 {
            layout = layout.open(Tree).open(GameList).open(Analysis);
            layout = layout.close(Tree).close(GameList).close(Analysis);
        }
        assert!(layout.is_valid());
        let mut deep = default();
        for kind in [Tree, GameList, Analysis, PaneKind::Graph] {
            deep = deep.open(kind);
            assert!(deep.is_valid());
        }
    }
}
