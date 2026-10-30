//! Extra contract tests of the questions about saving and of the quit of the
//! tree masks (spec M5-4, slice M5-4b: D1817, D1818, D1869, D1870, D1873,
//! D1874): the gate in `continue_quit`, the identity of the timer, the quit
//! that finishes once, the edit made while the file is written, and the
//! combination with the other things the quit waits for.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::large_futures, // the rig owns the whole `AppState`
    clippy::cloned_ref_to_slice_refs
)]

mod m5_4b_common;

use m5_4b_common::*;
use mychess_app::{BasesMessage, DiscardChoice, FillSide, MaskEffect, MaskPending, SlotId};
use mychess_db::tree::mask::stm;
use tempfile::TempDir;

fn dir() -> TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn exits(effects: &[Effect]) -> usize {
    effects.iter().filter(|e| matches!(e, Effect::Exit)).count()
}

fn mask_deadline(effects: &[Effect]) -> Option<u64> {
    effects.iter().find_map(|e| match e {
        Effect::Mask(MaskEffect::Deadline { generation, .. }) => Some(*generation),
        _ => None,
    })
}

fn fill_job(effects: Vec<Effect>) -> Effect {
    effects
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(MaskEffect::Fill { .. })))
        .expect("a fill job")
}

fn write_job(effects: Vec<Effect>) -> Effect {
    effects
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(MaskEffect::Write { .. })))
        .expect("a write job")
}

fn deadline(rig: &mut Rig, generation: u64) -> Vec<Effect> {
    rig.raw(TreeMessage::Mask(MaskMessage::Deadline(generation)))
}

fn positions(rig: &Rig) -> usize {
    rig.state.tree_view().mask.expect("a mask").positions
}

fn rig_with_dirty_mask(dir: &std::path::Path) -> Rig {
    let mut rig = Rig::std_with_tree(dir);
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    assert!(dirty(&rig));
    rig
}

// ---- the identity of the timer (D1874) ----

#[test]
fn a_timer_of_an_earlier_wait_does_not_end_the_next_wait() {
    let dir = dir();
    let mut rig = rig_with_dirty_mask(dir.path());
    // First quit: it waits for a fill, the timer of generation `first` is out.
    let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All))));
    let quit = rig.raw(Message::Quit);
    let first = mask_deadline(&quit).expect("a timer for the first wait");
    // The fill ends, the user is asked, and gives up the quit.
    let answer = rig.perform(job);
    rig.raw(answer);
    assert!(rig.state.mask_prompt().is_some());
    rig.raw(TreeMessage::Mask(MaskMessage::Answer(
        DiscardChoice::Cancel,
    )));
    // A second fill and a second quit: a new wait with its own timer.
    let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All))));
    let quit = rig.raw(Message::Quit);
    assert!(!quit.contains(&Effect::Exit));
    let second = mask_deadline(&quit).expect("the second wait arms its own timer");
    assert_ne!(first, second, "the timers must not share an identity");
    // The old timer arrives: it ends nothing.
    let stale = deadline(&mut rig, first);
    assert!(stale.is_empty(), "{stale:?}");
    assert!(
        rig.state.mask_busy().is_some(),
        "the fill is still waited for"
    );
    assert!(rig.state.mask_prompt().is_none());
    // Its own timer gives the job up: the quit goes on to the question.
    let own = deadline(&mut rig, second);
    assert!(!own.contains(&Effect::Exit), "the mask is altered: ask");
    assert!(rig.state.mask_prompt().is_some());
    drop(job);
}

#[test]
fn a_timer_of_the_list_or_of_the_cache_does_not_end_the_wait_of_the_mask() {
    let dir = dir();
    let mut rig = rig_with_dirty_mask(dir.path());
    rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All)));
    let quit = rig.raw(Message::Quit);
    let generation = mask_deadline(&quit).expect("timer");
    assert!(
        !rig.raw(BasesMessage::QuitDeadline).contains(&Effect::Exit),
        "the generic timer of the list"
    );
    assert!(
        !rig.raw(TreeMessage::CacheDeadline(generation))
            .contains(&Effect::Exit),
        "the timer of the cache has the same number"
    );
    assert!(rig.state.mask_busy().is_some());
    assert!(deadline(&mut rig, generation + 9).is_empty());
    assert!(rig.state.mask_busy().is_some());
}

#[test]
fn a_timer_with_no_wait_does_nothing() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    for generation in [0, 1, 2, u64::MAX] {
        assert!(deadline(&mut rig, generation).is_empty());
    }
}

#[test]
fn the_deadline_of_a_wait_that_already_ended_does_not_finish_the_quit_again() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::Open(Some(a)));
    edit(&mut rig, "d4", MaskEdit::Add);
    // A save is in flight when the quit starts: the quit waits for it.
    let write = write_job(rig.raw(TreeMessage::Mask(MaskMessage::Save)));
    let quit = rig.raw(Message::Quit);
    assert!(!quit.contains(&Effect::Exit));
    let generation = mask_deadline(&quit).expect("timer");
    // The save ends in time: the quit finishes, once.
    let answer = rig.perform(write);
    let first = rig.raw(answer);
    assert_eq!(exits(&first), 1, "{first:?}");
    // The timer of that wait arrives late: nothing more.
    let late = deadline(&mut rig, generation);
    assert_eq!(exits(&late), 0, "finish_quit must not run twice: {late:?}");
}

#[test]
fn a_late_answer_after_the_deadline_does_not_finish_the_quit_again() {
    let dir = dir();
    let mut rig = Rig::std_with_tree(dir.path());
    m(&mut rig, MaskMessage::New);
    let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All))));
    let quit = rig.raw(Message::Quit);
    let generation = mask_deadline(&quit).expect("timer");
    let first = deadline(&mut rig, generation);
    assert_eq!(exits(&first), 1, "{first:?}");
    let answer = rig.perform(job);
    let late = rig.raw(answer);
    assert!(late.is_empty(), "{late:?}");
    assert!(!dirty(&rig), "the given-up job changed nothing");
}

#[test]
fn quit_with_a_read_in_flight_waits_and_a_late_answer_is_dropped() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::Open(Some(a))));
    let read = effects
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(MaskEffect::Read { .. })))
        .expect("read");
    let quit = rig.raw(Message::Quit);
    assert!(!quit.contains(&Effect::Exit));
    let generation = mask_deadline(&quit).expect("timer");
    let out = deadline(&mut rig, generation);
    assert_eq!(exits(&out), 1, "{out:?}");
    let answer = rig.perform(read);
    let late = rig.raw(answer);
    assert!(late.is_empty(), "{late:?}");
    assert!(!rig.state.mask_open(), "a read given up is not installed");
}

#[test]
fn the_deadline_with_an_altered_mask_asks_instead_of_leaving() {
    let dir = dir();
    let mut rig = rig_with_dirty_mask(dir.path());
    let job = fill_job(rig.raw(TreeMessage::Mask(MaskMessage::FillFilter(FillSide::All))));
    let quit = rig.raw(Message::Quit);
    let generation = mask_deadline(&quit).expect("timer");
    let out = deadline(&mut rig, generation);
    assert_eq!(exits(&out), 0);
    let prompt = rig.state.mask_prompt().expect("the question").clone();
    assert_eq!(prompt.then, MaskPending::Quit);
    // The given-up fill answers while the question is open: dropped.
    let before = positions(&rig);
    let answer = rig.perform(job);
    rig.raw(answer);
    assert_eq!(positions(&rig), before);
    let out = rig.raw(TreeMessage::Mask(MaskMessage::Answer(
        DiscardChoice::DontSave,
    )));
    assert_eq!(exits(&out), 1, "{out:?}");
}

// ---- the questions ----

/// Tries an edit while a write is in flight. D1870: an edit made during the
/// write is kept and the mask asks again; the implementation may also refuse
/// the edit (`Busy`). Either way the edit must never be lost silently.
/// Returns whether the edit was accepted.
fn edit_during_write(rig: &mut Rig) -> bool {
    let before = rig.state.tree_view().mask.expect("a mask").positions;
    edit(rig, "Nf3", MaskEdit::Add);
    let after = rig.state.tree_view().mask.expect("a mask").positions;
    let accepted = after != before;
    assert!(
        accepted || notice(rig) == Some(MaskNotice::Busy),
        "a refused edit says so: {:?}",
        notice(rig)
    );
    accepted
}

#[test]
fn a_save_with_an_edit_made_during_the_write_asks_again() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    let b = dir.path().join("b.stm");
    mask_with(&a, "e4");
    mask_with(&b, "d4");
    for then in [
        MaskMessage::Open(Some(b.clone())),
        MaskMessage::New,
        MaskMessage::Close,
    ] {
        mask_with(&a, "e4");
        let mut rig = Rig::new();
        m(&mut rig, MaskMessage::Open(Some(a.clone())));
        edit(&mut rig, "c4", MaskEdit::Add);
        rig.raw(TreeMessage::Mask(then.clone()));
        assert!(
            rig.state.mask_prompt().is_some(),
            "{then:?} {:?}",
            rig.state.notice
        );
        let write = write_job(rig.raw(TreeMessage::Mask(MaskMessage::Answer(DiscardChoice::Save))));
        let accepted = edit_during_write(&mut rig);
        let answer = rig.perform(write);
        let out = rig.raw(answer);
        if accepted {
            assert!(
                out.iter()
                    .all(|e| !matches!(e, Effect::Mask(MaskEffect::Read { .. }))),
                "{then:?}: an edit that is not in the file must not be dropped: {out:?}"
            );
            assert!(dirty(&rig), "{then:?}: the edit is not in the file");
            let prompt = rig
                .state
                .mask_prompt()
                .unwrap_or_else(|| panic!("{then:?}: asked again"));
            assert_eq!(prompt.name.as_deref(), Some("a.stm"));
            rig.raw(TreeMessage::Mask(MaskMessage::Answer(
                DiscardChoice::Cancel,
            )));
            assert_eq!(view_name(&rig).as_deref(), Some("a.stm"));
            assert!(dirty(&rig));
        } else {
            // Refused: the file has everything the mask has, and it went on.
            let (saved, _) = stm::read(&a).unwrap();
            assert_eq!(
                saved
                    .position(&mychess_core::Position::new())
                    .unwrap()
                    .moves
                    .len(),
                2,
                "{then:?}: e4 and c4 are in the file"
            );
        }
    }
}

fn view_name(rig: &Rig) -> Option<String> {
    rig.state.tree_view().mask.expect("a mask").file_name
}

#[test]
fn a_save_with_an_edit_made_during_the_write_asks_again_on_quit() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::Open(Some(a.clone())));
    edit(&mut rig, "c4", MaskEdit::Add);
    rig.raw(Message::Quit);
    let write = write_job(rig.raw(TreeMessage::Mask(MaskMessage::Answer(DiscardChoice::Save))));
    let accepted = edit_during_write(&mut rig);
    let answer = rig.perform(write);
    let out = rig.raw(answer);
    if accepted {
        assert_eq!(exits(&out), 0, "the edit is not in the file: {out:?}");
        assert_eq!(
            rig.state.mask_prompt().map(|p| p.then.clone()),
            Some(MaskPending::Quit)
        );
        let out = rig.raw(TreeMessage::Mask(MaskMessage::Answer(
            DiscardChoice::DontSave,
        )));
        assert_eq!(exits(&out), 1);
    } else {
        assert_eq!(exits(&out), 1, "{out:?}");
    }
}

#[test]
fn a_second_request_during_a_write_never_starts_a_second_write() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::Open(Some(a)));
    edit(&mut rig, "c4", MaskEdit::Add);
    let write = write_job(rig.raw(TreeMessage::Mask(MaskMessage::Save)));
    let out = rig.raw(TreeMessage::Mask(MaskMessage::Save));
    assert!(
        out.iter()
            .all(|e| !matches!(e, Effect::Mask(MaskEffect::Write { .. }))),
        "one write at a time: {out:?}"
    );
    rig.raw(TreeMessage::Mask(MaskMessage::Close));
    if rig.state.mask_prompt().is_some() {
        let out = rig.raw(TreeMessage::Mask(MaskMessage::Answer(DiscardChoice::Save)));
        assert!(
            out.iter()
                .all(|e| !matches!(e, Effect::Mask(MaskEffect::Write { .. }))),
            "{out:?}"
        );
        assert!(rig.state.mask_prompt().is_some(), "the question stays");
    }
    let answer = rig.perform(write);
    rig.raw(answer);
    // Nothing of what was asked is lost: the mask is open or was closed on
    // purpose, never half-way.
    assert!(rig.state.mask_prompt().is_none() || rig.state.mask_open());
}

#[test]
fn a_question_that_is_open_becomes_the_question_of_the_quit() {
    let dir = dir();
    let mut rig = rig_with_dirty_mask(dir.path());
    rig.raw(TreeMessage::Mask(MaskMessage::Close));
    assert_eq!(
        rig.state.mask_prompt().map(|p| p.then.clone()),
        Some(MaskPending::Close)
    );
    rig.raw(Message::Quit);
    assert_eq!(
        rig.state.mask_prompt().map(|p| p.then.clone()),
        Some(MaskPending::Quit)
    );
    // Quitting again does not open a second question and does not leave.
    let out = rig.raw(Message::Quit);
    assert_eq!(exits(&out), 0);
    // Cancel: nothing of the close remains pending.
    rig.raw(TreeMessage::Mask(MaskMessage::Answer(
        DiscardChoice::Cancel,
    )));
    assert!(rig.state.mask_prompt().is_none());
    assert!(rig.state.mask_open() && dirty(&rig));
    // A stray answer with no question does nothing.
    let out = rig.raw(TreeMessage::Mask(MaskMessage::Answer(
        DiscardChoice::DontSave,
    )));
    assert!(out.is_empty());
    assert!(rig.state.mask_open());
}

#[test]
fn the_question_is_modal_and_ends_with_the_answer() {
    let dir = dir();
    let mut rig = rig_with_dirty_mask(dir.path());
    assert!(!rig.state.modal_open());
    rig.raw(TreeMessage::Mask(MaskMessage::New));
    assert!(rig.state.modal_open());
    // The game cannot be edited under the question.
    rig.play("e2e4");
    assert_eq!(rig.ply(), 0);
    rig.raw(TreeMessage::Mask(MaskMessage::Answer(
        DiscardChoice::Cancel,
    )));
    assert!(!rig.state.modal_open());
    rig.play("e2e4");
    assert_eq!(rig.ply(), 1);
}

#[test]
fn a_second_request_while_the_question_is_open_does_not_replace_it() {
    let dir = dir();
    let b = dir.path().join("b.stm");
    mask_with(&b, "d4");
    let mut rig = rig_with_dirty_mask(dir.path());
    rig.raw(TreeMessage::Mask(MaskMessage::New));
    rig.raw(TreeMessage::Mask(MaskMessage::Open(Some(b))));
    assert_eq!(
        rig.state.mask_prompt().map(|p| p.then.clone()),
        Some(MaskPending::New),
        "the first question is the one that was asked"
    );
}

#[test]
fn the_file_is_asked_for_before_the_question() {
    let dir = dir();
    let b = dir.path().join("b.stm");
    mask_with(&b, "d4");
    let mut rig = rig_with_dirty_mask(dir.path());
    let host = m(&mut rig, MaskMessage::Open(None));
    assert!(rig.state.mask_prompt().is_none(), "D1869: the dialog first");
    // The user gives up the dialog: no question, nothing changes.
    let out = pick(&mut rig, &host, None);
    assert!(out.is_empty());
    assert!(rig.state.mask_prompt().is_none());
    assert!(dirty(&rig));
    // A name was chosen: now the question appears.
    let host = m(&mut rig, MaskMessage::Open(None));
    pick(&mut rig, &host, Some(b.clone()));
    let prompt = rig.state.mask_prompt().expect("the question").clone();
    assert_eq!(prompt.then, MaskPending::Open(b));
}

// ---- the quit with other waits ----

#[test]
fn quit_with_an_altered_mask_and_a_cache_to_write_leaves_once() {
    let dir = dir();
    let mut rig = Rig::new();
    let slot: SlotId = rig.open_si4(dir.path(), "std", STD);
    let _ = slot;
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::SetCacheAutosave(true));
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    // The mask is asked about before anything else is written.
    let quit = rig.raw(Message::Quit);
    assert!(!quit.contains(&Effect::Exit));
    assert!(
        quit.iter().all(|e| !matches!(e, Effect::TreeCache { .. })),
        "the cache waits for the answer: {quit:?}"
    );
    assert!(rig.state.mask_prompt().is_some());
    let after = rig.raw(TreeMessage::Mask(MaskMessage::Answer(
        DiscardChoice::DontSave,
    )));
    assert_eq!(exits(&after), 0, "the cache is still being written");
    let saves: Vec<Effect> = after
        .iter()
        .filter(|e| matches!(e, Effect::TreeCache { .. }))
        .cloned()
        .collect();
    assert_eq!(saves.len(), 1, "{after:?}");
    let mut total = 0;
    for effect in saves {
        let answer = rig.perform(effect);
        total += exits(&rig.raw(answer));
    }
    assert_eq!(total, 1, "one exit in all");
    // No timer of the mask can make it leave again.
    assert_eq!(exits(&deadline(&mut rig, 1)), 0);
}

#[test]
fn cancelling_the_question_stops_the_quit_before_the_cache_is_touched() {
    let dir = dir();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::SetCacheAutosave(true));
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    rig.raw(Message::Quit);
    let out = rig.raw(TreeMessage::Mask(MaskMessage::Answer(
        DiscardChoice::Cancel,
    )));
    assert!(out.is_empty(), "{out:?}");
    assert!(rig.state.mask_open() && dirty(&rig));
}

#[test]
fn going_to_a_position_with_an_altered_game_and_an_altered_mask_asks_only_about_the_game() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    rig.play("d2d4");
    let key = mychess_app::MaskKey::from_position(&mychess_core::Position::new());
    m(&mut rig, MaskMessage::GoTo(key.clone()));
    assert!(rig.state.discard_prompt.is_some());
    assert!(rig.state.mask_prompt().is_none(), "the mask is not touched");
    // Cancel: the game and the mask stay.
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert_eq!(rig.ply(), 1);
    assert!(rig.state.mask_open() && dirty(&rig));
    // Again, discarding: the game is replaced, the mask is not.
    m(&mut rig, MaskMessage::GoTo(key));
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(rig.ply(), 0);
    assert!(rig.state.mask_open() && dirty(&rig));
    assert_eq!(positions(&rig), 1);
}

#[test]
fn going_to_a_position_with_a_clean_game_replaces_it_without_asking() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    let after_e4 = {
        let mut p = mychess_core::Position::new();
        let mv = p.parse_uci("e2e4").unwrap();
        p.play(mv).unwrap();
        p
    };
    let key = mychess_app::MaskKey::from_position(&after_e4);
    m(&mut rig, MaskMessage::GoTo(key));
    assert!(rig.state.discard_prompt.is_none());
    assert_eq!(
        rig.state.session.position().fen().split(' ').next(),
        Some("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR")
    );
    assert_eq!(rig.ply(), 0, "a new game that starts from the position");
}
