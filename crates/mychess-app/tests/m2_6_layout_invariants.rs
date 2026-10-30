//! M2-6 phase A: invariants of `LayoutNode::is_valid` (D1000), the default
//! layout (D1001) and the legacy default.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m2_6_layout_common;

use m2_6_layout_common::*;
use mychess_app::{LayoutNode, MAX_LAYOUT_DEPTH, PaneKind::*};

fn beside_board(group: LayoutNode) -> LayoutNode {
    h(0.5, pane(Board), group)
}

#[test]
fn default_is_board_beside_a_notation_group_with_ratio_060() {
    let d = LayoutNode::default_layout();
    assert!(d.is_valid());
    let LayoutNode::Split { ratio, a, b, .. } = &d else {
        panic!("default must be a split");
    };
    assert!((ratio - 0.6).abs() < 1e-6);
    assert_eq!(**a, pane(Board));
    assert_eq!(**b, grp(&[Notation, Info, Comment], 0));
}

#[test]
fn legacy_default_is_valid_and_has_no_comment() {
    let legacy = LayoutNode::legacy_default_layout();
    assert!(legacy.is_valid());
    assert!(!legacy.contains(Comment));
    assert_ne!(legacy, LayoutNode::default_layout());
}

#[test]
fn constructors_build_groups() {
    assert_eq!(
        pane(Info),
        LayoutNode::Group {
            tabs: vec![Info],
            active: 0
        }
    );
    assert_eq!(
        LayoutNode::group(vec![Info, Comment], 1),
        LayoutNode::Group {
            tabs: vec![Info, Comment],
            active: 1
        }
    );
}

#[test]
fn a_lone_board_is_valid() {
    assert!(pane(Board).is_valid());
}

#[test]
fn rejects_a_layout_without_the_board() {
    assert!(!pane(Info).is_valid());
    assert!(!grp(&[Info, Comment], 0).is_valid());
}

#[test]
fn rejects_every_panel_repeated_in_two_groups() {
    for kind in ALL.into_iter().filter(|k| *k != Board) {
        let layout = h(0.5, pane(Board), h(0.5, pane(kind), pane(kind)));
        assert!(!layout.is_valid(), "{kind:?} twice in two groups");
    }
    assert!(!h(0.5, pane(Board), pane(Board)).is_valid());
}

#[test]
fn rejects_a_panel_repeated_inside_one_group() {
    assert!(!beside_board(grp(&[Info, Info], 0)).is_valid());
    assert!(!beside_board(grp(&[Info, Comment, Info], 0)).is_valid());
}

#[test]
fn rejects_an_empty_group() {
    assert!(!beside_board(grp(&[], 0)).is_valid());
}

#[test]
fn rejects_an_active_index_out_of_range() {
    assert!(beside_board(grp(&[Info, Comment], 1)).is_valid());
    assert!(!beside_board(grp(&[Info, Comment], 2)).is_valid());
    assert!(!beside_board(grp(&[Info], 1)).is_valid());
    assert!(!beside_board(grp(&[Info], usize::MAX)).is_valid());
}

#[test]
fn rejects_a_board_that_shares_its_group() {
    assert!(!grp(&[Board, Info], 0).is_valid());
    assert!(!grp(&[Info, Board], 1).is_valid());
    assert!(!h(0.5, grp(&[Board, Comment], 0), pane(Tree)).is_valid());
}

#[test]
fn ratio_limits_are_inclusive_and_nan_is_rejected() {
    for ratio in [0.1_f32, 0.5, 0.9] {
        assert!(h(ratio, pane(Board), pane(Info)).is_valid(), "{ratio}");
    }
    for ratio in [0.0_f32, 0.05, 0.95, 1.0, -0.5, f32::NAN, f32::INFINITY] {
        assert!(!h(ratio, pane(Board), pane(Info)).is_valid(), "{ratio}");
    }
}

#[test]
fn rejects_a_tree_deeper_than_the_limit() {
    // Repeated panels make it invalid anyway; the depth limit is checked
    // by the nesting being refused no matter the leaves.
    let mut deep = pane(Board);
    for _ in 0..=MAX_LAYOUT_DEPTH {
        deep = v(0.5, deep, pane(Info));
    }
    assert!(!deep.is_valid());
}

#[test]
fn the_deepest_chain_the_panels_allow_is_valid() {
    // Eight distinct panels: seven nested splits, still within the limit.
    let mut chain = pane(Board);
    for kind in [Info, Notation, Comment, GameList, Analysis, Graph, Tree] {
        chain = v(0.5, chain, pane(kind));
    }
    assert!(chain.is_valid());
}

#[test]
fn queries_describe_groups_in_layout_order() {
    let layout = h(
        0.6,
        pane(Board),
        v(0.5, grp(&[Notation, Info], 1), grp(&[Tree, Graph], 0)),
    );
    assert!(layout.is_valid());
    assert_eq!(layout.panes(), [Board, Notation, Info, Tree, Graph]);
    assert_eq!(layout.visible_panes(), [Board, Info, Tree]);
    let groups = layout.groups();
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[1], (&[Notation, Info][..], 1));
    assert_eq!(layout.group_of(Graph), Some((&[Tree, Graph][..], 0)));
    assert_eq!(layout.group_of(Comment), None);
    assert!(layout.contains(Graph));
    assert!(!layout.contains(Comment));
    // "Open" is being a tab, "visible" is being the active one.
    assert!(layout.contains(Notation));
    assert!(!layout.visible_panes().contains(&Notation));
}
