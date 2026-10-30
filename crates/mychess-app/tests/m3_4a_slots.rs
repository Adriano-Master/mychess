//! Contract tests of the eight slots and the Clipbase (spec M3-4, rules 1-6;
//! Scid `file.tcl` 143-571, `tkscid.cpp` `sc_base_slot/open/close/switch`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

mod m3_4a_common;
use m3_4a_common::*;

fn rig_with(n: usize, dir: &std::path::Path) -> Rig {
    let mut rig = Rig::new();
    for i in 1..=n {
        let p = write(dir, &format!("g{i}.pgn"), TWO);
        rig.open(&p);
    }
    rig
}

#[test]
fn a_new_state_has_only_the_clipbase_in_position_nine_and_current() {
    let rig = Rig::new();
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    assert_eq!(rig.current().number(), 9);
    assert_eq!(rig.open_count(), 1);
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).expect("clipbase");
    assert_eq!(clip.kind, BaseKind::Clipbase);
    assert_eq!(clip.path, None);
    assert!(!clip.read_only);
}

#[test]
fn eight_distinct_files_take_slots_one_to_eight_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    for i in 1..=8 {
        let p = write(dir.path(), &format!("g{i}.pgn"), TWO);
        rig.open(&p);
        assert_eq!(rig.current(), sel(i), "file {i} must take slot {i}");
        assert_eq!(rig.state.bases.slot(sel(i)).unwrap().kind, BaseKind::Pgn);
    }
    assert_eq!(rig.open_count(), 9);
    assert_eq!(rig.state.bases.first_free(), None);
    assert!(rig.state.bases.slot(SlotId::CLIPBASE).is_some());
}

#[test]
fn the_ninth_file_reports_no_free_slot_and_opens_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_with(8, dir.path());
    rig.select(4);
    let ninth = write(dir.path(), "g9.pgn", TWO);
    rig.open(&ninth);
    assert_eq!(rig.notice(), Some(&BasesNotice::NoFreeSlot));
    assert_eq!(rig.open_count(), 9);
    assert_eq!(rig.current(), sel(4), "the current slot does not change");
    assert!(
        rig.state
            .bases
            .open_paths()
            .iter()
            .all(|p| !p.ends_with("g9.pgn")),
        "the ninth file must not be in any slot"
    );
    // No open stays pending: after closing a slot the ninth file opens.
    rig.send(BasesMessage::Close(sel(2)));
    rig.open(&ninth);
    assert_eq!(rig.current(), sel(2), "the freed slot is reused");
}

#[test]
fn opening_an_open_file_by_other_spellings_activates_the_slot() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let path = write(dir.path(), "a.pgn", TWO);
    let mut rig = Rig::new();
    rig.open(&path);
    rig.open(&write(dir.path(), "b.pgn", TWO));
    assert_eq!(rig.current(), sel(2));
    let spellings = [
        path.clone(),
        dir.path().join(".").join("a.pgn"),
        sub.join("..").join("a.pgn"),
        sub.join("..").join(".").join("a.pgn"),
    ];
    for spelling in spellings {
        rig.select(2);
        rig.open(&spelling);
        assert_eq!(
            rig.current(),
            sel(1),
            "{} must hit slot 1",
            spelling.display()
        );
        assert_eq!(
            rig.open_count(),
            3,
            "no duplicate for {}",
            spelling.display()
        );
    }
}

#[cfg(unix)]
#[test]
fn opening_a_symlink_to_an_open_file_activates_the_slot() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "a.pgn", TWO);
    let link = dir.path().join("link.pgn");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let mut rig = Rig::new();
    rig.open(&path);
    rig.select(9);
    rig.open(&link);
    assert_eq!(rig.current(), sel(1));
    assert_eq!(rig.open_count(), 2);
}

#[cfg(unix)]
#[test]
fn opening_a_hard_link_to_an_open_file_activates_the_slot() {
    // "Same physical file" (spec rule 3, D518: same device and inode).
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "a.pgn", TWO);
    let hard = dir.path().join("hard.pgn");
    std::fs::hard_link(&path, &hard).unwrap();
    let mut rig = Rig::new();
    rig.open(&path);
    rig.select(9);
    rig.open(&hard);
    assert_eq!(rig.current(), sel(1));
    assert_eq!(rig.open_count(), 2);
}

#[test]
fn select_of_an_empty_slot_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_with(2, dir.path());
    for n in 3..=8 {
        rig.send(BasesMessage::Select(sel(n)));
        assert_eq!(rig.current(), sel(2), "slot {n} is empty");
    }
}

#[test]
fn select_switches_to_any_open_slot_including_the_clipbase() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_with(3, dir.path());
    rig.select(1);
    assert_eq!(rig.current(), sel(1));
    rig.select(9);
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    rig.select(3);
    assert_eq!(rig.current(), sel(3));
    rig.select(3);
    assert_eq!(
        rig.current(),
        sel(3),
        "selecting the current slot is a no-op"
    );
}

#[test]
fn next_and_previous_cycle_over_open_slots_with_the_clipbase_last() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    // Files in slots 1, 2 and 3; close 2 so 1, 3 and the Clipbase remain.
    for i in 1..=3 {
        rig.open(&write(dir.path(), &format!("g{i}.pgn"), TWO));
    }
    rig.send(BasesMessage::Close(sel(2)));
    rig.select(1);
    let mut seen = Vec::new();
    for _ in 0..4 {
        rig.send(BasesMessage::Next);
        seen.push(rig.current().number());
    }
    assert_eq!(
        seen,
        [3, 9, 1, 3],
        "empty slot 2 is skipped; 9 comes after 3"
    );
    let mut back = Vec::new();
    for _ in 0..4 {
        rig.send(BasesMessage::Previous);
        back.push(rig.current().number());
    }
    assert_eq!(back, [1, 9, 3, 1]);
}

#[test]
fn next_and_previous_with_only_the_clipbase_stay_put() {
    let mut rig = Rig::new();
    rig.send(BasesMessage::Next);
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    rig.send(BasesMessage::Previous);
    assert_eq!(rig.current(), SlotId::CLIPBASE);
}

#[test]
fn bases_cycle_goes_through_open_slots_with_the_clipbase_last() {
    let dir = tempfile::tempdir().unwrap();
    let rig = rig_with(2, dir.path());
    let b = &rig.state.bases;
    assert_eq!(b.cycle(sel(1), true), sel(2));
    assert_eq!(b.cycle(sel(2), true), SlotId::CLIPBASE);
    assert_eq!(b.cycle(SlotId::CLIPBASE, true), sel(1));
    assert_eq!(b.cycle(sel(1), false), SlotId::CLIPBASE);
    assert_eq!(b.cycle(SlotId::CLIPBASE, false), sel(2));
    // From an empty position the cycle still moves to an open one.
    assert_eq!(b.cycle(sel(5), true), SlotId::CLIPBASE);
}

#[test]
fn closing_the_current_slot_activates_the_clipbase() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_with(3, dir.path());
    rig.select(2);
    rig.send(BasesMessage::Close(sel(2)));
    assert!(rig.state.bases.slot(sel(2)).is_none());
    assert_eq!(rig.current(), SlotId::CLIPBASE, "not slot 1 or 3");
    assert_eq!(rig.open_count(), 3);
}

#[test]
fn closing_another_slot_keeps_the_current_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_with(3, dir.path());
    rig.select(3);
    rig.send(BasesMessage::Close(sel(1)));
    assert!(rig.state.bases.slot(sel(1)).is_none());
    assert_eq!(rig.current(), sel(3));
    assert!(rig.state.discard_prompt.is_none());
    // The freed position is the first free one.
    assert_eq!(rig.state.bases.first_free(), Some(sel(1)));
}

#[test]
fn closing_an_empty_slot_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_with(1, dir.path());
    rig.send(BasesMessage::Close(sel(6)));
    assert_eq!(rig.open_count(), 2);
    assert_eq!(rig.current(), sel(1));
}

#[test]
fn the_clipbase_cannot_be_closed_only_reset() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "one.pgn", ONE));
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total(), 1);
    rig.send(BasesMessage::Close(SlotId::CLIPBASE));
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).expect("still there");
    assert_eq!((clip.total(), clip.filtered()), (0, 0));
    assert_eq!(
        rig.current(),
        sel(1),
        "closing the Clipbase does not switch"
    );
    assert!(rig.state.discard_prompt.is_none());
    // Closing it when it is the current one keeps it current.
    rig.select(9);
    rig.send(BasesMessage::Close(SlotId::CLIPBASE));
    assert_eq!(rig.current(), SlotId::CLIPBASE);
}

#[test]
fn closing_the_current_slot_with_an_altered_game_asks_and_cancel_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "one.pgn", ONE));
    rig.play("g1f3");
    assert!(rig.state.is_dirty());
    rig.send(BasesMessage::Close(sel(1)));
    assert!(rig.state.discard_prompt.is_some(), "must ask");
    assert!(
        rig.state.bases.slot(sel(1)).is_some(),
        "not closed before the answer"
    );
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(rig.state.bases.slot(sel(1)).is_some());
    assert_eq!(rig.current(), sel(1));
    assert!(rig.state.is_dirty(), "the edit survives Cancel");
    assert_eq!(rig.plies(), 3);
    assert!(rig.state.discard_prompt.is_none());
}

#[test]
fn closing_with_an_altered_game_and_dont_save_closes_and_activates_the_clipbase() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "one.pgn", ONE));
    rig.play("g1f3");
    rig.send(BasesMessage::Close(sel(1)));
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert!(rig.state.bases.slot(sel(1)).is_none());
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    assert!(!rig.state.is_dirty());
}

#[test]
fn closing_a_clean_slot_does_not_ask() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "one.pgn", ONE));
    assert!(!rig.state.is_dirty());
    rig.send(BasesMessage::Close(sel(1)));
    assert!(rig.state.discard_prompt.is_none());
    assert!(rig.state.bases.slot(sel(1)).is_none());
}

#[test]
fn closing_another_slot_with_an_altered_game_asks_and_cancel_keeps_it_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "a.pgn", ONE));
    rig.play("g1f3");
    rig.open(&write(dir.path(), "b.pgn", TWO));
    rig.send(BasesMessage::Close(sel(1)));
    assert!(
        rig.state.discard_prompt.is_some(),
        "the altered game must be asked about"
    );
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(rig.state.bases.slot(sel(1)).is_some());
    // Whichever slot is current now, the edited game is intact.
    rig.select(1);
    assert!(rig.state.is_dirty());
    assert_eq!(rig.plies(), 3);
}

#[test]
fn closing_a_slot_while_its_open_is_pending_does_not_corrupt_the_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_with(1, dir.path());
    let b = write(dir.path(), "b.pgn", TWO);
    let effects = rig.raw(BasesMessage::Open(b));
    assert_eq!(effects.len(), 1, "one OpenBase effect");
    // While b is loading, close slot 1.
    rig.send(BasesMessage::Close(sel(1)));
    let answers: Vec<_> = effects.into_iter().map(|e| rig.perform(e)).collect();
    for a in answers {
        rig.send(a);
    }
    // b opened in some free slot, a is gone, the Clipbase is intact.
    assert_eq!(rig.open_count(), 2);
    assert!(rig.state.bases.slot(SlotId::CLIPBASE).is_some());
    let paths = rig.state.bases.open_paths();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("b.pgn"));
}

#[test]
fn a_game_load_pending_for_a_closed_slot_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_with(1, dir.path());
    let effects = rig.raw(BasesMessage::LoadGame {
        slot: sel(1),
        number: 2,
    });
    assert_eq!(effects.len(), 1);
    rig.send(BasesMessage::Close(sel(1)));
    for e in effects {
        let a = rig.perform(e);
        rig.send(a);
    }
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    assert!(rig.state.bases.slot(sel(1)).is_none());
    assert!(
        rig.state.session.game().mainline().next().is_none(),
        "no game appears"
    );
    assert!(!rig.state.is_dirty());
}

#[test]
fn a_game_load_pending_for_a_slot_that_was_left_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_with(1, dir.path());
    let effects = rig.raw(BasesMessage::LoadGame {
        slot: sel(1),
        number: 2,
    });
    rig.select(9);
    for e in effects {
        let a = rig.perform(e);
        rig.send(a);
    }
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    assert!(
        rig.state.session.game().mainline().next().is_none(),
        "the game of slot 1 must not land in the Clipbase editor"
    );
}
