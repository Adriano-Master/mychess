//! M5-3b contract tests of the invalidation (D1050, D1767) and of the
//! auto-save (D1761, D1769): every change of a database drops the cache and
//! its `.stc` file; closing a database keeps the file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::{CacheFill, CacheStatus, CopyScope, ListMessage, PrefChange, PrefsMessage};
use tempfile::tempdir;

/// An `.si4` base with the tree open and its cache saved.
fn saved(dir: &std::path::Path) -> (Rig, SlotId, std::path::PathBuf) {
    let mut rig = Rig::new();
    let slot = rig.open_si4(dir, "std", STD);
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::SaveCache);
    let stc = dir.join("std.stc");
    assert!(stc.exists(), "setup: the cache file is written");
    (rig, slot, stc)
}

#[test]
fn flagging_a_game_as_deleted_removes_the_stc() {
    let dir = tempdir().unwrap();
    let (mut rig, slot, stc) = saved(dir.path());
    rig.send(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1],
    }));
    rig.send(BasesMessage::List(ListMessage::ToggleDeleted { slot }));
    assert!(!stc.exists(), "the base was rewritten: the cache is stale");
    // The tree works on, with a fresh cache that is not on disk.
    assert_eq!(rig.view().body, TreeBody::Rows);
    rig.send(TreeMessage::CacheInfo);
    assert!(matches!(
        rig.view().cache.status,
        CacheStatus::Info(info) if info.file_bytes.is_none()
    ));
}

#[test]
fn copying_games_into_the_base_removes_its_stc() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    let target = rig.open_si4(dir.path(), "target", STD);
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::SaveCache);
    let stc = dir.path().join("target.stc");
    assert!(stc.exists());
    let source = rig.open_si4(dir.path(), "source", STD);
    assert_ne!(source, target);
    rig.send(BasesMessage::List(ListMessage::CopyGames {
        from: source,
        to: target,
        scope: CopyScope::Filter,
    }));
    assert_eq!(rig.filtered(target), 10, "the copy happened");
    assert!(!stc.exists());
}

#[test]
fn closing_the_base_keeps_the_stc_file() {
    let dir = tempdir().unwrap();
    let (mut rig, slot, stc) = saved(dir.path());
    rig.send(BasesMessage::Close(slot));
    assert!(stc.exists(), "only changes remove the file (Scid)");
}

#[test]
fn a_search_that_changes_nothing_keeps_the_stc_file() {
    let dir = tempdir().unwrap();
    let (mut rig, _, stc) = saved(dir.path());
    rig.play("e2e4");
    rig.nav(Nav::Start);
    rig.send(TreeMessage::FillCache(CacheFill::StandardLines));
    assert!(stc.exists());
}

#[test]
fn a_cache_of_the_old_content_is_not_served_after_the_change() {
    let dir = tempdir().unwrap();
    let (mut rig, slot, _stc) = saved(dir.path());
    rig.send(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1],
    }));
    rig.send(BasesMessage::List(ListMessage::ToggleDeleted { slot }));
    // The reopened base is searched from scratch, not from the old cache.
    assert_eq!(rig.view().body, TreeBody::Rows);
    assert!(!rig.view().from_cache);
    assert_eq!(rig.view().cache.busy, None);
}

// ---- auto-save ----

#[test]
fn closing_the_panel_saves_when_auto_save_is_on() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::Open);
    let stc = dir.path().join("std.stc");
    assert!(!stc.exists());
    rig.send(TreeMessage::Close);
    assert!(stc.exists());
}

#[test]
fn closing_the_panel_saves_nothing_when_auto_save_is_off() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::Close);
    assert!(!dir.path().join("std.stc").exists());
}

#[test]
fn a_cache_already_on_disk_is_not_written_again() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::Close);
    let stc = dir.path().join("std.stc");
    assert!(stc.exists());
    std::fs::remove_file(&stc).unwrap();
    rig.send(TreeMessage::Open);
    // The same position is served from the cache: nothing new to save.
    rig.send(TreeMessage::Close);
    assert!(!stc.exists());
}

#[test]
fn closing_the_base_saves_when_auto_save_is_on() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::Open);
    rig.send(BasesMessage::Close(slot));
    assert!(dir.path().join("std.stc").exists());
}

#[test]
fn quitting_waits_for_the_save_and_then_exits() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::Open);
    // The first answer of `update` has the save, not the exit.
    let effects = rig.raw(Message::Quit);
    assert!(!effects.contains(&Effect::Exit));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::TreeCache { .. }))
    );
    let mut host = Vec::new();
    for effect in effects {
        let answer = rig.perform(effect);
        host.extend(rig.send(answer));
    }
    assert!(host.contains(&Effect::Exit));
    assert!(dir.path().join("std.stc").exists());
}

#[test]
fn quitting_without_auto_save_exits_at_once() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::Open);
    let effects = rig.raw(Message::Quit);
    assert!(effects.contains(&Effect::Exit));
    assert!(!dir.path().join("std.stc").exists());
}

#[test]
fn quitting_during_a_fill_cancels_it_and_saves_what_it_did() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::Open);
    let fill = rig.raw(TreeMessage::FillCache(CacheFill::StandardLines));
    let quit = rig.raw(Message::Quit);
    assert!(!quit.contains(&Effect::Exit));
    let mut host = Vec::new();
    for effect in quit.into_iter().chain(fill) {
        let answer = rig.perform(effect);
        host.extend(rig.send(answer));
    }
    assert!(host.contains(&Effect::Exit));
    assert!(dir.path().join("std.stc").exists());
}
