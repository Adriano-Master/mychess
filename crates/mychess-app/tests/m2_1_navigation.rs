//! Spec m2-1, rules 8-20: cursor navigation over a game with three levels of
//! nested variations.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m2_1_common;

use m2_1_common::*;
use mychess_app::{EditSettings, GameMessage, GameSession, Nav, Notice, VariationTarget};
use rstest::rstest;

fn at(node: impl Fn(&Nested) -> NodeId, nav_msg: Nav) -> (GameSession, Nested) {
    let (mut s, n) = nested_session();
    goto(&mut s, node(&n));
    nav(&mut s, nav_msg);
    (s, n)
}

// ---- Back (rule 9, D2-73) ----

#[test]
fn back_zero_does_nothing() {
    let (s, n) = at(|n| n.m3, Nav::Back(0));
    assert_eq!(s.cursor(), n.m3);
}

#[test]
fn back_on_main_line_walks_to_root_and_stops() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m6);
    nav(&mut s, Nav::Back(2));
    assert_eq!(s.cursor(), n.m4);
    nav(&mut s, Nav::Back(100));
    assert_eq!(s.cursor(), s.game().root());
    nav(&mut s, Nav::Back(1));
    assert_eq!(s.cursor(), s.game().root(), "Back at the root is a no-op");
}

#[test]
fn back_from_first_move_of_a_variation_goes_straight_to_the_parent() {
    let (s, n) = at(|n| n.a1, Nav::Back(1));
    assert_eq!(s.cursor(), n.m1);
}

#[rstest]
#[case::inside_variation_c(|n: &Nested| n.c2, |n: &Nested| n.b1)]
#[case::inside_variation_b(|n: &Nested| n.b2, |n: &Nested| n.a1)]
#[case::inside_variation_a(|n: &Nested| n.a3, |n: &Nested| n.m1)]
fn back_with_large_count_stops_at_the_position_the_variation_starts_from(
    #[case] from: fn(&Nested) -> NodeId,
    #[case] expected: fn(&Nested) -> NodeId,
) {
    let (s, n) = at(from, Nav::Back(50));
    assert_eq!(s.cursor(), expected(&n));
}

#[test]
fn second_back_after_stopping_continues_to_the_root() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.a3);
    nav(&mut s, Nav::Back(50));
    assert_eq!(s.cursor(), n.m1);
    nav(&mut s, Nav::Back(50));
    assert_eq!(s.cursor(), s.game().root());
}

// ---- Forward (rule 10) ----

#[test]
fn forward_follows_first_child_and_stops_at_leaf() {
    let (mut s, n) = nested_session();
    nav(&mut s, Nav::Forward(2));
    assert_eq!(s.cursor(), n.m2, "never enters the variation a1");
    nav(&mut s, Nav::Forward(0));
    assert_eq!(s.cursor(), n.m2);
    nav(&mut s, Nav::Forward(1000));
    assert_eq!(s.cursor(), n.m6);
}

#[test]
fn forward_inside_variation_stays_in_the_variation() {
    let (s, n) = at(|n| n.a1, Nav::Forward(5));
    assert_eq!(s.cursor(), n.a3);
}

#[test]
fn forward_with_max_count_does_not_overflow() {
    let (s, n) = at(|n| n.m1, Nav::Forward(u32::MAX));
    assert_eq!(s.cursor(), n.m6);
}

// ---- Start / GameEnd (rule 11) ----

#[rstest]
#[case(|n: &Nested| n.c2)]
#[case(|n: &Nested| n.m4)]
fn start_goes_to_root_from_anywhere(#[case] from: fn(&Nested) -> NodeId) {
    let (s, _) = at(from, Nav::Start);
    assert_eq!(s.cursor(), s.game().root());
}

#[rstest]
#[case(|n: &Nested| n.c2)]
#[case(|n: &Nested| n.a1)]
fn game_end_goes_to_main_line_leaf_from_a_variation(#[case] from: fn(&Nested) -> NodeId) {
    let (s, n) = at(from, Nav::GameEnd);
    assert_eq!(s.cursor(), n.m6);
}

// ---- Home (rule 12) ----

#[test]
fn home_on_main_line_goes_to_root() {
    let (s, _) = at(|n| n.m4, Nav::Home);
    assert_eq!(s.cursor(), s.game().root());
}

#[test]
fn home_inside_variation_goes_to_line_start_then_climbs_one_level_per_press() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.c2);
    nav(&mut s, Nav::Home);
    assert_eq!(s.cursor(), n.b1, "line_start(c2) = parent(c1)");
    nav(&mut s, Nav::Home);
    assert_eq!(s.cursor(), n.a1);
    nav(&mut s, Nav::Home);
    assert_eq!(s.cursor(), n.m1);
    nav(&mut s, Nav::Home);
    assert_eq!(s.cursor(), s.game().root());
}

// ---- End (rule 13) ----

#[test]
fn end_in_two_steps_from_variation_leaf_to_main_line_leaf() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.a1);
    nav(&mut s, Nav::End);
    assert_eq!(s.cursor(), n.a3, "first press: end of the current line");
    nav(&mut s, Nav::End);
    assert_eq!(s.cursor(), n.m6, "second press: end of the main line");
    nav(&mut s, Nav::End);
    assert_eq!(s.cursor(), n.m6, "main-line leaf: stays");
}

#[test]
fn end_from_main_line_non_leaf_goes_to_main_leaf() {
    let (s, n) = at(|n| n.m2, Nav::End);
    assert_eq!(s.cursor(), n.m6);
}

#[test]
fn end_from_innermost_variation_start_goes_to_its_leaf() {
    let (s, n) = at(|n| n.c1, Nav::End);
    assert_eq!(s.cursor(), n.c2);
}

// ---- ExitVariation (rule 14) ----

#[test]
fn exit_variation_goes_to_parent_of_line_head_and_is_silent_on_main_line() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.c2);
    nav(&mut s, Nav::ExitVariation);
    assert_eq!(s.cursor(), n.b1);
    nav(&mut s, Nav::ExitVariation);
    assert_eq!(s.cursor(), n.a1, "b1 is itself a head");
    nav(&mut s, Nav::ExitVariation);
    assert_eq!(s.cursor(), n.m1);
    nav(&mut s, Nav::ExitVariation);
    assert_eq!(s.cursor(), n.m1);
    assert!(s.notice().is_none(), "no warning on the main line");
}

// ---- EnterVariation (rule 15) ----

#[test]
fn enter_variation_selects_child_by_index() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    nav(&mut s, Nav::EnterVariation(1));
    assert_eq!(s.cursor(), n.a1);
    goto(&mut s, n.m1);
    nav(&mut s, Nav::EnterVariation(0));
    assert_eq!(s.cursor(), n.m2);
}

#[rstest]
#[case(2)]
#[case(usize::MAX)]
fn enter_variation_out_of_range_does_nothing(#[case] index: usize) {
    let (s, n) = at(|n| n.m1, Nav::EnterVariation(index));
    assert_eq!(s.cursor(), n.m1);
}

#[test]
fn enter_variation_on_leaf_does_nothing() {
    let (s, n) = at(|n| n.m6, Nav::EnterVariation(0));
    assert_eq!(s.cursor(), n.m6);
}

// ---- GoToNode (rule 16) ----

#[test]
fn go_to_stale_node_reports_stale_node_and_keeps_cursor() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m3);
    s.update(GameMessage::DeleteVariation(VariationTarget::Node(n.a1)));
    assert!(!s.game().contains(n.a1));
    nav(&mut s, Nav::GoToNode(n.a1));
    assert_eq!(s.notice(), Some(&Notice::StaleNode));
    assert_eq!(s.cursor(), n.m3);
}

// ---- GoToMainlinePly (rule 17) ----

#[rstest]
#[case(0, 0)]
#[case(1, 1)]
#[case(3, 3)]
#[case(6, 6)]
#[case(7, 6)]
#[case(u32::MAX, 6)]
fn go_to_mainline_ply_clamps_to_the_leaf(#[case] ply: u32, #[case] expected_ply: u32) {
    let (mut s, n) = nested_session();
    goto(&mut s, n.c2);
    nav(&mut s, Nav::GoToMainlinePly(ply));
    assert_eq!(s.game().ply(s.cursor()).unwrap(), expected_ply);
    let on_main = s.game().mainline().any(|id| id == s.cursor()) || s.cursor() == s.game().root();
    assert!(on_main, "must land on the main line");
}

// ---- GoToMoveNumber / GoToBlackMoveNumber (rule 18, resolved question 9) ----

#[rstest]
#[case(0, None)] // ignored: cursor stays
#[case(1, Some(1))]
#[case(2, Some(3))]
#[case(3, Some(5))]
#[case(4, Some(6))] // past the end: leaf
#[case(u32::MAX, Some(6))]
fn go_to_move_number_standard_game_equals_ply_2n_minus_1(
    #[case] number: u32,
    #[case] ply: Option<u32>,
) {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m4);
    nav(&mut s, Nav::GoToMoveNumber(number));
    match ply {
        None => assert_eq!(s.cursor(), n.m4, "n == 0 is ignored"),
        Some(p) => assert_eq!(s.game().ply(s.cursor()).unwrap(), p),
    }
}

#[rstest]
#[case(1, 2)]
#[case(2, 4)]
#[case(3, 6)]
#[case(4, 6)]
fn go_to_black_move_number_lands_after_blacks_move(#[case] number: u32, #[case] ply: u32) {
    let (mut s, _) = nested_session();
    nav(&mut s, Nav::GoToBlackMoveNumber(number));
    assert_eq!(s.game().ply(s.cursor()).unwrap(), ply);
}

#[test]
fn go_to_move_number_with_black_to_move_start() {
    // Black to move, fullmove 1; moves: ...e5 2.Nf3 Nc6
    let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
    let mut game = Game::from_position(Position::from_fen(fen).unwrap());
    let root = game.root();
    let x1 = add(&mut game, root, "e7e5");
    let x2 = add(&mut game, x1, "g1f3");
    let x3 = add(&mut game, x2, "b8c6");
    let mut s = GameSession::new(game);
    goto(&mut s, x2);

    // White never played a move numbered 1 in this game: go to the root.
    nav(&mut s, Nav::GoToMoveNumber(1));
    assert_eq!(s.cursor(), s.game().root());
    nav(&mut s, Nav::GoToMoveNumber(2));
    assert_eq!(s.cursor(), x2, "after White's move 2");
    nav(&mut s, Nav::GoToMoveNumber(9));
    assert_eq!(s.cursor(), x3, "past the end: leaf");
    nav(&mut s, Nav::GoToBlackMoveNumber(1));
    assert_eq!(s.cursor(), x1, "Black's move 1 is the first move");
    nav(&mut s, Nav::GoToBlackMoveNumber(2));
    assert_eq!(s.cursor(), x3);
}

// ---- rules 8 and 19: side effects of navigation ----

#[test]
fn navigation_creates_no_undo_point_and_does_not_touch_dirty_or_revision() {
    let (mut s, n) = nested_session();
    let revision = s.revision();
    for m in [
        Nav::GoToNode(n.c2),
        Nav::Back(3),
        Nav::Forward(2),
        Nav::Home,
        Nav::End,
        Nav::ExitVariation,
        Nav::EnterVariation(1),
        Nav::GoToMainlinePly(3),
        Nav::GoToMoveNumber(2),
        Nav::Start,
        Nav::GameEnd,
    ] {
        nav(&mut s, m);
    }
    assert_eq!(s.undo_depth(), 0);
    assert!(!s.can_undo() && !s.can_redo());
    assert!(!s.is_dirty());
    assert_eq!(s.revision(), revision);
}

#[test]
fn navigation_after_edit_keeps_dirty_and_undo_depth() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m6);
    s.update(GameMessage::SetComment {
        node: n.m6,
        text: "x".into(),
    });
    assert!(s.is_dirty());
    nav(&mut s, Nav::Start);
    nav(&mut s, Nav::GameEnd);
    assert!(s.is_dirty());
    assert_eq!(s.undo_depth(), 1);
}

#[test]
fn navigation_clears_entry_closes_chooser_and_disarms_pending_variation() {
    let (mut s, n) = nested_session();
    // entry
    s.update(GameMessage::MoveEntryChar('e'));
    assert!(!s.move_entry().text().is_empty());
    nav(&mut s, Nav::Forward(1));
    assert!(s.move_entry().text().is_empty());
    assert!(s.move_entry().candidates().is_empty());
    // chooser
    goto(&mut s, n.m1);
    s.update(GameMessage::OpenVariationChooser);
    assert!(s.chooser().is_some());
    nav(&mut s, Nav::Back(1));
    assert!(s.chooser().is_none());
    // pending variation
    goto(&mut s, n.m1);
    s.update(GameMessage::StartVariation);
    assert_eq!(s.pending_variation(), Some(n.m1));
    nav(&mut s, Nav::Forward(1));
    assert_eq!(s.pending_variation(), None);
}

// ---- rule 20: show_var_popup ----

#[test]
fn forward_opens_chooser_instead_of_moving_when_popup_is_on_and_branching() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::ApplySettings(EditSettings {
        show_var_popup: true,
        ..EditSettings::default()
    }));
    goto(&mut s, n.m1);
    nav(&mut s, Nav::Forward(1));
    assert_eq!(s.cursor(), n.m1, "did not move");
    let chooser = s.chooser().expect("chooser opened");
    assert_eq!(chooser.parent, n.m1);
    assert_eq!(chooser.selected, 0);
}

#[test]
fn forward_with_popup_on_moves_normally_when_not_branching() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::ApplySettings(EditSettings {
        show_var_popup: true,
        ..EditSettings::default()
    }));
    goto(&mut s, n.m2);
    nav(&mut s, Nav::Forward(1));
    assert_eq!(s.cursor(), n.m3);
    assert!(s.chooser().is_none());
}

#[test]
fn forward_with_popup_off_moves_without_opening_the_chooser() {
    let (mut s, n) = nested_session();
    assert!(!s.settings().show_var_popup, "Scid default is off");
    goto(&mut s, n.m1);
    nav(&mut s, Nav::Forward(1));
    assert_eq!(s.cursor(), n.m2);
    assert!(s.chooser().is_none());
}

#[test]
fn forward_with_popup_on_does_not_open_chooser_while_autoplay_runs() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::ApplySettings(EditSettings {
        show_var_popup: true,
        ..EditSettings::default()
    }));
    s.update(GameMessage::StartAutoplay);
    assert!(s.autoplay().running);
    assert_eq!(s.cursor(), n.m1);
    nav(&mut s, Nav::Forward(1));
    assert_eq!(s.cursor(), n.m2);
    assert!(s.chooser().is_none());
}
