//! In-crate tests of the crosstable flow that need the crate-private
//! `BaseSlot::data_changed` (names or ratings rewritten under a slot).

use std::fmt::Write as _;

use super::*;
use crate::bases::BasesMessage;
use crate::crosstable::CrosstableMessage;
use crate::message::Message;
use crate::{perform, update};

fn pgn() -> String {
    let mut out = String::new();
    for (round, white, black, result, moves) in [
        ("1", "A", "B", "1-0", "1. e4 e5 1-0"),
        ("2", "A", "C", "1/2-1/2", "1. d4 d5 1/2-1/2"),
        ("3", "B", "C", "1-0", "1. c4 c5 1-0"),
        ("4", "D", "E", "0-1", "1. Nf3 d5 0-1"),
    ] {
        let event = if round == "4" { "Open" } else { "Club" };
        let _ = write!(
            out,
            "[Event \"{event}\"]\n[Site \"Rio\"]\n[Date \"2020.05.01\"]\n[Round \"{round}\"]\n\
             [White \"{white}\"]\n[Black \"{black}\"]\n[Result \"{result}\"]\n\n{moves}\n\n"
        );
    }
    out
}

/// `update` plus the non-host effects run to the end (delays are dropped).
fn pump(state: &mut AppState, rt: &tokio::runtime::Runtime, message: Message) {
    let mut queue = vec![message];
    while let Some(message) = queue.pop() {
        for effect in update(state, message) {
            let delay = matches!(effect, Effect::Crosstable(CrosstableEffect::Delay { .. }));
            if !effect.runs_on_host() && !delay {
                queue.push(rt.block_on(perform(effect)));
            }
        }
    }
}

#[test]
fn set_filter_is_stale_when_the_games_changed_under_the_slot() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("t.pgn");
    std::fs::write(&path, pgn()).expect("write");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let mut state = AppState {
        open_pgn_in_slots: true,
        ..AppState::default()
    };
    pump(&mut state, &rt, BasesMessage::Open(path).into());
    let slot = state.bases.current();
    pump(
        &mut state,
        &rt,
        BasesMessage::LoadGame { slot, number: 1 }.into(),
    );
    pump(&mut state, &rt, CrosstableMessage::Open.into());
    assert!(state.crosstable_slot().is_some(), "the table is on screen");
    assert_eq!(state.bases.slot(slot).expect("slot").filtered(), 4);

    // Names are rewritten in place: same source, same total, other data.
    state.bases.slot_mut(slot).expect("slot").data_changed();
    update(&mut state, CrosstableMessage::SetFilter(None).into());
    assert_eq!(
        state.crosstable().notice,
        Some(CrosstableNotice::Stale),
        "the table is older than the data"
    );
    assert_eq!(
        state.bases.slot(slot).expect("slot").filtered(),
        4,
        "the filter was not touched"
    );
}
