//! M5-2c contract tests, app side: the *Lock* of the tree on a database
//! (spec `m5-2-janela-da-arvore.md`, D960, D1630-D1632; Scid `tree(locked$n)`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::{BestMessage, DiscardChoice};
use tempfile::tempdir;

const OTHER: &str = "[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n";

/// `a.pgn` (the standard five games) and `b.pgn` (one game), both open,
/// `a` current, the tree open. Returns the rig and the two slots.
fn two(dir: &std::path::Path) -> (Rig, SlotId, SlotId) {
    let mut rig = Rig::new();
    let a = rig.open_pgn(dir, "a.pgn", STD);
    let b = rig.open_pgn(dir, "b.pgn", OTHER);
    rig.send(BasesMessage::Select(a));
    rig.open_tree();
    (rig, a, b)
}

fn games(rig: &Rig) -> Option<u32> {
    rig.view().total.map(|t| t.games)
}

#[test]
fn without_a_lock_the_tree_follows_the_current_base() {
    let dir = tempdir().unwrap();
    let (mut rig, a, b) = two(dir.path());
    assert!(!rig.view().locked);
    assert_eq!(games(&rig), Some(5));
    rig.send(BasesMessage::Select(b));
    assert_eq!(games(&rig), Some(1));
    rig.send(BasesMessage::Select(a));
    assert_eq!(games(&rig), Some(5));
}

#[test]
fn locking_fixes_the_target_on_the_current_base() {
    let dir = tempdir().unwrap();
    let (mut rig, _, _) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    let view = rig.view();
    assert!(view.locked);
    assert!(rig.state.tree().locked());
    assert_eq!(view.target_name.as_deref(), Some("a"));
    assert!(!view.target_is_clipbase);
}

#[test]
fn locking_does_not_search_again() {
    let dir = tempdir().unwrap();
    let (mut rig, _, _) = two(dir.path());
    let started = rig.started();
    rig.send(TreeMessage::SetLocked(true));
    assert_eq!(rig.started(), started);
}

#[test]
fn switching_the_current_base_does_not_change_the_table_or_the_search() {
    let dir = tempdir().unwrap();
    let (mut rig, _, b) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    let before = rig.view();
    let started = rig.started();
    rig.send(BasesMessage::Select(b));
    assert_eq!(rig.state.bases.current(), b);
    let after = rig.view();
    // The mark of "next move of the game" follows the game, not the base.
    let key = |v: &TreeView| -> Vec<(String, u32, [u32; 4])> {
        v.rows
            .iter()
            .map(|r| (r.san.clone(), r.games, r.results))
            .collect()
    };
    assert_eq!(key(&after), key(&before));
    assert_eq!(after.total, before.total);
    assert_eq!(after.target_name.as_deref(), Some("a"));
    assert_eq!(rig.started(), started, "no new search");
    assert!(!after.searching);
}

#[test]
fn unlocking_goes_back_to_following_the_current_base() {
    let dir = tempdir().unwrap();
    let (mut rig, _, b) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    rig.send(BasesMessage::Select(b));
    assert_eq!(games(&rig), Some(5));
    rig.send(TreeMessage::SetLocked(false));
    assert!(!rig.view().locked);
    assert_eq!(games(&rig), Some(1));
    assert_eq!(rig.view().target_name.as_deref(), Some("b"));
}

#[test]
fn locking_twice_or_unlocking_without_a_lock_changes_nothing() {
    let dir = tempdir().unwrap();
    let (mut rig, _, _) = two(dir.path());
    rig.send(TreeMessage::SetLocked(false));
    assert!(!rig.state.tree().locked());
    rig.send(TreeMessage::SetLocked(true));
    rig.send(TreeMessage::SetLocked(true));
    assert!(rig.state.tree().locked());
    assert_eq!(games(&rig), Some(5));
}

#[test]
fn locking_again_while_another_base_is_current_moves_the_lock_to_it() {
    let dir = tempdir().unwrap();
    let (mut rig, _, b) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    rig.send(BasesMessage::Select(b));
    rig.send(TreeMessage::SetLocked(true));
    assert_eq!(rig.view().target_name.as_deref(), Some("b"));
    assert_eq!(games(&rig), Some(1));
}

#[test]
fn the_position_always_comes_from_the_current_game() {
    let dir = tempdir().unwrap();
    let (mut rig, _, b) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    rig.send(BasesMessage::Select(b));
    // Selecting `b` loaded its game (1.e4 e5); go to the position after 1.e4.
    rig.nav(Nav::Forward(1));
    assert_eq!(rig.ply(), 1);
    // The locked base `a` is searched at 1.e4: games 1, 3 and 5.
    assert_eq!(games(&rig), Some(3));
    assert_eq!(rig.sorted_sans(), ["[end]", "c5", "e5"]);
}

#[test]
fn a_click_on_a_row_plays_in_the_current_game_whatever_the_target() {
    let dir = tempdir().unwrap();
    let (mut rig, _, b) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    rig.send(BasesMessage::Select(b));
    rig.nav(Nav::Forward(1));
    let index = rig.row("e5").index;
    rig.send(TreeMessage::RowClicked(index));
    assert_eq!(rig.ply(), 2);
    assert!(rig.state.session.position().fen().contains("/4p3/4P3/"));
}

#[test]
fn the_filter_adjust_acts_on_the_filter_of_the_target_only() {
    let dir = tempdir().unwrap();
    let (mut rig, a, b) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    rig.send(BasesMessage::Select(b));
    rig.nav(Nav::Forward(1));
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filter_numbers(a), [1, 3, 5], "the target is adjusted");
    assert_eq!(rig.filtered(b), 1, "the current base keeps its filter");
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filtered(a), 5, "closing gives the old filter back");
    assert_eq!(rig.filtered(b), 1);
}

#[test]
fn the_best_games_of_a_locked_tree_are_of_the_target() {
    let dir = tempdir().unwrap();
    let (mut rig, _, b) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    rig.send(BasesMessage::Select(b));
    rig.send(TreeMessage::Best(BestMessage::Toggle));
    let rows = rig.view().best.unwrap().rows;
    assert_eq!(rows.len(), 5, "the five games of `a`, not the one of `b`");
}

#[test]
fn the_training_of_a_locked_tree_uses_the_target_and_needs_it_usable() {
    let dir = tempdir().unwrap();
    let (mut rig, _, _) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    // The current base is the Clipbase, empty; the target is `a`.
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    assert!(rig.view().training_available);
    rig.send(TreeMessage::SetTraining(true));
    assert!(rig.state.tree().training());
    rig.play("e2e4");
    assert_eq!(rig.ply(), 2, "answered from the games of `a`");
}

#[test]
fn locking_on_the_clipbase_says_so() {
    let dir = tempdir().unwrap();
    let (mut rig, _, _) = two(dir.path());
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.send(TreeMessage::SetLocked(true));
    assert!(rig.view().locked);
    assert!(rig.view().target_is_clipbase);
    assert_eq!(rig.view().target_name, None);
}

#[test]
fn the_lock_falls_when_the_slot_closes() {
    let dir = tempdir().unwrap();
    let (mut rig, a, b) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    rig.send(BasesMessage::Select(b));
    rig.send(BasesMessage::Close(a));
    assert!(!rig.state.tree().locked());
    assert!(!rig.view().locked);
    // The tree follows the current base again.
    assert_eq!(games(&rig), Some(1));
    assert_eq!(rig.view().target_name.as_deref(), Some("b"));
}

#[test]
fn the_lock_falls_when_the_slot_holds_another_base() {
    let dir = tempdir().unwrap();
    let (mut rig, a, _) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    rig.send(BasesMessage::Close(a));
    // The first free slot is the one `a` had.
    let c = rig.open_pgn(dir.path(), "c.pgn", OTHER);
    assert_eq!(c, a, "precondition: the slot is reused");
    assert!(!rig.state.tree().locked(), "another base, no lock");
    assert_eq!(rig.view().target_name.as_deref(), Some("c"));
}

#[test]
fn closing_a_locked_base_with_unsaved_changes_asks_first_and_the_lock_stays_until_it_closes() {
    let dir = tempdir().unwrap();
    let (mut rig, a, b) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    // Load a game of `a` and alter it.
    rig.send(BasesMessage::LoadGame { slot: a, number: 1 });
    rig.nav(Nav::GameEnd);
    rig.play("b8c6");
    rig.send(BasesMessage::Select(b));
    rig.send(BasesMessage::Select(a));
    rig.send(BasesMessage::Close(a));
    assert!(rig.state.discard_prompt.is_some(), "the usual question");
    assert!(rig.state.tree().locked(), "still asking");
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(rig.state.tree().locked(), "cancelled: the base is open");
    rig.send(BasesMessage::Close(a));
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert!(rig.state.bases.slot(a).is_none());
    assert!(!rig.state.tree().locked());
}

#[test]
fn the_lock_survives_closing_and_reopening_the_panel() {
    let dir = tempdir().unwrap();
    let (mut rig, _, b) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    rig.send(TreeMessage::Close);
    rig.send(BasesMessage::Select(b));
    rig.send(TreeMessage::Open);
    if rig.state.tree().locked() {
        assert_eq!(games(&rig), Some(5));
    } else {
        assert_eq!(games(&rig), Some(1));
    }
}

#[test]
fn a_late_answer_for_the_old_target_is_not_shown_after_unlocking() {
    let dir = tempdir().unwrap();
    let (mut rig, _, b) = two(dir.path());
    rig.send(TreeMessage::SetLocked(true));
    rig.send(BasesMessage::Select(b));
    // A search of the locked base at 1.e4 flies; the user unlocks meanwhile.
    let mut sent = jobs(rig.raw(GameMessage::Navigate(Nav::Forward(1))));
    assert_eq!(sent.len(), 1);
    let old = sent.remove(0);
    let mut queue = rig.raw(TreeMessage::SetLocked(false));
    let answer = rig.run_job(old);
    queue.extend(rig.raw(answer));
    while let Some(effect) = queue.pop() {
        if effect.runs_on_host() {
            continue;
        }
        let message = rig.perform(effect);
        queue.extend(rig.raw(message));
    }
    // Unlocked: the table is of `b` (one game), not of `a` (three).
    assert_eq!(games(&rig), Some(1));
    assert_eq!(rig.view().target_name.as_deref(), Some("b"));
}

#[test]
fn locking_without_any_base_open_locks_nothing_harmful() {
    let mut rig = Rig::new();
    rig.send(TreeMessage::SetLocked(true));
    rig.send(TreeMessage::Open);
    // Whatever the state, the panel does not claim a base that is not there.
    assert_ne!(rig.view().body, TreeBody::Rows);
}
