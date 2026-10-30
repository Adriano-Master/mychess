//! M2-6 phase A: pure operations merge/detach/close/select/open
//! (D1003-D1007, D1012, D1015, D1016).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m2_6_layout_common;

use m2_6_layout_common::*;
use mychess_app::{LayoutNode, PaneKind, PaneKind::*, SplitAxis};
use proptest::prelude::*;

fn d() -> LayoutNode {
    default_layout()
}

/// `[ Board | [ {Notation,Info}(0) / {GameList,Tree}(1) ] ]`
fn two_groups() -> LayoutNode {
    h(
        0.6,
        pane(Board),
        v(0.3, grp(&[Notation, Info], 0), grp(&[GameList, Tree], 1)),
    )
}

fn assert_split(layout: &LayoutNode, axis: SplitAxis, ratio: f32) {
    let LayoutNode::Split {
        axis: got_axis,
        ratio: got_ratio,
        ..
    } = layout
    else {
        panic!("expected a split at the root: {layout:?}");
    };
    assert_eq!(*got_axis, axis);
    assert!((got_ratio - ratio).abs() < 1e-6, "ratio {got_ratio}");
}

// ---- merge ----

#[test]
fn merge_appends_to_the_end_and_activates_the_dragged_active_tab() {
    let merged = two_groups().merge(Tree, Notation);
    assert!(merged.is_valid());
    assert_eq!(
        merged,
        h(0.6, pane(Board), grp(&[Notation, Info, GameList, Tree], 3))
    );
}

#[test]
fn merge_activates_the_dragged_active_tab_not_the_last_one() {
    let layout = h(
        0.6,
        pane(Board),
        v(0.3, grp(&[Notation, Info], 1), grp(&[GameList, Tree], 0)),
    );
    let merged = layout.merge(GameList, Info);
    assert_eq!(
        merged,
        h(0.6, pane(Board), grp(&[Notation, Info, GameList, Tree], 2))
    );
}

#[test]
fn merge_of_a_lone_panel_makes_it_the_last_active_tab() {
    let layout = d().open(GameList);
    let merged = layout.merge(GameList, Comment);
    assert_eq!(
        tabs_of(&merged, Notation),
        (vec![Notation, Info, Comment, GameList], 3)
    );
    assert_eq!(merged.groups().len(), 2);
    assert!(merged.is_valid());
}

#[test]
fn merge_removes_the_source_group_and_the_sibling_takes_the_parent_place() {
    let layout = h(
        0.6,
        pane(Board),
        v(0.3, grp(&[Notation], 0), h(0.4, pane(GameList), pane(Tree))),
    );
    let merged = layout.merge(GameList, Notation);
    // The inner split is replaced by Tree; the other ratios stay.
    assert_eq!(
        merged,
        h(
            0.6,
            pane(Board),
            v(0.3, grp(&[Notation, GameList], 1), pane(Tree))
        )
    );
}

#[test]
fn merge_whose_source_is_a_child_of_the_root_promotes_the_sibling_to_root() {
    let layout = v(0.7, h(0.6, pane(Board), pane(Info)), pane(Tree));
    let merged = layout.merge(Tree, Info);
    assert_eq!(merged, h(0.6, pane(Board), grp(&[Info, Tree], 1)));
}

#[test]
fn merge_identifies_groups_by_any_member() {
    let a = two_groups().merge(GameList, Info);
    let b = two_groups().merge(Tree, Notation);
    assert_eq!(a, b);
}

#[test]
fn merge_without_effect_cases_return_the_same_layout() {
    let layout = two_groups();
    // Same group.
    assert_eq!(layout.merge(Notation, Info), layout);
    assert_eq!(layout.merge(Notation, Notation), layout);
    // The board as source or target.
    assert_eq!(layout.merge(Board, Notation), layout);
    assert_eq!(layout.merge(Notation, Board), layout);
    assert_eq!(layout.merge(Board, Board), layout);
    // Panels that are not open.
    assert_eq!(layout.merge(Analysis, Notation), layout);
    assert_eq!(layout.merge(Notation, Analysis), layout);
    assert_eq!(layout.merge(Analysis, Graph), layout);
}

// ---- detach ----

#[test]
fn detach_makes_a_split_to_the_right_with_ratio_half() {
    let detached = d().detach(Comment);
    assert!(detached.is_valid());
    assert_eq!(
        detached,
        h(
            0.6,
            pane(Board),
            h(0.5, grp(&[Notation, Info], 0), pane(Comment))
        )
    );
}

#[rstest::rstest]
// (active before, panel detached, tabs left, active left)
#[case(0, Notation, vec![Info, Comment], 0)]
#[case(1, Notation, vec![Info, Comment], 0)]
#[case(2, Notation, vec![Info, Comment], 1)]
#[case(1, Info, vec![Notation, Comment], 1)]
#[case(2, Info, vec![Notation, Comment], 1)]
#[case(2, Comment, vec![Notation, Info], 1)]
#[case(0, Comment, vec![Notation, Info], 0)]
fn detach_keeps_the_active_tab_or_moves_to_the_neighbour(
    #[case] active: usize,
    #[case] kind: PaneKind,
    #[case] left: Vec<PaneKind>,
    #[case] left_active: usize,
) {
    let layout = h(0.6, pane(Board), grp(&[Notation, Info, Comment], active));
    let detached = layout.detach(kind);
    assert!(detached.is_valid());
    assert_eq!(
        tabs_of(&detached, if left[0] == kind { left[1] } else { left[0] }),
        (left, left_active)
    );
    assert_eq!(tabs_of(&detached, kind), (vec![kind], 0));
}

#[test]
fn detach_keeps_the_same_tab_active_when_another_leaves() {
    // Active Info (1); Notation, before it, leaves: Info stays selected.
    let layout = h(0.6, pane(Board), grp(&[Notation, Info, Comment], 1));
    let detached = layout.detach(Notation);
    let (tabs, active) = tabs_of(&detached, Info);
    assert_eq!(tabs[active], Info);
}

#[test]
fn detach_in_a_nested_group_keeps_the_other_ratios() {
    let detached = two_groups().detach(Info);
    assert_eq!(
        detached,
        h(
            0.6,
            pane(Board),
            v(
                0.3,
                h(0.5, pane(Notation), pane(Info)),
                grp(&[GameList, Tree], 1)
            )
        )
    );
}

#[test]
fn detach_without_effect_cases() {
    let layout = d().detach(Comment);
    assert_eq!(layout.detach(Comment), layout, "group of one tab");
    assert_eq!(d().detach(Board), d());
    assert_eq!(d().detach(Tree), d(), "not open");
}

// ---- close ----

#[test]
fn close_keeps_the_same_tab_active() {
    let layout = h(0.6, pane(Board), grp(&[Notation, Info, Comment], 1));
    assert_eq!(
        layout.close(Comment),
        h(0.6, pane(Board), grp(&[Notation, Info], 1))
    );
    assert_eq!(
        layout.close(Notation),
        h(0.6, pane(Board), grp(&[Info, Comment], 0))
    );
}

#[test]
fn close_of_the_active_tab_selects_the_neighbour_at_the_same_index() {
    let at = |active| h(0.6, pane(Board), grp(&[Notation, Info, Comment], active));
    // Active Notation closes: Info takes index 0.
    assert_eq!(
        at(0).close(Notation),
        h(0.6, pane(Board), grp(&[Info, Comment], 0))
    );
    // Active Info closes: Comment takes index 1.
    assert_eq!(
        at(1).close(Info),
        h(0.6, pane(Board), grp(&[Notation, Comment], 1))
    );
    // Active last tab closes: limited to the end.
    assert_eq!(
        at(2).close(Comment),
        h(0.6, pane(Board), grp(&[Notation, Info], 1))
    );
}

#[test]
fn close_of_the_last_tab_removes_the_group_and_the_sibling_takes_its_place() {
    let layout = two_groups();
    let closed = layout.close(GameList).close(Tree);
    assert_eq!(closed, h(0.6, pane(Board), grp(&[Notation, Info], 0)));
    let nested = h(
        0.6,
        pane(Board),
        v(0.3, grp(&[Notation], 0), h(0.4, pane(GameList), pane(Tree))),
    );
    assert_eq!(
        nested.close(GameList),
        h(0.6, pane(Board), v(0.3, pane(Notation), pane(Tree)))
    );
}

#[test]
fn close_of_a_lone_panel_beside_the_board_leaves_the_board_group() {
    let layout = h(0.6, pane(Board), pane(Info));
    assert_eq!(layout.close(Info), pane(Board));
}

#[test]
fn closing_everything_but_the_board_gives_a_lone_board() {
    let mut layout = d().open(GameList).open(Analysis).open(Graph).open(Tree);
    for kind in ALL.into_iter().filter(|k| *k != Board) {
        layout = layout.close(kind);
        assert!(layout.is_valid());
    }
    assert_eq!(layout, pane(Board));
}

#[test]
fn close_undoes_open_of_a_wrapping_panel() {
    for kind in [GameList, Analysis, Graph, Tree] {
        assert_eq!(d().open(kind).close(kind), d(), "{kind:?}");
    }
}

#[test]
fn board_cannot_be_closed_and_unknown_panels_are_ignored() {
    assert_eq!(d().close(Board), d());
    assert_eq!(pane(Board).close(Board), pane(Board));
    assert_eq!(d().close(Tree), d());
}

// ---- select ----

#[test]
fn select_changes_only_the_active_index_of_its_group() {
    let layout = two_groups().select(Info);
    assert_eq!(
        layout,
        h(
            0.6,
            pane(Board),
            v(0.3, grp(&[Notation, Info], 1), grp(&[GameList, Tree], 1))
        )
    );
    assert_eq!(
        two_groups().select(GameList).visible_panes(),
        [Board, Notation, GameList]
    );
}

#[test]
fn select_is_idempotent_and_ignores_unknown_panels() {
    let once = d().select(Comment);
    assert_eq!(once.select(Comment), once);
    assert_eq!(d().select(Tree), d());
    assert_eq!(d().select(Board), d());
    assert_eq!(d().select(Notation), d());
}

// ---- open ----

#[test]
fn open_of_an_open_panel_only_selects_it() {
    for kind in [Notation, Info, Comment] {
        assert_eq!(d().open(kind), d().select(kind));
    }
    let layout = d().open(Tree);
    assert_eq!(layout.open(Tree), layout);
    assert_eq!(d().open(Board), d());
}

#[test]
fn reopened_family_member_becomes_the_last_tab_and_is_selected() {
    for kind in [Notation, Info, Comment] {
        let layout = d().close(kind);
        let reopened = layout.open(kind);
        let (tabs, active) = tabs_of(&reopened, kind);
        assert_eq!(tabs.len(), 3, "{kind:?}");
        assert_eq!(tabs.last(), Some(&kind));
        assert_eq!(active, 2);
        assert!(reopened.is_valid());
    }
}

#[test]
fn open_family_member_uses_the_first_group_in_layout_order() {
    // Two groups hold a family member: the left one wins.
    let layout = h(
        0.6,
        pane(Board),
        v(0.5, grp(&[Info, GameList], 0), pane(Comment)),
    );
    let opened = layout.open(Notation);
    assert_eq!(tabs_of(&opened, Info), (vec![Info, GameList, Notation], 2));
    assert_eq!(tabs_of(&opened, Comment), (vec![Comment], 0));
    // Order matters, not kind: now the comment group comes first.
    let layout = h(
        0.6,
        pane(Board),
        v(0.5, pane(Comment), grp(&[Info, GameList], 0)),
    );
    let opened = layout.open(Notation);
    assert_eq!(tabs_of(&opened, Comment), (vec![Comment, Notation], 1));
}

#[test]
fn open_family_member_ignores_groups_without_family_members() {
    let layout = h(
        0.6,
        pane(Board),
        v(0.5, grp(&[GameList, Tree], 0), pane(Info)),
    );
    let opened = layout.open(Comment);
    assert_eq!(tabs_of(&opened, Info), (vec![Info, Comment], 1));
    assert_eq!(tabs_of(&opened, GameList), (vec![GameList, Tree], 0));
}

#[test]
fn open_family_member_without_anchor_splits_to_the_right_of_the_board() {
    for kind in [Notation, Info, Comment] {
        let opened = pane(Board).open(kind);
        assert_eq!(opened, h(0.6, pane(Board), pane(kind)), "{kind:?}");
    }
}

#[test]
fn open_family_member_without_anchor_splits_the_board_leaf_inside_a_bigger_layout() {
    let layout = v(0.58, pane(Board), pane(GameList));
    let opened = layout.open(Info);
    assert_eq!(
        opened,
        v(0.58, h(0.6, pane(Board), pane(Info)), pane(GameList))
    );
    assert!(opened.is_valid());
}

#[rstest::rstest]
#[case(GameList, SplitAxis::Vertical, 0.58)]
#[case(Analysis, SplitAxis::Horizontal, 0.64)]
#[case(Graph, SplitAxis::Vertical, 0.70)]
#[case(Tree, SplitAxis::Horizontal, 0.56)]
fn open_wraps_the_whole_layout_with_the_historical_axis_and_ratio(
    #[case] kind: PaneKind,
    #[case] axis: SplitAxis,
    #[case] ratio: f32,
) {
    let base = d();
    let opened = base.open(kind);
    assert!(opened.is_valid());
    assert_split(&opened, axis, ratio);
    let (tabs, active) = tabs_of(&opened, kind);
    assert_eq!((tabs, active), (vec![kind], 0));
    // The previous arrangement is intact on the other side.
    assert_eq!(opened.close(kind), base);
}

#[test]
fn open_never_exceeds_the_depth_limit_even_with_the_longest_chain() {
    let mut chain = pane(Board);
    for kind in [Info, Notation, Comment, GameList, Analysis, Graph] {
        chain = v(0.5, chain, pane(kind));
    }
    assert!(chain.is_valid());
    let opened = chain.open(Tree);
    assert!(opened.is_valid());
    assert!(opened.contains(Tree));
}

#[test]
fn operations_on_a_too_deep_input_return_it_unchanged_without_panicking() {
    let mut deep = pane(Board);
    for _ in 0..20 {
        deep = v(0.5, deep, pane(Info));
    }
    assert!(!deep.is_valid());
    assert_eq!(deep.open(Tree), deep);
    assert_eq!(deep.open(GameList), deep);
    assert_eq!(deep.close(Tree), deep);
    assert_eq!(deep.select(Info), deep);
    assert_eq!(deep.merge(Info, Tree), deep);
    assert_eq!(deep.detach(Info), deep);
}

// ---- properties ----

#[derive(Debug, Clone)]
enum Op {
    Open(usize),
    Close(usize),
    Merge(usize, usize),
    Detach(usize),
    Select(usize),
}

fn op() -> impl Strategy<Value = Op> {
    let k = 0..ALL.len();
    prop_oneof![
        k.clone().prop_map(Op::Open),
        k.clone().prop_map(Op::Close),
        (k.clone(), k.clone()).prop_map(|(a, b)| Op::Merge(a, b)),
        k.clone().prop_map(Op::Detach),
        k.prop_map(Op::Select),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn random_operation_sequences_keep_the_layout_valid_and_panels_unique(
        ops in proptest::collection::vec(op(), 0..60)
    ) {
        let mut layout = d();
        for op in ops {
            let before = layout.clone();
            layout = match op {
                Op::Open(i) => {
                    let next = before.open(ALL[i]);
                    if ALL[i] != Board {
                        prop_assert!(next.contains(ALL[i]));
                        let (tabs, active) = next.group_of(ALL[i]).unwrap();
                        prop_assert_eq!(tabs[active], ALL[i]);
                    }
                    next
                }
                Op::Close(i) => {
                    let next = before.close(ALL[i]);
                    if ALL[i] != Board {
                        prop_assert!(!next.contains(ALL[i]));
                    }
                    next
                }
                Op::Merge(i, j) => {
                    let next = before.merge(ALL[i], ALL[j]);
                    prop_assert!(same_panes(&before, &next));
                    next
                }
                Op::Detach(i) => {
                    let next = before.detach(ALL[i]);
                    prop_assert!(same_panes(&before, &next));
                    next
                }
                Op::Select(i) => {
                    let next = before.select(ALL[i]);
                    prop_assert!(same_panes(&before, &next));
                    if let Some((tabs, active)) = next.group_of(ALL[i]) {
                        prop_assert_eq!(tabs[active], ALL[i]);
                    }
                    next
                }
            };
            prop_assert!(layout.is_valid(), "{:?}", layout);
            let panes = layout.panes();
            let mut sorted: Vec<String> = panes.iter().map(|k| format!("{k:?}")).collect();
            sorted.sort();
            sorted.dedup();
            prop_assert_eq!(sorted.len(), panes.len());
            prop_assert!(layout.contains(Board));
            prop_assert_eq!(layout.visible_panes().len(), layout.groups().len());
        }
    }

    #[test]
    fn merge_and_detach_never_change_the_set_of_open_panels(
        opens in proptest::collection::vec(0..ALL.len(), 0..8),
        a in 0..ALL.len(),
        b in 0..ALL.len(),
    ) {
        let mut layout = d();
        for i in opens {
            layout = layout.open(ALL[i]);
        }
        prop_assert!(same_panes(&layout, &layout.merge(ALL[a], ALL[b])));
        prop_assert!(same_panes(&layout, &layout.detach(ALL[a])));
    }
}
