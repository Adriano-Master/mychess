//! Spec m2-1, rules 21-31 (variation chooser and autoplay) and the
//! `perform` effect of the autoplay timer.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m2_1_common;

use std::time::Duration;

use m2_1_common::*;
use mychess_app::{
    AppState, EditSettings, Effect, GameMessage, GameSession, MAX_AUTOPLAY_DELAY,
    MIN_AUTOPLAY_DELAY, Message, Nav, Notice, PlayOptions, Prompt, VariationTarget, perform,
    update,
};
use rstest::rstest;

fn choose(s: &mut GameSession, m: GameMessage) {
    s.update(m);
}

// ---- Chooser (rules 21-25) ----

#[test]
fn open_chooser_requires_two_or_more_continuations() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m2); // one child
    s.update(GameMessage::OpenVariationChooser);
    assert!(s.chooser().is_none());
    goto(&mut s, n.m6); // leaf
    s.update(GameMessage::OpenVariationChooser);
    assert!(s.chooser().is_none());
    nav(&mut s, Nav::Start); // root has one child
    s.update(GameMessage::OpenVariationChooser);
    assert!(s.chooser().is_none());
}

#[test]
fn open_chooser_selects_the_main_line_row_and_does_not_move() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    s.update(GameMessage::OpenVariationChooser);
    let c = s.chooser().unwrap();
    assert_eq!((c.parent, c.selected), (n.m1, 0));
    assert_eq!(s.cursor(), n.m1);
}

#[test]
fn chooser_move_saturates_without_wrapping() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1); // 2 rows
    s.update(GameMessage::OpenVariationChooser);
    choose(&mut s, GameMessage::ChooserMove(-1));
    assert_eq!(s.chooser().unwrap().selected, 0);
    choose(&mut s, GameMessage::ChooserMove(1));
    assert_eq!(s.chooser().unwrap().selected, 1);
    choose(&mut s, GameMessage::ChooserMove(1));
    assert_eq!(s.chooser().unwrap().selected, 1, "no wrap at the bottom");
    choose(&mut s, GameMessage::ChooserMove(isize::MAX));
    assert_eq!(s.chooser().unwrap().selected, 1);
    choose(&mut s, GameMessage::ChooserMove(isize::MIN));
    assert_eq!(s.chooser().unwrap().selected, 0);
}

#[test]
fn chooser_messages_without_chooser_do_nothing() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    choose(&mut s, GameMessage::ChooserMove(1));
    choose(&mut s, GameMessage::ChooserConfirm(Some(1)));
    choose(&mut s, GameMessage::ChooserConfirm(None));
    choose(&mut s, GameMessage::CloseVariationChooser);
    assert_eq!(s.cursor(), n.m1);
    assert!(s.chooser().is_none());
}

#[rstest]
#[case::main_line(0, 0)]
#[case::variation_key_v(1, 1)]
fn chooser_confirm_with_digit_enters_that_continuation(#[case] digit: usize, #[case] index: usize) {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    s.update(GameMessage::OpenVariationChooser);
    choose(&mut s, GameMessage::ChooserConfirm(Some(digit)));
    assert!(s.chooser().is_none(), "confirming closes the chooser");
    let expected = if index == 0 { n.m2 } else { n.a1 };
    assert_eq!(s.cursor(), expected);
}

#[test]
fn chooser_confirm_none_uses_the_highlighted_row() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    s.update(GameMessage::OpenVariationChooser);
    choose(&mut s, GameMessage::ChooserMove(1));
    choose(&mut s, GameMessage::ChooserConfirm(None));
    assert_eq!(s.cursor(), n.a1);
    assert!(s.chooser().is_none());
}

#[rstest]
#[case(2)]
#[case(9)]
#[case(usize::MAX)]
fn chooser_confirm_out_of_range_does_nothing(#[case] digit: usize) {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    s.update(GameMessage::OpenVariationChooser);
    choose(&mut s, GameMessage::ChooserConfirm(Some(digit)));
    assert_eq!(s.cursor(), n.m1);
    assert!(s.chooser().is_some(), "stays open");
}

#[test]
fn close_chooser_does_not_move() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    s.update(GameMessage::OpenVariationChooser);
    choose(&mut s, GameMessage::CloseVariationChooser);
    assert!(s.chooser().is_none());
    assert_eq!(s.cursor(), n.m1);
}

#[test]
fn effective_edit_closes_the_chooser() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    s.update(GameMessage::OpenVariationChooser);
    s.update(GameMessage::SetComment {
        node: n.m6,
        text: "c".into(),
    });
    assert!(s.chooser().is_none());
}

#[test]
fn three_way_branch_offers_all_rows_in_order() {
    let (mut g, n) = nested_game();
    let extra = add(&mut g, n.m1, "g8f6");
    let mut s = GameSession::new(g);
    goto(&mut s, n.m1);
    s.update(GameMessage::OpenVariationChooser);
    let nodes: Vec<_> = s.variation_choices().iter().map(|c| c.node).collect();
    assert_eq!(nodes, vec![n.m2, n.a1, extra]);
    choose(&mut s, GameMessage::ChooserMove(5));
    assert_eq!(s.chooser().unwrap().selected, 2);
    choose(&mut s, GameMessage::ChooserConfirm(None));
    assert_eq!(s.cursor(), extra);
}

#[test]
fn variation_choices_list_one_entry_per_child_with_san_preview() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    let choices = s.variation_choices();
    assert_eq!(choices.len(), 2);
    assert_eq!((choices[0].index, choices[0].node), (0, n.m2));
    assert_eq!((choices[1].index, choices[1].node), (1, n.a1));
    assert_eq!(choices[0].san, ["e5", "Nf3", "Nc6", "Bb5", "a6"]);
    assert_eq!(choices[1].san, ["c5", "Nf3", "d6"]);
    // With the chooser open it is the chooser's parent that counts.
    s.update(GameMessage::OpenVariationChooser);
    assert_eq!(s.variation_choices().len(), 2);
}

#[test]
fn variation_choices_preview_is_limited_to_five_moves() {
    let mut g = line(&[
        "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6", "e1g1",
    ]);
    let root = g.root();
    add(&mut g, root, "d2d4");
    let s = GameSession::new(g);
    let choices = s.variation_choices();
    assert_eq!(choices[0].san, ["e4", "e5", "Nf3", "Nc6", "Bb5"]);
    assert_eq!(choices[1].san, ["d4"]);
}

#[test]
fn variation_choices_preview_of_exactly_five_and_six_moves() {
    let mut g = line(&["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"]);
    let root = g.root();
    add(&mut g, root, "d2d4");
    let s = GameSession::new(g);
    assert_eq!(s.variation_choices()[0].san.len(), 5);
}

#[test]
fn variation_choices_preview_san_carries_check_suffix() {
    let mut g = line(&["e2e4", "f7f5", "d1h5"]);
    let root = g.root();
    add(&mut g, root, "d2d4");
    let s = GameSession::new(g);
    assert_eq!(s.variation_choices()[0].san, ["e4", "f5", "Qh5+"]);
}

#[test]
fn variation_choices_at_non_branching_node_has_one_entry() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m2);
    assert_eq!(s.variation_choices().len(), 1);
    goto(&mut s, n.m6);
    assert!(s.variation_choices().is_empty());
}

// ---- Autoplay (rules 26-31) ----

fn effects_gen(effects: &[Effect]) -> (u64, Duration) {
    match effects {
        [Effect::ScheduleAutoplayTick { generation, delay }] => (*generation, *delay),
        other => panic!("expected exactly one tick, got {other:?}"),
    }
}

#[test]
fn autoplay_starts_with_one_immediate_step_and_one_scheduled_tick() {
    let (mut s, n) = nested_session();
    let effects = s.update(GameMessage::StartAutoplay);
    assert!(s.autoplay().running);
    assert_eq!(s.cursor(), n.m1, "immediate step");
    let (generation, delay) = effects_gen(&effects);
    assert_eq!(generation, s.autoplay().generation);
    assert_eq!(delay, Duration::from_secs(5), "Scid default 5000 ms");
}

#[test]
fn autoplay_default_delay_is_five_seconds_and_stopped() {
    let s = GameSession::new(Game::new());
    assert!(!s.autoplay().running);
    assert_eq!(s.autoplay().delay, Duration::from_secs(5));
}

#[test]
fn autoplay_tick_with_current_generation_steps_and_reschedules() {
    let (mut s, n) = nested_session();
    let g0 = effects_gen(&s.update(GameMessage::StartAutoplay)).0;
    let effects = s.update(GameMessage::AutoplayTick { generation: g0 });
    assert_eq!(s.cursor(), n.m2);
    let (g1, _) = effects_gen(&effects);
    assert_ne!(g1, g0, "each scheduled tick has a new generation");
}

#[test]
fn autoplay_tick_with_stale_generation_is_ignored() {
    let (mut s, n) = nested_session();
    let g0 = effects_gen(&s.update(GameMessage::StartAutoplay)).0;
    s.update(GameMessage::AutoplayTick { generation: g0 });
    let cursor = s.cursor();
    let effects = s.update(GameMessage::AutoplayTick { generation: g0 });
    assert!(effects.is_empty());
    assert_eq!(s.cursor(), cursor);
    assert_eq!(cursor, n.m2);
    assert!(s.autoplay().running);
}

#[test]
fn autoplay_tick_with_future_generation_is_ignored() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::StartAutoplay);
    let effects = s.update(GameMessage::AutoplayTick {
        generation: u64::MAX,
    });
    assert!(effects.is_empty());
    assert_eq!(s.cursor(), n.m1);
}

#[test]
fn autoplay_tick_while_stopped_is_ignored() {
    let (mut s, _) = nested_session();
    let effects = s.update(GameMessage::AutoplayTick { generation: 0 });
    assert!(effects.is_empty());
    assert_eq!(s.cursor(), s.game().root());
    assert!(!s.autoplay().running);
}

#[test]
fn autoplay_runs_to_the_leaf_with_one_effect_per_step_and_then_stops_silently() {
    let mut s = GameSession::new(line(&["e2e4", "e7e5", "g1f3", "b8c6"]));
    let mut effects = s.update(GameMessage::StartAutoplay);
    let mut scheduled = 0;
    while !effects.is_empty() {
        let (generation, _) = effects_gen(&effects);
        scheduled += 1;
        effects = s.update(GameMessage::AutoplayTick { generation });
        assert!(scheduled < 20, "autoplay must terminate");
    }
    assert_eq!(scheduled, 3, "4 moves: immediate step + 3 ticks");
    assert!(!s.autoplay().running, "stops on the leaf");
    assert_eq!(s.cursor(), s.game().mainline().last().unwrap());
}

#[test]
fn autoplay_started_on_a_leaf_stops_without_effects() {
    let mut s = GameSession::new(line(&["e2e4"]));
    nav(&mut s, Nav::GameEnd);
    let effects = s.update(GameMessage::StartAutoplay);
    assert!(effects.is_empty());
    assert!(!s.autoplay().running);
}

#[test]
fn autoplay_following_a_variation_stays_in_it() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    nav(&mut s, Nav::EnterVariation(1)); // a1
    assert_eq!(s.cursor(), n.a1);
    let effects = s.update(GameMessage::StartAutoplay);
    assert_eq!(s.cursor(), n.a2, "children[0] of a1");
    assert_eq!(effects.len(), 1);
    let (g, _) = effects_gen(&effects);
    let effects = s.update(GameMessage::AutoplayTick { generation: g });
    assert_eq!(s.cursor(), n.a3);
    assert!(effects.is_empty(), "a3 is a leaf");
    assert!(!s.autoplay().running);
}

#[test]
fn autoplay_never_opens_the_chooser_even_with_show_var_popup() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::ApplySettings(EditSettings {
        show_var_popup: true,
        ..EditSettings::default()
    }));
    let g = effects_gen(&s.update(GameMessage::StartAutoplay)).0;
    assert_eq!(s.cursor(), n.m1);
    s.update(GameMessage::AutoplayTick { generation: g });
    assert_eq!(s.cursor(), n.m2);
    assert!(s.chooser().is_none());
}

#[test]
fn stop_autoplay_invalidates_the_tick_in_flight() {
    let (mut s, n) = nested_session();
    let g = effects_gen(&s.update(GameMessage::StartAutoplay)).0;
    let before = s.autoplay().generation;
    s.update(GameMessage::StopAutoplay);
    assert!(!s.autoplay().running);
    assert!(s.autoplay().generation > before, "generation bumped");
    let effects = s.update(GameMessage::AutoplayTick { generation: g });
    assert!(effects.is_empty());
    assert_eq!(s.cursor(), n.m1);
}

#[test]
fn toggle_autoplay_alternates_start_and_stop() {
    let (mut s, n) = nested_session();
    let effects = s.update(GameMessage::ToggleAutoplay);
    assert!(s.autoplay().running);
    assert_eq!(effects.len(), 1);
    assert_eq!(s.cursor(), n.m1);
    let effects = s.update(GameMessage::ToggleAutoplay);
    assert!(!s.autoplay().running);
    assert!(effects.is_empty());
}

#[test]
fn start_autoplay_while_running_does_not_double_schedule() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::StartAutoplay);
    let effects = s.update(GameMessage::StartAutoplay);
    assert!(effects.is_empty());
    assert_eq!(s.cursor(), n.m1);
}

#[rstest]
#[case::zero(Duration::ZERO, MIN_AUTOPLAY_DELAY)]
#[case::below_min(Duration::from_millis(99), MIN_AUTOPLAY_DELAY)]
#[case::min(Duration::from_millis(100), Duration::from_millis(100))]
#[case::normal(Duration::from_secs(7), Duration::from_secs(7))]
#[case::max(Duration::from_secs(300), Duration::from_secs(300))]
#[case::above_max(Duration::from_millis(300_001), MAX_AUTOPLAY_DELAY)]
#[case::huge(Duration::MAX, MAX_AUTOPLAY_DELAY)]
fn autoplay_delay_is_clamped_between_100_ms_and_300_s(
    #[case] requested: Duration,
    #[case] expected: Duration,
) {
    let (mut s, _) = nested_session();
    let effects = s.update(GameMessage::SetAutoplayDelay(requested));
    assert!(effects.is_empty());
    assert_eq!(s.autoplay().delay, expected);
    let (_, delay) = effects_gen(&s.update(GameMessage::StartAutoplay));
    assert_eq!(delay, expected, "the next scheduling uses the new delay");
}

#[test]
fn constants_match_the_spec_limits() {
    assert_eq!(MIN_AUTOPLAY_DELAY, Duration::from_millis(100));
    assert_eq!(MAX_AUTOPLAY_DELAY, Duration::from_secs(300));
}

#[test]
fn new_delay_applies_only_to_the_next_scheduling() {
    let (mut s, _) = nested_session();
    let (g, d0) = effects_gen(&s.update(GameMessage::StartAutoplay));
    assert_eq!(d0, Duration::from_secs(5));
    let effects = s.update(GameMessage::SetAutoplayDelay(Duration::from_secs(1)));
    assert!(effects.is_empty(), "the tick in flight is not rescheduled");
    let (_, d1) = effects_gen(&s.update(GameMessage::AutoplayTick { generation: g }));
    assert_eq!(d1, Duration::from_secs(1));
}

#[test]
fn opening_a_prompt_stops_autoplay_and_discards_pending_tick() {
    let (mut s, n) = nested_session();
    let g = effects_gen(&s.update(GameMessage::StartAutoplay)).0;
    assert_eq!(s.cursor(), n.m1);
    // A new move at a node with children asks "replace?".
    play(&mut s, "d7d5");
    assert!(matches!(s.prompt(), Some(Prompt::ReplaceMove(_))));
    assert!(!s.autoplay().running);
    assert!(s.autoplay().generation > g);
    let before = s.cursor();
    let effects = s.update(GameMessage::AutoplayTick {
        generation: s.autoplay().generation,
    });
    assert!(effects.is_empty());
    assert_eq!(s.cursor(), before);
    assert_ne!(
        s.notice(),
        Some(&Notice::PromptPending),
        "tick dropped silently"
    );
}

#[test]
fn start_autoplay_with_a_prompt_open_is_refused() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.m1);
    play(&mut s, "d7d5");
    assert!(s.prompt().is_some());
    let effects = s.update(GameMessage::StartAutoplay);
    assert!(effects.is_empty());
    assert!(!s.autoplay().running);
    assert_eq!(s.notice(), Some(&Notice::PromptPending));
}

#[test]
fn manual_navigation_edit_undo_and_trial_do_not_stop_autoplay() {
    let (mut s, n) = nested_session();
    s.update(GameMessage::StartAutoplay);
    let generation = s.autoplay().generation;
    nav(&mut s, Nav::Forward(1));
    assert!(s.autoplay().running, "navigation");
    s.update(GameMessage::SetComment {
        node: n.m6,
        text: "x".into(),
    });
    assert!(s.autoplay().running, "edit");
    s.update(GameMessage::Undo);
    assert!(s.autoplay().running, "undo");
    s.update(GameMessage::EnterTrial { null_move: false });
    assert!(s.autoplay().running, "enter trial");
    s.update(GameMessage::ExitTrial);
    assert!(s.autoplay().running, "exit trial");
    assert_eq!(s.autoplay().generation, generation, "no generation bump");
}

#[test]
fn replace_game_stops_autoplay_and_bumps_generation() {
    let (mut s, _) = nested_session();
    s.update(GameMessage::StartAutoplay);
    let g = s.autoplay().generation;
    s.replace_game(Game::new());
    assert!(!s.autoplay().running);
    assert!(s.autoplay().generation > g);
}

#[test]
fn autoplay_after_deleting_the_variation_it_was_in_keeps_a_valid_cursor() {
    let (mut s, n) = nested_session();
    goto(&mut s, n.a1);
    let g = effects_gen(&s.update(GameMessage::StartAutoplay)).0;
    s.update(GameMessage::DeleteVariation(VariationTarget::Node(n.a1)));
    s.update(GameMessage::AutoplayTick { generation: g });
    assert!(s.game().contains(s.cursor()));
}

// ---- perform (async, paused clock) ----

#[tokio::test(start_paused = true)]
async fn perform_waits_for_the_delay_and_delivers_the_tick() {
    let effect = Effect::ScheduleAutoplayTick {
        generation: 3,
        delay: Duration::from_secs(5),
    };
    let handle = tokio::spawn(perform(effect));
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(4_999)).await;
    tokio::task::yield_now().await;
    assert!(!handle.is_finished(), "must not fire before the delay");
    tokio::time::advance(Duration::from_millis(1)).await;
    let message = handle.await.unwrap();
    assert_eq!(
        message,
        Message::Game(GameMessage::AutoplayTick { generation: 3 })
    );
}

#[tokio::test(start_paused = true)]
async fn perform_handles_the_maximum_delay() {
    let effect = Effect::ScheduleAutoplayTick {
        generation: 1,
        delay: MAX_AUTOPLAY_DELAY,
    };
    let handle = tokio::spawn(perform(effect));
    tokio::task::yield_now().await;
    tokio::time::advance(MAX_AUTOPLAY_DELAY).await;
    let message = handle.await.unwrap();
    assert_eq!(
        message,
        Message::Game(GameMessage::AutoplayTick { generation: 1 })
    );
}

#[tokio::test(start_paused = true)]
async fn autoplay_loop_through_update_and_perform_reaches_the_leaf() {
    let mut state = AppState::default();
    state.session.replace_game(line(&["e2e4", "e7e5", "g1f3"]));
    let mut effects = update(&mut state, Message::Game(GameMessage::StartAutoplay));
    let mut rounds = 0;
    while let Some(effect) = effects.pop() {
        rounds += 1;
        assert!(rounds < 10);
        let handle = tokio::spawn(perform(effect));
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(5)).await;
        let message = handle.await.unwrap();
        effects = update(&mut state, message);
    }
    assert_eq!(rounds, 2);
    assert!(!state.session.autoplay().running);
    assert_eq!(
        state.session.cursor(),
        state.session.game().mainline().last().unwrap()
    );
}

#[tokio::test(start_paused = true)]
async fn stale_tick_delivered_by_perform_is_ignored_after_stop() {
    let mut state = AppState::default();
    state.session.replace_game(line(&["e2e4", "e7e5", "g1f3"]));
    let mut effects = update(&mut state, Message::Game(GameMessage::StartAutoplay));
    let effect = effects.pop().unwrap();
    let handle = tokio::spawn(perform(effect));
    update(&mut state, Message::Game(GameMessage::StopAutoplay));
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(5)).await;
    let message = handle.await.unwrap();
    let cursor = state.session.cursor();
    let effects = update(&mut state, message);
    assert!(effects.is_empty());
    assert_eq!(state.session.cursor(), cursor);
}

#[test]
fn play_options_default_does_not_force_a_variation() {
    assert!(!PlayOptions::default().force_variation);
}
