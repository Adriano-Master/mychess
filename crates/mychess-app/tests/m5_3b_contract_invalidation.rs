//! M5-3b additional contract tests of the invalidation (D1050, D1767) and of
//! the auto-save (D1761, D1769): which operations drop the cache and its
//! `.stc`, which must not, switching the base of a slot and the quit flow.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use std::path::{Path, PathBuf};

use m5_2_common::*;
use mychess_app::{
    BasesMessage, CacheFill, CacheStatus, ClassifyChoice, CopyScope, EcoMessage, ListColumn,
    ListMessage, PrefChange, PrefsMessage, SharedEco,
};
use mychess_core::EcoBook;
use mychess_db::tree::StcCache;
use tempfile::tempdir;

const BOOK: &str = "B20 \"Sicilian Defence\" 1.e4 c5 *\n\
    C20 \"King's Pawn Game\" 1.e4 e5 *\n";

fn saved_base(rig: &mut Rig, dir: &Path, name: &str) -> (SlotId, PathBuf) {
    let slot = rig.open_si4(dir, name, STD);
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::SaveCache);
    let stc = dir.join(format!("{name}.stc"));
    assert!(stc.exists(), "setup: {name}.stc written");
    (slot, stc)
}

fn select(rig: &mut Rig, slot: SlotId, numbers: &[u32]) {
    rig.send(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: numbers.to_vec(),
    }));
}

fn no_confirm(rig: &mut Rig) {
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
}

// ---- operations that rewrite the base ----

#[test]
fn classifying_the_eco_codes_removes_the_stc() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, stc) = saved_base(&mut rig, dir.path(), "std");
    rig.state.eco_book = Some(SharedEco::new(EcoBook::parse(BOOK).unwrap()));
    rig.send(Message::Eco(EcoMessage::OpenClassify));
    rig.send(Message::Eco(EcoMessage::SetClassifyChoice(
        ClassifyChoice::All,
    )));
    rig.send(Message::Eco(EcoMessage::ClassifyRun));
    assert!(!stc.exists(), "the codes were written: the cache is stale");
    // The tree is searched again from the reopened base.
    assert_eq!(rig.view().body, TreeBody::Rows);
    assert!(!rig.view().from_cache);
    assert_eq!(rig.row("e4").games, 3);
    let _ = slot;
}

#[test]
fn a_classification_preview_keeps_the_stc() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, stc) = saved_base(&mut rig, dir.path(), "std");
    rig.state.eco_book = Some(SharedEco::new(EcoBook::parse(BOOK).unwrap()));
    rig.send(Message::Eco(EcoMessage::OpenClassify));
    rig.send(Message::Eco(EcoMessage::SetClassifyChoice(
        ClassifyChoice::All,
    )));
    rig.send(Message::Eco(EcoMessage::ClassifyPreview));
    assert!(stc.exists(), "a preview writes nothing");
}

#[test]
fn undoing_the_deleted_flag_removes_the_stc_again() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, stc) = saved_base(&mut rig, dir.path(), "std");
    select(&mut rig, slot, &[1]);
    rig.send(BasesMessage::List(ListMessage::ToggleDeleted { slot }));
    assert!(!stc.exists());
    rig.send(TreeMessage::SaveCache);
    assert!(stc.exists());
    select(&mut rig, slot, &[1]);
    rig.send(BasesMessage::List(ListMessage::ToggleDeleted { slot }));
    assert!(!stc.exists(), "the second toggle rewrites the base too");
    assert_eq!(rig.row("e4").games, 3);
}

#[test]
fn a_change_with_no_stc_on_disk_is_harmless() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::Open);
    select(&mut rig, slot, &[2]);
    rig.send(BasesMessage::List(ListMessage::ToggleDeleted { slot }));
    assert_eq!(rig.view().body, TreeBody::Rows);
    assert!(!dir.path().join("std.stc").exists());
}

#[test]
fn a_change_does_not_touch_the_stc_of_other_bases() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (a, stc_a) = saved_base(&mut rig, dir.path(), "a");
    let (b, stc_b) = saved_base(&mut rig, dir.path(), "b");
    assert_ne!(a, b);
    select(&mut rig, b, &[1]);
    rig.send(BasesMessage::List(ListMessage::ToggleDeleted { slot: b }));
    assert!(!stc_b.exists());
    assert!(stc_a.exists(), "the other base did not change");
}

#[test]
fn copying_into_a_base_keeps_the_stc_of_the_source() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    no_confirm(&mut rig);
    let (source, stc_source) = saved_base(&mut rig, dir.path(), "source");
    let (target, stc_target) = saved_base(&mut rig, dir.path(), "target");
    rig.send(BasesMessage::List(ListMessage::CopyGames {
        from: source,
        to: target,
        scope: CopyScope::Filter,
    }));
    assert_eq!(rig.filtered(target), 10);
    assert!(!stc_target.exists());
    assert!(stc_source.exists(), "the source is not written");
}

#[test]
fn copying_into_a_base_makes_the_tree_count_the_new_games() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    no_confirm(&mut rig);
    let source = rig.open_si4(dir.path(), "source", STD);
    let target = rig.open_si4(dir.path(), "target", STD);
    rig.send(BasesMessage::Select(target));
    rig.send(TreeMessage::Open);
    assert_eq!(rig.row("e4").games, 3);
    rig.send(BasesMessage::List(ListMessage::CopyGames {
        from: source,
        to: target,
        scope: CopyScope::Filter,
    }));
    rig.send(BasesMessage::Select(target));
    assert_eq!(rig.row("e4").games, 6, "the cache of 5 games was dropped");
}

#[test]
fn view_only_changes_keep_the_stc() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, stc) = saved_base(&mut rig, dir.path(), "std");
    rig.send(BasesMessage::List(ListMessage::SortBy {
        slot,
        column: ListColumn::Result,
    }));
    assert!(stc.exists(), "sorting the view does not rewrite the base");
    select(&mut rig, slot, &[1, 2]);
    rig.send(BasesMessage::List(ListMessage::RemoveSelected { slot }));
    assert!(stc.exists(), "the filter is not part of the base");
    rig.send(BasesMessage::List(ListMessage::ResetFilter { slot }));
    assert!(stc.exists());
}

#[test]
fn copying_games_into_the_clipbase_refreshes_a_tree_on_it() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_si4(dir.path(), "std", STD);
    select(&mut rig, slot, &[1]);
    rig.send(BasesMessage::List(ListMessage::CopySelected { slot }));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.send(TreeMessage::Open);
    assert_eq!(rig.sorted_sans(), ["e4"]);
    rig.send(BasesMessage::Select(slot));
    select(&mut rig, slot, &[2]);
    rig.send(BasesMessage::List(ListMessage::CopySelected { slot }));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    assert_eq!(
        rig.sorted_sans(),
        ["d4", "e4"],
        "the number of games changed: the cache of the Clipbase is dropped"
    );
}

// ---- switching the base of a slot ----

#[test]
fn closing_a_base_and_opening_another_never_serves_the_old_trees() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, stc_a) = saved_base(&mut rig, dir.path(), "a");
    rig.send(BasesMessage::Close(slot));
    rig.open_si4(
        dir.path(),
        "b",
        &game_pgn("1. c4 e5", "1-0", 0, 0, "2020.01.01", None),
    );
    rig.send(TreeMessage::Open);
    assert_eq!(rig.sorted_sans(), ["c4"], "base b, not the cache of a");
    assert!(stc_a.exists(), "closing keeps the file of a");
    assert!(!dir.path().join("b.stc").exists());
}

#[test]
fn reopening_a_closed_base_reads_its_stc_back() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = saved_base(&mut rig, dir.path(), "a");
    rig.send(BasesMessage::Close(slot));
    rig.send(BasesMessage::Open(dir.path().join("a.si4")));
    rig.send(TreeMessage::Open);
    assert!(
        rig.view().from_cache,
        "the file of the earlier session is used"
    );
    assert_eq!(rig.row("e4").games, 3);
}

#[test]
fn closing_a_base_with_a_fill_in_flight_does_not_remove_the_file() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, stc) = saved_base(&mut rig, dir.path(), "a");
    let running = rig.raw(TreeMessage::FillCache(CacheFill::Base));
    rig.send(BasesMessage::Close(slot));
    for effect in running {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
    assert!(stc.exists());
}

// ---- auto-save details ----

#[test]
fn auto_save_never_replaces_the_trees_of_a_file_by_fewer() {
    let dir = tempdir().unwrap();
    let before = {
        let mut rig = Rig::new();
        let slot = rig.open_si4(dir.path(), "std", STD);
        rig.send(TreeMessage::Open);
        rig.send(TreeMessage::FillCache(CacheFill::StandardLines));
        rig.send(TreeMessage::SaveCache);
        rig.send(BasesMessage::Close(slot));
        let mut c = StcCache::new(None);
        c.read_from(&dir.path().join("std.si4")).unwrap();
        c.info().0
    };
    assert!(before > 50);
    // A new session that only looks at the start position.
    let mut rig = Rig::new();
    rig.send(BasesMessage::Open(dir.path().join("std.si4")));
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::Close);
    let mut c = StcCache::new(None);
    c.read_from(&dir.path().join("std.si4")).unwrap();
    assert!(c.info().0 >= before, "{} < {before}", c.info().0);
}

#[test]
fn auto_save_off_writes_nothing_on_any_exit_path() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::FillCache(CacheFill::StandardLines));
    rig.send(TreeMessage::Close);
    rig.send(BasesMessage::Close(slot));
    let effects = rig.raw(Message::Quit);
    assert!(effects.contains(&Effect::Exit));
    assert!(!dir.path().join("std.stc").exists());
}

#[test]
fn closing_the_base_after_a_fill_saves_the_filled_trees() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::FillCache(CacheFill::StandardLines));
    rig.send(BasesMessage::Close(slot));
    let mut c = StcCache::new(None);
    c.read_from(&dir.path().join("std.si4")).unwrap();
    assert!(c.info().0 > 50);
}

#[test]
fn a_clean_cache_is_not_written_when_the_base_closes_or_the_app_quits() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, stc) = saved_base(&mut rig, dir.path(), "std");
    rig.send(TreeMessage::SetCacheAutosave(true));
    std::fs::remove_file(&stc).unwrap();
    rig.send(BasesMessage::Close(slot));
    assert!(!stc.exists(), "saved explicitly, nothing new since");
    let effects = rig.raw(Message::Quit);
    assert!(effects.contains(&Effect::Exit));
}

#[test]
fn quitting_waits_for_every_dirty_cache_and_a_second_quit_starts_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "a", STD);
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::Open);
    rig.open_si4(dir.path(), "b", STD);
    let effects = rig.raw(Message::Quit);
    let saves: Vec<Effect> = effects
        .into_iter()
        .filter(|e| matches!(e, Effect::TreeCache { .. }))
        .collect();
    assert_eq!(saves.len(), 2, "one save per dirty cache");
    // A second quit request while waiting does not start more saves.
    let again = rig.raw(Message::Quit);
    assert!(!again.iter().any(|e| matches!(e, Effect::TreeCache { .. })));
    assert!(!again.contains(&Effect::Exit));
    let mut host = Vec::new();
    for (i, effect) in saves.into_iter().enumerate() {
        let answer = rig.perform(effect);
        let out = rig.send(answer);
        if i == 0 {
            assert!(!out.contains(&Effect::Exit), "one save is still running");
        }
        host.extend(out);
    }
    assert!(host.contains(&Effect::Exit));
    assert!(dir.path().join("a.stc").exists());
    assert!(dir.path().join("b.stc").exists());
}

#[test]
fn a_failed_auto_save_does_not_keep_the_app_from_quitting() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::Open);
    std::fs::create_dir(dir.path().join("std.stc")).unwrap();
    let effects = rig.raw(Message::Quit);
    assert!(!effects.contains(&Effect::Exit));
    let mut host = Vec::new();
    for effect in effects {
        let answer = rig.perform(effect);
        host.extend(rig.send(answer));
    }
    assert!(host.contains(&Effect::Exit), "a write error is only logged");
}

#[test]
fn the_quit_status_is_not_polluted_by_the_automatic_saves() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::Close);
    // The automatic save is not a user job: no result line, no Busy.
    assert_eq!(rig.view().cache.status, CacheStatus::None);
    assert!(rig.view().cache.busy.is_none());
    rig.send(TreeMessage::CacheInfo);
    assert!(matches!(rig.view().cache.status, CacheStatus::Info(_)));
}
