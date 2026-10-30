//! Quitting with altered games (spec M3-4 D514 and the "pendencias da 4a").
//!
//! D514 says an altered game of any slot is asked about before it is lost;
//! the implementer lists "Quit does not ask about the games parked in other
//! slots" as a known pending item. These tests state the contract.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

mod m3_4a_common;
use m3_4a_common::*;

#[test]
fn quit_with_an_altered_current_game_asks_instead_of_exiting() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", ONE));
    rig.play("g1f3");
    let effects = rig.raw(Message::Quit);
    assert!(rig.state.discard_prompt.is_some());
    assert!(!effects.contains(&Effect::Exit));
}

#[test]
fn quit_with_a_clean_state_exits() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", ONE));
    let effects = rig.raw(Message::Quit);
    assert!(effects.contains(&Effect::Exit));
}

/// Known pending item of slice 4a: this test is expected to FAIL until `Quit`
/// asks about altered games parked in other slots.
#[test]
fn quit_with_an_altered_game_parked_in_another_slot_asks_before_exiting() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", ONE));
    rig.play("g1f3"); // altered game stays in slot 1 ...
    rig.open(&write(dir.path(), "b.pgn", TWO)); // ... while slot 2 is current
    assert_eq!(rig.current(), sel(2));
    assert!(!rig.state.is_dirty(), "the current editor is clean");
    assert_eq!(rig.state.bases.slots_with_altered_game(), [sel(1)]);
    let effects = rig.raw(Message::Quit);
    assert!(
        !effects.contains(&Effect::Exit),
        "quit must not exit and lose the altered game of slot 1"
    );
    assert!(rig.state.discard_prompt.is_some(), "the user must be asked");
}
