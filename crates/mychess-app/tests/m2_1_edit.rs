//! Spec m2-1, rules 55-61: variation operations, truncating, stripping,
//! with structure before/after and the cursor position.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m2_1_common;

use m2_1_common::*;
use mychess_app::{GameMessage, GameSession, Nav, Notice, VariationTarget};

fn node_count(s: &GameSession) -> usize {
    s.game().node_count()
}

/// Root with four children: e4 (main), d4, c4, Nf3.
fn four_openings() -> (GameSession, [NodeId; 4]) {
    let mut g = Game::new();
    let root = g.root();
    let e4 = add(&mut g, root, "e2e4");
    let d4 = add(&mut g, root, "d2d4");
    let c4 = add(&mut g, root, "c2c4");
    let nf3 = add(&mut g, root, "g1f3");
    (GameSession::new(g), [e4, d4, c4, nf3])
}

fn assert_no_history(s: &GameSession) {
    assert_eq!(s.undo_depth(), 0, "no undo point expected");
    assert!(!s.is_dirty());
}

// ---- PromoteVariation (rule 55) ----

#[test]
fn promote_moves_the_head_to_the_front_keeping_the_order_of_the_rest() {
    let (mut s, [e4, d4, c4, nf3]) = four_openings();
    let root = s.game().root();
    s.update(GameMessage::PromoteVariation(VariationTarget::Node(nf3)));
    assert_eq!(children(&s, root), vec![nf3, e4, d4, c4]);
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
    assert_eq!(s.cursor(), root, "cursor unchanged");
}

#[test]
fn promote_current_resolves_the_head_of_the_variation_the_cursor_is_in() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.c2);
    s.update(GameMessage::PromoteVariation(VariationTarget::Current));
    assert_eq!(children(&s, n.b1), vec![n.c1, n.b2]);
    assert_eq!(s.cursor(), n.c2, "cursor unchanged");
}

#[test]
fn promote_a_main_line_node_is_not_a_variation() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::PromoteVariation(VariationTarget::Node(n.m2)));
    assert_eq!(s.notice(), Some(&Notice::NotAVariation));
    assert_no_history(&s);
}

#[test]
fn promote_a_node_in_the_middle_of_a_variation_is_not_a_variation() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::PromoteVariation(VariationTarget::Node(n.a2)));
    assert_eq!(s.notice(), Some(&Notice::NotAVariation));
    assert_no_history(&s);
}

#[test]
fn promote_current_on_the_main_line_reports_not_in_variation() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m4);
    s.update(GameMessage::PromoteVariation(VariationTarget::Current));
    assert_eq!(s.notice(), Some(&Notice::NotInVariation));
    assert_no_history(&s);
}

#[test]
fn promote_stale_node_is_reported() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::DeleteVariation(VariationTarget::Node(n.a1)));
    s.update(GameMessage::PromoteVariation(VariationTarget::Node(n.a1)));
    assert_eq!(s.notice(), Some(&Notice::StaleNode));
}

// ---- MakeFirstVariation (rule 56) ----

#[test]
fn make_first_moves_the_variation_to_slot_one_without_touching_the_main_line() {
    let (mut s, [e4, d4, c4, nf3]) = four_openings();
    s.update(GameMessage::MakeFirstVariation(VariationTarget::Node(nf3)));
    assert_eq!(children(&s, s.game().root()), vec![e4, nf3, d4, c4]);
    assert_eq!(s.undo_depth(), 1);
}

#[test]
fn make_first_on_the_first_variation_changes_nothing() {
    let (mut s, [_, d4, _, _]) = four_openings();
    s.update(GameMessage::MakeFirstVariation(VariationTarget::Node(d4)));
    assert_no_history(&s);
}

#[test]
fn make_first_current_and_errors() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::MakeFirstVariation(VariationTarget::Current));
    assert_eq!(s.notice(), Some(&Notice::NotInVariation));
    s.update(GameMessage::MakeFirstVariation(VariationTarget::Node(n.m2)));
    assert_eq!(s.notice(), Some(&Notice::NotAVariation));
    assert_no_history(&s);
}

// ---- DeleteVariation (rule 57) ----

#[test]
fn delete_variation_removes_the_subtree_and_keeps_the_cursor_outside_it() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m4);
    s.update(GameMessage::DeleteVariation(VariationTarget::Node(n.a1)));
    assert_eq!(children(&s, n.m1), vec![n.m2]);
    for gone in [n.a1, n.a2, n.a3, n.b1, n.b2, n.c1, n.c2] {
        assert!(!s.game().contains(gone));
    }
    assert_eq!(s.cursor(), n.m4);
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
}

#[test]
fn delete_variation_with_cursor_inside_moves_to_the_parent_of_the_head() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.c2);
    s.update(GameMessage::DeleteVariation(VariationTarget::Node(n.a1)));
    assert_eq!(s.cursor(), n.m1);
}

#[test]
fn delete_inner_variation_with_cursor_inside_goes_to_its_parent() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.c2);
    s.update(GameMessage::DeleteVariation(VariationTarget::Node(n.b1)));
    assert_eq!(s.cursor(), n.a1);
    assert!(s.game().contains(n.a3), "sibling line a2/a3 survives");
}

#[test]
fn delete_current_variation_removes_the_one_the_cursor_is_in() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.c2);
    s.update(GameMessage::DeleteVariation(VariationTarget::Current));
    assert_eq!(children(&s, n.b1), vec![n.b2]);
    assert_eq!(s.cursor(), n.b1);
}

#[test]
fn delete_variation_errors_leave_everything_intact() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m3);
    s.update(GameMessage::DeleteVariation(VariationTarget::Node(n.m2)));
    assert_eq!(s.notice(), Some(&Notice::NotAVariation));
    s.update(GameMessage::DeleteVariation(VariationTarget::Current));
    assert_eq!(s.notice(), Some(&Notice::NotInVariation));
    assert_no_history(&s);
    assert_eq!(s.cursor(), n.m3);
}

// ---- TruncateEnd (rule 58) ----

#[test]
fn truncate_end_removes_everything_after_the_cursor_and_keeps_its_move() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m3);
    s.update(GameMessage::TruncateEnd);
    assert!(s.game().children(n.m3).unwrap().is_empty());
    assert!(s.game().contains(n.m3));
    for gone in [n.m4, n.m5, n.m6] {
        assert!(!s.game().contains(gone));
    }
    assert!(s.game().contains(n.a1), "variations before the cursor stay");
    assert_eq!(s.cursor(), n.m3);
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
}

#[test]
fn truncate_end_removes_the_variations_of_the_next_move_too() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    s.update(GameMessage::TruncateEnd);
    assert!(s.game().children(n.m1).unwrap().is_empty());
    assert_eq!(node_count(&s), 2, "root and e4");
}

#[test]
fn truncate_end_inside_a_variation() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.a1);
    s.update(GameMessage::TruncateEnd);
    assert_eq!(node_count(&s), nested_game().0.node_count() - 6);
    assert!(s.game().children(n.a1).unwrap().is_empty());
    assert!(s.game().contains(n.m6), "main line untouched");
}

#[test]
fn truncate_end_on_a_leaf_does_nothing() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m6);
    s.update(GameMessage::TruncateEnd);
    assert_no_history(&s);
}

#[test]
fn truncate_end_at_the_root_empties_the_game_and_undo_restores_it() {
    let (mut s, _) = nested_session();
    let original = s.game().clone();
    s.update(GameMessage::TruncateEnd);
    assert_eq!(node_count(&s), 1);
    s.update(GameMessage::Undo);
    assert!(s.game().same_content(&original));
}

// ---- TruncateFrom (resolved question 5) ----

#[test]
fn truncate_from_removes_this_move_and_the_rest_but_keeps_siblings() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m3);
    s.update(GameMessage::TruncateFrom);
    for gone in [n.m3, n.m4, n.m5, n.m6] {
        assert!(!s.game().contains(gone));
    }
    assert!(s.game().contains(n.m2));
    assert!(s.game().children(n.m2).unwrap().is_empty());
    assert!(s.game().contains(n.a3), "variations elsewhere stay");
    assert!(s.game().contains(s.cursor()));
    assert_eq!(s.cursor(), n.m2, "moves to the parent");
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
}

#[test]
fn truncate_from_a_variation_head_keeps_the_main_line() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.a1);
    s.update(GameMessage::TruncateFrom);
    assert_eq!(children(&s, n.m1), vec![n.m2]);
    assert_eq!(s.cursor(), n.m1);
}

#[test]
fn truncate_from_the_main_move_promotes_the_first_variation() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m2);
    s.update(GameMessage::TruncateFrom);
    assert_eq!(children(&s, n.m1), vec![n.a1]);
    assert_eq!(s.cursor(), n.m1);
}

#[test]
fn truncate_from_the_root_reports_no_move_here() {
    let (mut s, _) = nested_session();
    s.update(GameMessage::TruncateFrom);
    assert_eq!(s.notice(), Some(&Notice::NoMoveHere));
    assert_no_history(&s);
}

// ---- TruncateStart (rule 59) ----

#[test]
fn truncate_start_makes_the_cursor_position_the_start_of_a_new_game() {
    let (mut s, n) = nested_session();
    let fen_at_m3 = s.game().position_at(n.m3).unwrap().fen();
    goto(&mut s, n.m3);
    s.update(GameMessage::TruncateStart);
    assert_eq!(s.cursor(), s.game().root());
    assert_eq!(s.game().start_position().fen(), fen_at_m3);
    assert_eq!(s.game().mainline().count(), 3, "Nc6 Bb5 a6");
    assert!(!s.game().contains(n.m3), "new identity: old ids are stale");
    assert!(!s.game().contains(n.m4));
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
}

#[test]
fn truncate_start_keeps_the_variations_below_the_cursor() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    s.update(GameMessage::TruncateStart);
    // 14 nodes before (root + 13); the root is replaced by e4's position.
    assert_eq!(node_count(&s), 13);
    assert_eq!(s.game().children(s.game().root()).unwrap().len(), 2);
}

#[test]
fn truncate_start_keeps_tags_and_turns_the_comment_into_the_initial_comment() {
    let (mut g, n) = nested_game();
    g.tags_mut().white = "Alice".to_owned();
    let mut s = GameSession::new(g);
    s.update(GameMessage::SetComment {
        node: n.m3,
        text: "after Nf3".into(),
    });
    goto(&mut s, n.m3);
    s.update(GameMessage::TruncateStart);
    assert_eq!(s.game().tags().white, "Alice");
    assert_eq!(s.game().initial_comment(), Some("after Nf3"));
}

#[test]
fn truncate_start_inside_a_variation_uses_the_cursor_node() {
    let (mut s, n) = nested_session();
    let fen = s.game().position_at(n.c2).unwrap().fen();
    goto(&mut s, n.c2);
    s.update(GameMessage::TruncateStart);
    assert_eq!(s.game().start_position().fen(), fen);
    assert_eq!(node_count(&s), 1, "c2 was a leaf");
}

#[test]
fn truncate_start_at_the_root_does_nothing() {
    let (mut s, _) = nested_session();
    s.update(GameMessage::TruncateStart);
    assert_no_history(&s);
}

#[test]
fn truncate_start_is_undone_with_the_old_cursor_and_redone_at_the_new_root() {
    let (mut s, n) = nested_session();
    let original = s.game().clone();
    goto(&mut s, n.m3);
    s.update(GameMessage::TruncateStart);
    let truncated = s.game().clone();
    s.update(GameMessage::Undo);
    assert!(s.game().same_content(&original));
    assert_eq!(s.cursor(), n.m3, "cursor where it was before the edit");
    assert!(!s.is_dirty());
    s.update(GameMessage::Redo);
    assert!(s.game().same_content(&truncated));
    assert_eq!(s.cursor(), s.game().root());
    assert!(s.is_dirty());
}

#[test]
fn old_node_ids_are_stale_after_truncate_start() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m3);
    s.update(GameMessage::TruncateStart);
    nav_stale(&mut s, n.m4);
}

fn nav_stale(s: &mut GameSession, id: NodeId) {
    let cursor = s.cursor();
    s.update(GameMessage::Navigate(Nav::GoToNode(id)));
    assert_eq!(s.notice(), Some(&Notice::StaleNode));
    assert_eq!(s.cursor(), cursor);
}

// ---- Strip (rules 60-61) ----

fn annotated() -> (GameSession, Nested) {
    let (mut s, n) = nested_session();
    s.update(GameMessage::SetComment {
        node: s.game().root(),
        text: "initial".into(),
    });
    s.update(GameMessage::SetComment {
        node: n.m2,
        text: "after e5".into(),
    });
    s.update(GameMessage::SetCommentBefore {
        node: n.m2,
        text: "before e5".into(),
    });
    s.update(GameMessage::SetComment {
        node: n.c2,
        text: "deep".into(),
    });
    goto(&mut s, n.m2);
    s.update(GameMessage::AddNag(nag(1)));
    goto(&mut s, n.c2);
    s.update(GameMessage::AddNag(nag(14)));
    (s, n)
}

#[test]
fn strip_comments_removes_all_comments_but_keeps_nags() {
    let (mut s, n) = annotated();
    let depth = s.undo_depth();
    s.update(GameMessage::StripComments);
    assert_eq!(s.undo_depth(), depth + 1);
    assert_eq!(s.game().initial_comment(), None);
    for id in [n.m2, n.c2] {
        assert_eq!(s.game().comment(id).unwrap(), None);
        assert_eq!(s.game().comment_before(id).unwrap(), None);
    }
    assert_eq!(s.game().nags(n.m2).unwrap(), [nag(1)]);
    assert_eq!(s.game().nags(n.c2).unwrap(), [nag(14)]);
    assert_eq!(s.cursor(), n.c2);
}

#[test]
fn strip_nags_removes_all_nags_but_keeps_comments() {
    let (mut s, n) = annotated();
    s.update(GameMessage::StripNags);
    assert!(s.game().nags(n.m2).unwrap().is_empty());
    assert!(s.game().nags(n.c2).unwrap().is_empty());
    assert_eq!(s.game().comment(n.m2).unwrap(), Some("after e5"));
    assert_eq!(s.game().initial_comment(), Some("initial"));
}

#[test]
fn strip_variations_keeps_the_main_line_and_moves_the_cursor_to_the_nearest_live_ancestor() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.c2);
    s.update(GameMessage::StripVariations);
    assert_eq!(node_count(&s), 7, "root + 6 main-line moves");
    assert_eq!(
        s.cursor(),
        n.m1,
        "path root,m1,a1,... : m1 is the last live node"
    );
    assert!(s.game().contains(n.m6));
    for gone in [n.a1, n.b1, n.c1, n.c2] {
        assert!(!s.game().contains(gone));
    }
}

#[test]
fn strip_variations_with_the_cursor_on_the_main_line_keeps_it() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m5);
    s.update(GameMessage::StripVariations);
    assert_eq!(s.cursor(), n.m5);
}

#[test]
fn strip_variations_is_undone_with_the_cursor_inside_the_variation() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.c2);
    s.update(GameMessage::StripVariations);
    s.update(GameMessage::Undo);
    assert_eq!(s.cursor(), n.c2);
    assert_eq!(node_count(&s), nested_game().0.node_count());
}

#[test]
fn strip_operations_with_nothing_to_remove_make_no_undo_point() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5"]));
    s.update(GameMessage::StripComments);
    s.update(GameMessage::StripVariations);
    s.update(GameMessage::StripNags);
    assert_no_history(&s);
}

#[test]
fn deleting_nodes_through_undo_redo_never_leaves_a_dead_cursor() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.c2);
    s.update(GameMessage::StripVariations);
    s.update(GameMessage::Undo);
    s.update(GameMessage::Redo);
    assert!(s.game().contains(s.cursor()));
}
