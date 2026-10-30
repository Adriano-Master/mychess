//! Contract tests of slice M3-4a that need the internals (`slot_mut`, the
//! filter of a slot). Derived from the spec rules 5, 7, 9 and 11.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::bases::{BasesMessage, SlotId};
use crate::effect::{Effect, perform};
use crate::message::Message;
use crate::state::{AppState, update};

const THREE: &str =
    "[White \"A\"]\n\n1. e4 *\n\n[White \"B\"]\n\n1. d4 *\n\n[White \"C\"]\n\n1. c4 *\n";

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    path
}

#[allow(clippy::field_reassign_with_default)]
fn state() -> AppState {
    let mut state = AppState::default();
    state.open_pgn_in_slots = true;
    state
}

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

fn sel(n: usize) -> SlotId {
    SlotId::from_number(n).expect("slot")
}

fn whites(state: &AppState, id: SlotId) -> Vec<String> {
    state
        .bases
        .list_page(id, 1, 100)
        .into_iter()
        .map(|r| r.white)
        .collect()
}

#[tokio::test]
async fn the_filter_of_one_slot_never_changes_the_list_of_another() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "a.pgn", THREE)),
    )
    .await;
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "b.pgn", THREE)),
    )
    .await;
    state.bases.slot_mut(sel(1)).expect("slot").filter.set(2, 0);
    assert_eq!(whites(&state, sel(1)), ["A", "C"]);
    assert_eq!(whites(&state, sel(2)), ["A", "B", "C"]);
    assert_eq!(state.bases.slot(sel(1)).expect("s").filtered(), 2);
    assert_eq!(state.bases.slot(sel(2)).expect("s").filtered(), 3);
    // The filters survive switching around.
    for n in [9, 1, 2, 9] {
        drive(&mut state, BasesMessage::Select(sel(n))).await;
    }
    assert_eq!(whites(&state, sel(1)), ["A", "C"]);
    assert_eq!(whites(&state, sel(2)), ["A", "B", "C"]);
}

#[tokio::test]
async fn a_reduced_filter_keeps_total_and_scroll_is_clamped_to_the_filter() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "a.pgn", THREE)),
    )
    .await;
    state.bases.slot_mut(sel(1)).expect("slot").filter.set(1, 0);
    state.bases.slot_mut(sel(1)).expect("slot").filter.set(2, 0);
    drive(
        &mut state,
        Message::Bases(BasesMessage::SetListFirst {
            slot: sel(1),
            first: 3,
        }),
    )
    .await;
    let slot = state.bases.slot(sel(1)).expect("slot");
    assert_eq!((slot.total(), slot.filtered()), (3, 1));
    assert_eq!(slot.list.first, 1, "max(1, filtered) = 1");
    // An empty filter: rule 12, scroll stays at 1.
    state.bases.slot_mut(sel(1)).expect("slot").filter.fill(0);
    drive(
        &mut state,
        Message::Bases(BasesMessage::SetListFirst {
            slot: sel(1),
            first: 9,
        }),
    )
    .await;
    assert_eq!(state.bases.slot(sel(1)).expect("slot").list.first, 1);
    assert!(state.bases.list_page(sel(1), 1, 10).is_empty());
}

#[tokio::test]
async fn load_game_outside_the_filter_still_loads_as_in_scid() {
    // Scid loads any valid number (1..=total); the filter only controls what
    // the list shows. Numbers of 0 or beyond the total are ignored.
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "a.pgn", THREE)),
    )
    .await;
    drive(&mut state, BasesMessage::Select(SlotId::CLIPBASE)).await;
    state.bases.slot_mut(sel(1)).expect("slot").filter.set(2, 0);
    drive(
        &mut state,
        BasesMessage::LoadGame {
            slot: sel(1),
            number: 2,
        },
    )
    .await;
    assert_eq!(state.bases.current(), sel(1));
    assert_eq!(state.session.game().tags().white, "B");
    let effects = update(
        &mut state,
        BasesMessage::LoadGame {
            slot: sel(1),
            number: 4,
        }
        .into(),
    );
    assert!(effects.is_empty(), "game 4 does not exist");
    let effects = update(
        &mut state,
        BasesMessage::LoadGame {
            slot: sel(1),
            number: 0,
        }
        .into(),
    );
    assert!(effects.is_empty());
}

#[tokio::test]
async fn select_row_outside_the_filter_is_ignored() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "a.pgn", THREE)),
    )
    .await;
    state.bases.slot_mut(sel(1)).expect("slot").filter.set(2, 0);
    drive(
        &mut state,
        BasesMessage::SelectRow {
            slot: sel(1),
            number: Some(2),
        },
    )
    .await;
    assert_eq!(state.bases.slot(sel(1)).expect("slot").list.selected, None);
    drive(
        &mut state,
        BasesMessage::SelectRow {
            slot: sel(1),
            number: Some(3),
        },
    )
    .await;
    assert_eq!(
        state.bases.slot(sel(1)).expect("slot").list.selected,
        Some(3)
    );
}

#[tokio::test]
async fn a_game_copied_to_the_clipbase_enters_its_filter_even_if_others_were_removed() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "a.pgn", THREE)),
    )
    .await;
    drive(
        &mut state,
        BasesMessage::LoadGame {
            slot: sel(1),
            number: 1,
        },
    )
    .await;
    drive(&mut state, BasesMessage::ClipbaseCopy).await;
    drive(&mut state, BasesMessage::ClipbaseCopy).await;
    state
        .bases
        .slot_mut(SlotId::CLIPBASE)
        .expect("clip")
        .filter
        .set(1, 0);
    drive(&mut state, BasesMessage::ClipbaseCopy).await;
    let clip = state.bases.slot(SlotId::CLIPBASE).expect("clip");
    assert_eq!((clip.total(), clip.filtered()), (3, 2));
    let numbers: Vec<u32> = state
        .bases
        .list_page(SlotId::CLIPBASE, 1, 10)
        .iter()
        .map(|r| r.number)
        .collect();
    assert_eq!(numbers, [2, 3]);
}

#[tokio::test]
async fn clear_resets_a_reduced_clipbase_filter_to_empty() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(
        &mut state,
        BasesMessage::Open(write(dir.path(), "a.pgn", THREE)),
    )
    .await;
    drive(
        &mut state,
        BasesMessage::LoadGame {
            slot: sel(1),
            number: 1,
        },
    )
    .await;
    drive(&mut state, BasesMessage::ClipbaseCopy).await;
    state
        .bases
        .slot_mut(SlotId::CLIPBASE)
        .expect("clip")
        .filter
        .set(1, 0);
    drive(&mut state, BasesMessage::ClipbaseClear).await;
    drive(&mut state, BasesMessage::ClipbaseCopy).await;
    let clip = state.bases.slot(SlotId::CLIPBASE).expect("clip");
    assert_eq!(
        (clip.total(), clip.filtered()),
        (1, 1),
        "a new game is back in the filter"
    );
}
