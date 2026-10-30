//! Contract tests of the game browser (slice M3-4e) that need the internals
//! (`slot_mut`: the ply stored in the filter; `close_slot`). Derived from the
//! spec (D597, D606) and the Scid (`browseGames` 98-101, `::gbrowser::load`).

use std::path::{Path, PathBuf};

use crate::bases::{BasesMessage, BrowseMessage, BrowseMode, GameStep, SlotId};
use crate::effect::{Effect, perform};
use crate::message::Message;
use crate::state::{AppState, update};
use mychess_db::FILTER_LAST_MOVE;

/// `n` games of six plies.
#[allow(clippy::format_push_string)]
fn pgn(n: usize) -> String {
    let mut text = String::new();
    for i in 1..=n {
        text.push_str(&format!(
            "[White \"W{i}\"]\n[Black \"B{i}\"]\n[Result \"1-0\"]\n\n\
             1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1-0\n\n"
        ));
    }
    text
}

fn write(dir: &Path, n: usize) -> PathBuf {
    let path = dir.join("g.pgn");
    std::fs::write(&path, pgn(n)).expect("write");
    path
}

#[allow(clippy::field_reassign_with_default)]
fn state() -> AppState {
    let mut state = AppState::default();
    state.open_pgn_in_slots = true;
    state
}

fn drive(state: &mut AppState, message: impl Into<Message>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let mut queue: Vec<Effect> = update(state, message.into());
    while let Some(effect) = queue.pop() {
        if effect.runs_on_host() || matches!(effect, Effect::BrowserTick { .. }) {
            continue;
        }
        let answer = rt.block_on(perform(effect));
        queue.extend(update(state, answer));
    }
}

fn open(state: &mut AppState, dir: &Path, n: usize) -> SlotId {
    drive(state, BasesMessage::Open(write(dir, n)));
    state.bases.current()
}

fn set_filter(state: &mut AppState, slot: SlotId, values: &[(u32, u8)]) {
    let slot = state.bases.slot_mut(slot).expect("slot");
    for (game, value) in values {
        assert!(slot.filter.set(*game, *value), "game {game}");
    }
}

fn browse(message: BrowseMessage) -> BasesMessage {
    BasesMessage::Browse(message)
}

fn plies(state: &AppState) -> Vec<usize> {
    state
        .browser
        .as_ref()
        .expect("open")
        .games
        .iter()
        .map(|g| g.ready().map_or(usize::MAX, super::ReadyGame::ply))
        .collect()
}

fn open_browser(state: &mut AppState, slot: SlotId, numbers: &[u32]) {
    drive(
        state,
        browse(BrowseMessage::Open {
            slot,
            numbers: numbers.to_vec(),
        }),
    );
}

// ---- the starting ply comes from the filter (single game) ----

#[test]
fn a_single_game_opens_at_the_ply_of_the_filter() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    let slot = open(&mut state, dir.path(), 6);
    // Scid: value v > 0 means ply v - 1; 0 (out of the filter) the start;
    // 255 the end of the game; beyond the end is the end.
    set_filter(
        &mut state,
        slot,
        &[
            (1, 4),
            (2, 7),
            (3, 100),
            (4, FILTER_LAST_MOVE),
            (5, 0),
            (6, 1),
        ],
    );
    let expected = [(1, 3), (2, 6), (3, 6), (4, 6), (5, 0), (6, 0)];
    for (game, ply) in expected {
        open_browser(&mut state, slot, &[game]);
        assert_eq!(plies(&state), [ply], "game {game}");
    }
}

#[test]
fn a_grid_opens_at_the_end_unless_the_filter_has_a_ply_inside_the_game() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    let slot = open(&mut state, dir.path(), 6);
    // browseGames: ply = filter ply; if > 0 then ply - 1; if > max or 0 -> max.
    set_filter(
        &mut state,
        slot,
        &[
            (1, 4),
            (2, 1),
            (3, 0),
            (4, 100),
            (5, FILTER_LAST_MOVE),
            (6, 2),
        ],
    );
    open_browser(&mut state, slot, &[1, 2, 3, 4, 5, 6]);
    assert_eq!(plies(&state), [3, 6, 6, 6, 6, 1]);
}

#[test]
fn changing_game_uses_the_ply_of_the_filter_of_the_new_game() {
    // D597: the Scid uses the old game's ply (apparent defect); we use the
    // new game's.
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    let slot = open(&mut state, dir.path(), 3);
    set_filter(&mut state, slot, &[(1, 3), (2, 5), (3, 1)]);
    open_browser(&mut state, slot, &[1]);
    assert_eq!(plies(&state), [2]);
    drive(&mut state, browse(BrowseMessage::GoToGame(GameStep::Next)));
    assert_eq!(plies(&state), [4], "ply of game 2");
    drive(&mut state, browse(BrowseMessage::GoToGame(GameStep::Next)));
    assert_eq!(plies(&state), [0], "ply of game 3");
    drive(&mut state, browse(BrowseMessage::GoToGame(GameStep::First)));
    assert_eq!(plies(&state), [2]);
}

#[test]
fn load_lands_at_the_ply_of_the_filter_when_nothing_moved() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    let slot = open(&mut state, dir.path(), 2);
    set_filter(&mut state, slot, &[(2, 5)]);
    open_browser(&mut state, slot, &[2]);
    drive(
        &mut state,
        browse(BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        }),
    );
    assert!(state.browser.is_none());
    let ply = state
        .session
        .game()
        .ply(state.session.cursor())
        .expect("ply");
    assert_eq!(ply, 4);
}

// ---- the slot of the browser (D606) ----

#[test]
fn closing_the_slot_of_the_browser_closes_it_and_another_slot_does_not() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    let first = open(&mut state, dir.path(), 4);
    let other = dir.path().join("h.pgn");
    std::fs::write(&other, pgn(3)).expect("write");
    drive(&mut state, BasesMessage::Open(other));
    let second = state.bases.current();
    assert_ne!(first, second);
    open_browser(&mut state, first, &[1, 2]);
    state.close_slot(second);
    assert!(state.browser.is_some(), "another slot closed");
    state.close_slot(first);
    assert!(state.browser.is_none());
    assert!(!state.modal_open());
}

#[test]
fn closing_the_slot_while_the_read_is_in_flight_cancels_it_and_drops_the_answer() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    let slot = open(&mut state, dir.path(), 4);
    let other = dir.path().join("h.pgn");
    std::fs::write(&other, pgn(3)).expect("write");
    drive(&mut state, BasesMessage::Open(other));
    let effects = update(
        &mut state,
        browse(BrowseMessage::Open {
            slot,
            numbers: vec![1],
        })
        .into(),
    );
    let Some(Effect::ReadBrowserGames { control, .. }) = effects.first().cloned() else {
        panic!("a read");
    };
    let effect = effects.into_iter().next().expect("effect");
    state.close_slot(slot);
    assert!(state.browser.is_none());
    assert!(control.is_cancelled());
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let answer = rt.block_on(perform(effect));
    update(&mut state, answer);
    assert!(state.browser.is_none());
}

#[test]
fn clearing_the_clipbase_under_its_browser_does_not_panic_or_load_ghosts() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    let slot = open(&mut state, dir.path(), 3);
    drive(&mut state, BasesMessage::LoadGame { slot, number: 1 });
    drive(&mut state, BasesMessage::ClipbaseCopy);
    drive(&mut state, BasesMessage::LoadGame { slot, number: 2 });
    drive(&mut state, BasesMessage::ClipbaseCopy);
    open_browser(&mut state, SlotId::CLIPBASE, &[1, 2]);
    assert_eq!(
        state.browser.as_ref().map(|b| b.mode),
        Some(BrowseMode::Grid)
    );
    state.close_slot(SlotId::CLIPBASE);
    let before = state.session.game().tags().white.clone();
    drive(
        &mut state,
        browse(BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        }),
    );
    if state.browser.is_some() {
        assert_eq!(state.session.game().tags().white, before);
    }
    drive(&mut state, browse(BrowseMessage::Close));
    assert!(state.browser.is_none());
}
