//! Regression tests of the review of slice M3-4a: dialogs open while a
//! database arrives, quit with altered games in other slots, saves that
//! outlive a switch of slot, trial mode, stale open answers.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::bases::{BasesMessage, BasesNotice, SlotId};
use crate::effect::{Effect, perform};
use crate::error::RequestId;
use crate::files::GameOrigin;
use crate::flow::{AppNotice, Command, DiscardChoice};
use crate::host::PickKind;
use crate::message::Message;
use crate::session::{GameMessage, Nav, PlayOptions};
use crate::state::{AppState, update};

const ONE: &str = "[White \"Solo\"]\n\n1. c4 c5 *\n";
const TWO: &str = "[White \"A\"]\n\n1. e4 e5 *\n\n[White \"C\"]\n\n1. d4 *\n";

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    path
}

#[allow(clippy::field_reassign_with_default)] // private fields forbid struct update
fn state() -> AppState {
    let mut state = AppState::default();
    state.open_pgn_in_slots = true;
    state
}

fn sel(n: usize) -> SlotId {
    SlotId::from_number(n).expect("slot")
}

async fn feed(state: &mut AppState, effects: Vec<Effect>) {
    let mut queue: VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        let answer = perform(effect).await;
        queue.extend(update(state, answer));
    }
}

async fn drive(state: &mut AppState, message: impl Into<Message>) {
    let effects = update(state, message.into());
    feed(state, effects).await;
}

fn play(state: &mut AppState, uci: &str) {
    update(state, Message::Game(GameMessage::Navigate(Nav::GameEnd)));
    let mv = state.session.position().parse_uci(uci).expect("uci");
    update(
        state,
        Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    );
}

fn notice(state: &AppState) -> Option<&BasesNotice> {
    match &state.notice {
        Some(AppNotice::Bases(n)) => Some(n),
        _ => None,
    }
}

fn open_request(effects: &[Effect]) -> Option<(RequestId, bool)> {
    effects.iter().find_map(|e| match e {
        Effect::OpenBase {
            request, allow_new, ..
        } => Some((*request, *allow_new)),
        _ => None,
    })
}

#[tokio::test]
async fn a_database_arriving_under_a_question_does_not_switch_slots() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "a.pgn", TWO)),
    )
    .await;
    play(&mut state, "e2e4");
    // Start opening the second file, then ask a question about the game.
    let effects = update(
        &mut state,
        BasesMessage::Open(write(dir.path(), "b.pgn", ONE)).into(),
    );
    update(&mut state, Command::NewGame.into());
    assert!(state.discard_prompt.is_some());
    feed(&mut state, effects).await;
    assert_eq!(state.bases.current(), sel(1));
    assert!(state.discard_prompt.is_some());
    assert!(state.is_dirty());
    // The database is installed, just not current.
    assert!(state.bases.slot(sel(2)).is_some());
    assert_eq!(notice(&state), Some(&BasesNotice::NotActivated));
    // The single game was not loaded either.
    assert_ne!(state.session.game().tags().white, "Solo");
}

#[tokio::test]
async fn quit_asks_about_every_altered_parked_game_and_cancel_stops() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    for name in ["a.pgn", "b.pgn", "c.pgn"] {
        drive(&mut state, BasesMessage::Open(write(dir.path(), name, ONE))).await;
        play(&mut state, "g1f3");
    }
    // Slots 1 and 2 hold altered games and slot 3 (current) too.
    assert_eq!(state.bases.slots_with_altered_game(), [sel(1), sel(2)]);
    let effects = update(&mut state, Message::Quit);
    assert!(effects.is_empty() && state.discard_prompt.is_some());
    // Discard the current one: the next altered slot is brought forward.
    let effects = update(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave));
    assert!(effects.is_empty());
    assert_eq!(state.bases.current(), sel(1));
    assert!(state.discard_prompt.is_some());
    // Cancel ends the quit.
    update(&mut state, Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(state.discard_prompt.is_none());
    // Quit again: nothing was marked clean, so everything asks again.
    update(&mut state, Message::Quit);
    assert!(state.discard_prompt.is_some());
    assert_eq!(state.bases.current(), sel(1));
    update(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(state.bases.current(), sel(2));
    assert!(state.discard_prompt.is_some());
    update(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(state.bases.current(), sel(3));
    assert!(state.discard_prompt.is_some());
    let effects = update(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave));
    assert!(effects.contains(&Effect::Exit), "{effects:?}");
}

#[tokio::test]
async fn a_save_that_outlives_a_switch_of_slot_keeps_the_origin_of_the_new_editor() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "a.pgn", ONE)),
    )
    .await;
    play(&mut state, "g1f3");
    let effects = update(&mut state, Command::SaveAs.into());
    let Some(Effect::PickFile(spec)) = effects.into_iter().next() else {
        unreachable!("save as asks for a file");
    };
    assert_eq!(spec.kind, PickKind::SavePgn);
    let target = dir.path().join("out.pgn");
    let effects = update(&mut state, Message::FilePicked(spec.request, Some(target)));
    // Switch to the Clipbase while the write is in flight.
    update(&mut state, BasesMessage::Select(SlotId::CLIPBASE).into());
    feed(&mut state, effects).await;
    assert_eq!(state.bases.current(), SlotId::CLIPBASE);
    assert_eq!(state.origin, GameOrigin::New);
}

#[tokio::test]
async fn copying_in_trial_mode_stores_the_game_without_the_trial_moves() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "a.pgn", ONE)),
    )
    .await;
    update(
        &mut state,
        Message::Game(GameMessage::Navigate(Nav::GameEnd)),
    );
    update(
        &mut state,
        Message::Game(GameMessage::EnterTrial { null_move: false }),
    );
    assert!(state.session.in_trial());
    play(&mut state, "g1f3");
    drive(&mut state, BasesMessage::ClipbaseCopy).await;
    assert!(!state.session.in_trial());
    let rows = state.bases.list_page(SlotId::CLIPBASE, 1, 5);
    assert_eq!(rows.len(), 1);
    // Two half moves of the file, not three.
    assert_eq!(rows[0].length, 1);
}

#[tokio::test]
async fn a_refused_open_is_retried_when_a_slot_was_freed_meanwhile() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    for n in 1..=8 {
        let path = write(dir.path(), &format!("g{n}.pgn"), TWO);
        drive(&mut state, BasesMessage::Open(path)).await;
    }
    let ninth = write(dir.path(), "g9.pgn", TWO);
    let effects = update(&mut state, BasesMessage::Open(ninth).into());
    assert_eq!(open_request(&effects).map(|r| r.1), Some(false));
    // A slot is closed while the request is in flight.
    update(&mut state, BasesMessage::Close(sel(3)).into());
    let mut answers = Vec::new();
    for effect in effects {
        answers.push(perform(effect).await);
    }
    let again: Vec<Effect> = answers
        .into_iter()
        .flat_map(|m| update(&mut state, m))
        .collect();
    assert_eq!(open_request(&again).map(|r| r.1), Some(true));
    feed(&mut state, again).await;
    assert!(state.bases.slot(sel(3)).is_some());
    assert_eq!(state.bases.current(), sel(3));
}

#[tokio::test]
async fn an_already_open_answer_for_a_slot_closed_meanwhile_opens_it_again() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    let path = write(dir.path(), "a.pgn", TWO);
    drive(&mut state, BasesMessage::Open(path.clone())).await;
    drive(&mut state, BasesMessage::Select(SlotId::CLIPBASE)).await;
    std::fs::create_dir(dir.path().join("sub")).expect("sub");
    let other_spelling = dir.path().join("sub").join("..").join("a.pgn");
    let effects = update(&mut state, BasesMessage::Open(other_spelling).into());
    assert!(open_request(&effects).is_some());
    update(&mut state, BasesMessage::Close(sel(1)).into());
    assert!(state.bases.slot(sel(1)).is_none());
    let mut again = Vec::new();
    for effect in effects {
        let answer = perform(effect).await;
        again.extend(update(&mut state, answer));
    }
    assert!(open_request(&again).is_some(), "it must be opened again");
    feed(&mut state, again).await;
    assert!(state.bases.slot(sel(1)).is_some());
}
