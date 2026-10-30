//! M5-3b additional contract tests of the cache jobs (D1762-D1766, D1769):
//! one user job at a time, stale and replayed answers, cancelling, the lock
//! that a search only tries, the Clipbase and damaged `.stc` files.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use m5_2_common::*;
use mychess_app::{BasesMessage, CacheFill, CacheIssue, CacheStatus, ListMessage};
use mychess_core::Position;
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

fn si4_rig(dir: &Path) -> (Rig, SlotId, PathBuf) {
    let mut rig = Rig::new();
    let slot = rig.open_si4(dir, "std", STD);
    rig.send(TreeMessage::Open);
    (rig, slot, dir.join("std.stc"))
}

/// Deterministic pseudo-random games, `plies` half-moves each.
fn random_games(count: usize, plies: usize) -> String {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut out = String::new();
    for _ in 0..count {
        let mut pos = Position::new();
        let mut text = String::new();
        for ply in 0..plies {
            let moves = pos.legal_moves();
            if moves.is_empty() {
                break;
            }
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let mv = moves[usize::try_from(seed >> 33).unwrap() % moves.len()];
            let san = pos.san(mv).unwrap();
            if ply % 2 == 0 {
                let _ = write!(text, "{}. ", ply / 2 + 1);
            }
            text.push_str(&san);
            text.push(' ');
            pos.play(mv).unwrap();
        }
        out.push_str(&game_pgn(text.trim(), "*", 0, 0, "2020.01.01", None));
    }
    out
}

fn finish(rig: &mut Rig, effects: Vec<Effect>) {
    for effect in effects {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
}

#[test]
fn every_pair_of_jobs_answers_busy_to_the_second_and_starts_nothing() {
    let dir = tempdir().unwrap();
    let (mut rig, _, _) = si4_rig(dir.path());
    let all = [
        TreeMessage::SaveCache,
        TreeMessage::FillCache(CacheFill::Base),
        TreeMessage::FillCache(CacheFill::Game),
        TreeMessage::FillCache(CacheFill::StandardLines),
        TreeMessage::CacheInfo,
    ];
    for first in &all {
        let running = rig.raw(first.clone());
        assert_eq!(cache_jobs(&running), 1, "{first:?}");
        for second in &all {
            let effects = rig.raw(second.clone());
            assert_eq!(cache_jobs(&effects), 0, "{first:?} then {second:?}");
            assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::Busy));
        }
        // Options are not jobs: they never answer Busy.
        rig.raw(TreeMessage::DismissCacheStatus);
        let effects = rig.raw(TreeMessage::SetCacheAutosave(false));
        assert_eq!(cache_jobs(&effects), 0);
        assert_eq!(status(&rig), CacheStatus::None);
        finish(&mut rig, running);
        assert!(rig.view().cache.busy.is_none(), "{first:?} is over");
    }
}

#[test]
fn cancelling_with_no_job_changes_nothing() {
    let dir = tempdir().unwrap();
    let (mut rig, _, _) = si4_rig(dir.path());
    let effects = rig.raw(TreeMessage::CancelCacheJob);
    assert!(effects.is_empty());
    assert_eq!(status(&rig), CacheStatus::None);
    assert!(rig.view().cache.busy.is_none());
}

#[test]
fn dismissing_clears_a_failure_too() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::SaveCache);
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::NotScidBase));
    rig.send(TreeMessage::DismissCacheStatus);
    assert_eq!(status(&rig), CacheStatus::None);
}

#[test]
fn a_new_job_clears_the_previous_result_while_it_runs() {
    let dir = tempdir().unwrap();
    let (mut rig, _, _) = si4_rig(dir.path());
    rig.send(TreeMessage::SaveCache);
    assert!(matches!(status(&rig), CacheStatus::Saved { .. }));
    let running = rig.raw(TreeMessage::CacheInfo);
    assert_eq!(status(&rig), CacheStatus::None);
    assert!(rig.view().cache.busy.is_some());
    finish(&mut rig, running);
    assert!(matches!(status(&rig), CacheStatus::Info(_)));
}

#[test]
fn the_busy_view_names_the_kind_of_the_job() {
    let dir = tempdir().unwrap();
    let (mut rig, _, _) = si4_rig(dir.path());
    let running = rig.raw(TreeMessage::SaveCache);
    let busy = rig.view().cache.busy.expect("busy");
    assert_eq!(busy.kind, mychess_app::CacheJobKind::Save);
    finish(&mut rig, running);
    let running = rig.raw(TreeMessage::FillCache(CacheFill::Game));
    let busy = rig.view().cache.busy.expect("busy");
    assert_eq!(busy.kind, mychess_app::CacheJobKind::Fill(CacheFill::Game));
    finish(&mut rig, running);
}

#[test]
fn an_answer_replayed_later_is_ignored_by_the_next_job() {
    let dir = tempdir().unwrap();
    let (mut rig, _, _) = si4_rig(dir.path());
    let first = rig.raw(TreeMessage::CacheInfo);
    let old_answer = rig.perform(first.into_iter().next().unwrap());
    rig.send(old_answer.clone());
    assert!(matches!(status(&rig), CacheStatus::Info(_)));
    rig.send(TreeMessage::DismissCacheStatus);

    let second = rig.raw(TreeMessage::SaveCache);
    assert!(rig.view().cache.busy.is_some());
    // The old answer arrives again while the second job runs.
    rig.send(old_answer);
    assert!(rig.view().cache.busy.is_some(), "the job is still running");
    assert_eq!(
        status(&rig),
        CacheStatus::None,
        "the old answer said nothing"
    );
    finish(&mut rig, second);
    assert!(matches!(status(&rig), CacheStatus::Saved { .. }));
}

#[test]
fn the_answer_of_a_save_of_a_cache_that_was_dropped_is_stale() {
    let dir = tempdir().unwrap();
    let (mut rig, slot, stc) = si4_rig(dir.path());
    rig.send(TreeMessage::SaveCache);
    assert!(stc.exists());
    let running = rig.raw(TreeMessage::SaveCache);
    // The base is rewritten while the job is in flight.
    rig.send(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1],
    }));
    rig.send(BasesMessage::List(ListMessage::ToggleDeleted { slot }));
    assert!(!stc.exists(), "setup: the change removed the file");
    finish(&mut rig, running);
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::Stale));
    assert!(rig.view().cache.busy.is_none());
    // The old cache must not leave its file behind (spec D1050).
    assert!(
        !stc.exists(),
        "a save of the old cache finished after the change and brought the stale file back"
    );
}

#[test]
fn closing_the_base_during_a_fill_ends_the_job_without_a_panic() {
    let dir = tempdir().unwrap();
    let (mut rig, slot, _) = si4_rig(dir.path());
    let running = rig.raw(TreeMessage::FillCache(CacheFill::Base));
    rig.send(BasesMessage::Close(slot));
    finish(&mut rig, running);
    assert!(rig.view().cache.busy.is_none());
    // The menu works again afterwards.
    let again = rig.raw(TreeMessage::CacheInfo);
    assert_eq!(cache_jobs(&again), 0, "no base: no job");
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::NoBase));
}

#[test]
fn a_cancelled_fill_keeps_what_the_cache_had_and_the_save_still_works() {
    let dir = tempdir().unwrap();
    let (mut rig, _, stc) = si4_rig(dir.path());
    let running = rig.raw(TreeMessage::FillCache(CacheFill::Base));
    rig.raw(TreeMessage::CancelCacheJob);
    finish(&mut rig, running);
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::Cancelled));
    rig.send(TreeMessage::SaveCache);
    assert!(matches!(status(&rig), CacheStatus::Saved { .. }));
    let mut cache = StcCache::new(None);
    cache.read_from(&stc.with_extension("si4")).unwrap();
    assert!(cache.info().0 >= 1, "the tree searched before is kept");
}

#[test]
fn a_cancelled_fill_marks_the_cache_dirty_for_the_auto_save() {
    let dir = tempdir().unwrap();
    let (mut rig, _, stc) = si4_rig(dir.path());
    rig.send(TreeMessage::SetCacheAutosave(true));
    rig.send(TreeMessage::SaveCache);
    assert!(stc.exists());
    std::fs::remove_file(&stc).unwrap();
    // Clean now: closing the panel writes nothing.
    rig.send(TreeMessage::Close);
    assert!(!stc.exists());
    rig.send(TreeMessage::Open);
    let running = rig.raw(TreeMessage::FillCache(CacheFill::StandardLines));
    rig.raw(TreeMessage::CancelCacheJob);
    finish(&mut rig, running);
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::Cancelled));
    rig.send(TreeMessage::Close);
    assert!(stc.exists(), "what the cancelled fill did is saved");
}

#[test]
fn a_search_runs_while_a_fill_holds_the_cache_and_the_results_agree() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "big", &random_games(80, 40));
    rig.send(TreeMessage::Open);
    let reference = rig.sorted_sans();
    let total: u32 = rig.view().rows.iter().map(|r| r.games).sum();
    assert!(total > 0);

    let effects = rig.raw(TreeMessage::FillCache(CacheFill::Base));
    let effect = effects.into_iter().next().expect("job");
    let worker = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(perform(effect))
    });
    // Searches keep answering (they never wait for the cache).
    let mut rounds = 0;
    loop {
        let done = worker.is_finished();
        rig.nav(Nav::Start);
        rig.play(&reference_first_move(&reference));
        rig.nav(Nav::Start);
        assert_eq!(rig.view().body, TreeBody::Rows);
        assert_eq!(rig.sorted_sans(), reference, "round {rounds}");
        rounds += 1;
        if done || rounds > 400 {
            break;
        }
    }
    let answer = worker.join().expect("the job thread");
    rig.send(answer);
    assert!(
        matches!(status(&rig), CacheStatus::Filled { .. }),
        "{:?}",
        status(&rig)
    );
    // After the fill, the tree comes from the cache.
    rig.play(&reference_first_move(&reference));
    rig.nav(Nav::Start);
    assert!(rig.view().from_cache);
    assert_eq!(rig.sorted_sans(), reference);
}

fn reference_first_move(_sans: &[String]) -> String {
    let pos = Position::new();
    let mv = pos.legal_moves()[0];
    pos.uci(mv)
}

// ---- the Clipbase and files that are not Scid bases ----

#[test]
fn the_clipbase_keeps_its_cache_in_memory_and_refuses_a_file() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_si4(dir.path(), "std", STD);
    rig.send(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1],
    }));
    rig.send(BasesMessage::List(ListMessage::CopySelected { slot }));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.send(TreeMessage::Open);
    assert_eq!(rig.sans(), ["e4"]);
    assert!(!rig.view().cache.has_file);
    let before: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();

    let effects = rig.raw(TreeMessage::SaveCache);
    assert_eq!(cache_jobs(&effects), 0);
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::NotScidBase));
    let effects = rig.raw(TreeMessage::FillCache(CacheFill::Base));
    assert_eq!(cache_jobs(&effects), 0);
    assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::NotScidBase));
    // The information still works (memory only, no file).
    rig.send(TreeMessage::CacheInfo);
    assert!(matches!(
        status(&rig),
        CacheStatus::Info(info) if info.file_bytes.is_none() && info.used >= 1
    ));
    let after: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(before.len(), after.len(), "nothing was created");
}

#[test]
fn the_information_of_a_pgn_base_has_no_file_and_creates_none() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::CacheInfo);
    assert!(matches!(
        status(&rig),
        CacheStatus::Info(info) if info.file_bytes.is_none() && info.used >= 1
    ));
    assert!(!dir.path().join("std.stc").exists());
    assert!(!rig.view().cache.has_file);
}

#[test]
fn the_menu_without_a_database_answers_no_base_for_every_job() {
    let mut rig = Rig::new();
    for message in [
        TreeMessage::SaveCache,
        TreeMessage::CacheInfo,
        TreeMessage::FillCache(CacheFill::Base),
        TreeMessage::FillCache(CacheFill::Game),
        TreeMessage::FillCache(CacheFill::StandardLines),
    ] {
        let effects = rig.raw(message.clone());
        assert_eq!(cache_jobs(&effects), 0, "{message:?}");
        assert_eq!(status(&rig), CacheStatus::Failed(CacheIssue::NoBase));
    }
}

// ---- damaged files ----

fn saved_file_bytes(dir: &Path) -> Vec<u8> {
    let (mut rig, _, stc) = si4_rig(dir);
    rig.send(TreeMessage::FillCache(CacheFill::StandardLines));
    rig.send(TreeMessage::SaveCache);
    std::fs::read(stc).unwrap()
}

#[test]
fn a_damaged_stc_never_stops_the_tree_and_a_save_replaces_it() {
    let dir = tempdir().unwrap();
    let good = saved_file_bytes(dir.path());
    assert!(good.len() > 100);
    let damaged: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("one byte", vec![0x53]),
        (
            "garbage",
            b"this is not a tree cache at all, not even close".to_vec(),
        ),
        ("half", good[..good.len() / 2].to_vec()),
        ("header only", good[..good.len().min(22)].to_vec()),
        ("one byte short", good[..good.len() - 1].to_vec()),
        ("zeros", vec![0; good.len()]),
        ("ones", vec![0xFF; good.len()]),
        (
            "flipped payload",
            good.iter()
                .enumerate()
                .map(|(i, b)| if i >= 30 { !b } else { *b })
                .collect(),
        ),
    ];
    for (name, bytes) in damaged {
        let dir = tempdir().unwrap();
        let mut rig = Rig::new();
        rig.open_si4(dir.path(), "std", STD);
        let stc = dir.path().join("std.stc");
        std::fs::write(&stc, &bytes).unwrap();
        rig.send(TreeMessage::Open);
        assert_eq!(rig.view().body, TreeBody::Rows, "{name}");
        assert_eq!(rig.sorted_sans(), ["Nf3", "d4", "e4"], "{name}");
        // The numbers are right whatever the file said.
        assert_eq!(rig.row("e4").games, 3, "{name}");
        rig.send(TreeMessage::CacheInfo);
        assert!(matches!(status(&rig), CacheStatus::Info(_)), "{name}");
        rig.send(TreeMessage::SaveCache);
        assert!(
            matches!(status(&rig), CacheStatus::Saved { .. }),
            "{name}: {:?}",
            status(&rig)
        );
        let mut cache = StcCache::new(None);
        cache
            .read_from(&dir.path().join("std.si4"))
            .unwrap_or_else(|e| panic!("{name}: the saved file is not readable: {e}"));
        assert!(cache.info().0 >= 1, "{name}");
    }
}

#[test]
fn a_damaged_stc_does_not_change_the_numbers_of_a_fill_either() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    std::fs::write(
        dir.path().join("std.stc"),
        b"garbage garbage garbage garbage",
    )
    .unwrap();
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::FillCache(CacheFill::StandardLines));
    assert!(
        matches!(status(&rig), CacheStatus::Filled { .. }),
        "{:?}",
        status(&rig)
    );
    rig.play("e2e4");
    assert_eq!(rig.row("e5").games, 1);
    assert_eq!(rig.row("c5").games, 1);
}

#[test]
fn an_unwritable_destination_is_a_file_issue_not_a_panic() {
    let dir = tempdir().unwrap();
    let (mut rig, _, stc) = si4_rig(dir.path());
    // A directory where the file should go: no platform can write there.
    std::fs::create_dir(&stc).unwrap();
    rig.send(TreeMessage::SaveCache);
    assert!(
        matches!(status(&rig), CacheStatus::Failed(CacheIssue::File(_))),
        "{:?}",
        status(&rig)
    );
    assert!(rig.view().cache.busy.is_none());
    // The menu is usable afterwards.
    rig.send(TreeMessage::CacheInfo);
    assert!(matches!(status(&rig), CacheStatus::Info(_)));
}
