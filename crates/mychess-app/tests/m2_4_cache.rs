//! The notation document cached in `AppState` (D160, rule 11).
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

fn game() -> Game {
    let mut g = Game::new();
    let ns = main_line(&mut g, "e4 e5 Nf3 Nc6");
    san(&mut g, ns[0], "c5");
    g
}

fn fresh(state: &AppState) -> NotationDoc {
    let mut d = build_notation_with_pending(
        state.session.game(),
        state.notation_options(),
        0,
        state.session.pending_variation(),
    );
    d.revision = state.notation().revision;
    d
}

fn same_content(state: &AppState) -> bool {
    let want = fresh(state);
    let got = state.notation();
    got.flat_text() == want.flat_text() && got.header == want.header && got.result == want.result
}

#[test]
fn a_new_state_has_an_up_to_date_empty_document() {
    let state = AppState::default();
    assert!(state.notation().blocks.is_empty());
    assert_eq!(*state.notation_options(), NotationOptions::default());
}

#[test]
fn notation_follows_every_kind_of_edit_after_update() {
    let mut state = state_with(game());
    assert!(same_content(&state));
    let node = state.session.cursor();
    let steps: Vec<GameMessage> = vec![
        GameMessage::SetComment {
            node,
            text: "hi".into(),
        },
        GameMessage::AddNag(nag(1)),
        GameMessage::Undo,
        GameMessage::Redo,
        GameMessage::ToggleNag(nag(14)),
        GameMessage::SetNags(vec![nag(18)]),
        GameMessage::ClearNags,
        GameMessage::Navigate(Nav::Start),
        GameMessage::StripVariations,
        GameMessage::StripComments,
        GameMessage::Undo,
        GameMessage::Undo,
        GameMessage::PlayNullMove,
        GameMessage::EnterTrial { null_move: false },
        GameMessage::ExitTrial,
    ];
    for step in steps {
        let label = format!("{step:?}");
        send(&mut state, step);
        assert!(same_content(&state), "stale notation after {label}");
    }
}

#[test]
fn replacing_the_game_by_a_command_refreshes_the_document() {
    let mut state = state_with(game());
    assert!(!state.notation().blocks.is_empty());
    update(&mut state, Message::Command(Command::NewGame));
    // New game may ask to discard first; answer "discard" if it did.
    if state.discard_prompt.is_some() {
        update(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave));
    }
    assert!(same_content(&state));
    assert!(state.notation().blocks.is_empty());
}

#[test]
fn moving_the_cursor_never_rebuilds() {
    let mut state = state_with(game());
    let revision = state.notation().revision;
    let navs = [
        Nav::Start,
        Nav::Forward(1),
        Nav::Forward(2),
        Nav::Back(1),
        Nav::End,
        Nav::Home,
        Nav::GameEnd,
        Nav::GoToMainlinePly(2),
        Nav::GoToMoveNumber(2),
    ];
    for nav in navs {
        send(&mut state, GameMessage::Navigate(nav));
        assert_eq!(state.notation().revision, revision, "{nav:?}");
    }
    send(&mut state, GameMessage::Navigate(Nav::Start));
    send(&mut state, GameMessage::Navigate(Nav::EnterVariation(1)));
    send(&mut state, GameMessage::Navigate(Nav::ExitVariation));
    assert_eq!(state.notation().revision, revision);
}

#[test]
fn typing_in_the_move_entry_and_opening_a_chooser_do_not_rebuild() {
    let mut state = state_with(game());
    let revision = state.notation().revision;
    send(&mut state, GameMessage::MoveEntryChar('e'));
    send(&mut state, GameMessage::MoveEntryClear);
    send(&mut state, GameMessage::OpenVariationChooser);
    send(&mut state, GameMessage::CloseVariationChooser);
    assert_eq!(state.notation().revision, revision);
}

#[test]
fn an_edit_raises_the_revision_and_keeps_old_revisions_on_untouched_blocks() {
    let mut state = state_with(game());
    let before = state.notation().clone();
    let node = state.session.cursor();
    send(
        &mut state,
        GameMessage::SetComment {
            node,
            text: "x".into(),
        },
    );
    let after = state.notation();
    assert!(after.revision > before.revision);
    assert!(
        after.blocks.iter().any(|b| b.rev == before.revision),
        "an untouched block must keep its revision"
    );
    assert!(after.blocks.iter().any(|b| b.rev == after.revision));
}

#[test]
fn a_no_op_edit_does_not_rebuild() {
    let mut state = state_with(game());
    let revision = state.notation().revision;
    let node = state.session.cursor();
    // Blank comment on a node without comment changes nothing.
    send(
        &mut state,
        GameMessage::SetComment {
            node,
            text: "  ".into(),
        },
    );
    send(&mut state, GameMessage::ClearNags);
    assert_eq!(state.notation().revision, revision);
}

#[test]
fn changing_a_notation_option_rebuilds_and_other_preferences_do_not() {
    let mut state = state_with(game());
    let revision = state.notation().revision;
    update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::AnimationMs(10))),
    );
    update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::HighlightLastMove(false))),
    );
    assert_eq!(
        state.notation().revision,
        revision,
        "board prefs are not notation"
    );
    let o = opts(|o| o.layout = NotationLayout::Inline);
    update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::Notation(o))),
    );
    assert!(state.notation().revision > revision);
    assert_eq!(
        state.notation().blocks.len(),
        1,
        "inline layout is one block"
    );
    assert_eq!(*state.notation_options(), o);
}

#[test]
fn setting_the_same_notation_options_again_does_not_rebuild() {
    let mut state = state_with(game());
    let o = opts(|o| o.symbolic_nags = false);
    update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::Notation(o))),
    );
    let revision = state.notation().revision;
    update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::Notation(o))),
    );
    assert_eq!(state.notation().revision, revision);
}

#[test]
fn hover_delay_above_the_limit_is_clamped_in_the_state() {
    let mut state = AppState::default();
    let o = opts(|o| o.hover_preview = HoverPreview::Delay(u32::MAX));
    update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::Notation(o))),
    );
    assert_eq!(
        state.notation_options().hover_preview,
        HoverPreview::Delay(5000)
    );
}

#[test]
fn the_pending_variation_placeholder_appears_and_goes_with_the_arming() {
    let mut state = state_with(game());
    send(&mut state, GameMessage::Navigate(Nav::Start));
    send(&mut state, GameMessage::Navigate(Nav::Forward(1)));
    send(&mut state, GameMessage::StartVariation);
    assert!(state.session.pending_variation().is_some());
    assert!(state.notation().flat_text().contains("( ... )"));
    assert!(same_content(&state));
    send(&mut state, GameMessage::Navigate(Nav::GameEnd));
    assert!(!state.notation().flat_text().contains("( ... )"));
}

#[test]
fn editing_in_trial_mode_shows_and_leaving_restores_the_document() {
    let mut state = state_with(game());
    let original = state.notation().flat_text();
    send(&mut state, GameMessage::EnterTrial { null_move: false });
    let node = state.session.cursor();
    send(
        &mut state,
        GameMessage::SetComment {
            node,
            text: "trial".into(),
        },
    );
    assert!(state.notation().flat_text().contains("trial"));
    send(&mut state, GameMessage::ExitTrial);
    assert_eq!(state.notation().flat_text(), original);
}

#[test]
fn undo_returns_to_the_text_before_the_edit() {
    let mut state = state_with(game());
    let original = state.notation().flat_text();
    send(&mut state, GameMessage::StripVariations);
    assert_ne!(state.notation().flat_text(), original);
    send(&mut state, GameMessage::Undo);
    assert_eq!(state.notation().flat_text(), original);
}

#[test]
fn revisions_never_go_backwards() {
    let mut state = state_with(game());
    let mut last = state.notation().revision;
    for i in 0..10 {
        let node = state.session.cursor();
        send(
            &mut state,
            GameMessage::SetComment {
                node,
                text: format!("c{i}"),
            },
        );
        send(&mut state, GameMessage::Undo);
        let r = state.notation().revision;
        assert!(r >= last);
        last = r;
    }
}

#[test]
fn direct_session_edit_needs_refresh_notation() {
    let mut state = state_with(game());
    let node = state.session.cursor();
    state.session.update(GameMessage::SetComment {
        node,
        text: "direct".into(),
    });
    state.refresh_notation();
    assert!(state.notation().flat_text().contains("direct"));
}

#[test]
fn stale_and_foreign_node_ids_are_notices_not_panics() {
    let mut big = Game::new();
    let ns = main_line(&mut big, "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6");
    let foreign = *ns.last().unwrap();
    let mut state = state_with(game());
    let revision = state.notation().revision;
    send(
        &mut state,
        GameMessage::SetComment {
            node: foreign,
            text: "x".into(),
        },
    );
    assert_eq!(state.session.notice(), Some(&Notice::StaleNode));
    send(
        &mut state,
        GameMessage::SetCommentBefore {
            node: foreign,
            text: "x".into(),
        },
    );
    send(&mut state, GameMessage::Navigate(Nav::GoToNode(foreign)));
    send(
        &mut state,
        GameMessage::DeleteVariation(VariationTarget::Node(foreign)),
    );
    assert_eq!(state.notation().revision, revision);
    assert!(state.notation().find_move(foreign).is_none());
}

#[test]
fn one_mebibyte_comment_through_the_state_does_not_panic() {
    let mut state = state_with(game());
    let node = state.session.cursor();
    send(
        &mut state,
        GameMessage::SetComment {
            node,
            text: "é".repeat(1 << 19),
        },
    );
    assert!(state.notation().flat_text().len() >= 1 << 19);
}
