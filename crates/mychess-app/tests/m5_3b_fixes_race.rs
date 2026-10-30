//! M5-3b regression tests of the review: a save in flight must not write the
//! old trees after the base changed (D1772), the quit saves a fill (D1774),
//! answers of a cache that is gone are ignored (D1773), a failed save keeps
//! the cache dirty (D1775) and the quit has a deadline (D1776).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::{BasesMessage, CacheFill, CacheIssue, CacheStatus, ListMessage};
use tempfile::tempdir;

fn toggle_deleted(rig: &mut Rig, slot: SlotId) {
    rig.send(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1],
    }));
    rig.send(BasesMessage::List(ListMessage::ToggleDeleted { slot }));
}

fn rig_with_tree(dir: &std::path::Path) -> (Rig, SlotId) {
    let mut rig = Rig::new();
    let slot = rig.open_si4(dir, "std", STD);
    rig.send(TreeMessage::Open);
    (rig, slot)
}

fn run_all(rig: &mut Rig, effects: Vec<Effect>) -> Vec<Effect> {
    let mut host = Vec::new();
    for effect in effects {
        let answer = rig.perform(effect);
        host.extend(rig.send(answer));
    }
    host
}

#[test]
fn a_save_in_flight_does_not_write_the_old_trees_after_a_change() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = rig_with_tree(dir.path());
    let stc = dir.path().join("std.stc");
    // The save is asked for but has not run yet.
    let save = rig.raw(TreeMessage::SaveCache);
    assert!(!stc.exists());
    // The base changes: the discard runs (and removes nothing yet).
    toggle_deleted(&mut rig, slot);
    // The save runs late: it must not leave the file of the old content.
    run_all(&mut rig, save);
    assert!(!stc.exists(), "a stale save wrote the old trees");
    assert_eq!(
        rig.view().cache.status,
        CacheStatus::Failed(CacheIssue::Stale)
    );
}

#[test]
fn a_discard_removes_the_file_after_a_save_that_got_in_first() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = rig_with_tree(dir.path());
    rig.send(TreeMessage::SaveCache);
    let stc = dir.path().join("std.stc");
    assert!(stc.exists());
    toggle_deleted(&mut rig, slot);
    assert!(!stc.exists());
    // The fresh cache of the new content can be saved again.
    rig.send(TreeMessage::SaveCache);
    assert!(stc.exists());
}

#[test]
fn quitting_during_a_fill_asks_for_a_save_even_if_nothing_else_is_dirty() {
    // What it proves: the fill alone made the cache dirty (it was saved and
    // its file removed just before), so the quit emits the save, waits for
    // the cancelled fill to release the cache and leaves with the file.
    let dir = tempdir().unwrap();
    let (mut rig, _) = rig_with_tree(dir.path());
    rig.send(TreeMessage::SetCacheAutosave(true));
    // Make sure nothing but the fill could be dirty.
    rig.send(TreeMessage::SaveCache);
    std::fs::remove_file(dir.path().join("std.stc")).unwrap();
    let fill = rig.raw(TreeMessage::FillCache(CacheFill::StandardLines));
    let quit = rig.raw(Message::Quit);
    assert!(!quit.contains(&Effect::Exit));
    assert!(
        quit.iter().any(|e| matches!(e, Effect::TreeCache { .. })),
        "the save of the quit is missing"
    );
    let mut all = quit;
    all.extend(fill);
    let host = run_all(&mut rig, all);
    assert!(host.contains(&Effect::Exit));
    assert!(dir.path().join("std.stc").exists());
}

#[test]
fn the_quit_asks_for_a_deadline_while_it_waits_for_the_saves() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = rig_with_tree(dir.path());
    rig.send(TreeMessage::SetCacheAutosave(true));
    let quit = rig.raw(Message::Quit);
    let generation = quit
        .iter()
        .find_map(|e| match e {
            Effect::TreeCacheDeadline { generation, .. } => Some(*generation),
            _ => None,
        })
        .expect("the timer of the cache");
    // The deadline arrives before the save answers: the program leaves.
    let host = rig.raw(TreeMessage::CacheDeadline(generation));
    assert!(host.contains(&Effect::Exit));
}

#[test]
fn a_failed_save_keeps_the_cache_dirty() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::Open);
    // A directory where the file goes makes the rename fail.
    std::fs::create_dir(dir.path().join("std.stc")).unwrap();
    rig.send(TreeMessage::SaveCache);
    assert!(matches!(
        rig.view().cache.status,
        CacheStatus::Failed(CacheIssue::File(_))
    ));
    std::fs::remove_dir(dir.path().join("std.stc")).unwrap();
    // The close still has something to save.
    rig.send(TreeMessage::Close);
    assert!(dir.path().join("std.stc").exists());
}

#[test]
fn the_answer_of_a_cache_that_is_gone_does_not_touch_the_new_one() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = rig_with_tree(dir.path());
    let info = rig.raw(TreeMessage::CacheInfo);
    toggle_deleted(&mut rig, slot);
    // The new cache has one tree from the search after the change; the old
    // job answers now and must not change it or report it as current.
    run_all(&mut rig, info);
    assert!(rig.view().cache.busy.is_none());
    rig.send(TreeMessage::SaveCache);
    assert!(matches!(rig.view().cache.status, CacheStatus::Saved { .. }));
}
