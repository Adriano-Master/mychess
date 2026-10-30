//! The safety net that undoes a cancelled quit for the spelling flow must not
//! fire while `finish_quit` is still waiting for something (D3509). One test
//! per condition of the guard; the rest of the cancelled-quit behaviour is in
//! `tests/m6_3b_fixes2.rs`.

use super::super::{AppState, update};
use crate::flow::{DiscardPrompt, Pending};
use crate::message::Message;
use crate::spelling_ui::SpellingMessage;
use crate::state::engines_flow::Shutdown;

/// A state whose spelling flow was cut by a quit that has not exited.
fn quitting() -> AppState {
    let mut state = AppState::default();
    state.spelling_cancel();
    assert!(state.spelling.quitting);
    state
}

fn reload(state: &mut AppState) {
    update(state, Message::Spelling(SpellingMessage::Reload));
}

fn assert_quit_kept(mut state: AppState, why: &str) {
    reload(&mut state);
    assert!(state.spelling.quitting, "the quit goes on: {why}");
}

#[test]
fn a_spelling_command_undoes_a_quit_nothing_waits_for() {
    let mut state = quitting();
    reload(&mut state);
    assert!(!state.spelling.quitting);
}

#[test]
fn quit_finishing_is_false_for_a_fresh_state() {
    assert!(!AppState::default().quit_finishing());
}

#[test]
fn the_net_keeps_the_quit_while_the_question_is_open() {
    let mut state = quitting();
    state.discard_prompt = Some(DiscardPrompt {
        pending: Pending::Quit,
    });
    assert_quit_kept(state, "discard prompt");
}

#[test]
fn the_net_keeps_the_quit_while_the_preferences_are_saved() {
    let mut state = quitting();
    state.flow.exit_when_saved = true;
    assert_quit_kept(state, "exit_when_saved");
}

#[test]
fn the_net_keeps_the_quit_while_a_job_is_waited_for() {
    let mut state = quitting();
    state.flow.quit_waiting_job = true;
    assert_quit_kept(state, "quit_waiting_job");
}

#[test]
fn the_net_keeps_the_quit_while_a_batch_step_is_waited_for() {
    let mut state = quitting();
    state.flow.quit_waiting_batch = true;
    assert_quit_kept(state, "quit_waiting_batch");
}

#[test]
fn the_net_keeps_the_quit_while_the_bookmarks_are_written() {
    let mut state = quitting();
    state.flow.quit_waiting_bookmarks = true;
    assert_quit_kept(state, "quit_waiting_bookmarks");
}

#[test]
fn the_net_keeps_the_quit_while_the_score_of_the_tactics_is_written() {
    let mut state = quitting();
    state.flow.tactics.quit_wait = true;
    assert_quit_kept(state, "tactics");
}

#[test]
fn the_net_keeps_the_quit_while_a_trainer_is_written() {
    let mut state = quitting();
    state.flow.opening.quit_wait = true;
    assert_quit_kept(state, "opening trainer");
    let mut state = quitting();
    state.flow.calvar.quit_wait = true;
    assert_quit_kept(state, "calvar trainer");
}

#[test]
fn the_net_keeps_the_quit_while_the_tournament_ends() {
    let mut state = quitting();
    state.flow.tournament.quit_wait = true;
    assert_quit_kept(state, "tournament quit_wait");
    let mut state = quitting();
    state.flow.tournament.quitting = true;
    assert_quit_kept(state, "tournament quitting");
}

#[test]
fn the_net_keeps_the_quit_once_the_caches_of_the_trees_started() {
    let mut state = quitting();
    state.tree.cache.quit_started = true;
    assert_quit_kept(state, "tree cache");
}

#[test]
fn the_net_keeps_the_quit_while_the_engines_end() {
    let mut state = quitting();
    state.flow.engines.quit_waiting_save = true;
    assert_quit_kept(state, "engines save");
    let mut state = quitting();
    state.flow.engines.shutdown = Shutdown::Waiting;
    assert_quit_kept(state, "engines shutdown");
}

#[test]
fn a_quit_abandoned_after_every_wait_is_undone_again() {
    // `Done` is not a wait: the engines ended and the quit was given up.
    let mut state = quitting();
    state.flow.engines.shutdown = Shutdown::Done;
    reload(&mut state);
    assert!(!state.spelling.quitting);
}

#[tokio::test]
async fn closing_a_slot_with_the_question_of_the_quit_open_gives_the_quit_up() {
    // The message `Close` is ignored while a question is open, so the path is
    // reached through the flow function, as `Pending::CloseBase` does.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.pgn");
    std::fs::write(&path, "[Event \"x\"]\n\n1. e4 e5 *\n").unwrap();
    let mut state = AppState {
        open_pgn_in_slots: true,
        ..AppState::default()
    };
    let mut queue: Vec<_> = update(&mut state, crate::bases::BasesMessage::Open(path).into());
    while let Some(effect) = queue.pop() {
        if !effect.runs_on_host() {
            let answer = crate::effect::perform(effect).await;
            queue.extend(update(&mut state, answer));
        }
    }
    let slot = state.bases.current();
    assert!(!slot.is_clipbase(), "a real slot");
    state.spelling_cancel();
    state.discard_prompt = Some(DiscardPrompt {
        pending: Pending::Quit,
    });
    state.flow.quit_asked.push(slot);
    let mut effects = Vec::new();
    state.close_slot_in_flow(slot, &mut effects);
    assert!(state.discard_prompt.is_none());
    assert!(state.flow.quit_asked.is_empty(), "the next quit asks again");
    assert!(!state.spelling.quitting, "the quit was given up");
}

#[tokio::test]
async fn closing_a_slot_keeps_the_quit_when_the_question_is_not_about_it() {
    let mut state = AppState::default();
    state.spelling_cancel();
    let mut effects = Vec::new();
    // Nothing to close and no question: the quit is not touched.
    state.close_slot_in_flow(crate::bases::SlotId::CLIPBASE, &mut effects);
    assert!(state.spelling.quitting);
}

fn with_cut_work() -> AppState {
    use crate::bases::OpenControl;
    use crate::spelling_ui::{LoadRun, Running};
    let mut state = quitting();
    let request = state.new_request();
    state.spelling.cut_load = Some(LoadRun {
        run: Running {
            request,
            control: OpenControl::new(),
        },
        remember: None,
        path: None,
        quiet: false,
        user_cancelled: false,
    });
    state.spelling.photos_cut = true;
    state
}

#[test]
fn finish_quit_drops_the_cut_load_and_the_cut_photos() {
    let mut state = with_cut_work();
    let mut effects = Vec::new();
    state.finish_quit(&mut effects);
    assert!(state.spelling.cut_load.is_none());
    assert!(!state.spelling.photos_cut);
    // Nothing starts again even if the quit is (wrongly) given up later.
    state.spelling_quit_undone(&mut effects);
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, crate::effect::Effect::Spelling(_))),
        "{effects:?}"
    );
}

#[test]
fn unload_drops_the_cut_load_and_the_cut_photos() {
    let mut state = with_cut_work();
    // `Unload` is a user command: the net undoes the quit first, which would
    // restart the load, so the quit is kept by a wait of `finish_quit`.
    state.flow.exit_when_saved = true;
    let effects = update(&mut state, Message::Spelling(SpellingMessage::Unload));
    assert!(state.spelling.cut_load.is_none());
    assert!(!state.spelling.photos_cut);
    assert!(effects.is_empty(), "{effects:?}");
}
