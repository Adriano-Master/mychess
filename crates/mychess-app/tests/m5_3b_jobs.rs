//! M5-3b contract tests of the jobs of the cache menu: save, fill, info and
//! cancel (D1762-D1766).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::{CacheFill, CacheIssue, CacheStatus};
use mychess_db::tree::StcCache;
use tempfile::tempdir;

fn cache_jobs(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::TreeCache { .. }))
        .count()
}

fn status(rig: &Rig) -> CacheStatus {
    rig.view().cache.status
}

/// An `.si4` base of the standard games with the tree open (one search done).
fn si4_rig(dir: &std::path::Path) -> (Rig, std::path::PathBuf) {
    let mut rig = Rig::new();
    rig.open_si4(dir, "std", STD);
    rig.send(TreeMessage::Open);
    (rig, dir.join("std.si4"))
}

#[test]
fn saving_writes_the_stc_next_to_the_si4() {
    let dir = tempdir().unwrap();
    let (mut rig, si4) = si4_rig(dir.path());
    let stc = dir.path().join("std.stc");
    assert!(!stc.exists(), "nothing is written by itself");
    rig.send(TreeMessage::SaveCache);
    assert!(stc.exists());
    assert!(matches!(status(&rig), CacheStatus::Saved { bytes, .. } if bytes > 0));
    // The file is the one `StcCache` reads back, with the tree searched.
    let mut cache = StcCache::new(None);
    cache.read_from(&si4).expect("readable");
    assert!(cache.info().0 >= 1);
    assert!(rig.view().cache.busy.is_none());
}

#[test]
fn saving_in_a_pgn_file_is_refused_without_a_job() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let effects = rig.raw(TreeMessage::SaveCache);
    assert_eq!(cache_jobs(&effects), 0);
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::NotScidBase));
    assert!(!dir.path().join("std.stc").exists());
    let effects = rig.raw(TreeMessage::FillCache(CacheFill::StandardLines));
    assert_eq!(cache_jobs(&effects), 0);
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::NotScidBase));
}

#[test]
fn filling_with_the_standard_lines_makes_the_next_search_a_hit() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = si4_rig(dir.path());
    rig.send(TreeMessage::FillCache(CacheFill::StandardLines));
    match status(&rig) {
        CacheStatus::Filled { searches, .. } => assert!(searches >= 100, "{searches}"),
        other => panic!("unexpected {other:?}"),
    }
    rig.play("e2e4");
    assert!(rig.view().from_cache, "1.e4 is one of the standard lines");
    assert_eq!(rig.sorted_sans(), ["[end]", "c5", "e5"]);
}

#[test]
fn filling_with_the_current_game_searches_every_position() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = si4_rig(dir.path());
    rig.play("e2e4");
    rig.play("e7e5");
    rig.send(TreeMessage::FillCache(CacheFill::Game));
    // The start position and one per move.
    assert!(matches!(
        status(&rig),
        CacheStatus::Filled { searches: 3, .. }
    ));
}

#[test]
fn filling_with_the_base_goes_through_the_games_of_the_filter() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = si4_rig(dir.path());
    rig.send(TreeMessage::FillCache(CacheFill::Base));
    assert!(matches!(
        status(&rig),
        CacheStatus::Filled { searches, .. } if searches > 5
    ));
}

#[test]
fn the_information_describes_the_cache_and_the_file() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = si4_rig(dir.path());
    rig.send(TreeMessage::CacheInfo);
    let CacheStatus::Info(info) = status(&rig) else {
        panic!("expected the information, got {:?}", status(&rig));
    };
    assert_eq!(info.capacity, 1000);
    assert_eq!(info.backup_capacity, 100);
    assert!(info.used >= 1);
    assert_eq!(info.file_bytes, None);
    rig.send(TreeMessage::SaveCache);
    rig.send(TreeMessage::CacheInfo);
    let CacheStatus::Info(info) = status(&rig) else {
        panic!("expected the information");
    };
    assert!(info.file_bytes.is_some_and(|n| n > 22), "{info:?}");
    rig.send(TreeMessage::DismissCacheStatus);
    assert_eq!(status(&rig), CacheStatus::None);
}

#[test]
fn a_second_request_during_a_job_is_busy_and_starts_nothing() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = si4_rig(dir.path());
    let first = rig.raw(TreeMessage::FillCache(CacheFill::StandardLines));
    assert_eq!(cache_jobs(&first), 1);
    assert!(rig.view().cache.busy.is_some());
    let second = rig.raw(TreeMessage::SaveCache);
    assert_eq!(cache_jobs(&second), 0);
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::Busy));
    // The first one still finishes normally.
    for effect in first {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
    assert!(rig.view().cache.busy.is_none());
    assert!(matches!(status(&rig), CacheStatus::Filled { .. }));
}

#[test]
fn cancelling_a_fill_keeps_the_cache_and_reports_it() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = si4_rig(dir.path());
    let effects = rig.raw(TreeMessage::FillCache(CacheFill::StandardLines));
    rig.raw(TreeMessage::CancelCacheJob);
    for effect in effects {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::Cancelled));
    assert!(rig.view().cache.busy.is_none());
    // A new job is accepted after it.
    rig.send(TreeMessage::CacheInfo);
    assert!(matches!(status(&rig), CacheStatus::Info(_)));
}

#[test]
fn without_a_database_the_menu_reports_it() {
    let mut rig = Rig::new();
    let effects = rig.raw(TreeMessage::SaveCache);
    assert_eq!(cache_jobs(&effects), 0);
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::NoBase));
}

#[test]
fn saving_a_cache_never_used_in_the_session_keeps_the_file_of_the_scid() {
    // D1056/D1762: the job reads the file before writing, so the trees of a
    // file made earlier are not replaced by an empty one.
    let dir = tempdir().unwrap();
    let (mut rig, si4) = si4_rig(dir.path());
    rig.send(TreeMessage::FillCache(CacheFill::StandardLines));
    rig.send(TreeMessage::SaveCache);
    let before = {
        let mut c = StcCache::new(None);
        c.read_from(&si4).unwrap();
        c.info().0
    };
    assert!(before > 50);
    // A new session: the tree is open on the same file; nothing searched yet.
    let mut again = Rig::new();
    again.open_si4(dir.path(), "std2", STD);
    std::fs::copy(dir.path().join("std.stc"), dir.path().join("std2.stc")).unwrap();
    again.send(TreeMessage::SaveCache);
    let mut c = StcCache::new(None);
    c.read_from(&dir.path().join("std2.si4")).unwrap();
    assert_eq!(c.info().0, before);
}
