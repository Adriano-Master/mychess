//! Sanity tests of the slots and the Clipbase (slice M3-4a). The full
//! contract suite is the tester's.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::bases::{BaseError, BaseKind, BasesMessage, BasesNotice, SlotId};
use crate::effect::{Effect, perform};
use crate::files::GameOrigin;
use crate::flow::{AppNotice, Command, DiscardChoice};
use crate::message::Message;
use crate::session::{GameMessage, Nav, PlayOptions};
use crate::state::{AppState, update};

const TWO: &str = "[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n\
                   [White \"C\"]\n[Black \"D\"]\n\n1. d4 *\n";
const ONE: &str = "[White \"Solo\"]\n\n1. c4 c5 *\n";

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    path
}

#[allow(clippy::field_reassign_with_default)] // private fields forbid the struct update
fn state() -> AppState {
    let mut state = AppState::default();
    state.open_pgn_in_slots = true;
    state
}

/// Sends `message` and runs every effect that is not for the host, feeding
/// the answers back, until nothing is left.
async fn drive(state: &mut AppState, message: impl Into<Message>) {
    let mut queue: VecDeque<Effect> = update(state, message.into()).into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        let answer = perform(effect).await;
        queue.extend(update(state, answer));
    }
}

/// Plays `uci` at the end of the main line.
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

fn sel(n: usize) -> SlotId {
    SlotId::from_number(n).expect("slot")
}

fn notice(state: &AppState) -> Option<&BasesNotice> {
    match &state.notice {
        Some(AppNotice::Bases(n)) => Some(n),
        _ => None,
    }
}

#[tokio::test]
async fn a_pgn_with_several_games_takes_the_first_slot_without_the_chooser() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), "two.pgn", TWO);
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    assert!(state.chooser.is_none());
    assert_eq!(state.bases.current(), sel(1));
    let slot = state.bases.slot(sel(1)).expect("slot 1");
    assert_eq!((slot.kind, slot.read_only), (BaseKind::Pgn, true));
    assert_eq!((slot.total(), slot.filtered()), (2, 2));
    assert_eq!(slot.current_game, None);
    assert_eq!(state.prefs.files.recent.len(), 1);
    // The editor shows an empty game until one is loaded from the list.
    assert!(state.session.game().mainline().next().is_none());
}

#[tokio::test]
async fn a_pgn_with_one_game_loads_it_directly() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), "one.pgn", ONE);
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    assert_eq!(state.session.game().tags().white, "Solo");
    assert!(matches!(
        state.origin,
        GameOrigin::PgnFile {
            index: 0,
            total: 1,
            ..
        }
    ));
    assert!(!state.is_dirty());
    assert_eq!(
        state.bases.slot(sel(1)).expect("slot").current_game,
        Some(1)
    );
}

#[tokio::test]
async fn the_old_flow_still_opens_the_chooser_when_slots_are_off() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), "two.pgn", TWO);
    let mut state = AppState::default();
    state.prefs.files.recent.push(path);
    drive(&mut state, Command::OpenRecent(0)).await;
    assert!(state.chooser.is_some());
    assert_eq!(state.bases.iter().count(), 1);
}

#[tokio::test]
async fn opening_the_same_file_again_activates_its_slot() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), "two.pgn", TWO);
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path.clone())).await;
    drive(&mut state, BasesMessage::Select(SlotId::CLIPBASE)).await;
    assert_eq!(state.bases.current(), SlotId::CLIPBASE);
    // Another spelling of the same path.
    let other = dir.path().join(".").join("two.pgn");
    drive(&mut state, BasesMessage::Open(other)).await;
    assert_eq!(state.bases.current(), sel(1));
    assert_eq!(state.bases.iter().count(), 2);
}

#[tokio::test]
async fn with_every_slot_taken_nothing_new_opens() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    for n in 1..=8 {
        let path = write(dir.path(), &format!("g{n}.pgn"), TWO);
        drive(&mut state, BasesMessage::Open(path)).await;
    }
    assert_eq!(state.bases.first_free(), None);
    assert_eq!(state.bases.iter().count(), 9);
    let ninth = write(dir.path(), "g9.pgn", TWO);
    drive(&mut state, BasesMessage::Open(ninth)).await;
    assert_eq!(notice(&state), Some(&BasesNotice::NoFreeSlot));
    assert_eq!(state.bases.iter().count(), 9);
    // A file that is open still just activates.
    let again = dir.path().join("g3.pgn");
    drive(&mut state, BasesMessage::Open(again)).await;
    assert_eq!(state.bases.current(), sel(3));
}

#[tokio::test]
async fn failures_are_notices_and_si3_asks_for_an_upgrade() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(&mut state, BasesMessage::Open(dir.path().join("old.si3"))).await;
    assert_eq!(
        notice(&state),
        Some(&BasesNotice::Failed(BaseError::Si3NeedsUpgrade))
    );
    drive(&mut state, BasesMessage::Open(dir.path().join("none.pgn"))).await;
    assert!(matches!(
        notice(&state),
        Some(BasesNotice::Failed(BaseError::File(_)))
    ));
    assert_eq!(state.bases.iter().count(), 1);
    assert!(state.flow.open_base.is_none());
}

#[tokio::test]
async fn switching_keeps_the_game_of_each_slot_with_its_altered_flag() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), "one.pgn", ONE);
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    play(&mut state, "g1f3");
    assert!(state.is_dirty());

    drive(&mut state, BasesMessage::Select(SlotId::CLIPBASE)).await;
    assert!(!state.is_dirty());
    assert!(state.session.game().mainline().next().is_none());
    assert_eq!(state.origin, GameOrigin::New);
    play(&mut state, "d2d4");

    drive(&mut state, BasesMessage::Select(sel(1))).await;
    assert!(state.is_dirty());
    assert_eq!(state.session.game().tags().white, "Solo");
    assert_eq!(state.session.game().mainline().count(), 3);
    drive(&mut state, BasesMessage::Select(SlotId::CLIPBASE)).await;
    assert_eq!(state.session.game().mainline().count(), 1);
    assert!(state.is_dirty());
}

#[tokio::test]
async fn next_and_previous_cycle_over_the_open_slots() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    for n in 1..=2 {
        let path = write(dir.path(), &format!("g{n}.pgn"), TWO);
        drive(&mut state, BasesMessage::Open(path)).await;
    }
    assert_eq!(state.bases.current(), sel(2));
    drive(&mut state, BasesMessage::Next).await;
    assert_eq!(state.bases.current(), SlotId::CLIPBASE);
    drive(&mut state, BasesMessage::Next).await;
    assert_eq!(state.bases.current(), sel(1));
    drive(&mut state, BasesMessage::Previous).await;
    assert_eq!(state.bases.current(), SlotId::CLIPBASE);
    // An empty slot is ignored.
    drive(&mut state, BasesMessage::Select(sel(5))).await;
    assert_eq!(state.bases.current(), SlotId::CLIPBASE);
}

#[tokio::test]
async fn closing_the_current_slot_asks_when_altered_and_activates_the_clipbase() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), "one.pgn", ONE);
    let mut state = state();
    drive(&mut state, BasesMessage::Open(path)).await;
    play(&mut state, "g1f3");
    drive(&mut state, BasesMessage::Close(sel(1))).await;
    assert!(state.discard_prompt.is_some());
    assert!(state.bases.slot(sel(1)).is_some());

    drive(&mut state, Message::AnswerDiscard(DiscardChoice::Cancel)).await;
    assert!(state.bases.slot(sel(1)).is_some());
    assert!(state.is_dirty());

    drive(&mut state, BasesMessage::Close(sel(1))).await;
    drive(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave)).await;
    assert!(state.bases.slot(sel(1)).is_none());
    assert_eq!(state.bases.current(), SlotId::CLIPBASE);
    assert!(!state.is_dirty());
}

#[tokio::test]
async fn closing_another_slot_keeps_the_current_one_and_its_game() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    for n in 1..=2 {
        let path = write(dir.path(), &format!("g{n}.pgn"), TWO);
        drive(&mut state, BasesMessage::Open(path)).await;
    }
    play(&mut state, "e2e4");
    drive(&mut state, BasesMessage::Close(sel(1))).await;
    assert!(state.discard_prompt.is_none());
    assert!(state.bases.slot(sel(1)).is_none());
    assert_eq!(state.bases.current(), sel(2));
    assert!(state.is_dirty());
}

#[tokio::test]
async fn closing_an_altered_game_of_another_slot_brings_it_forward_to_ask() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "a.pgn", TWO)),
    )
    .await;
    play(&mut state, "e2e4");
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "b.pgn", TWO)),
    )
    .await;
    drive(&mut state, BasesMessage::Close(sel(1))).await;
    assert_eq!(state.bases.current(), sel(1));
    assert!(state.discard_prompt.is_some());
    drive(&mut state, Message::AnswerDiscard(DiscardChoice::Cancel)).await;
    assert!(state.bases.slot(sel(1)).is_some());
}

#[tokio::test]
async fn loading_from_a_list_activates_the_slot_and_asks_before_replacing() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "a.pgn", TWO)),
    )
    .await;
    drive(&mut state, BasesMessage::Select(SlotId::CLIPBASE)).await;
    play(&mut state, "e2e4");
    drive(
        &mut state,
        BasesMessage::LoadGame {
            slot: sel(1),
            number: 2,
        },
    )
    .await;
    // The altered game of the Clipbase is parked, so no question.
    assert!(state.discard_prompt.is_none());
    assert_eq!(state.bases.current(), sel(1));
    assert_eq!(state.session.game().tags().white, "C");
    assert_eq!(
        state.bases.slot(sel(1)).expect("slot").current_game,
        Some(2)
    );
    // Loading again over the same slot's altered game asks.
    play(&mut state, "d7d5");
    drive(
        &mut state,
        BasesMessage::LoadGame {
            slot: sel(1),
            number: 1,
        },
    )
    .await;
    assert!(state.discard_prompt.is_some());
    // Out of range numbers do nothing.
    drive(&mut state, Message::AnswerDiscard(DiscardChoice::Cancel)).await;
    drive(
        &mut state,
        BasesMessage::LoadGame {
            slot: sel(1),
            number: 9,
        },
    )
    .await;
    assert_eq!(state.session.game().tags().white, "C");
}

#[tokio::test]
async fn the_clipbase_copies_pastes_and_refuses_to_copy_itself() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    // In the Clipbase: copy is refused, paste does nothing.
    drive(&mut state, BasesMessage::ClipbaseCopy).await;
    assert_eq!(notice(&state), Some(&BasesNotice::AlreadyInClipbase));
    drive(&mut state, BasesMessage::ClipbasePaste).await;
    assert!(!state.is_dirty());

    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "one.pgn", ONE)),
    )
    .await;
    state.notice = None;
    drive(&mut state, BasesMessage::ClipbaseCopy).await;
    drive(&mut state, BasesMessage::ClipbaseCopy).await;
    let clip = state.bases.slot(SlotId::CLIPBASE).expect("clipbase");
    assert_eq!((clip.total(), clip.filtered()), (2, 2));
    assert_eq!(clip.current_game, Some(2));
    assert_eq!(state.bases.list_page(SlotId::CLIPBASE, 1, 10).len(), 2);

    // Paste replaces the game with the copy and counts it as altered.
    drive(&mut state, BasesMessage::Select(SlotId::CLIPBASE)).await;
    drive(&mut state, BasesMessage::Select(sel(1))).await;
    assert!(!state.is_dirty());
    drive(&mut state, BasesMessage::ClipbaseClear).await;
    assert_eq!(state.bases.slot(SlotId::CLIPBASE).expect("clip").total(), 0);
    drive(&mut state, BasesMessage::ClipbasePaste).await;
    assert_eq!(notice(&state), Some(&BasesNotice::ClipbaseEmpty));

    drive(&mut state, BasesMessage::ClipbaseCopy).await;
    drive(&mut state, BasesMessage::Select(SlotId::CLIPBASE)).await;
    assert_eq!(state.session.game().tags().white, "Solo");
    drive(&mut state, BasesMessage::Select(sel(1))).await;
    state.session.replace_game(mychess_core::Game::new());
    drive(&mut state, BasesMessage::ClipbasePaste).await;
    assert_eq!(state.session.game().tags().white, "Solo");
    assert!(state.is_dirty());
    assert_eq!(state.origin, GameOrigin::Clipboard);
}

#[tokio::test]
async fn the_clipbase_limit_refuses_without_corrupting() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    state.bases.set_clipbase_limit(2);
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "one.pgn", ONE)),
    )
    .await;
    for _ in 0..3 {
        drive(&mut state, BasesMessage::ClipbaseCopy).await;
    }
    assert_eq!(
        notice(&state),
        Some(&BasesNotice::Failed(BaseError::ClipbaseLimit { limit: 2 }))
    );
    let clip = state.bases.slot(SlotId::CLIPBASE).expect("clipbase");
    assert_eq!((clip.total(), clip.filtered()), (2, 2));
    assert_eq!(state.bases.list_page(SlotId::CLIPBASE, 1, 5).len(), 2);
}

#[tokio::test]
async fn closing_the_clipbase_only_clears_it() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "one.pgn", ONE)),
    )
    .await;
    drive(&mut state, BasesMessage::ClipbaseCopy).await;
    drive(&mut state, BasesMessage::Close(SlotId::CLIPBASE)).await;
    let clip = state.bases.slot(SlotId::CLIPBASE).expect("still there");
    assert_eq!(clip.total(), 0);
}

#[tokio::test]
async fn list_pages_come_from_the_filter_in_database_order() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "two.pgn", TWO)),
    )
    .await;
    let rows = state.bases.list_page(sel(1), 1, 10);
    let numbers: Vec<u32> = rows.iter().map(|r| r.number).collect();
    assert_eq!(numbers, [1, 2]);
    assert_eq!(rows[0].white, "A");
    assert_eq!(rows[0].black, "B");
    assert_eq!(rows[0].result, mychess_core::GameResult::White);
    // Two half moves are one full move (Scid: (half moves + 1) / 2).
    assert_eq!(rows[0].length, 1);
    assert_eq!(rows[1].country, "?");
    assert!(!rows[0].deleted && rows[0].opening.is_empty());
    // Second page, and a filter that removed a game.
    assert_eq!(state.bases.list_page(sel(1), 2, 10)[0].number, 2);
    state.bases.slot_mut(sel(1)).expect("slot").filter.set(1, 0);
    let rows = state.bases.list_page(sel(1), 1, 10);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].number, 2);
    assert!(state.bases.list_page(sel(1), 0, 10).is_empty());
    assert!(state.bases.list_page(sel(1), 5, 10).is_empty());
    assert!(state.bases.list_page(sel(7), 1, 10).is_empty());
}

#[tokio::test]
async fn list_state_is_kept_per_slot_and_clamped() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "two.pgn", TWO)),
    )
    .await;
    update(
        &mut state,
        BasesMessage::SetListFirst {
            slot: sel(1),
            first: 99,
        }
        .into(),
    );
    update(
        &mut state,
        BasesMessage::SelectRow {
            slot: sel(1),
            number: Some(2),
        }
        .into(),
    );
    update(
        &mut state,
        BasesMessage::SelectRow {
            slot: sel(1),
            number: Some(77),
        }
        .into(),
    );
    update(
        &mut state,
        BasesMessage::SetSearchText {
            slot: sel(1),
            text: "carlsen".to_owned(),
        }
        .into(),
    );
    let list = &state.bases.slot(sel(1)).expect("slot").list;
    assert_eq!((list.first, list.selected), (2, Some(2)));
    assert_eq!(list.search, "carlsen");
    assert_eq!(
        state.bases.slot(SlotId::CLIPBASE).expect("clip").list.first,
        1
    );
}
