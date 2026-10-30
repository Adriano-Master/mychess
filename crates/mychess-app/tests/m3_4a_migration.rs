//! Migration of the M2 file flow to slots (spec M3-4, D503, D517): with
//! `open_pgn_in_slots` the "Open" and "Recent" commands open a slot; the old
//! flow (chooser) is unchanged when it is off.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

mod m3_4a_common;
use m3_4a_common::*;
use mychess_app::Command;

fn pick(rig: &mut Rig, host: &[Effect], path: &std::path::Path) {
    let spec = host
        .iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec.clone()),
            _ => None,
        })
        .expect("a PickFile effect");
    rig.send(Message::FilePicked(spec.request, Some(path.to_path_buf())));
}

#[test]
fn open_command_with_slots_opens_a_slot_for_a_multi_game_pgn_without_the_chooser() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "two.pgn", TWO);
    let mut rig = Rig::new();
    let host = rig.send(Command::Open);
    pick(&mut rig, &host, &path);
    assert!(rig.state.chooser.is_none());
    assert_eq!(rig.current(), sel(1));
    let slot = rig.state.bases.slot(sel(1)).unwrap();
    assert_eq!((slot.kind, slot.total()), (BaseKind::Pgn, 2));
    assert_eq!(rig.state.prefs.files.recent.len(), 1);
}

#[test]
fn open_command_with_slots_loads_a_single_game_pgn_directly() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "one.pgn", ONE);
    let mut rig = Rig::new();
    let host = rig.send(Command::Open);
    pick(&mut rig, &host, &path);
    assert!(rig.state.chooser.is_none());
    assert_eq!(rig.white(), "Solo");
    assert!(!rig.state.is_dirty());
    assert!(matches!(
        rig.state.origin,
        GameOrigin::PgnFile {
            index: 0,
            total: 1,
            ..
        }
    ));
    assert_eq!(rig.state.bases.slot(sel(1)).unwrap().current_game, Some(1));
}

#[test]
fn recent_files_with_slots_open_a_slot_and_keep_the_path_registered() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "two.pgn", TWO);
    let mut rig = Rig::new();
    rig.state.prefs.files.recent.push(path.clone());
    rig.send(Command::OpenRecent(0));
    assert!(rig.state.chooser.is_none());
    assert_eq!(rig.current(), sel(1));
    assert_eq!(rig.state.prefs.files.recent.len(), 1, "no duplicate entry");
    // Opening the same recent file again just activates the slot.
    rig.select(9);
    rig.send(Command::OpenRecent(0));
    assert_eq!(rig.current(), sel(1));
    assert_eq!(rig.open_count(), 2);
}

#[test]
fn recent_files_that_are_gone_report_and_leave_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let ghost = dir.path().join("gone.pgn");
    let mut rig = Rig::new();
    rig.state.prefs.files.recent.push(ghost);
    rig.send(Command::OpenRecent(0));
    assert_eq!(rig.open_count(), 1);
    assert!(rig.state.prefs.files.recent.is_empty());
    assert!(rig.state.chooser.is_none());
}

#[test]
fn recent_index_out_of_range_does_nothing() {
    let mut rig = Rig::new();
    rig.send(Command::OpenRecent(7));
    assert_eq!(rig.open_count(), 1);
}

#[test]
fn the_old_flow_still_uses_the_chooser_when_slots_are_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "two.pgn", TWO);
    let mut rig = Rig::new();
    rig.state.open_pgn_in_slots = false;
    rig.state.prefs.files.recent.push(path);
    rig.send(Command::OpenRecent(0));
    assert!(rig.state.chooser.is_some());
    assert_eq!(rig.open_count(), 1, "no slot is taken");
}

#[test]
fn the_flag_is_off_by_default() {
    assert!(!AppState::default().open_pgn_in_slots);
}

#[test]
fn bases_open_works_even_when_the_flag_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.state.open_pgn_in_slots = false;
    rig.open(&write(dir.path(), "two.pgn", TWO));
    assert_eq!(rig.current(), sel(1));
    assert!(rig.state.chooser.is_none());
}

#[test]
fn a_second_open_while_one_is_loading_does_not_replace_the_first() {
    // A second pick while the first open is still loading gets the
    // in-progress notice rather than silently replacing the first.
    let dir = tempfile::tempdir().unwrap();
    let a = write(dir.path(), "a.pgn", TWO);
    let b = write(dir.path(), "b.pgn", TWO);
    let mut rig = Rig::new();
    let first = rig.raw(BasesMessage::Open(a));
    let second = rig.raw(BasesMessage::Open(b.clone()));
    assert!(second.is_empty());
    for e in first {
        let m = rig.perform(e);
        rig.send(m);
    }
    assert_eq!(rig.open_count(), 2);
    assert!(
        rig.state
            .bases
            .find_by_path(&std::fs::canonicalize(&b).unwrap())
            .is_none()
    );
}
