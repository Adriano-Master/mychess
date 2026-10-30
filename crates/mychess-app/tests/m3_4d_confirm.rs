//! Contract tests of the question before a copy and of the preference that
//! turns it off (spec M3-4, D580; Scid `copyFilter` 1173-1224 and
//! `confirmCopy` in `start.tcl` 160).
#![allow(
    clippy::assert_is_empty,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4d_common;

use m3_4d_common::*;
use mychess_app::{GameMessage, PlayOptions, load_preferences, render_preferences};

fn asking() -> Pair {
    let mut p = pair(&numbered(5), &numbered(3));
    confirm(&mut p.rig, true);
    p
}

fn answer(rig: &mut Rig, yes: bool) {
    rig.send(l(ListMessage::AnswerCopy(yes)));
}

#[test]
fn the_default_is_to_ask() {
    let state = mychess_app::AppState::default();
    assert!(state.prefs.editing.confirm_copy);
}

#[test]
fn a_copy_to_a_file_asks_first_and_runs_nothing() {
    let mut p = asking();
    let effects = p.rig.raw(copy_msg(p.from, p.to, CopyScope::Filter));
    assert!(effects.is_empty(), "nothing runs before the answer");
    let prompt = p.rig.state.copy_prompt.clone().expect("a question");
    assert_eq!(prompt.count, 5);
    assert_eq!(prompt.scope, CopyScope::Filter);
    assert_eq!(prompt.from, end(p.from, "src.pgn"));
    assert_eq!(prompt.to, end(p.to, "dst.si4"));
    assert!(p.rig.state.list_job().is_none());
    assert_eq!(disk(&p.path).len(), 3);
}

#[test]
fn the_count_of_a_selection_question_is_the_size_of_the_selection() {
    let mut p = asking();
    p.rig.select_all(p.from, &[1, 4]);
    p.rig.raw(copy_msg(p.from, p.to, CopyScope::Selection));
    let prompt = p.rig.state.copy_prompt.clone().unwrap();
    assert_eq!((prompt.count, prompt.scope), (2, CopyScope::Selection));
}

#[test]
fn the_count_of_a_filter_question_is_the_size_of_the_filter() {
    let mut p = asking();
    p.rig.select_all(p.from, &[1, 4]);
    p.rig.send(l(ListMessage::RemoveSelected { slot: p.from }));
    p.rig.raw(copy_msg(p.from, p.to, CopyScope::Filter));
    assert_eq!(p.rig.state.copy_prompt.clone().unwrap().count, 3);
}

#[test]
fn yes_copies_and_closes_the_question() {
    let mut p = asking();
    p.rig.raw(copy_msg(p.from, p.to, CopyScope::Filter));
    answer(&mut p.rig, true);
    assert!(p.rig.state.copy_prompt.is_none());
    assert!(!p.rig.state.modal_open());
    assert_eq!(disk(&p.path).len(), 8);
    assert!(matches!(
        p.rig.notice(),
        Some(BasesNotice::FilterCopied { count: 5, .. })
    ));
}

#[test]
fn no_closes_the_question_and_copies_nothing() {
    let mut p = asking();
    p.rig.raw(copy_msg(p.from, p.to, CopyScope::Filter));
    let effects = p.rig.raw(l(ListMessage::AnswerCopy(false)));
    assert!(effects.is_empty());
    assert!(p.rig.state.copy_prompt.is_none() && !p.rig.state.modal_open());
    assert_eq!(disk(&p.path).len(), 3);
    assert!(p.rig.state.list_job().is_none());
}

#[test]
fn an_answer_without_a_question_does_nothing() {
    let mut p = asking();
    for yes in [true, false] {
        p.rig.clear_notice();
        let effects = p.rig.raw(l(ListMessage::AnswerCopy(yes)));
        assert!(effects.is_empty());
        assert!(p.rig.notice().is_none());
    }
    assert_eq!(disk(&p.path).len(), 3);
}

#[test]
fn an_answer_is_used_once() {
    let mut p = asking();
    p.rig.raw(copy_msg(p.from, p.to, CopyScope::Filter));
    answer(&mut p.rig, true);
    answer(&mut p.rig, true);
    assert_eq!(disk(&p.path).len(), 8, "the second yes copied nothing");
}

#[test]
fn the_question_is_modal_for_editing_slots_and_the_list() {
    let mut p = asking();
    let (from, to) = (p.from, p.to);
    p.rig.send(BasesMessage::Select(from));
    p.rig.raw(copy_msg(from, to, CopyScope::Filter));
    assert!(p.rig.state.modal_open());
    // Editing the game is ignored.
    let before = p.rig.state.session.cursor();
    let mv = p.rig.state.session.position().parse_uci("e2e4").unwrap();
    p.rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert_eq!(p.rig.state.session.cursor(), before);
    // Switching slots, closing and list actions wait.
    p.rig.send(BasesMessage::Select(to));
    assert_eq!(p.rig.state.bases.current(), from);
    p.rig.send(BasesMessage::Close(to));
    assert!(p.rig.state.bases.slot(to).is_some());
    p.rig.send(l(ListMessage::NegateFilter { slot: from }));
    assert_eq!(p.rig.state.bases.slot(from).unwrap().filtered(), 5);
    p.rig.send(BasesMessage::LoadGame {
        slot: from,
        number: 2,
    });
    assert_eq!(p.rig.state.bases.slot(from).unwrap().current_game, None);
    // A second drag during the question does not replace it.
    p.rig
        .send(copy_msg(from, SlotId::CLIPBASE, CopyScope::Filter));
    assert_eq!(p.rig.state.copy_prompt.clone().unwrap().to.slot, to);
    assert_eq!(whites(&p.rig, SlotId::CLIPBASE).len(), 0);
}

#[test]
fn a_database_that_arrives_during_the_question_is_not_activated() {
    // D527: activating under a question would change what "Copy" acts on.
    let mut p = asking();
    p.rig.send(BasesMessage::Select(p.from));
    p.rig.raw(copy_msg(p.from, p.to, CopyScope::Filter));
    let extra = write(p.dir.path(), "late.pgn", &pgn(&numbered(3)));
    p.rig.send(BasesMessage::Open(extra));
    // Open requests are ignored while the question is open.
    assert_eq!(p.rig.state.bases.current(), p.from);
    assert_eq!(p.rig.state.bases.iter().count(), 3, "two slots + Clipbase");
    // Hence the question still refers to the same databases.
    assert_eq!(p.rig.state.copy_prompt.clone().unwrap().from.slot, p.from);
    answer(&mut p.rig, false);
    p.rig
        .send(BasesMessage::Open(p.dir.path().join("late.pgn")));
    assert_eq!(p.rig.state.bases.iter().count(), 4);
}

#[test]
fn with_the_question_off_the_copy_starts_at_once() {
    let mut p = asking();
    confirm(&mut p.rig, false);
    let effects = p.rig.raw(copy_msg(p.from, p.to, CopyScope::Filter));
    assert_eq!(effects.len(), 1);
    assert!(p.rig.state.copy_prompt.is_none());
    assert_eq!(
        p.rig.state.list_job().map(|j| j.0),
        Some(ListJobKind::CopyGames)
    );
}

#[test]
fn a_refusal_comes_before_the_question() {
    let mut p = asking();
    // Same database, read-only target: no question, the refusal.
    p.rig.raw(copy_msg(p.from, p.from, CopyScope::Filter));
    assert!(p.rig.state.copy_prompt.is_none());
    assert_eq!(refusal(&p.rig).unwrap().0, CopyRefusal::SameBase);
    p.rig.raw(copy_msg(p.to, p.from, CopyScope::Filter));
    assert!(p.rig.state.copy_prompt.is_none());
    assert_eq!(refusal(&p.rig).unwrap().0, CopyRefusal::TargetReadOnly);
}

#[test]
fn the_preference_message_turns_it_on_and_off_and_saves_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    let mut state = mychess_app::AppState::with_preferences_path(Some(path.clone()));
    assert!(state.prefs.editing.confirm_copy);
    let effects = mychess_app::update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(false))),
    );
    assert!(!state.prefs.editing.confirm_copy);
    let save = effects
        .into_iter()
        .find(|e| matches!(e, Effect::SavePreferences { .. }))
        .expect("the preference is saved");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let answer = rt.block_on(mychess_app::perform(save));
    mychess_app::update(&mut state, answer);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("confirm_copy = false"), "{text}");
    mychess_app::update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(true))),
    );
    assert!(state.prefs.editing.confirm_copy);
}

#[test]
fn the_preference_round_trips_through_the_toml_and_an_old_file_keeps_the_default() {
    let mut rig = Rig::new();
    confirm(&mut rig, false);
    let text = render_preferences(&rig.state.prefs);
    assert!(text.contains("confirm_copy = false"), "{text}");
    assert!(!load_preferences(&text).prefs.editing.confirm_copy);
    confirm(&mut rig, true);
    let text = render_preferences(&rig.state.prefs);
    assert!(load_preferences(&text).prefs.editing.confirm_copy);
    // Old files: no `[editing]` section, or a section without the key.
    for old in [
        "",
        "[editing]\nask_to_replace_moves = false\n",
        "[general]\nlanguage = \"pt-BR\"\n",
    ] {
        let loaded = load_preferences(old);
        assert!(loaded.prefs.editing.confirm_copy, "{old:?}");
        assert!(loaded.issues.is_empty(), "{old:?}: {:?}", loaded.issues);
    }
    // A wrong type is reported and falls back to the default.
    let bad = load_preferences("[editing]\nconfirm_copy = \"nope\"\n");
    assert!(bad.prefs.editing.confirm_copy);
}
