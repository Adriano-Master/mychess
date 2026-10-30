//! Contract tests of the "deleted" flag of the game list (spec M3-4, slice
//! 4c: D560, D561, criterion "Ctrl+Delete alterna a flag D"; Scid
//! `gamelist.tcl` `ToggleFlag` 1298, `Delete` 656).
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4c_common;

use std::path::{Path, PathBuf};

use m3_4c_common::*;
use mychess_db::GameSource;
use mychess_db::si4::{GameFlags, Si4Database, Si4Writer};

fn games() -> Vec<G> {
    vec![
        G::new("Echo", "x").moves("1. e4 e5 *"),
        G::new("alpha", "x").moves("1. d4 *"),
        G::new("Charlie", "x").moves("1. c4 c5 2. Nc3 *"),
        G::new("Bravo", "x").moves("1. Nf3 *"),
        G::new("delta", "x").moves("1. g3 g6 *"),
    ]
}

fn setup() -> (Rig, SlotId, PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path(), "db", &pgn(&games()));
    let mut rig = Rig::new();
    let slot = rig.open(&path);
    assert_eq!(
        rig.state.bases.slot(slot).expect("slot").kind,
        BaseKind::Si4
    );
    (rig, slot, path, dir)
}

/// The numbers marked deleted, read from the disk by an independent reader.
fn deleted_on_disk(path: &Path) -> Vec<u32> {
    let db = Si4Database::open(path).expect("open independent reader");
    (1..=GameSource::len(&db))
        .filter(|n| {
            db.entry(*n)
                .expect("entry")
                .flags
                .contains(GameFlags::DELETE)
        })
        .collect()
}

fn toggle(rig: &mut Rig, slot: SlotId) {
    rig.send(l(ListMessage::ToggleDeleted { slot }));
}

fn bytes(path: &Path, ext: &str) -> Vec<u8> {
    std::fs::read(path.with_extension(ext)).expect("read file")
}

// ---- the happy path ----

#[test]
fn toggling_marks_only_the_selected_games_on_disk() {
    let (mut rig, slot, path, _d) = setup();
    assert!(!rig.state.bases.slot(slot).expect("slot").read_only);
    rig.select_all(slot, &[2, 4]);
    toggle(&mut rig, slot);
    assert_eq!(deleted_on_disk(&path), [2, 4]);
    assert!(rig.state.list_job().is_none());
    assert_eq!(rig.notice(), None, "no error");
}

#[test]
fn the_rows_show_the_deleted_mark_after_the_slot_reloads() {
    let (mut rig, slot, _path, _d) = setup();
    rig.select_all(slot, &[3]);
    toggle(&mut rig, slot);
    let rows = rig.state.bases.list_page(slot, 1, 10);
    let flagged: Vec<u32> = rows
        .iter()
        .filter(|r| r.deleted)
        .map(|r| r.number)
        .collect();
    assert_eq!(flagged, [3]);
    assert!(
        rows.iter().all(|r| !r.flags.contains('D')),
        "D has its own column"
    );
}

#[test]
fn a_second_toggle_restores_the_flag_and_the_file_bytes() {
    let (mut rig, slot, path, _d) = setup();
    let before = bytes(&path, "si4");
    rig.select_all(slot, &[1, 5]);
    toggle(&mut rig, slot);
    assert_ne!(bytes(&path, "si4"), before);
    toggle(&mut rig, slot);
    assert_eq!(deleted_on_disk(&path), Vec::<u32>::new());
    assert_eq!(
        bytes(&path, "si4"),
        before,
        "the second toggle puts the index back byte for byte"
    );
}

#[test]
fn toggling_leaves_the_game_files_untouched_and_changes_few_index_bytes() {
    let (mut rig, slot, path, _d) = setup();
    let (si4, sg4, sn4) = (
        bytes(&path, "si4"),
        bytes(&path, "sg4"),
        bytes(&path, "sn4"),
    );
    rig.select_all(slot, &[2, 3]);
    toggle(&mut rig, slot);
    assert_eq!(bytes(&path, "sg4"), sg4, ".sg4 is not rewritten");
    assert_eq!(bytes(&path, "sn4"), sn4, ".sn4 is not rewritten");
    let after = bytes(&path, "si4");
    assert_eq!(after.len(), si4.len(), "the index keeps its size");
    let changed = si4.iter().zip(&after).filter(|(a, b)| a != b).count();
    // Only the flags byte of each toggled entry (2) may differ; the header
    // may also record something (a few bytes at most).
    assert!(changed >= 2, "the flags changed: {changed}");
    assert!(
        changed <= 2 + 8,
        "only the toggled entries (and at most the header) changed: {changed} bytes differ"
    );
}

#[test]
fn other_flags_of_a_game_are_not_changed_by_the_delete_toggle() {
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path(), "db", &pgn(&games()));
    // Give game 2 another flag with an independent writer.
    {
        let mut w = Si4Writer::open(&path).expect("writer");
        w.set_flag(2, GameFlags::WHITE_OPENING, mychess_db::si4::FlagOp::Set)
            .expect("set");
    }
    let before = Si4Database::open(&path)
        .expect("db")
        .entry(2)
        .expect("entry")
        .flags;
    let mut rig = Rig::new();
    let slot = rig.open(&path);
    rig.select_all(slot, &[2]);
    toggle(&mut rig, slot);
    let after = Si4Database::open(&path)
        .expect("db")
        .entry(2)
        .expect("entry")
        .flags;
    assert_eq!(after, before | GameFlags::DELETE);
    toggle(&mut rig, slot);
    let restored = Si4Database::open(&path)
        .expect("db")
        .entry(2)
        .expect("entry")
        .flags;
    assert_eq!(restored, before);
}

#[test]
fn the_list_keeps_filter_selection_position_and_sort_after_the_reload() {
    let (mut rig, slot, path, _d) = setup();
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    let view_before = rig.view(slot);
    assert_eq!(view_before, [2, 4, 3, 5, 1]);
    // Narrow the filter to 2, 3, 4, 5 and scroll.
    rig.pick(slot, 1, PickMode::Only);
    rig.send(l(ListMessage::RemoveSelected { slot }));
    rig.select_all(slot, &[3, 4]);
    rig.send(BasesMessage::SetListFirst { slot, first: 2 });
    let first = rig.first(slot);
    let source_before = rig.state.bases.slot(slot).expect("slot").source.clone();
    toggle(&mut rig, slot);
    let after = rig.state.bases.slot(slot).expect("slot");
    assert_ne!(after.source, source_before, "the source was reopened");
    assert_eq!(rig.filter_numbers(slot), [2, 3, 4, 5]);
    assert_eq!(rig.selection(slot), [3, 4]);
    assert_eq!(rig.first(slot), first);
    assert_eq!(
        rig.state.bases.slot(slot).expect("slot").sort(),
        Some((ListColumn::White, SortDir::Ascending))
    );
    assert_eq!(rig.view(slot), [2, 4, 3, 5]);
    assert_eq!(deleted_on_disk(&path), [3, 4]);
    assert_eq!(rig.state.bases.current(), slot);
}

#[test]
fn sorting_by_the_deleted_column_is_dropped_by_the_reload() {
    // D558: a reloaded database loses the sort by Deleted (the key changed).
    let (mut rig, slot, _path, _d) = setup();
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::Deleted,
    }));
    rig.select_all(slot, &[2]);
    toggle(&mut rig, slot);
    assert_eq!(rig.state.bases.slot(slot).expect("slot").sort(), None);
    assert_eq!(rig.view(slot), [1, 2, 3, 4, 5]);
}

#[test]
fn deleted_games_sort_after_the_others_and_the_flip_puts_them_first() {
    let (mut rig, slot, _path, _d) = setup();
    rig.select_all(slot, &[2, 4]);
    toggle(&mut rig, slot);
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::Deleted,
    }));
    assert_eq!(rig.view(slot), [1, 3, 5, 2, 4], "not deleted first (Scid)");
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::Deleted,
    }));
    assert_eq!(rig.view(slot), [4, 2, 5, 3, 1]);
}

#[test]
fn a_game_can_still_be_loaded_from_the_reopened_database() {
    let (mut rig, slot, _path, _d) = setup();
    rig.select_all(slot, &[3]);
    toggle(&mut rig, slot);
    rig.send(BasesMessage::LoadGame { slot, number: 3 });
    assert_eq!(rig.state.session.game().tags().white, "Charlie");
    assert_eq!(rig.state.session.game().mainline().count(), 3);
}

#[test]
fn flagging_every_game_of_a_larger_database_works() {
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path(), "big", &pgn(&numbered(700)));
    let mut rig = Rig::new();
    let slot = rig.open(&path);
    let all: Vec<u32> = (1..=700).collect();
    rig.select_all(slot, &all);
    toggle(&mut rig, slot);
    assert_eq!(deleted_on_disk(&path), all);
}

#[test]
fn the_flag_of_one_slot_does_not_touch_another_slot() {
    let dir = tempfile::tempdir().expect("dir");
    let a = make_si4(dir.path(), "a", &pgn(&games()));
    let b = make_si4(dir.path(), "b", &pgn(&games()));
    let mut rig = Rig::new();
    let sa = rig.open(&a);
    let sb = rig.open(&b);
    rig.select_all(sa, &[1]);
    rig.select_all(sb, &[2]);
    toggle(&mut rig, sa);
    assert_eq!(deleted_on_disk(&a), [1]);
    assert_eq!(deleted_on_disk(&b), Vec::<u32>::new());
    assert_eq!(rig.selection(sb), [2]);
    assert!(
        rig.state
            .bases
            .list_page(sb, 1, 10)
            .iter()
            .all(|r| !r.deleted)
    );
}

// ---- refusals ----

#[test]
fn nothing_selected_says_so_and_starts_no_job() {
    let (mut rig, slot, path, _d) = setup();
    let before = bytes(&path, "si4");
    let effects = rig.raw(l(ListMessage::ToggleDeleted { slot }));
    assert!(effects.is_empty());
    assert_eq!(rig.notice(), Some(&BasesNotice::NothingSelected));
    assert_eq!(bytes(&path, "si4"), before);
}

#[test]
fn a_pgn_database_refuses_the_flag() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "g.pgn", &games());
    rig.select_all(slot, &[1]);
    let effects = rig.raw(l(ListMessage::ToggleDeleted { slot }));
    assert!(effects.is_empty());
    assert_eq!(rig.notice(), Some(&BasesNotice::ReadOnly));
    assert!(rig.state.list_job().is_none());
}

#[test]
fn the_clipbase_refuses_the_flag() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "g.pgn", &games());
    rig.select_all(slot, &[1, 2]);
    rig.send(l(ListMessage::CopySelected { slot }));
    rig.select_all(SlotId::CLIPBASE, &[1]);
    rig.clear_notice();
    let effects = rig.raw(l(ListMessage::ToggleDeleted {
        slot: SlotId::CLIPBASE,
    }));
    assert!(effects.is_empty());
    assert_eq!(rig.notice(), Some(&BasesNotice::ReadOnly));
    assert!(
        rig.state
            .bases
            .list_page(SlotId::CLIPBASE, 1, 5)
            .iter()
            .all(|r| !r.deleted)
    );
}

#[test]
fn a_session_read_only_mark_blocks_the_flag_and_can_be_undone() {
    let (mut rig, slot, path, _d) = setup();
    let before = bytes(&path, "si4");
    rig.send(l(ListMessage::SetReadOnly {
        slot,
        read_only: true,
    }));
    assert!(rig.state.bases.slot(slot).expect("slot").read_only);
    rig.select_all(slot, &[1]);
    let effects = rig.raw(l(ListMessage::ToggleDeleted { slot }));
    assert!(effects.is_empty());
    assert_eq!(rig.notice(), Some(&BasesNotice::ReadOnly));
    assert_eq!(bytes(&path, "si4"), before, "nothing was written");
    rig.send(l(ListMessage::SetReadOnly {
        slot,
        read_only: false,
    }));
    toggle(&mut rig, slot);
    assert_eq!(deleted_on_disk(&path), [1]);
}

#[test]
fn the_read_only_mark_lives_only_in_the_session() {
    let (mut rig, slot, path, _d) = setup();
    rig.send(l(ListMessage::SetReadOnly {
        slot,
        read_only: true,
    }));
    rig.send(BasesMessage::Close(slot));
    let again = rig.open(&path);
    assert!(
        !rig.state.bases.slot(again).expect("slot").read_only,
        "reopening gives an editable database again"
    );
}

#[test]
fn the_read_only_mark_cannot_make_a_pgn_or_the_clipbase_writable() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "g.pgn", &games());
    rig.send(l(ListMessage::SetReadOnly {
        slot,
        read_only: false,
    }));
    assert!(rig.state.bases.slot(slot).expect("slot").read_only);
    rig.send(l(ListMessage::SetReadOnly {
        slot: SlotId::CLIPBASE,
        read_only: true,
    }));
    assert!(
        !rig.state
            .bases
            .slot(SlotId::CLIPBASE)
            .expect("clip")
            .read_only
    );
}

#[test]
fn a_database_whose_file_is_not_writable_opens_read_only_and_refuses_the_flag() {
    let dir = tempfile::tempdir().expect("dir");
    let path = make_si4(dir.path(), "ro", &pgn(&games()));
    let mut perms = std::fs::metadata(&path).expect("meta").permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms.clone()).expect("chmod");
    // A user that may write anyway (root) cannot show the case.
    let writable_anyway = std::fs::OpenOptions::new().append(true).open(&path).is_ok();
    let mut rig = Rig::new();
    let slot = rig.open(&path);
    let read_only = rig.state.bases.slot(slot).expect("slot").read_only;
    let before = bytes(&path, "si4");
    rig.select_all(slot, &[1]);
    toggle(&mut rig, slot);
    let unchanged = bytes(&path, "si4") == before;
    // Restore the permission so the temporary directory can be removed.
    #[allow(clippy::permissions_set_readonly_false)]
    {
        perms.set_readonly(false);
        std::fs::set_permissions(&path, perms).expect("restore permissions");
    }
    if !writable_anyway {
        assert!(read_only, "D560: an unwritable file opens read-only");
        assert!(unchanged, "nothing may be written");
        assert_eq!(rig.notice(), Some(&BasesNotice::ReadOnly));
    }
}

// ---- failures ----

#[test]
fn a_database_locked_by_another_writer_gives_a_typed_error_and_the_slot_stays_intact() {
    let (mut rig, slot, path, _d) = setup();
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    rig.select_all(slot, &[2, 3]);
    let source_before = rig.state.bases.slot(slot).expect("slot").source.clone();
    let view_before = rig.view(slot);
    let before = bytes(&path, "si4");
    let lock = Si4Writer::open(&path).expect("external writer");
    rig.clear_notice();
    toggle(&mut rig, slot);
    assert_eq!(
        rig.notice(),
        Some(&BasesNotice::Failed(BaseError::AlreadyOpen)),
        "typed error, not a panic"
    );
    assert!(rig.state.list_job().is_none(), "no job left pending");
    let after = rig.state.bases.slot(slot).expect("slot");
    assert_eq!(after.source, source_before, "the source was not replaced");
    assert_eq!(rig.selection(slot), [2, 3]);
    assert_eq!(rig.view(slot), view_before);
    drop(lock);
    assert_eq!(bytes(&path, "si4"), before, "nothing was written");
    // Once the other writer is gone the same request works.
    toggle(&mut rig, slot);
    assert_eq!(deleted_on_disk(&path), [2, 3]);
}

#[test]
fn the_database_file_vanishing_gives_an_error_and_keeps_the_slot() {
    let (mut rig, slot, path, _d) = setup();
    rig.select_all(slot, &[1]);
    for ext in ["si4", "sg4", "sn4"] {
        std::fs::remove_file(path.with_extension(ext)).expect("remove");
    }
    toggle(&mut rig, slot);
    assert!(
        matches!(rig.notice(), Some(BasesNotice::Failed(_))),
        "{:?}",
        rig.notice()
    );
    assert!(rig.state.list_job().is_none());
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 5);
    assert_eq!(rig.selection(slot), [1]);
}

#[test]
fn a_cancelled_flag_job_writes_nothing() {
    let (mut rig, slot, path, _d) = setup();
    let before = bytes(&path, "si4");
    rig.select_all(slot, &[1, 2]);
    let mut effects = rig.raw(l(ListMessage::ToggleDeleted { slot }));
    assert_eq!(
        rig.state.list_job().map(|(k, _)| k),
        Some(ListJobKind::Flags)
    );
    rig.raw(l(ListMessage::CancelJob));
    rig.clear_notice();
    let answer = rig.perform(effects.remove(0));
    rig.send(answer);
    assert_eq!(bytes(&path, "si4"), before);
    assert!(rig.state.list_job().is_none());
    assert_eq!(rig.notice(), None);
}

#[test]
fn a_flag_job_in_flight_blocks_other_list_actions() {
    let (mut rig, slot, _path, _d) = setup();
    rig.select_all(slot, &[1]);
    let _held = rig.raw(l(ListMessage::ToggleDeleted { slot }));
    rig.clear_notice();
    assert!(rig.raw(l(ListMessage::ToggleDeleted { slot })).is_empty());
    assert_eq!(rig.notice(), Some(&BasesNotice::ListBusy));
    rig.clear_notice();
    assert!(rig.raw(l(ListMessage::ResetFilter { slot })).is_empty());
    assert_eq!(rig.notice(), Some(&BasesNotice::ListBusy));
}

#[test]
fn closing_the_slot_during_a_flag_job_drops_the_answer() {
    let dir = tempfile::tempdir().expect("dir");
    let a = make_si4(dir.path(), "a", &pgn(&games()));
    let b = make_si4(dir.path(), "b", &pgn(&numbered(3)));
    let mut rig = Rig::new();
    let slot = rig.open(&a);
    rig.select_all(slot, &[1]);
    let mut effects = rig.raw(l(ListMessage::ToggleDeleted { slot }));
    let answer = rig.perform(effects.remove(0));
    rig.send(BasesMessage::Close(slot));
    let again = rig.open(&b);
    assert_eq!(again, slot);
    let source = rig.state.bases.slot(slot).expect("slot").source.clone();
    rig.send(answer);
    let now = rig.state.bases.slot(slot).expect("slot");
    assert_eq!(now.source, source, "the stale reopen is not installed");
    assert_eq!(now.total(), 3);
}
