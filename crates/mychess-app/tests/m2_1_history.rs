//! Spec m2-1, rules 1-8 and 69-76: session construction, the edit funnel,
//! undo/redo, the "altered" flag and trial mode.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m2_1_common;

use std::time::Duration;

use m2_1_common::*;
use mychess_app::{
    EditSettings, GameMessage, GameSession, Nav, Notice, PlayOptions, Prompt, ReplaceChoice,
    UNDO_LIMIT,
};

fn comment(s: &mut GameSession, node: NodeId, text: &str) {
    s.update(GameMessage::SetComment {
        node,
        text: text.to_owned(),
    });
}

fn undo(s: &mut GameSession) {
    s.update(GameMessage::Undo);
}

fn redo(s: &mut GameSession) {
    s.update(GameMessage::Redo);
}

// ---- rules 1-2: new / replace_game ----

#[test]
fn new_session_has_the_documented_initial_state() {
    let s = GameSession::new(Game::new());
    assert_eq!(s.cursor(), s.game().root());
    assert!(!s.is_dirty());
    assert!(!s.can_undo() && !s.can_redo());
    assert_eq!(s.undo_depth(), 0);
    assert!(!s.in_trial());
    assert!(s.prompt().is_none() && s.chooser().is_none());
    assert_eq!(s.pending_variation(), None);
    assert!(s.move_entry().text().is_empty() && s.move_entry().candidates().is_empty());
    assert!(!s.autoplay().running);
    assert_eq!(s.autoplay().delay, s.settings().autoplay_delay);
    assert!(s.notice().is_none());
}

#[test]
fn default_settings_are_the_scid_defaults() {
    let st = GameSession::new(Game::new()).settings().clone();
    assert!(st.ask_to_replace_moves);
    assert!(!st.show_var_popup);
    assert!(st.move_entry.enabled && !st.move_entry.auto_expand && st.move_entry.coordinate);
    assert_eq!(st.autoplay_delay, Duration::from_secs(5));
    assert!(st.exclusive_nags);
}

#[test]
fn replace_game_resets_everything_but_keeps_preferences() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::ApplySettings(EditSettings {
        ask_to_replace_moves: false,
        ..EditSettings::default()
    }));
    comment(&mut s, n.m1, "x");
    s.update(GameMessage::EnterTrial { null_move: false });
    s.update(GameMessage::StartAutoplay);
    let generation = s.autoplay().generation;
    s.replace_game(line(&["d2d4"]));
    assert_eq!(s.cursor(), s.game().root());
    assert!(!s.is_dirty() && !s.in_trial());
    assert!(!s.can_undo() && !s.can_redo());
    assert!(!s.autoplay().running);
    assert!(s.autoplay().generation > generation);
    assert!(!s.settings().ask_to_replace_moves, "preferences kept");
    assert_eq!(s.game().node_count(), 2);
    assert!(s.prompt().is_none() && s.chooser().is_none() && s.pending_variation().is_none());
}

#[test]
fn replace_game_discards_the_old_history_even_for_undo_after_a_prompt() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    play(&mut s, "d7d5");
    assert!(s.prompt().is_some());
    s.replace_game(Game::new());
    assert!(s.prompt().is_none());
    undo(&mut s);
    assert_eq!(s.game().node_count(), 1);
}

// ---- rules 3-5: the funnel ----

#[test]
fn stale_node_in_any_node_message_produces_a_notice_and_changes_nothing() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::DeleteVariation(
        mychess_app::VariationTarget::Node(n.a1),
    ));
    let stale = n.b2;
    let snapshot = s.game().clone();
    let depth = s.undo_depth();
    for msg in [
        GameMessage::Navigate(Nav::GoToNode(stale)),
        GameMessage::SetComment {
            node: stale,
            text: "x".into(),
        },
        GameMessage::SetCommentBefore {
            node: stale,
            text: "x".into(),
        },
        GameMessage::PromoteVariation(mychess_app::VariationTarget::Node(stale)),
        GameMessage::MakeFirstVariation(mychess_app::VariationTarget::Node(stale)),
        GameMessage::DeleteVariation(mychess_app::VariationTarget::Node(stale)),
    ] {
        s.update(msg.clone());
        assert_eq!(s.notice(), Some(&Notice::StaleNode), "{msg:?}");
        assert!(s.game().same_content(&snapshot));
        assert_eq!(s.undo_depth(), depth);
    }
}

#[test]
fn every_message_clears_the_previous_notice() {
    let (mut s, _) = nested_session();
    s.update(GameMessage::ClearNags); // at the root: NoMoveHere
    assert_eq!(s.notice(), Some(&Notice::NoMoveHere));
    s.update(GameMessage::Navigate(Nav::Forward(1)));
    assert!(s.notice().is_none(), "the next message clears it");
}

#[test]
fn ineffective_edit_leaves_revision_dirty_and_redo_untouched() {
    let (mut s, n) = nested_session();
    comment(&mut s, n.m1, "a");
    undo(&mut s);
    assert!(s.can_redo());
    let revision = s.revision();
    s.update(GameMessage::ClearNags); // error at root
    s.update(GameMessage::StripNags); // nothing to strip
    s.update(GameMessage::TruncateStart); // root
    comment(&mut s, n.m1, "   "); // blank where none
    assert_eq!(s.revision(), revision);
    assert!(s.can_redo(), "no effective edit: redo survives");
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn effective_edit_empties_the_redo_stack() {
    let (mut s, n) = nested_session();
    comment(&mut s, n.m1, "a");
    undo(&mut s);
    assert!(s.can_redo());
    comment(&mut s, n.m1, "b");
    assert!(!s.can_redo());
    redo(&mut s);
    assert_eq!(s.game().comment(n.m1).unwrap(), Some("b"));
}

#[test]
fn navigation_keeps_the_redo_stack() {
    let (mut s, n) = nested_session();
    comment(&mut s, n.m1, "a");
    undo(&mut s);
    nav(&mut s, Nav::GoToNode(n.m4));
    assert!(s.can_redo());
}

// ---- rules 69-71: undo and redo ----

#[test]
fn undo_and_redo_with_empty_stacks_do_nothing_silently() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m3);
    undo(&mut s);
    redo(&mut s);
    assert_eq!(s.cursor(), n.m3);
    assert!(s.notice().is_none());
    assert!(!s.is_dirty());
}

#[test]
fn undo_restores_game_and_cursor_and_redo_goes_back() {
    let mut s = GameSession::new(line(&["e2e4"]));
    nav(&mut s, Nav::GameEnd);
    let leaf = s.cursor();
    play(&mut s, "e7e5");
    let after = s.cursor();
    undo(&mut s);
    assert_eq!(s.cursor(), leaf);
    assert_eq!(s.game().node_count(), 2);
    redo(&mut s);
    assert_eq!(s.cursor(), after);
    assert_eq!(s.game().node_count(), 3);
}

#[test]
fn undo_closes_chooser_entry_and_armed_variation() {
    let (mut s, n) = nested_session();
    comment(&mut s, n.m1, "a");
    goto(&mut s, n.m1);
    s.update(GameMessage::OpenVariationChooser);
    s.update(GameMessage::StartVariation);
    s.update(GameMessage::MoveEntryChar('d'));
    undo(&mut s);
    assert!(s.chooser().is_none());
    assert_eq!(s.pending_variation(), None);
    assert!(s.move_entry().text().is_empty());
}

#[test]
fn undo_stack_is_limited_to_twenty_levels() {
    assert_eq!(UNDO_LIMIT, 20);
    let (mut s, n) = nested_session();
    for i in 0..25 {
        comment(&mut s, n.m1, &format!("c{i}"));
        assert!(s.undo_depth() <= 20);
    }
    assert_eq!(s.undo_depth(), 20);
    for _ in 0..20 {
        undo(&mut s);
    }
    assert!(!s.can_undo());
    assert_eq!(
        s.game().comment(n.m1).unwrap(),
        Some("c4"),
        "the 5 oldest points fell off"
    );
}

#[test]
fn redo_stack_survives_the_limit_and_replays_all_undone_edits() {
    let (mut s, n) = nested_session();
    for i in 0..25 {
        comment(&mut s, n.m1, &format!("c{i}"));
    }
    for _ in 0..20 {
        undo(&mut s);
    }
    for _ in 0..20 {
        redo(&mut s);
    }
    assert_eq!(s.game().comment(n.m1).unwrap(), Some("c24"));
    assert_eq!(s.undo_depth(), 20);
}

// ---- rule 72: round trip ----

#[test]
fn n_edits_then_n_undos_restore_the_initial_game_and_n_redos_the_final_one() {
    let (mut s, n) = nested_session();
    let mut before: Vec<(Game, NodeId)> = Vec::new();
    let steps: Vec<(NodeId, GameMessage)> = vec![
        (
            n.m3,
            GameMessage::SetComment {
                node: n.m3,
                text: "hi".into(),
            },
        ),
        (n.m3, GameMessage::AddNag(nag(1))),
        (
            n.m1,
            GameMessage::PromoteVariation(mychess_app::VariationTarget::Node(n.a1)),
        ),
        (n.m5, GameMessage::TruncateEnd),
        (n.m4, GameMessage::StripVariations),
        (n.m2, GameMessage::TruncateFrom),
        (n.m1, GameMessage::TruncateStart),
    ];
    for (at, msg) in steps {
        if s.game().contains(at) {
            goto(&mut s, at);
        }
        let snapshot = (s.game().clone(), s.cursor());
        let depth = s.undo_depth();
        s.update(msg);
        if s.undo_depth() > depth {
            before.push(snapshot);
        }
    }
    let final_game = s.game().clone();
    let final_cursor = s.cursor();
    let applied = s.undo_depth();
    assert!(applied >= 5, "most steps are effective: {applied}");
    for (game, cursor) in before.iter().rev().take(applied) {
        undo(&mut s);
        assert_eq!(s.cursor(), *cursor, "cursor from before the edit");
        assert!(s.game().same_content(game));
    }
    assert!(!s.can_undo());
    for _ in 0..applied {
        redo(&mut s);
    }
    assert!(s.game().same_content(&final_game));
    assert_eq!(s.cursor(), final_cursor);
}

// ---- rule 73: dirty ----

#[test]
fn dirty_follows_edits_undo_and_mark_saved() {
    let (mut s, n) = nested_session();
    assert!(!s.is_dirty());
    comment(&mut s, n.m1, "a");
    assert!(s.is_dirty());
    undo(&mut s);
    assert!(!s.is_dirty(), "back to the loaded revision");
    redo(&mut s);
    assert!(s.is_dirty());
    s.mark_saved();
    assert!(!s.is_dirty());
    comment(&mut s, n.m1, "b");
    assert!(s.is_dirty());
    undo(&mut s);
    assert!(!s.is_dirty(), "back to the saved revision");
    undo(&mut s);
    assert!(s.is_dirty(), "before the saved state");
    redo(&mut s);
    assert!(!s.is_dirty());
}

#[test]
fn dirty_is_not_confused_by_a_different_edit_after_undo() {
    let (mut s, n) = nested_session();
    comment(&mut s, n.m1, "A");
    s.mark_saved();
    undo(&mut s);
    comment(&mut s, n.m1, "B");
    assert!(s.is_dirty(), "a new edit never matches the saved revision");
    undo(&mut s);
    assert!(s.is_dirty());
}

#[test]
fn dirty_stays_true_forever_when_the_saved_state_fell_off_the_undo_stack() {
    let (mut s, n) = nested_session();
    for i in 0..21 {
        comment(&mut s, n.m1, &format!("c{i}"));
    }
    for _ in 0..20 {
        undo(&mut s);
    }
    assert!(!s.can_undo());
    assert!(s.is_dirty(), "the loaded state is out of reach");
}

#[test]
fn dirty_is_clean_when_the_saved_state_is_exactly_the_oldest_point() {
    let (mut s, n) = nested_session();
    for i in 0..20 {
        comment(&mut s, n.m1, &format!("c{i}"));
    }
    for _ in 0..20 {
        undo(&mut s);
    }
    assert!(!s.is_dirty());
}

#[test]
fn navigation_does_not_change_dirty() {
    let (mut s, n) = nested_session();
    comment(&mut s, n.m1, "a");
    nav(&mut s, Nav::Start);
    assert!(s.is_dirty());
    s.mark_saved();
    nav(&mut s, Nav::GameEnd);
    assert!(!s.is_dirty());
}

// ---- rules 74-76: trial ----

#[test]
fn enter_trial_edits_in_place_without_undo_points_and_exit_restores_everything() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m3);
    let original = s.game().clone();
    let revision = s.revision();
    s.update(GameMessage::EnterTrial { null_move: false });
    assert!(s.in_trial());
    comment(&mut s, n.m3, "trial");
    s.update(GameMessage::TruncateEnd);
    play(&mut s, "b8c6");
    assert!(!s.game().same_content(&original));
    assert_eq!(s.undo_depth(), 0);
    s.update(GameMessage::ExitTrial);
    assert!(!s.in_trial());
    assert!(s.game().same_content(&original));
    assert_eq!(s.cursor(), n.m3);
    assert_eq!(s.revision(), revision);
    assert!(!s.is_dirty());
}

#[test]
fn trial_does_not_disturb_the_existing_history() {
    let (mut s, n) = nested_session();
    comment(&mut s, n.m1, "before");
    let depth = s.undo_depth();
    s.update(GameMessage::EnterTrial { null_move: false });
    comment(&mut s, n.m1, "during");
    s.update(GameMessage::ExitTrial);
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(s.game().comment(n.m1).unwrap(), Some("before"));
    undo(&mut s);
    assert_eq!(s.game().comment(n.m1).unwrap(), None);
}

#[test]
fn undo_and_redo_are_disabled_in_trial_mode_with_notices() {
    let (mut s, n) = nested_session();
    comment(&mut s, n.m1, "a");
    s.update(GameMessage::EnterTrial { null_move: false });
    let game = s.game().clone();
    undo(&mut s);
    assert_eq!(s.notice(), Some(&Notice::UndoDisabledInTrial));
    redo(&mut s);
    assert_eq!(s.notice(), Some(&Notice::RedoDisabledInTrial));
    assert!(s.game().same_content(&game));
}

#[test]
fn dirty_in_trial_shows_the_value_from_before_entering() {
    for edited_before in [false, true] {
        let (mut s, n) = nested_session();
        if edited_before {
            comment(&mut s, n.m1, "x");
        }
        s.update(GameMessage::EnterTrial { null_move: false });
        comment(&mut s, n.m2, "t");
        assert_eq!(s.is_dirty(), edited_before);
        s.update(GameMessage::ExitTrial);
        assert_eq!(s.is_dirty(), edited_before);
    }
}

#[test]
fn entering_trial_twice_keeps_the_first_snapshot() {
    let (mut s, n) = nested_session();
    let original = s.game().clone();
    s.update(GameMessage::EnterTrial { null_move: false });
    comment(&mut s, n.m1, "t");
    s.update(GameMessage::EnterTrial { null_move: false });
    s.update(GameMessage::ExitTrial);
    assert!(s.game().same_content(&original));
}

#[test]
fn exit_trial_outside_trial_does_nothing() {
    let (mut s, n) = nested_session();
    comment(&mut s, n.m1, "a");
    let game = s.game().clone();
    s.update(GameMessage::ExitTrial);
    assert!(s.game().same_content(&game));
    assert!(s.is_dirty());
}

#[test]
fn toggle_trial_alternates() {
    let (mut s, _) = nested_session();
    s.update(GameMessage::ToggleTrial { null_move: false });
    assert!(s.in_trial());
    s.update(GameMessage::ToggleTrial { null_move: false });
    assert!(!s.in_trial());
}

#[test]
fn trial_with_null_move_at_a_leaf_appends_it_as_continuation() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m6);
    s.update(GameMessage::EnterTrial { null_move: true });
    assert!(s.in_trial());
    assert!(s.game().mv(s.cursor()).unwrap().is_null());
    assert_eq!(s.game().parent(s.cursor()).unwrap(), Some(n.m6));
    s.update(GameMessage::ExitTrial);
    assert_eq!(s.cursor(), n.m6);
    assert!(s.game().children(n.m6).unwrap().is_empty());
}

#[test]
fn trial_with_null_move_at_a_branch_adds_a_variation_without_prompt() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m2);
    s.update(GameMessage::EnterTrial { null_move: true });
    assert!(s.prompt().is_none());
    let kids = children(&s, n.m2);
    assert_eq!(kids.len(), 2);
    assert_eq!(kids[0], n.m3, "main line kept");
    assert_eq!(s.cursor(), kids[1]);
    assert!(s.game().mv(kids[1]).unwrap().is_null());
}

#[test]
fn trial_with_null_move_in_check_only_enters_trial() {
    let mut s = session_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1");
    s.update(GameMessage::EnterTrial { null_move: true });
    assert!(s.in_trial());
    assert_eq!(s.notice(), Some(&Notice::NullMoveInCheck));
    assert_eq!(s.game().node_count(), 1);
}

#[test]
fn exit_trial_closes_prompt_chooser_and_armed_variation() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    s.update(GameMessage::EnterTrial { null_move: false });
    s.update(GameMessage::OpenVariationChooser);
    s.update(GameMessage::StartVariation);
    s.update(GameMessage::ExitTrial);
    assert!(s.chooser().is_none());
    assert_eq!(s.pending_variation(), None);
    assert!(s.prompt().is_none());
}

#[test]
fn trial_replace_without_prompt_even_when_asking_is_on() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    s.update(GameMessage::EnterTrial { null_move: false });
    s.update(GameMessage::PlayMove {
        mv: mv_at(s.game(), n.m1, "d7d5"),
        options: PlayOptions::default(),
    });
    assert!(s.prompt().is_none());
    assert_eq!(children(&s, n.m1), vec![s.cursor()]);
}

#[test]
fn answering_try_variation_enters_trial_and_reverts_on_exit() {
    let (mut s, n) = nested_session();
    let original = s.game().clone();
    goto(&mut s, n.m1);
    play(&mut s, "d7d5");
    assert!(matches!(s.prompt(), Some(Prompt::ReplaceMove(_))));
    s.update(GameMessage::AnswerReplace(ReplaceChoice::Trial));
    assert!(s.in_trial());
    s.update(GameMessage::ExitTrial);
    assert!(s.game().same_content(&original));
    assert_eq!(s.cursor(), n.m1);
}
