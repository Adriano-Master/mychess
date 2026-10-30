//! Contract tests of the current game kept per slot (spec M3-4, rules 5, 6,
//! 9, 11; D514, D519; Scid: each base keeps its own game, `SwitchToBase`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

mod m3_4a_common;
use m3_4a_common::*;

#[test]
fn switching_away_and_back_keeps_game_cursor_and_altered_flag() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", ONE));
    rig.play("g1f3");
    rig.send(GameMessage::Navigate(Nav::Start));
    let cursor = rig.state.session.cursor();
    rig.open(&write(dir.path(), "b.pgn", TWO));
    assert_eq!(rig.current(), sel(2));
    assert!(!rig.state.is_dirty(), "slot B has its own (unaltered) game");
    assert_eq!(rig.plies(), 0);
    rig.select(1);
    assert!(rig.state.is_dirty());
    assert_eq!(rig.white(), "Solo");
    assert_eq!(rig.plies(), 3);
    assert_eq!(rig.state.session.cursor(), cursor);
}

#[test]
fn every_slot_keeps_its_own_edit_through_a_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", ONE));
    rig.play("g1f3");
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.play("e2e4");
    rig.play("e7e5");
    rig.open(&write(dir.path(), "b.pgn", ONE));
    rig.play("d2d4");
    let expected = [(1, 3), (9, 2), (2, 3)];
    for _ in 0..2 {
        for (slot, plies) in expected {
            rig.select(slot);
            assert_eq!(rig.plies(), plies, "slot {slot}");
            assert!(rig.state.is_dirty(), "slot {slot} stays altered");
        }
    }
}

#[test]
fn switching_slots_never_asks_about_the_altered_game() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", ONE));
    rig.play("g1f3");
    for n in [9, 1, 9] {
        rig.select(n);
        assert!(rig.state.discard_prompt.is_none());
    }
}

#[test]
fn next_and_previous_also_keep_the_altered_games() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", ONE));
    rig.play("g1f3");
    rig.send(BasesMessage::Next);
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    rig.send(BasesMessage::Previous);
    assert_eq!(rig.current(), sel(1));
    assert!(rig.state.is_dirty());
    assert_eq!(rig.plies(), 3);
}

#[test]
fn a_clean_loaded_game_stays_clean_after_a_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", TWO));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 2,
    });
    rig.select(9);
    rig.select(1);
    assert!(!rig.state.is_dirty());
    assert_eq!(rig.white(), "C");
    assert_eq!(rig.state.bases.slot(sel(1)).unwrap().current_game, Some(2));
}

#[test]
fn load_game_activates_its_slot_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", TWO));
    rig.open(&write(dir.path(), "b.pgn", TWO));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 2,
    });
    assert_eq!(rig.current(), sel(1));
    assert_eq!(rig.white(), "C");
}

#[test]
fn loading_over_an_altered_game_of_the_same_slot_asks_and_cancel_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", TWO));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    rig.play("g1f3");
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 2,
    });
    assert!(rig.state.discard_prompt.is_some());
    assert_eq!(rig.white(), "A", "not replaced before the answer");
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(rig.white(), "A");
    assert!(rig.state.is_dirty());
    assert_eq!(rig.state.bases.slot(sel(1)).unwrap().current_game, Some(1));
    // DontSave now loads game 2.
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 2,
    });
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(rig.white(), "C");
    assert!(!rig.state.is_dirty());
    assert_eq!(rig.state.bases.slot(sel(1)).unwrap().current_game, Some(2));
}

#[test]
fn loading_from_another_slot_does_not_touch_the_altered_game_left_behind() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", ONE));
    rig.play("g1f3");
    rig.open(&write(dir.path(), "b.pgn", TWO));
    rig.send(BasesMessage::LoadGame {
        slot: sel(2),
        number: 2,
    });
    assert!(rig.state.discard_prompt.is_none());
    assert_eq!(rig.white(), "C");
    rig.select(1);
    assert!(rig.state.is_dirty());
    assert_eq!(rig.plies(), 3);
}

#[test]
fn a_game_loaded_from_the_clipbase_uses_origin_new() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", ONE));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    rig.send(BasesMessage::ClipbaseCopy);
    rig.send(BasesMessage::LoadGame {
        slot: SlotId::CLIPBASE,
        number: 1,
    });
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    assert_eq!(rig.white(), "Solo");
    assert_eq!(rig.state.origin, GameOrigin::New);
    assert!(!rig.state.is_dirty());
}

#[test]
fn the_list_state_is_independent_per_slot_and_survives_switching() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", TWO));
    rig.open(&write(dir.path(), "b.pgn", TWO));
    rig.send(BasesMessage::SetListFirst {
        slot: sel(1),
        first: 2,
    });
    rig.send(BasesMessage::SelectRow {
        slot: sel(1),
        number: Some(2),
    });
    rig.send(BasesMessage::SetSearchText {
        slot: sel(1),
        text: "x".into(),
    });
    rig.select(9);
    rig.select(2);
    let a = &rig.state.bases.slot(sel(1)).unwrap().list;
    let b = &rig.state.bases.slot(sel(2)).unwrap().list;
    assert_eq!((a.first, a.selected, a.search.as_str()), (2, Some(2), "x"));
    assert_eq!((b.first, b.selected, b.search.as_str()), (1, None, ""));
}

#[test]
fn list_state_messages_are_clamped_and_ignore_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", TWO));
    rig.send(BasesMessage::SetListFirst {
        slot: sel(1),
        first: u32::MAX,
    });
    assert_eq!(
        rig.state.bases.slot(sel(1)).unwrap().list.first,
        2,
        "max(1, filtered)"
    );
    rig.send(BasesMessage::SetListFirst {
        slot: sel(1),
        first: 0,
    });
    assert_eq!(rig.state.bases.slot(sel(1)).unwrap().list.first, 1);
    rig.send(BasesMessage::SelectRow {
        slot: sel(1),
        number: Some(2),
    });
    for bad in [0, 3, u32::MAX] {
        rig.send(BasesMessage::SelectRow {
            slot: sel(1),
            number: Some(bad),
        });
        assert_eq!(
            rig.state.bases.slot(sel(1)).unwrap().list.selected,
            Some(2),
            "row {bad} is not in the filter"
        );
    }
    rig.send(BasesMessage::SelectRow {
        slot: sel(1),
        number: None,
    });
    assert_eq!(rig.state.bases.slot(sel(1)).unwrap().list.selected, None);
    // Empty slots: ignored, no panic.
    rig.send(BasesMessage::SetListFirst {
        slot: sel(6),
        first: 4,
    });
    rig.send(BasesMessage::SelectRow {
        slot: sel(6),
        number: Some(1),
    });
    rig.send(BasesMessage::SetSearchText {
        slot: sel(6),
        text: "y".into(),
    });
    assert!(rig.state.bases.slot(sel(6)).is_none());
}

#[test]
fn a_slot_closed_and_reopened_starts_with_a_fresh_list_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let path = write(dir.path(), "a.pgn", TWO);
    rig.open(&path);
    rig.send(BasesMessage::SetListFirst {
        slot: sel(1),
        first: 2,
    });
    rig.send(BasesMessage::Close(sel(1)));
    rig.open(&path);
    let slot = rig.state.bases.slot(sel(1)).unwrap();
    assert_eq!(slot.list.first, 1);
    assert_eq!(slot.current_game, None);
}

#[test]
fn trial_mode_ends_when_switching_slots() {
    // Scid: SwitchToBase ends trial mode (spec "Slots" item 3).
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", ONE));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    rig.send(GameMessage::EnterTrial { null_move: false });
    assert!(rig.state.session.in_trial());
    rig.select(9);
    assert!(!rig.state.session.in_trial());
    rig.select(1);
    assert!(!rig.state.session.in_trial());
    assert_eq!(rig.plies(), 2, "trial moves are not kept");
}
