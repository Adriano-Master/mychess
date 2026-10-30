//! M5-3b regression tests of the second review: the timer of the quit has an
//! identity (D1780), the quit waits for the discard of a stale file (D1781),
//! the discard is serialised per file (D1779), the quit continues once
//! (D1782) and the answers of a gone cache are ignored (D1773).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::{BasesMessage, CacheFill, CacheIssue, CacheStatus, ListMessage};
use tempfile::tempdir;

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

fn generation_of(effects: &[Effect]) -> u64 {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::TreeCacheDeadline { generation, .. } => Some(*generation),
            _ => None,
        })
        .expect("the timer of the cache")
}

/// Flags game 1 as deleted and returns the effects the answer produces
/// (the discard among them), none of them run.
fn change_base_without_running_the_discard(rig: &mut Rig, slot: SlotId) -> Vec<Effect> {
    rig.send(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1],
    }));
    let jobs = rig.raw(BasesMessage::List(ListMessage::ToggleDeleted { slot }));
    let mut out = Vec::new();
    for job in jobs {
        let answer = rig.perform(job);
        out.extend(rig.raw(answer));
    }
    assert!(
        out.iter()
            .any(|e| matches!(e, Effect::TreeCacheDiscard { .. })),
        "the change must ask for a discard"
    );
    out
}

fn is_discard(effect: &Effect) -> bool {
    matches!(effect, Effect::TreeCacheDiscard { .. })
}

// ---- D1780: the timer has an identity ----

#[test]
fn a_timer_of_another_wait_does_not_end_the_wait_of_the_cache() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = rig_with_tree(dir.path());
    rig.send(TreeMessage::SetCacheAutosave(true));
    let quit = rig.raw(Message::Quit);
    let generation = generation_of(&quit);
    // An old timer of the list or of a batch (no identity) and a timer of
    // another generation: neither leaves.
    assert!(!rig.raw(BasesMessage::QuitDeadline).contains(&Effect::Exit));
    let other = rig.raw(TreeMessage::CacheDeadline(generation + 7));
    assert!(!other.contains(&Effect::Exit));
    // Its own timer does.
    assert!(
        rig.raw(TreeMessage::CacheDeadline(generation))
            .contains(&Effect::Exit)
    );
}

#[test]
fn a_timer_without_a_wait_does_nothing() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = rig_with_tree(dir.path());
    assert!(rig.raw(TreeMessage::CacheDeadline(1)).is_empty());
}

// ---- D1781: the quit waits for the discard ----

#[test]
fn the_quit_waits_for_the_discard_of_a_stale_file() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = rig_with_tree(dir.path());
    rig.send(TreeMessage::SaveCache);
    let stc = dir.path().join("std.stc");
    assert!(stc.exists());
    let effects = change_base_without_running_the_discard(&mut rig, slot);
    let (discards, rest): (Vec<_>, Vec<_>) = effects.into_iter().partition(is_discard);
    let quit = rig.raw(Message::Quit);
    assert!(
        !quit.contains(&Effect::Exit),
        "the stale file is still there"
    );
    assert!(stc.exists());
    let generation = generation_of(&quit);
    assert!(generation > 0);
    let mut host = run_all(&mut rig, discards);
    host.extend(run_all(&mut rig, rest));
    assert!(host.contains(&Effect::Exit));
    assert!(!stc.exists(), "the file of the old content is gone at exit");
}

#[test]
fn a_discard_that_never_answers_ends_with_the_deadline() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = rig_with_tree(dir.path());
    rig.send(TreeMessage::SaveCache);
    let _held = change_base_without_running_the_discard(&mut rig, slot);
    let quit = rig.raw(Message::Quit);
    assert!(!quit.contains(&Effect::Exit));
    let generation = generation_of(&quit);
    assert!(
        rig.raw(TreeMessage::CacheDeadline(generation))
            .contains(&Effect::Exit)
    );
}

// ---- D1782: the quit continues once ----

#[test]
fn a_late_save_after_the_deadline_does_not_finish_the_quit_again() {
    let dir = tempdir().unwrap();
    let (mut rig, _) = rig_with_tree(dir.path());
    rig.send(TreeMessage::SetCacheAutosave(true));
    let quit = rig.raw(Message::Quit);
    let generation = generation_of(&quit);
    let saves: Vec<Effect> = quit
        .into_iter()
        .filter(|e| matches!(e, Effect::TreeCache { .. }))
        .collect();
    assert_eq!(saves.len(), 1);
    let first = rig.raw(TreeMessage::CacheDeadline(generation));
    assert_eq!(first.iter().filter(|e| **e == Effect::Exit).count(), 1);
    // The save answers late: nothing more happens.
    let mut later = Vec::new();
    for effect in saves {
        let answer = rig.perform(effect);
        later.extend(rig.raw(answer));
    }
    assert!(later.is_empty(), "{later:?}");
}

// ---- D1779: the discard is serialised per file ----

#[test]
fn a_save_of_a_cache_that_was_replaced_cannot_write_after_the_discard_of_the_next() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = rig_with_tree(dir.path());
    let stc = dir.path().join("std.stc");
    // The save of cache A is asked for and waits.
    let save_a = rig.raw(TreeMessage::SaveCache);
    // The base is closed (cache A goes) and opened again (cache B).
    rig.send(BasesMessage::Close(slot));
    rig.send(BasesMessage::Open(dir.path().join("std.si4")));
    let slot_b = rig.state.bases.current();
    rig.send(TreeMessage::Open);
    // The content changes: cache B is discarded with its file.
    rig.send(BasesMessage::List(ListMessage::SelectNumbers {
        slot: slot_b,
        numbers: vec![1],
    }));
    rig.send(BasesMessage::List(ListMessage::ToggleDeleted {
        slot: slot_b,
    }));
    assert!(!stc.exists());
    // The old save runs now: it must not put the old trees back.
    run_all(&mut rig, save_a);
    assert!(
        !stc.exists(),
        "the save of the old cache wrote the old trees"
    );
}

// ---- D1773: answers of a gone cache ----

#[test]
fn a_save_or_a_fill_of_a_gone_cache_reports_stale() {
    for fill in [false, true] {
        let dir = tempdir().unwrap();
        let (mut rig, slot) = rig_with_tree(dir.path());
        let job = if fill {
            rig.raw(TreeMessage::FillCache(CacheFill::StandardLines))
        } else {
            rig.raw(TreeMessage::SaveCache)
        };
        change_base_without_running_the_discard_and_run(&mut rig, slot);
        run_all(&mut rig, job);
        assert_eq!(
            rig.view().cache.status,
            CacheStatus::Failed(CacheIssue::Stale),
            "fill = {fill}"
        );
        assert!(rig.view().cache.busy.is_none());
    }
}

fn change_base_without_running_the_discard_and_run(rig: &mut Rig, slot: SlotId) {
    rig.send(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1],
    }));
    rig.send(BasesMessage::List(ListMessage::ToggleDeleted { slot }));
}

// ---- D1783: a notice of its own ----

#[test]
fn a_request_while_the_file_is_being_removed_says_so() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = rig_with_tree(dir.path());
    rig.send(TreeMessage::SaveCache);
    let effects = change_base_without_running_the_discard(&mut rig, slot);
    let (discards, rest): (Vec<_>, Vec<_>) = effects.into_iter().partition(is_discard);
    let started = rig.raw(TreeMessage::SaveCache);
    assert!(started.is_empty());
    assert_eq!(
        rig.view().cache.status,
        CacheStatus::Failed(CacheIssue::Discarding)
    );
    run_all(&mut rig, discards);
    run_all(&mut rig, rest);
    // After the answer the cache can be used again.
    rig.send(TreeMessage::SaveCache);
    assert!(matches!(rig.view().cache.status, CacheStatus::Saved { .. }));
}
