//! M5-2c contract tests, app side: "open base as tree" (spec
//! `m5-2-janela-da-arvore.md`, D1640-D1642; Scid `::file::openBaseAsTree`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::{PickKind, PickSpec};
use tempfile::tempdir;

const OTHER: &str = "[Result \"1-0\"]\n\n1. c4 c5 1-0\n\n";

/// Sends `OpenBaseAsTree`, answers the dialog with `path` and runs the rest.
/// Returns the host effects after the answer.
fn open_as_tree(rig: &mut Rig, path: Option<&std::path::Path>) -> Vec<Effect> {
    let host = rig.send(TreeMessage::OpenBaseAsTree);
    let spec = pick_spec(&host);
    rig.send(Message::FilePicked(spec.request, path.map(Into::into)))
}

fn pick_spec(effects: &[Effect]) -> PickSpec {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec.clone()),
            _ => None,
        })
        .expect("a file dialog is requested")
}

fn slots_open(rig: &Rig) -> usize {
    SlotId::all()
        .filter(|s| !s.is_clipbase() && rig.state.bases.slot(*s).is_some())
        .count()
}

fn write(dir: &std::path::Path, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    path
}

#[test]
fn the_command_asks_for_a_file_of_the_open_tree_base_kind() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let host = rig.send(TreeMessage::OpenBaseAsTree);
    assert_eq!(pick_spec(&host).kind, PickKind::OpenTreeBase);
    assert_eq!(slots_open(&rig), 1, "nothing is opened before the answer");
    assert!(!rig.state.tree().locked());
}

#[test]
fn a_cancelled_dialog_changes_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let current = rig.state.bases.current();
    open_as_tree(&mut rig, None);
    assert_eq!(slots_open(&rig), 1);
    assert_eq!(rig.state.bases.current(), current);
    assert!(!rig.state.tree().locked());
    assert!(!rig.state.take_tree_show_request());
}

#[test]
fn the_file_opens_in_a_free_slot_the_tree_locks_on_it_and_the_current_base_stays() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let first = rig.state.bases.current();
    let other = write(dir.path(), "b.pgn", OTHER);
    open_as_tree(&mut rig, Some(&other));
    assert_eq!(slots_open(&rig), 2);
    assert_eq!(rig.state.bases.current(), first, "the current base is back");
    assert!(rig.state.tree().locked());
    assert!(rig.state.tree().is_open());
    let view = rig.view();
    assert!(view.locked);
    assert_eq!(view.target_name.as_deref(), Some("b"));
    assert_eq!(view.total.map(|t| t.games), Some(1), "the table is of `b`");
}

#[test]
fn the_panel_is_shown_by_a_request_that_is_read_once() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    assert!(!rig.state.take_tree_show_request());
    let other = write(dir.path(), "b.pgn", OTHER);
    open_as_tree(&mut rig, Some(&other));
    assert!(rig.state.take_tree_show_request());
    assert!(!rig.state.take_tree_show_request());
}

#[test]
fn the_panel_opens_even_if_it_was_closed() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "a.pgn", STD);
    assert!(!rig.state.tree().is_open());
    let other = write(dir.path(), "b.pgn", OTHER);
    open_as_tree(&mut rig, Some(&other));
    assert!(rig.state.tree().is_open());
    assert!(rig.state.take_tree_show_request());
    assert_eq!(rig.view().total.map(|t| t.games), Some(1));
}

#[test]
fn the_game_being_edited_is_untouched_and_nothing_is_asked() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    let fen = rig.state.session.position().fen();
    let other = write(dir.path(), "b.pgn", OTHER);
    open_as_tree(&mut rig, Some(&other));
    assert!(rig.state.discard_prompt.is_none());
    assert_eq!(rig.state.session.position().fen(), fen);
    assert_eq!(rig.ply(), 1);
}

#[test]
fn the_position_of_the_tree_is_still_the_one_of_the_current_game() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("c2c4");
    let other = write(dir.path(), "b.pgn", OTHER);
    open_as_tree(&mut rig, Some(&other));
    // `b` plays 1.c4 c5: the table has the game, with the move c5.
    assert_eq!(rig.sans(), ["c5"]);
}

#[test]
fn a_file_that_is_already_open_is_only_locked() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_pgn(dir.path(), "a.pgn", STD);
    let b_path = write(dir.path(), "b.pgn", OTHER);
    rig.send(BasesMessage::Open(b_path.clone()));
    let b = rig.state.bases.current();
    rig.send(BasesMessage::Select(a));
    assert_eq!(slots_open(&rig), 2);
    open_as_tree(&mut rig, Some(&b_path));
    assert_eq!(slots_open(&rig), 2, "no second copy");
    assert_eq!(rig.state.bases.current(), a, "the current base is the same");
    assert!(rig.state.tree().locked());
    assert!(rig.state.tree().is_open());
    assert!(rig.state.take_tree_show_request());
    assert_eq!(rig.view().target_name.as_deref(), Some("b"));
    let _ = b;
}

#[test]
fn the_file_open_in_the_current_slot_is_locked_without_a_move_of_slot() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let path = dir.path().join("a.pgn");
    std::fs::write(&path, STD).unwrap();
    rig.send(BasesMessage::Open(path.clone()));
    let a = rig.state.bases.current();
    open_as_tree(&mut rig, Some(&path));
    assert_eq!(slots_open(&rig), 1);
    assert_eq!(rig.state.bases.current(), a);
    assert!(rig.state.tree().locked());
    assert_eq!(rig.view().total.map(|t| t.games), Some(5));
}

#[test]
fn it_works_with_no_base_open_and_gives_the_editor_its_base_back() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let before = rig.state.bases.current();
    let other = write(dir.path(), "b.pgn", OTHER);
    open_as_tree(&mut rig, Some(&other));
    assert_eq!(rig.state.bases.current(), before);
    assert!(rig.state.tree().locked());
    assert_eq!(rig.view().total.map(|t| t.games), Some(1));
}

#[test]
fn a_scid_database_opens_as_a_tree_too() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let first = rig.state.bases.current();
    let si4 = make_si4(dir.path(), "s", OTHER);
    open_as_tree(&mut rig, Some(&si4));
    assert_eq!(rig.state.bases.current(), first);
    assert!(rig.state.tree().locked());
    assert_eq!(rig.view().target_name.as_deref(), Some("s"));
    assert_eq!(rig.view().total.map(|t| t.games), Some(1));
}

#[test]
fn a_pgn_opens_as_a_read_only_slot_even_with_the_pgn_path_of_m2_on() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.state.open_pgn_in_slots = false;
    let other = write(dir.path(), "b.pgn", OTHER);
    let before = slots_open(&rig);
    open_as_tree(&mut rig, Some(&other));
    assert_eq!(slots_open(&rig), before + 1, "always in a slot (D1642)");
    assert!(rig.state.tree().locked());
    assert_eq!(rig.view().total.map(|t| t.games), Some(1));
}

#[test]
fn a_file_that_does_not_open_notifies_and_locks_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let missing = dir.path().join("nope.pgn");
    open_as_tree(&mut rig, Some(&missing));
    assert!(!rig.state.tree().locked());
    assert!(!rig.state.take_tree_show_request());
    assert_eq!(slots_open(&rig), 1);
    assert!(rig.state.notice.is_some(), "the usual notice of the bases");
    assert_eq!(rig.view().total.map(|t| t.games), Some(5));
}

#[test]
fn a_failed_open_does_not_lock_a_later_normal_open_of_the_same_file() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let path = dir.path().join("late.pgn");
    open_as_tree(&mut rig, Some(&path));
    assert!(!rig.state.tree().locked());
    std::fs::write(&path, OTHER).unwrap();
    rig.send(BasesMessage::Open(path));
    assert!(!rig.state.tree().locked(), "an ordinary open does not lock");
}

#[test]
fn without_a_free_slot_the_notice_is_the_normal_one_and_nothing_is_locked() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let mut n = 0;
    while rig.state.bases.first_free().is_some() {
        n += 1;
        assert!(n < 20, "the slots never fill");
        rig.open_pgn(dir.path(), &format!("f{n}.pgn"), OTHER);
    }
    let current = rig.state.bases.current();
    let count = slots_open(&rig);
    let extra = write(dir.path(), "extra.pgn", OTHER);
    open_as_tree(&mut rig, Some(&extra));
    assert_eq!(slots_open(&rig), count);
    assert_eq!(rig.state.bases.current(), current);
    assert!(!rig.state.tree().locked());
    assert!(rig.state.notice.is_some());
}

#[test]
fn opening_as_tree_twice_moves_the_lock_to_the_second_file() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let first = rig.state.bases.current();
    let b = write(dir.path(), "b.pgn", OTHER);
    let c = write(dir.path(), "c.pgn", "[Result \"0-1\"]\n\n1. d4 d5 0-1\n\n");
    open_as_tree(&mut rig, Some(&b));
    open_as_tree(&mut rig, Some(&c));
    assert_eq!(rig.state.bases.current(), first);
    assert_eq!(rig.view().target_name.as_deref(), Some("c"));
    assert_eq!(slots_open(&rig), 3);
}

#[test]
fn closing_the_opened_slot_drops_the_lock() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let b = write(dir.path(), "b.pgn", OTHER);
    open_as_tree(&mut rig, Some(&b));
    let target = rig.state.bases.find_by_path(&b).expect("open");
    rig.send(BasesMessage::Close(target));
    assert!(!rig.state.tree().locked());
    assert_eq!(rig.view().total.map(|t| t.games), Some(5));
}

#[test]
fn a_stale_pick_answer_is_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let host = rig.send(TreeMessage::OpenBaseAsTree);
    let first = pick_spec(&host);
    // A second request replaces the first.
    let host = rig.send(TreeMessage::OpenBaseAsTree);
    let second = pick_spec(&host);
    assert_ne!(first.request, second.request);
    let b = write(dir.path(), "b.pgn", OTHER);
    rig.send(Message::FilePicked(first.request, Some(b)));
    assert!(!rig.state.tree().locked());
    assert_eq!(slots_open(&rig), 1);
}
