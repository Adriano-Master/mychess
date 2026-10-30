//! M7-1b contract: flags in batch, the header fields, what each kind of
//! database allows, one job at a time, Stop, a file that changed (spec
//! `m7-1-manutencao.md`, criteria 23 to 26, sections 3 and 4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m7_1b_common;

use m7_1b_common::*;
use mychess_db::GameSource;
use mychess_db::si4::Si4Database;
use tempfile::tempdir;

fn flags(scope: FlagScope, flag: MaintFlag, action: FlagAction) -> MaintRequest {
    MaintRequest::Flags {
        scope,
        flag,
        action,
    }
}

fn run_effects(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Maintenance(MaintenanceEffect::Run { .. })))
        .count()
}

#[test]
fn a_flag_batch_returns_one_effect_and_writes_nothing_in_update() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "six", &six());
    let before = files_of(&path);
    let effects = rig.raw(MaintenanceMessage::Start(flags(
        FlagScope::All,
        MaintFlag::Delete,
        FlagAction::Set,
    )));
    assert_eq!(run_effects(&effects), 1, "{effects:?}");
    assert_eq!(effects.len(), 1, "only the job: {effects:?}");
    assert_eq!(files_of(&path), before, "update did no I/O");
    assert_eq!(rig.state.maintenance_job(), Some(MaintJobKind::Flags));
    // The work is the effect's.
    let answer = rig.perform(effects.into_iter().next().unwrap());
    assert_ne!(files_of(&path), before, "the effect wrote the flags");
    rig.raw(answer);
    assert_eq!(disk_deleted(&path), vec![true; 6]);
    assert_eq!(rig.state.maintenance_job(), None);
}

#[test]
fn flags_of_the_filter_reach_only_the_filter_games_and_a_second_run_changes_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "six", &six());
    rig.select(slot, &[1, 3, 5]);
    rig.send(BasesMessage::List(ListMessage::KeepSelected { slot }));
    assert_eq!(rig.filter_numbers(slot), vec![1, 3, 5]);
    let tactics = mychess_db::si4::GameFlags::TACTICS.bits();
    rig.ask(flags(
        FlagScope::Filter,
        MaintFlag::Tactics,
        FlagAction::Set,
    ));
    assert_eq!(
        rig.maint_notice(),
        Some(MaintNotice::FlagsDone { changed: 3 })
    );
    let on_disk = disk_flags(&path);
    let marked: Vec<bool> = on_disk.iter().map(|f| f & tactics != 0).collect();
    assert_eq!(marked, vec![true, false, true, false, true, false]);
    rig.ask(flags(
        FlagScope::Filter,
        MaintFlag::Tactics,
        FlagAction::Set,
    ));
    assert_eq!(
        rig.maint_notice(),
        Some(MaintNotice::FlagsDone { changed: 0 })
    );
    assert_eq!(disk_flags(&path), on_disk, "nothing was rewritten");
}

#[test]
fn the_all_scope_ignores_the_filter_and_unmarking_clears_the_flag() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "six", &six());
    rig.select(slot, &[2]);
    rig.send(BasesMessage::List(ListMessage::KeepSelected { slot }));
    rig.ask(flags(FlagScope::All, MaintFlag::Custom(2), FlagAction::Set));
    let custom2 = mychess_db::si4::GameFlags::CUSTOM2.bits();
    assert!(disk_flags(&path).iter().all(|f| f & custom2 != 0));
    rig.ask(flags(
        FlagScope::All,
        MaintFlag::Custom(2),
        FlagAction::Clear,
    ));
    assert!(disk_flags(&path).iter().all(|f| f & custom2 == 0));
    // The filter and the games did not move.
    assert_eq!(rig.filter_numbers(slot), vec![2]);
    assert_eq!(disk_games(&path).len(), 6);
}

#[test]
fn the_current_scope_needs_a_game_of_this_database() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "six", &six());
    let effects = rig.raw(MaintenanceMessage::Start(flags(
        FlagScope::Current,
        MaintFlag::Delete,
        FlagAction::Set,
    )));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(rig.maint_notice(), Some(MaintNotice::NoCurrentGame));
    rig.send(BasesMessage::LoadGame { slot, number: 4 });
    rig.ask(flags(
        FlagScope::Current,
        MaintFlag::Delete,
        FlagAction::Toggle,
    ));
    assert_eq!(
        disk_deleted(&path),
        vec![false, false, false, true, false, false]
    );
}

#[test]
fn a_custom_flag_out_of_range_is_refused_without_a_job() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "six", &six());
    let effects = rig.raw(MaintenanceMessage::Start(flags(
        FlagScope::All,
        MaintFlag::Custom(7),
        FlagAction::Set,
    )));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(rig.maint_notice(), Some(MaintNotice::InvalidValue));
    assert_eq!(rig.state.maintenance_job(), None);
}

// ---- the header ----

fn header_of(path: &std::path::Path) -> mychess_db::si4::Header {
    GameSource::header(&Si4Database::open(path).unwrap()).clone()
}

#[test]
fn the_header_fields_are_written_and_the_games_stay() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "six", &six());
    let games = disk_games(&path);
    rig.ask(MaintRequest::Header(HeaderEdit::BaseType(5)));
    rig.ask(MaintRequest::Header(HeaderEdit::Description(
        "My club".to_owned(),
    )));
    rig.ask(MaintRequest::Header(HeaderEdit::CustomFlag {
        number: 3,
        text: "Review".to_owned(),
    }));
    rig.ask(MaintRequest::Header(HeaderEdit::AutoLoad(3)));
    let header = header_of(&path);
    assert_eq!(header.base_type(), 5);
    assert_eq!(header.description(), b"My club");
    assert_eq!(header.custom_flag_descriptions()[2], b"Review");
    assert_eq!(header.auto_load(), Some(3));
    assert_eq!(disk_games(&path), games);
    // The window reads the same values from the slot reopened.
    let view = rig.state.maintenance_view();
    let shown = view.header.expect("header");
    assert_eq!(shown.description, "My club");
    assert_eq!(shown.custom_flags[2], "Review");
    assert_eq!(shown.auto_load, Some(3));
    assert_eq!(view.slot, slot);
}

#[test]
fn the_autoload_text_is_checked_before_anything_is_written() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "six", &six());
    let before = files_of(&path);
    for text in ["abc", "", "-1", "16777215", "99999999999"] {
        let effects = rig.raw(MaintenanceMessage::AutoLoadText(text.to_owned()));
        assert!(effects.is_empty(), "{text}: {effects:?}");
        assert_eq!(
            rig.maint_notice(),
            Some(MaintNotice::InvalidValue),
            "{text}"
        );
    }
    assert_eq!(files_of(&path), before);
    rig.send(MaintenanceMessage::AutoLoadText(" 2 ".to_owned()));
    assert_eq!(header_of(&path).auto_load(), Some(2));
    rig.send(MaintenanceMessage::AutoLoadText("0".to_owned()));
    assert_eq!(header_of(&path).auto_load(), None, "0 is no game");
}

#[test]
fn the_system_types_are_not_chosen_by_hand_and_a_type_above_31_is_refused() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "six", &six());
    for value in [1, 2, 3, 32, 4000] {
        let effects = rig.raw(MaintenanceMessage::Start(MaintRequest::Header(
            HeaderEdit::BaseType(value),
        )));
        assert!(effects.is_empty(), "{value}: {effects:?}");
        assert_eq!(
            rig.maint_notice(),
            Some(MaintNotice::InvalidValue),
            "{value}"
        );
    }
    assert_ne!(header_of(&path).base_type(), 2);
    let effects = rig.raw(MaintenanceMessage::Start(MaintRequest::Header(
        HeaderEdit::CustomFlag {
            number: 0,
            text: String::new(),
        },
    )));
    assert!(effects.is_empty());
}

// ---- what each kind of database allows ----

#[test]
fn the_table_of_access_follows_the_kind_of_database() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (si4, _) = rig.open_si4(dir.path(), "six", &six());
    let a = rig.state.maintenance_access(si4);
    assert!(a.flags && a.header && a.compact && a.sort && a.twins && a.check);
    assert!(a.strip && a.tag_list && a.tag_edit && a.names && a.cleaner && a.repair);
    // Read-only (marked): no writes, no sort (D3517), the check stays.
    rig.send(BasesMessage::List(ListMessage::SetReadOnly {
        slot: si4,
        read_only: true,
    }));
    let a = rig.state.maintenance_access(si4);
    assert!(!a.flags && !a.header && !a.compact && !a.sort && !a.twins);
    assert!(!a.strip && !a.tag_list && !a.tag_edit && !a.names && !a.cleaner);
    assert!(a.check && a.repair && a.filter_deleted);
    // A PGN file is the same.
    let pgn = rig.open_pgn(dir.path(), "six.pgn", &six());
    let a = rig.state.maintenance_access(pgn);
    assert!(!a.flags && !a.header && !a.compact && !a.sort && !a.twins && !a.cleaner);
    assert!(a.check && a.repair);
    // The Clipbase: flags, duplicates, tags to read; no compaction or Cleaner.
    let a = rig.state.maintenance_access(SlotId::CLIPBASE);
    assert!(a.flags && a.twins && a.tag_list && a.check);
    assert!(!a.header && !a.compact && !a.sort && !a.cleaner && !a.strip && !a.names);
    // An empty slot: only the repair, which needs no database.
    let empty = rig.state.maintenance_access(SlotId::new(6).unwrap());
    assert!(empty.repair);
    assert!(!empty.flags && !empty.check && !empty.sort && !empty.cleaner);
}

#[test]
fn a_read_only_pgn_or_clipbase_refuses_with_the_right_notice_and_no_effect() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (si4, path) = rig.open_si4(dir.path(), "six", &six());
    let before = files_of(&path);
    rig.send(BasesMessage::List(ListMessage::SetReadOnly {
        slot: si4,
        read_only: true,
    }));
    let ops = [
        flags(FlagScope::All, MaintFlag::Delete, FlagAction::Set),
        MaintRequest::Header(HeaderEdit::AutoLoad(1)),
        MaintRequest::Compact {
            names: true,
            games: true,
        },
        MaintRequest::Sort {
            keys: vec![SortCriterion::Year],
            reverse: false,
            compact_after: false,
        },
        MaintRequest::FindTwins,
        MaintRequest::Cleaner,
        MaintRequest::Strip {
            what: StripChoice::Comments,
            scope: ScopeChoice::All,
        },
        MaintRequest::Tags(TagRequest::Strip("Annotator".to_owned())),
    ];
    for op in &ops {
        let effects = rig.raw(MaintenanceMessage::Start(op.clone()));
        assert!(effects.is_empty(), "{op:?}: {effects:?}");
        assert_eq!(rig.maint_notice(), Some(MaintNotice::ReadOnly), "{op:?}");
    }
    assert_eq!(files_of(&path), before);
    // A PGN is told apart from a read-only database.
    rig.open_pgn(dir.path(), "six.pgn", &six());
    for op in &ops {
        let effects = rig.raw(MaintenanceMessage::Start(op.clone()));
        assert!(effects.is_empty(), "{op:?}: {effects:?}");
        assert_eq!(rig.maint_notice(), Some(MaintNotice::Pgn), "{op:?}");
    }
    // The Clipbase takes the flags but not the rest.
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    for op in [
        MaintRequest::Compact {
            names: true,
            games: true,
        },
        MaintRequest::Cleaner,
        MaintRequest::Header(HeaderEdit::AutoLoad(1)),
        MaintRequest::Sort {
            keys: vec![SortCriterion::Year],
            reverse: false,
            compact_after: false,
        },
        MaintRequest::Strip {
            what: StripChoice::Comments,
            scope: ScopeChoice::All,
        },
    ] {
        let effects = rig.raw(MaintenanceMessage::Start(op.clone()));
        assert!(effects.is_empty(), "{op:?}: {effects:?}");
        assert_eq!(
            rig.maint_notice(),
            Some(MaintNotice::ClipbaseUnsupported),
            "{op:?}"
        );
    }
}

#[test]
fn the_clipbase_takes_flags_in_memory() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    rig.select(slot, &[2, 4, 5]);
    rig.send(BasesMessage::List(ListMessage::CopySelected { slot }));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    let clip = SlotId::CLIPBASE;
    assert_eq!(rig.state.bases.slot(clip).unwrap().total(), 3);
    let rev = rig.state.bases.slot(clip).unwrap().data_rev();
    rig.ask(flags(FlagScope::All, MaintFlag::Delete, FlagAction::Set));
    assert_eq!(
        rig.maint_notice(),
        Some(MaintNotice::FlagsDone { changed: 3 })
    );
    let source = rig.state.bases.slot(clip).unwrap().source.clone();
    let deleted: Vec<bool> = (1..=3)
        .map(|n| {
            source
                .read()
                .entry(n)
                .unwrap()
                .flags
                .contains(mychess_db::si4::GameFlags::DELETE)
        })
        .collect();
    assert_eq!(deleted, vec![true, true, true]);
    assert_ne!(
        rig.state.bases.slot(clip).unwrap().data_rev(),
        rev,
        "the windows that follow the games are told"
    );
    // The first game only, again: nothing changes.
    rig.ask(flags(FlagScope::All, MaintFlag::Delete, FlagAction::Set));
    assert_eq!(
        rig.maint_notice(),
        Some(MaintNotice::FlagsDone { changed: 0 })
    );
}

// ---- one job at a time, Stop, a file that changed ----

#[test]
fn a_second_operation_while_one_runs_is_refused_with_list_busy() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "six", &six());
    let first = rig.raw(MaintenanceMessage::Start(flags(
        FlagScope::All,
        MaintFlag::Delete,
        FlagAction::Set,
    )));
    assert_eq!(run_effects(&first), 1);
    let second = rig.raw(MaintenanceMessage::Start(flags(
        FlagScope::All,
        MaintFlag::Tactics,
        FlagAction::Set,
    )));
    assert!(second.is_empty(), "{second:?}");
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Bases(BasesNotice::ListBusy))
    );
    // The list refuses its own jobs meanwhile too.
    let sorted = rig.raw(MaintenanceMessage::Start(MaintRequest::Sort {
        keys: vec![SortCriterion::Year],
        reverse: false,
        compact_after: false,
    }));
    assert!(sorted.is_empty());
    let answer = rig.perform(first.into_iter().next().unwrap());
    rig.raw(answer);
    assert_eq!(disk_deleted(&path), vec![true; 6]);
    // Free again.
    rig.ask(flags(FlagScope::All, MaintFlag::Tactics, FlagAction::Set));
    assert_eq!(
        rig.maint_notice(),
        Some(MaintNotice::FlagsDone { changed: 6 })
    );
}

#[test]
fn stop_cancels_the_job_and_the_files_stay_as_they_were() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "six", &six());
    let before = files_of(&path);
    let effects = rig.raw(MaintenanceMessage::Start(flags(
        FlagScope::All,
        MaintFlag::Delete,
        FlagAction::Set,
    )));
    rig.raw(MaintenanceMessage::Stop);
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert_eq!(files_of(&path), before, "stopped before the first write");
    assert_eq!(rig.state.maintenance_job(), None, "nothing stays running");
    assert!(rig.state.list_job().is_none());
    // The slot works.
    rig.ask(flags(FlagScope::All, MaintFlag::Delete, FlagAction::Set));
    assert_eq!(disk_deleted(&path), vec![true; 6]);
    assert!(rig.state.bases.slot(slot).is_some());
}

#[test]
fn a_file_that_changed_on_disk_refuses_the_write_with_stale_and_is_read_again() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "six", &six());
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
        .unwrap();
    drop(file);
    let before = disk_flags(&path);
    rig.ask(flags(FlagScope::All, MaintFlag::Delete, FlagAction::Set));
    assert_eq!(
        rig.maint_notice(),
        Some(MaintNotice::Failed(BaseError::Stale))
    );
    assert_eq!(disk_flags(&path), before, "nothing was written");
    // The slot was read again (the new stamp), so the next try goes through.
    rig.ask(flags(FlagScope::All, MaintFlag::Delete, FlagAction::Set));
    assert_eq!(disk_deleted(&path), vec![true; 6]);
}

#[test]
fn the_tree_cache_survives_a_header_change_and_a_non_delete_flag_but_not_a_delete() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "six", &six());
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::SaveCache);
    let stc = dir.path().join("six.stc");
    assert!(stc.exists(), "setup: the cache file is written");
    rig.ask(MaintRequest::Header(HeaderEdit::Description("x".into())));
    assert!(stc.exists(), "the header does not change the trees (D3534)");
    rig.ask(flags(FlagScope::All, MaintFlag::Tactics, FlagAction::Set));
    assert!(stc.exists(), "a marking flag does not change the trees");
    rig.select(slot, &[1]);
    rig.ask(flags(FlagScope::All, MaintFlag::Delete, FlagAction::Set));
    assert!(!stc.exists(), "deleting games removes the cache (D1050)");
}
