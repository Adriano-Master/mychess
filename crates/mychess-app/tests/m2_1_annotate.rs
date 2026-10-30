//! Spec m2-1, rules 62-68: comments, NAGs, categories, shortcut table and
//! the NAG field parser.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m2_1_common;

use std::collections::BTreeSet;

use m2_1_common::*;
use mychess_app::{
    EditSettings, GameMessage, GameSession, NAG_SHORTCUTS, NagCategory, Notice, nag_category,
    parse_nag_list,
};
use rstest::rstest;

fn session() -> (GameSession, Nested) {
    nested_session()
}

fn nags_at(s: &GameSession, node: NodeId) -> Vec<u8> {
    s.game()
        .nags(node)
        .unwrap()
        .iter()
        .map(|n| n.value())
        .collect()
}

fn exclusive(on: bool) -> GameMessage {
    GameMessage::ApplySettings(EditSettings {
        exclusive_nags: on,
        ..EditSettings::default()
    })
}

fn comment_msg(node: NodeId, text: &str) -> GameMessage {
    GameMessage::SetComment {
        node,
        text: text.to_owned(),
    }
}

// ---- comments (rules 62-63, 68) ----

#[test]
fn set_comment_targets_the_given_node_not_the_cursor() {
    let (mut s, n) = session();
    s.update(comment_msg(n.m3, "good move"));
    assert_eq!(s.game().comment(n.m3).unwrap(), Some("good move"));
    assert_eq!(s.cursor(), s.game().root(), "cursor untouched");
    assert_eq!((s.undo_depth(), s.is_dirty()), (1, true));
}

#[test]
fn comment_is_stored_literally_including_embedded_commands() {
    let (mut s, n) = session();
    let text = "  [%clk 0:01:00] [%eval 0.3] {brace} é ♞ \n second line  ";
    s.update(comment_msg(n.m3, text));
    assert_eq!(s.game().comment(n.m3).unwrap(), Some(text));
}

#[test]
fn same_comment_makes_no_undo_point() {
    let (mut s, n) = session();
    s.update(comment_msg(n.m3, "x"));
    s.update(GameMessage::Undo);
    s.update(GameMessage::Redo);
    let revision = s.revision();
    let depth = s.undo_depth();
    s.update(comment_msg(n.m3, "x"));
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(s.revision(), revision);
}

#[rstest]
#[case("")]
#[case("   ")]
#[case("\t\n ")]
fn blank_comment_removes_the_existing_one(#[case] blank: &str) {
    let (mut s, n) = session();
    s.update(comment_msg(n.m3, "x"));
    s.update(comment_msg(n.m3, blank));
    assert_eq!(s.game().comment(n.m3).unwrap(), None);
    assert_eq!(s.undo_depth(), 2);
}

#[test]
fn blank_comment_where_there_is_none_changes_nothing() {
    let (mut s, n) = session();
    s.update(comment_msg(n.m3, "  "));
    assert_eq!((s.undo_depth(), s.is_dirty()), (0, false));
}

#[test]
fn comment_on_the_root_is_the_initial_comment() {
    let (mut s, _) = session();
    let root = s.game().root();
    s.update(comment_msg(root, "intro"));
    assert_eq!(s.game().initial_comment(), Some("intro"));
    assert!(s.is_dirty());
}

#[test]
fn comment_before_is_set_and_blank_removes_it() {
    let (mut s, n) = session();
    s.update(GameMessage::SetCommentBefore {
        node: n.m3,
        text: "pre".into(),
    });
    assert_eq!(s.game().comment_before(n.m3).unwrap(), Some("pre"));
    s.update(GameMessage::SetCommentBefore {
        node: n.m3,
        text: " ".into(),
    });
    assert_eq!(s.game().comment_before(n.m3).unwrap(), None);
}

#[test]
fn comment_before_on_the_root_reports_no_move_here() {
    let (mut s, _) = session();
    let root = s.game().root();
    s.update(GameMessage::SetCommentBefore {
        node: root,
        text: "x".into(),
    });
    assert_eq!(s.notice(), Some(&Notice::NoMoveHere));
    assert_eq!((s.undo_depth(), s.is_dirty()), (0, false));
}

#[test]
fn comments_with_stale_ids_are_reported() {
    let (mut s, n) = session();
    s.update(GameMessage::DeleteVariation(
        mychess_app::VariationTarget::Node(n.a1),
    ));
    let depth = s.undo_depth();
    s.update(comment_msg(n.a2, "x"));
    assert_eq!(s.notice(), Some(&Notice::StaleNode));
    s.update(GameMessage::SetCommentBefore {
        node: n.a2,
        text: "x".into(),
    });
    assert_eq!(s.notice(), Some(&Notice::StaleNode));
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn huge_and_odd_comments_do_not_panic() {
    let (mut s, n) = session();
    s.update(comment_msg(n.m3, &"é".repeat(512 * 1024)));
    s.update(comment_msg(n.m3, "\0\u{202e}\u{feff}}{"));
    s.update(GameMessage::SetCommentBefore {
        node: n.m3,
        text: "x".repeat(1 << 20),
    });
    assert!(s.game().contains(s.cursor()));
}

// ---- AddNag (rule 64) ----

#[test]
fn add_nag_on_the_root_reports_no_move_here() {
    let (mut s, _) = session();
    s.update(GameMessage::AddNag(nag(1)));
    assert_eq!(s.notice(), Some(&Notice::NoMoveHere));
    assert_eq!((s.undo_depth(), s.is_dirty()), (0, false));
}

#[test]
fn add_nag_goes_to_the_cursor_and_ignores_duplicates() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    s.update(GameMessage::AddNag(nag(1)));
    assert_eq!(nags_at(&s, n.m3), [1]);
    let depth = s.undo_depth();
    s.update(GameMessage::AddNag(nag(1)));
    assert_eq!(nags_at(&s, n.m3), [1]);
    assert_eq!(s.undo_depth(), depth, "already present: nothing changes");
}

#[test]
fn exclusive_move_quality_replaces_the_previous_one() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    s.update(GameMessage::AddNag(nag(1)));
    s.update(GameMessage::AddNag(nag(3)));
    assert_eq!(nags_at(&s, n.m3), [3]);
    s.update(GameMessage::AddNag(nag(6)));
    assert_eq!(nags_at(&s, n.m3), [6]);
}

#[test]
fn exclusive_evaluation_replaces_the_previous_one() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    s.update(GameMessage::AddNag(nag(14)));
    s.update(GameMessage::AddNag(nag(21)));
    assert_eq!(nags_at(&s, n.m3), [21]);
    s.update(GameMessage::AddNag(nag(10)));
    assert_eq!(nags_at(&s, n.m3), [10]);
}

#[test]
fn exclusivity_is_per_category_and_other_nags_are_independent() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    for v in [7, 1, 14, 8, 22, 140] {
        s.update(GameMessage::AddNag(nag(v)));
    }
    assert_eq!(nags_at(&s, n.m3), [7, 1, 14, 8, 22, 140]);
    s.update(GameMessage::AddNag(nag(2))); // drops $1, appended at the end
    assert_eq!(nags_at(&s, n.m3), [7, 14, 8, 22, 140, 2]);
}

#[test]
fn exclusivity_off_accumulates_nags_of_the_same_category() {
    let (mut s, n) = session();
    s.update(exclusive(false));
    goto(&mut s, n.m3);
    s.update(GameMessage::AddNag(nag(1)));
    s.update(GameMessage::AddNag(nag(3)));
    s.update(GameMessage::AddNag(nag(14)));
    s.update(GameMessage::AddNag(nag(16)));
    assert_eq!(nags_at(&s, n.m3), [1, 3, 14, 16]);
}

#[test]
fn there_is_no_limit_on_the_number_of_nags() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    for v in 100..=140u16 {
        s.update(GameMessage::AddNag(nag(v)));
    }
    assert_eq!(nags_at(&s, n.m3).len(), 41, "the Scid stops at 7");
}

#[test]
fn nags_belong_to_the_cursor_move_not_to_the_next_one() {
    let (mut s, n) = session();
    goto(&mut s, n.m2);
    s.update(GameMessage::AddNag(nag(1)));
    assert_eq!(nags_at(&s, n.m2), [1]);
    assert!(nags_at(&s, n.m3).is_empty());
    assert!(nags_at(&s, n.a1).is_empty());
}

// ---- Toggle / Remove / Clear / Set (rule 65-66) ----

#[test]
fn toggle_nag_adds_then_removes() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    s.update(GameMessage::ToggleNag(nag(2)));
    assert_eq!(nags_at(&s, n.m3), [2]);
    s.update(GameMessage::ToggleNag(nag(2)));
    assert!(nags_at(&s, n.m3).is_empty());
    assert_eq!(s.undo_depth(), 2);
}

#[test]
fn toggle_nag_applies_exclusivity_when_adding() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    s.update(GameMessage::ToggleNag(nag(1)));
    s.update(GameMessage::ToggleNag(nag(4)));
    assert_eq!(nags_at(&s, n.m3), [4]);
}

#[test]
fn toggle_on_root_reports_no_move_here() {
    let (mut s, _) = session();
    s.update(GameMessage::ToggleNag(nag(1)));
    assert_eq!(s.notice(), Some(&Notice::NoMoveHere));
}

#[test]
fn remove_nag_absent_changes_nothing_and_present_removes() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    s.update(GameMessage::RemoveNag(nag(1)));
    assert_eq!(s.undo_depth(), 0);
    s.update(GameMessage::AddNag(nag(1)));
    s.update(GameMessage::AddNag(nag(7)));
    s.update(GameMessage::RemoveNag(nag(1)));
    assert_eq!(nags_at(&s, n.m3), [7]);
}

#[test]
fn clear_nags_empties_and_is_idempotent() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    s.update(GameMessage::ClearNags);
    assert_eq!(s.undo_depth(), 0);
    s.update(GameMessage::AddNag(nag(1)));
    s.update(GameMessage::AddNag(nag(7)));
    s.update(GameMessage::ClearNags);
    assert!(nags_at(&s, n.m3).is_empty());
    let depth = s.undo_depth();
    s.update(GameMessage::ClearNags);
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn set_nags_replaces_the_list_without_duplicates_keeping_order_and_ignoring_exclusivity() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    s.update(GameMessage::AddNag(nag(2)));
    s.update(GameMessage::SetNags(vec![
        nag(16),
        nag(1),
        nag(3),
        nag(16),
        nag(14),
        nag(1),
    ]));
    assert_eq!(nags_at(&s, n.m3), [16, 1, 3, 14]);
}

#[test]
fn set_nags_with_the_same_list_makes_no_undo_point() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    s.update(GameMessage::SetNags(vec![nag(1), nag(7)]));
    let depth = s.undo_depth();
    s.update(GameMessage::SetNags(vec![nag(1), nag(7)]));
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn set_nags_empty_on_a_move_without_nags_changes_nothing() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    s.update(GameMessage::SetNags(vec![]));
    assert_eq!((s.undo_depth(), s.is_dirty()), (0, false));
}

#[test]
fn nag_edits_are_undone() {
    let (mut s, n) = session();
    goto(&mut s, n.m3);
    s.update(GameMessage::AddNag(nag(1)));
    s.update(GameMessage::AddNag(nag(3)));
    s.update(GameMessage::Undo);
    assert_eq!(nags_at(&s, n.m3), [1]);
    s.update(GameMessage::Undo);
    assert!(nags_at(&s, n.m3).is_empty());
    assert!(!s.is_dirty());
}

// ---- categories (rule 64) ----

#[rstest]
#[case(1, NagCategory::MoveQuality)]
#[case(3, NagCategory::MoveQuality)]
#[case(6, NagCategory::MoveQuality)]
#[case(7, NagCategory::Other)]
#[case(8, NagCategory::Other)]
#[case(9, NagCategory::Other)]
#[case(10, NagCategory::Evaluation)]
#[case(16, NagCategory::Evaluation)]
#[case(21, NagCategory::Evaluation)]
#[case(22, NagCategory::Other)]
#[case(44, NagCategory::Other)]
#[case(255, NagCategory::Other)]
fn nag_category_boundaries(#[case] value: u16, #[case] expected: NagCategory) {
    assert_eq!(nag_category(nag(value)), expected);
}

// ---- shortcuts (rule 67) ----

#[test]
fn shortcut_table_matches_the_scid_list_and_parses_back() {
    let expected: BTreeSet<(&str, u8)> = [
        ("!", 1),
        ("!!", 3),
        ("?", 2),
        ("??", 4),
        ("!?", 5),
        ("?!", 6),
        ("=", 10),
        ("~", 13),
        ("~=", 44),
        ("+=", 14),
        ("=+", 15),
        ("+/-", 16),
        ("-/+", 17),
        ("+-", 18),
        ("-+", 19),
    ]
    .into_iter()
    .collect();
    let actual: BTreeSet<(&str, u8)> = NAG_SHORTCUTS.iter().copied().collect();
    assert_eq!(actual, expected);
    assert_eq!(NAG_SHORTCUTS.len(), 15, "no duplicates");
    for (symbol, number) in NAG_SHORTCUTS {
        assert_eq!(
            Nag::parse(symbol),
            Some(nag(u16::from(*number))),
            "{symbol}"
        );
    }
}

// ---- parse_nag_list (rule 66) ----

#[test]
fn parse_nag_list_spec_example() {
    assert_eq!(
        parse_nag_list("! $14 xx +-"),
        vec![nag(1), nag(14), nag(18)]
    );
}

#[rstest]
#[case("", &[])]
#[case("   \t\n", &[])]
#[case("!?", &[5])]
#[case("$1 $2 $3", &[1, 2, 3])]
#[case("14", &[14])]
#[case("garbage more-garbage", &[])]
#[case("$0 0 $256 999 -5 $", &[])]
#[case("  !!   ??  ", &[3, 4])]
fn parse_nag_list_cases(#[case] text: &str, #[case] expected: &[u16]) {
    let want: Vec<Nag> = expected.iter().map(|v| nag(*v)).collect();
    assert_eq!(parse_nag_list(text), want);
}

#[test]
fn parse_nag_list_survives_hostile_input() {
    let big = "!? ".repeat(400_000);
    let parsed = parse_nag_list(&big);
    assert!(
        parsed.len() <= 1,
        "duplicates collapsed or kept, but no panic"
    );
    drop(parse_nag_list("é♞\0\u{202e} $\u{0661} xyzzy"));
}
