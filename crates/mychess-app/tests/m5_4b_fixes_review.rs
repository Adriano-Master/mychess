//! Regression tests of the review of M5-4b (spec M5-4, D1875 to D1879).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::cloned_ref_to_slice_refs
)]

mod m5_4b_common;

use m5_4b_common::*;
use mychess_app::{DiscardChoice, FillSide, MaskEffect, MaskPrefs, load_preferences};
use mychess_db::tree::mask::stm;

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn open_mask(rig: &mut Rig, path: &std::path::Path) {
    m(rig, MaskMessage::Open(Some(path.to_path_buf())));
}

fn view(rig: &Rig) -> mychess_app::MaskView {
    rig.state.tree_view().mask.expect("a mask is open")
}

fn exits(host: &[Effect]) -> bool {
    host.iter().any(|e| matches!(e, Effect::Exit))
}

/// The first effect of the mask among `effects`.
fn mask_effect(effects: Vec<Effect>) -> Effect {
    effects
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(_)))
        .expect("a mask effect")
}

fn deadlines(effects: &[Effect]) -> Vec<u64> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Mask(MaskEffect::Deadline { generation, .. }) => Some(*generation),
            _ => None,
        })
        .collect()
}

#[test]
fn a_new_mask_cannot_replace_the_one_being_written() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "d4", MaskEdit::Add);
    let write = mask_effect(rig.raw(TreeMessage::Mask(MaskMessage::Save)));
    // While the file is being written, the mask is not replaced or closed.
    for message in [
        MaskMessage::New,
        MaskMessage::Close,
        MaskMessage::Open(Some(a.clone())),
    ] {
        m(&mut rig, message);
        assert_eq!(notice(&rig), Some(MaskNotice::Busy));
        assert_eq!(view(&rig).file_name.as_deref(), Some("a.stm"));
    }
    let answer = rig.perform(write);
    rig.send(answer);
    assert!(!dirty(&rig));
}

#[test]
fn a_late_save_does_not_give_its_path_to_another_mask() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "d4", MaskEdit::Add);
    // The question "save?" is open for a new mask; a save starts; the user
    // says "don't save", so the new mask replaces the old one at once.
    m(&mut rig, MaskMessage::New);
    assert!(rig.state.mask_prompt().is_some());
    let write = mask_effect(rig.raw(TreeMessage::Mask(MaskMessage::Save)));
    m(&mut rig, MaskMessage::Answer(DiscardChoice::DontSave));
    assert_eq!(view(&rig).file_name, None);
    let answer = rig.perform(write);
    rig.send(answer);
    // The new mask is still unnamed and clean, and the next save asks for
    // a name instead of overwriting the good file with the empty mask.
    let v = view(&rig);
    assert_eq!((v.path.clone(), v.dirty), (None, false));
    let host = m(&mut rig, MaskMessage::Save);
    assert!(matches!(host.as_slice(), [Effect::PickFile(_)]), "{host:?}");
    // The file has what the save wrote (e4 and d4), not the empty mask.
    let (read, _) = stm::read(&a).unwrap();
    let start = mychess_core::Position::new();
    assert_eq!(read.position(&start).unwrap().moves.len(), 2);
}

#[test]
fn a_quit_that_waits_for_the_read_goes_on_when_it_ends() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    let read = mask_effect(rig.raw(TreeMessage::Mask(MaskMessage::Open(Some(a)))));
    let quit = rig.raw(Message::Quit);
    assert_eq!(deadlines(&quit), [1]);
    assert!(!exits(&quit));
    let answer = rig.perform(read);
    let host = rig.send(answer);
    assert!(exits(&host), "no need for the timer: {host:?}");
}

#[test]
fn a_failed_read_also_lets_the_quit_go_on() {
    let dir = dir();
    let mut rig = Rig::new();
    let read = mask_effect(rig.raw(TreeMessage::Mask(MaskMessage::Open(Some(
        dir.path().join("gone.stm"),
    )))));
    rig.raw(Message::Quit);
    let answer = rig.perform(read);
    let host = rig.send(answer);
    assert!(exits(&host), "{host:?}");
}

#[test]
fn two_quits_in_a_row_arm_two_timers_and_the_old_one_is_ignored() {
    let dir = dir();
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All)));
    let first = rig.raw(Message::Quit);
    assert_eq!(deadlines(&first), [1]);
    // The user asks again while the job is stuck: a new timer.
    let second = rig.raw(Message::Quit);
    assert_eq!(deadlines(&second), [2], "{second:?}");
    // The old timer does not end the wait; the new one does.
    let old = rig.send(TreeMessage::Mask(MaskMessage::Deadline(1)));
    assert!(!exits(&old));
    assert!(rig.state.mask_busy().is_some());
    let host = rig.send(TreeMessage::Mask(MaskMessage::Deadline(2)));
    assert!(exits(&host), "{host:?}");
}

#[test]
fn a_failed_save_during_the_quit_cancels_it_with_no_timer_left() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "d4", MaskEdit::Add);
    std::fs::remove_file(&a).unwrap();
    std::fs::create_dir(&a).unwrap();
    let write = mask_effect(rig.raw(TreeMessage::Mask(MaskMessage::Save)));
    let quit = rig.raw(Message::Quit);
    assert_eq!(deadlines(&quit), [1]);
    let answer = rig.perform(write);
    let host = rig.send(answer);
    assert!(!exits(&host));
    assert!(matches!(notice(&rig), Some(MaskNotice::SaveFailed { .. })));
    // No question was reopened by the failure, and the old timer does
    // nothing.
    assert!(rig.state.mask_prompt().is_none());
    let late = rig.send(TreeMessage::Mask(MaskMessage::Deadline(1)));
    assert!(!exits(&late));
    assert!(rig.state.mask_prompt().is_none());
    // A new quit asks, as the first one would have.
    rig.send(Message::Quit);
    assert!(rig.state.mask_prompt().is_some());
}

#[test]
fn the_mask_is_not_edited_while_it_is_written_searched_or_outlined() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "d4", MaskEdit::Add);
    let write = mask_effect(rig.raw(TreeMessage::Mask(MaskMessage::Save)));
    edit(&mut rig, "c4", MaskEdit::Add);
    assert_eq!(notice(&rig), Some(MaskNotice::Busy));
    let answer = rig.perform(write);
    rig.send(answer);
    assert!(!dirty(&rig), "the refused edit did not happen");
    // A search in flight.
    let search = mask_effect(rig.raw(TreeMessage::Mask(MaskMessage::Search(Box::default()))));
    edit(&mut rig, "c4", MaskEdit::Add);
    assert_eq!(notice(&rig), Some(MaskNotice::Busy));
    m(&mut rig, MaskMessage::SetPositionComment("x".to_owned()));
    m(&mut rig, MaskMessage::FillGame);
    assert!(!dirty(&rig));
    let answer = rig.perform(search);
    rig.send(answer);
    // An outline in flight.
    let outline = mask_effect(rig.raw(TreeMessage::Mask(MaskMessage::BuildOutline)));
    edit(&mut rig, "c4", MaskEdit::Add);
    assert!(!dirty(&rig));
    let answer = rig.perform(outline);
    rig.send(answer);
    edit(&mut rig, "c4", MaskEdit::Add);
    assert!(dirty(&rig));
}

#[test]
fn an_edit_marks_the_search_and_the_outline_as_old() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    m(&mut rig, MaskMessage::Search(Box::default()));
    m(&mut rig, MaskMessage::BuildOutline);
    assert!(!rig.state.mask_search().unwrap().stale);
    assert!(!rig.state.mask_outline().unwrap().stale);
    edit(&mut rig, "d4", MaskEdit::Add);
    assert!(rig.state.mask_search().unwrap().stale);
    assert!(rig.state.mask_outline().unwrap().stale);
}

#[test]
fn after_the_quit_decided_the_mask_takes_no_edit_or_new_mask() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    rig.send(Message::Quit);
    let host = m(&mut rig, MaskMessage::Answer(DiscardChoice::DontSave));
    assert!(exits(&host));
    edit(&mut rig, "d4", MaskEdit::Add);
    assert_eq!(notice(&rig), Some(MaskNotice::Busy));
    m(&mut rig, MaskMessage::New);
    assert_eq!(notice(&rig), Some(MaskNotice::Busy));
    // A new quit starts over.
    let host = rig.send(Message::Quit);
    assert!(rig.state.mask_prompt().is_some(), "{host:?}");
}

#[test]
fn the_mask_is_not_edited_or_filled_while_the_question_is_open() {
    let dir = dir();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    m(&mut rig, MaskMessage::New);
    assert!(rig.state.mask_prompt().is_some());
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All)));
    assert!(effects.is_empty());
    assert_eq!(notice(&rig), Some(MaskNotice::Busy));
    edit(&mut rig, "d4", MaskEdit::Add);
    assert_eq!(notice(&rig), Some(MaskNotice::Busy));
}

#[test]
fn the_fill_waits_for_a_file_dialog() {
    let dir = dir();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    m(&mut rig, MaskMessage::New);
    m(&mut rig, MaskMessage::SaveAs);
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All)));
    assert!(effects.is_empty());
}

#[test]
fn the_options_keep_one_remark_for_every_path_that_is_not_absolute() {
    let text = "[tree_mask]\nrecent = ['a.stm', 'b.stm', 7]\n";
    let loaded = load_preferences(text);
    assert!(loaded.prefs.mask.recent.is_empty());
    assert_eq!(loaded.issues.len(), 1, "{:?}", loaded.issues);
    assert_eq!(*loaded.prefs.mask, MaskPrefs::default());
}

#[test]
fn a_game_with_too_many_nodes_is_not_copied_in_update() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    let mut game = mychess_core::Game::new();
    let mut node = game.root();
    let mut pos = mychess_core::Position::new();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    for i in 0..100_002 {
        let mv = pos.parse_uci(cycle[i % 4]).unwrap();
        pos.play(mv).unwrap();
        node = game.add_move(node, mv).unwrap();
    }
    rig.state.session.replace_game(game);
    m(&mut rig, MaskMessage::FillGame);
    assert_eq!(notice(&rig), Some(MaskNotice::GameTooLarge));
    assert!(!dirty(&rig));
    m(&mut rig, MaskMessage::FillLine(FillSide::All));
    assert_eq!(notice(&rig), Some(MaskNotice::GameTooLarge));
}
