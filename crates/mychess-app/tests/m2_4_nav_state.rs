//! `NavState` (D202): which navigation buttons are enabled.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::unreadable_literal,
    clippy::assigning_clones,
    clippy::manual_assert_eq,
    clippy::format_collect,
    clippy::unchecked_time_subtraction,
    clippy::field_reassign_with_default,
    clippy::cast_possible_truncation,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::needless_pass_by_value
)]

mod m2_4_common;

use m2_4_common::*;

/// `1.e4 e5 (1...c5 2.Nf3 (2.Nc3)) 2.Nf3 Nc6` plus a second root move `d4`.
struct Tree {
    session: GameSession,
    e4: NodeId,
    e5: NodeId,
    c5: NodeId,
    nf3_var: NodeId,
    nc3: NodeId,
    nc6: NodeId,
    d4: NodeId,
}

fn tree() -> Tree {
    let mut g = Game::new();
    let root = g.root();
    let e4 = san(&mut g, root, "e4");
    let e5 = san(&mut g, e4, "e5");
    let c5 = san(&mut g, e4, "c5");
    let nf3 = san(&mut g, e5, "Nf3");
    let nc6 = san(&mut g, nf3, "Nc6");
    let nf3_var = san(&mut g, c5, "Nf3");
    let nc3 = san(&mut g, c5, "Nc3");
    let d4 = san(&mut g, root, "d4");
    Tree {
        session: GameSession::new(g),
        e4,
        e5,
        c5,
        nf3_var,
        nc3,
        nc6,
        d4,
    }
}

fn at(node: NodeId, t: &mut Tree) -> NavState {
    t.session.update(GameMessage::Navigate(Nav::GoToNode(node)));
    assert_eq!(t.session.cursor(), node);
    NavState::of(&t.session)
}

#[test]
fn empty_game() {
    let n = NavState::of(&GameSession::default());
    assert!(n.at_start && n.at_end && !n.in_variation && !n.trial);
    assert!(!n.can_add_variation);
    assert_eq!(n.variation_count, 0);
    assert!(!n.can_undo && !n.can_redo);
}

#[test]
fn root_of_a_game_with_an_alternative_first_move() {
    let t = tree();
    let n = NavState::of(&t.session);
    assert!(n.at_start && !n.at_end && !n.in_variation);
    assert!(n.can_add_variation);
    assert_eq!(n.variation_count, 1, "d4 is the alternative to e4");
}

#[test]
fn root_with_a_single_continuation_has_no_variation_to_enter() {
    let mut g = Game::new();
    main_line(&mut g, "e4");
    let n = NavState::of(&GameSession::new(g));
    assert!(n.at_start && !n.at_end);
    assert!(
        n.can_add_variation,
        "a move to hang it from exists (continuation)"
    );
    assert_eq!(n.variation_count, 0);
}

#[test]
fn middle_of_the_main_line() {
    let mut t = tree();
    let n = at(t.e5, &mut t);
    assert!(!n.at_start && !n.at_end && !n.in_variation && n.can_add_variation);
    assert_eq!(n.variation_count, 0);
}

#[test]
fn node_with_alternatives_counts_them() {
    let mut t = tree();
    let n = at(t.e4, &mut t);
    assert_eq!(n.variation_count, 1, "c5 is the alternative to e5");
    assert!(!n.in_variation);
}

#[test]
fn end_of_the_main_line() {
    let mut t = tree();
    let n = at(t.nc6, &mut t);
    assert!(n.at_end && !n.at_start && !n.in_variation);
    assert!(
        n.can_add_variation,
        "after a move one can always arm a variation"
    );
    assert_eq!(n.variation_count, 0);
}

#[test]
fn head_of_a_variation_is_in_a_variation() {
    let mut t = tree();
    for node in [t.c5, t.d4] {
        let n = at(node, &mut t);
        assert!(n.in_variation, "{node:?}");
    }
    let n = at(t.c5, &mut t);
    assert_eq!(
        n.variation_count, 1,
        "Nc3 is the alternative to Nf3 below c5"
    );
}

#[test]
fn leaf_of_a_nested_variation() {
    let mut t = tree();
    let n = at(t.nc3, &mut t);
    assert!(n.in_variation && n.at_end && n.can_add_variation);
    let n = at(t.nf3_var, &mut t);
    assert!(n.in_variation && n.at_end);
}

#[test]
fn leaving_a_variation_clears_the_flag() {
    let mut t = tree();
    at(t.nc3, &mut t);
    // Nc3 is nested in the c5 line: one exit lands in c5's line.
    t.session.update(GameMessage::Navigate(Nav::ExitVariation));
    assert!(NavState::of(&t.session).in_variation);
    t.session.update(GameMessage::Navigate(Nav::ExitVariation));
    assert!(!NavState::of(&t.session).in_variation);
}

#[test]
fn trial_mode_flag_and_history_buttons() {
    let mut t = tree();
    t.session.update(GameMessage::StripComments); // no-op: nothing to strip
    t.session.update(GameMessage::StripVariations);
    assert!(NavState::of(&t.session).can_undo);
    t.session
        .update(GameMessage::EnterTrial { null_move: false });
    let n = NavState::of(&t.session);
    assert!(
        n.trial && !n.can_undo && !n.can_redo,
        "undo is off in trial mode"
    );
    t.session.update(GameMessage::ExitTrial);
    let n = NavState::of(&t.session);
    assert!(!n.trial && n.can_undo);
    t.session.update(GameMessage::Undo);
    assert!(NavState::of(&t.session).can_redo);
}

#[test]
fn an_open_prompt_does_not_change_the_navigation_flags() {
    let mut t = tree();
    at(t.e5, &mut t);
    let before = NavState::of(&t.session);
    t.session.update(GameMessage::RequestStrip {
        kind: StripKind::Variations,
        confirm: true,
    });
    assert!(t.session.prompt().is_some());
    assert_eq!(
        NavState::of(&t.session),
        before,
        "the shell disables buttons on prompt()"
    );
    t.session.update(GameMessage::Navigate(Nav::Start));
    assert_eq!(
        NavState::of(&t.session),
        before,
        "navigation is refused while the prompt is open"
    );
}

#[test]
fn state_nav_state_matches_the_session() {
    let mut state = state_with(tree().session.game().clone());
    assert_eq!(state.nav_state(), NavState::of(&state.session));
    send(&mut state, GameMessage::Navigate(Nav::Start));
    assert!(state.nav_state().at_start);
}

#[test]
fn replacing_the_game_resets_to_the_start() {
    let mut t = tree();
    at(t.nc3, &mut t);
    t.session.replace_game(Game::new());
    let n = NavState::of(&t.session);
    assert!(n.at_start && n.at_end && !n.in_variation);
}
