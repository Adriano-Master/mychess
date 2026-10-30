//! M4-3 phase 2 contract tests: the engine list, dialog and editor through
//! `update` (spec D424-D429, D684-D686, D725; rules 2, 4, 5, 6, 7).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string
)]

mod m4_3_common;

use std::path::PathBuf;

use m4_3_common::*;
use mychess_app::engines::{
    COPY_SUFFIX, EditorError, EditorMessage, EngineDraft, EngineEditor, EngineMessage,
    EngineNotice, Hotkey, MAX_ENGINES, MAX_TEXT_CHARS, StoreError, ValidationError,
};
use mychess_app::{AppState, Message, update};
use mychess_engine::Protocol;
use mychess_engine::testing::ScriptedEngine;

fn editor(state: &mut AppState, m: EditorMessage) -> Vec<mychess_app::Effect> {
    send(state, EngineMessage::Editor(m))
}

fn with_path() -> AppState {
    AppState::with_preferences_path(Some(PathBuf::from("cfg").join("preferences.toml")))
}

fn fill_new(state: &mut AppState, name: &str, command: &str) {
    send(state, EngineMessage::New);
    editor(state, EditorMessage::SetName(name.to_owned()));
    editor(state, EditorMessage::SetCommand(command.to_owned()));
}

#[test]
fn saving_a_new_engine_requests_exactly_one_save_to_engines_toml_next_to_preferences() {
    let mut state = with_path();
    fill_new(&mut state, "Stockfish", "stockfish");
    let effects = editor(&mut state, EditorMessage::Save);
    let saves = saves(&effects);
    assert_eq!(saves.len(), 1);
    let mychess_app::Effect::SaveEngines { path, list } = saves[0] else {
        panic!("not a save")
    };
    assert_eq!(path, &PathBuf::from("cfg").join("engines.toml"));
    assert_eq!(list.len(), 1);
    assert!(state.engine_editor.is_none());
}

#[test]
fn the_first_engine_added_to_a_list_without_keys_gets_f2() {
    // Spec D427/D725: "the 1st gets F2 and the 2nd F3". Added one at a time,
    // the list has a key after the first save, so the second gets none (see
    // the report: ambiguous wording).
    let mut state = AppState::default();
    fill_new(&mut state, "A", "a");
    editor(&mut state, EditorMessage::Save);
    let id = state.engines.engines()[0].id;
    assert_eq!(state.engines.hotkey_of(id), Some(Hotkey::F2));
}

#[test]
fn invalid_forms_stay_open_with_their_error_and_never_touch_the_list() {
    let mut state = with_path();
    // Empty name.
    send(&mut state, EngineMessage::New);
    editor(&mut state, EditorMessage::SetCommand("x".into()));
    let fx = editor(&mut state, EditorMessage::Save);
    assert!(saves(&fx).is_empty());
    assert_eq!(
        state.engine_editor.as_ref().unwrap().error,
        Some(EditorError::Invalid(ValidationError::EmptyName))
    );
    // Whitespace-only name still empty.
    editor(&mut state, EditorMessage::SetName("   ".into()));
    editor(&mut state, EditorMessage::Save);
    assert_eq!(
        state.engine_editor.as_ref().unwrap().error,
        Some(EditorError::Invalid(ValidationError::EmptyName))
    );
    // Empty command.
    editor(&mut state, EditorMessage::SetName("N".into()));
    editor(&mut state, EditorMessage::SetCommand(" ".into()));
    editor(&mut state, EditorMessage::Save);
    assert_eq!(
        state.engine_editor.as_ref().unwrap().error,
        Some(EditorError::Invalid(ValidationError::EmptyCommand))
    );
    // Elo not a number, above 4000, and unclosed quote in args.
    editor(&mut state, EditorMessage::SetCommand("x".into()));
    editor(&mut state, EditorMessage::SetElo("abc".into()));
    editor(&mut state, EditorMessage::Save);
    assert_eq!(
        state.engine_editor.as_ref().unwrap().error,
        Some(EditorError::EloNotANumber)
    );
    editor(&mut state, EditorMessage::SetElo("4001".into()));
    editor(&mut state, EditorMessage::Save);
    assert!(matches!(
        state.engine_editor.as_ref().unwrap().error,
        Some(EditorError::Invalid(ValidationError::EloOutOfRange(4001)))
    ));
    editor(&mut state, EditorMessage::SetElo("99999999999".into()));
    editor(&mut state, EditorMessage::Save);
    assert_eq!(
        state.engine_editor.as_ref().unwrap().error,
        Some(EditorError::EloNotANumber)
    );
    editor(&mut state, EditorMessage::SetElo("0".into()));
    editor(&mut state, EditorMessage::SetArgs("\"open".into()));
    editor(&mut state, EditorMessage::Save);
    assert!(matches!(
        state.engine_editor.as_ref().unwrap().error,
        Some(EditorError::Args(_))
    ));
    assert!(state.engines.is_empty());
    // Fix the args: it saves, with Elo 0 meaning unknown.
    editor(&mut state, EditorMessage::SetArgs("--a \"b c\"".into()));
    editor(&mut state, EditorMessage::Save);
    assert!(state.engine_editor.is_none());
    let e = &state.engines.engines()[0];
    assert_eq!(e.args, ["--a", "b c"]);
    assert_eq!(e.elo, None);
}

#[test]
fn editor_error_keys_are_stable_and_distinct() {
    let errors = [
        EditorError::Invalid(ValidationError::EmptyName),
        EditorError::Invalid(ValidationError::EmptyCommand),
        EditorError::Invalid(ValidationError::EloOutOfRange(5000)),
        EditorError::Invalid(ValidationError::NonUtf8Path),
        EditorError::Invalid(ValidationError::TextTooLong {
            field: "name",
            limit: 4096,
        }),
        EditorError::Args(mychess_app::engines::ArgsError::UnclosedQuote),
        EditorError::EloNotANumber,
    ];
    let keys: Vec<&str> = errors.iter().map(EditorError::message_key).collect();
    for k in &keys {
        assert!(k.starts_with("engines-error-"), "{k}");
    }
    let mut dedup = keys.clone();
    dedup.sort_unstable();
    dedup.dedup();
    assert_eq!(dedup.len(), keys.len(), "{keys:?}");
}

#[test]
fn giving_a_key_takes_it_from_the_owner_and_each_engine_has_one_key() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B"], &fake);
    send(
        &mut state,
        EngineMessage::SetHotkey(ids[0], Some(Hotkey::F2)),
    );
    send(
        &mut state,
        EngineMessage::SetHotkey(ids[1], Some(Hotkey::F2)),
    );
    assert_eq!(state.engines.hotkey_owner(Hotkey::F2), Some(ids[1]));
    assert_eq!(state.engines.hotkey_of(ids[0]), None);
    // Moving B to F4 frees F2.
    send(
        &mut state,
        EngineMessage::SetHotkey(ids[1], Some(Hotkey::F4)),
    );
    assert_eq!(state.engines.hotkey_owner(Hotkey::F2), None);
    assert_eq!(state.engines.hotkey_owner(Hotkey::F4), Some(ids[1]));
    // Freeing.
    send(&mut state, EngineMessage::SetHotkey(ids[1], None));
    assert_eq!(state.engines.hotkey_of(ids[1]), None);
}

#[test]
fn hotkey_assigned_in_the_editor_takes_the_key_from_another_engine() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B"], &fake);
    send(
        &mut state,
        EngineMessage::SetHotkey(ids[0], Some(Hotkey::F3)),
    );
    send(&mut state, EngineMessage::Edit(ids[1]));
    editor(&mut state, EditorMessage::SetHotkey(Some(Hotkey::F3)));
    editor(&mut state, EditorMessage::Save);
    assert_eq!(state.engines.hotkey_owner(Hotkey::F3), Some(ids[1]));
    assert_eq!(state.engines.hotkey_of(ids[0]), None);
}

#[test]
fn deleting_does_not_renumber_the_other_keys_and_frees_the_deleted_key() {
    let mut state = with_path();
    let ids: Vec<_> = ["A", "B", "C"]
        .iter()
        .map(|n| state.engines.add(EngineDraft::new(*n, *n)).unwrap())
        .collect();
    for (id, key) in ids.iter().zip(Hotkey::ALL) {
        send(&mut state, EngineMessage::SetHotkey(*id, Some(key)));
    }
    send(&mut state, EngineMessage::Saved(Ok(())));
    send(&mut state, EngineMessage::Saved(Ok(())));
    send(&mut state, EngineMessage::OpenDialog);
    send(&mut state, EngineMessage::Delete(ids[0]));
    // Nothing is deleted until confirmed.
    assert_eq!(state.engines.len(), 3);
    assert_eq!(
        state.engine_dialog.as_ref().unwrap().confirm_delete,
        Some(ids[0])
    );
    let fx = send(&mut state, EngineMessage::ConfirmDelete(ids[0]));
    assert_eq!(saves(&fx).len(), 1);
    assert_eq!(state.engines.hotkey_owner(Hotkey::F2), None);
    assert_eq!(state.engines.hotkey_owner(Hotkey::F3), Some(ids[1]));
    assert_eq!(state.engines.hotkey_owner(Hotkey::F4), Some(ids[2]));
}

#[test]
fn cancelled_or_mismatched_delete_confirmations_keep_the_engine() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B"], &fake);
    send(&mut state, EngineMessage::OpenDialog);
    send(&mut state, EngineMessage::Delete(ids[0]));
    send(&mut state, EngineMessage::CancelDelete);
    send(&mut state, EngineMessage::ConfirmDelete(ids[0]));
    assert_eq!(state.engines.len(), 2);
    // Confirming another id than the one asked is ignored.
    send(&mut state, EngineMessage::Delete(ids[0]));
    send(&mut state, EngineMessage::ConfirmDelete(ids[1]));
    assert_eq!(state.engines.len(), 2);
    // Deleting an unknown engine asks nothing.
    send(&mut state, EngineMessage::CancelDelete);
    send(
        &mut state,
        EngineMessage::Delete(mychess_app::engines::EngineId(999)),
    );
    assert_eq!(state.engine_dialog.as_ref().unwrap().confirm_delete, None);
}

#[test]
fn ids_are_never_reused_after_deleting_the_last_engine() {
    let mut state = AppState::default();
    send(&mut state, EngineMessage::OpenDialog);
    fill_new(&mut state, "A", "a");
    editor(&mut state, EditorMessage::Save);
    let first = state.engines.engines()[0].id;
    send(&mut state, EngineMessage::Delete(first));
    send(&mut state, EngineMessage::ConfirmDelete(first));
    assert!(state.engines.is_empty());
    fill_new(&mut state, "B", "b");
    editor(&mut state, EditorMessage::Save);
    assert_ne!(state.engines.engines()[0].id, first);
}

#[test]
fn move_up_and_down_swap_neighbours_and_do_nothing_at_the_limits() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B", "C"], &fake);
    let fx = send(&mut state, EngineMessage::MoveUp(ids[0]));
    assert!(
        saves(&fx).is_empty(),
        "first engine: no-op, nothing to save"
    );
    let fx = send(&mut state, EngineMessage::MoveDown(ids[2]));
    assert!(saves(&fx).is_empty());
    let order = |s: &AppState| -> Vec<_> { s.engines.engines().iter().map(|e| e.id).collect() };
    assert_eq!(order(&state), ids);
    // The state has no path, so no save is possible but the order changes.
    send(&mut state, EngineMessage::MoveDown(ids[0]));
    assert_eq!(order(&state), [ids[1], ids[0], ids[2]]);
    send(&mut state, EngineMessage::MoveUp(ids[2]));
    assert_eq!(order(&state), [ids[1], ids[2], ids[0]]);
}

#[test]
fn move_and_copy_and_options_ask_for_saves_when_there_is_a_path() {
    let mut state = with_path();
    let a = state
        .engines
        .add(EngineDraft::new("Alpha", "alpha"))
        .unwrap();
    let b = state.engines.add(EngineDraft::new("Beta", "beta")).unwrap();
    let fx = send(&mut state, EngineMessage::MoveDown(a));
    assert_eq!(saves(&fx).len(), 1);
    // Reply so the next one is not queued behind it.
    send(&mut state, EngineMessage::Saved(Ok(())));
    let fx = send(&mut state, EngineMessage::Copy(b));
    assert_eq!(saves(&fx).len(), 1);
    let copy = state.engines.engines().last().unwrap();
    assert_eq!(copy.name, format!("Beta{COPY_SUFFIX}"));
    assert_eq!(COPY_SUFFIX, "-new");
    assert_eq!(state.engines.hotkey_of(copy.id), None);
    send(&mut state, EngineMessage::Saved(Ok(())));
    let fx = send(
        &mut state,
        EngineMessage::SetOption {
            engine: a,
            name: "Hash".into(),
            value: "64".into(),
        },
    );
    assert_eq!(saves(&fx).len(), 1);
    assert_eq!(
        state.engines.get(a).unwrap().options,
        [("Hash".to_owned(), "64".to_owned())]
    );
}

#[test]
fn saves_are_one_in_flight_and_changes_meanwhile_make_one_more_save_with_the_newest_list() {
    let mut state = with_path();
    let a = state.engines.add(EngineDraft::new("A", "a")).unwrap();
    let first = send(&mut state, EngineMessage::SetHotkey(a, Some(Hotkey::F2)));
    assert_eq!(saves(&first).len(), 1);
    // Three more changes while the first save is in flight.
    let mut during = Vec::new();
    during.extend(send(
        &mut state,
        EngineMessage::SetHotkey(a, Some(Hotkey::F3)),
    ));
    during.extend(send(
        &mut state,
        EngineMessage::SetHotkey(a, Some(Hotkey::F4)),
    ));
    during.extend(send(&mut state, EngineMessage::Copy(a)));
    assert!(saves(&during).is_empty(), "no concurrent save");
    let after = send(&mut state, EngineMessage::Saved(Ok(())));
    let s = saves(&after);
    assert_eq!(s.len(), 1, "exactly one re-save");
    let mychess_app::Effect::SaveEngines { list, .. } = s[0] else {
        unreachable!()
    };
    assert_eq!(list.len(), 2, "the re-save carries the newest list");
    assert_eq!(list.hotkey_of(a), Some(Hotkey::F4));
    // And nothing further after the second answer.
    assert!(saves(&send(&mut state, EngineMessage::Saved(Ok(())))).is_empty());
}

#[test]
fn a_failed_save_warns_and_does_not_lose_the_list() {
    let mut state = with_path();
    let a = state.engines.add(EngineDraft::new("A", "a")).unwrap();
    send(&mut state, EngineMessage::SetHotkey(a, Some(Hotkey::F2)));
    send(&mut state, EngineMessage::Saved(Err(StoreError::Syntax)));
    assert_eq!(notice(&state), Some(&EngineNotice::SaveFailed));
    assert_eq!(state.engines.len(), 1);
    // A new change can be saved again (the flag was cleared).
    let fx = send(&mut state, EngineMessage::SetHotkey(a, Some(Hotkey::F3)));
    assert_eq!(saves(&fx).len(), 1);
}

#[test]
fn the_list_is_full_at_256_engines_and_new_copy_and_save_refuse_without_panic() {
    let mut state = AppState::default();
    for i in 0..MAX_ENGINES {
        state
            .engines
            .add(EngineDraft::new(format!("E{i}"), format!("e{i}")))
            .unwrap();
    }
    let first = state.engines.engines()[0].id;
    send(&mut state, EngineMessage::Copy(first));
    assert_eq!(state.engines.len(), MAX_ENGINES);
    assert_eq!(notice(&state), Some(&EngineNotice::ListFull));
    state.notice = None;
    fill_new(&mut state, "One more", "x");
    editor(&mut state, EditorMessage::Save);
    assert_eq!(state.engines.len(), MAX_ENGINES);
    assert_eq!(notice(&state), Some(&EngineNotice::ListFull));
}

#[test]
fn copying_an_engine_whose_name_is_at_the_limit_does_not_panic_or_corrupt_the_list() {
    let mut state = AppState::default();
    let long = "x".repeat(MAX_TEXT_CHARS);
    let id = state
        .engines
        .add(EngineDraft::new(long.clone(), "a"))
        .unwrap();
    send(&mut state, EngineMessage::Copy(id));
    // Either refused (list unchanged) or accepted within the limit.
    for e in state.engines.engines() {
        assert!(e.name.chars().count() <= MAX_TEXT_CHARS);
    }
}

#[test]
fn text_fields_above_4096_chars_are_refused_by_the_form() {
    let mut state = AppState::default();
    fill_new(&mut state, &"n".repeat(MAX_TEXT_CHARS + 1), "x");
    editor(&mut state, EditorMessage::Save);
    assert!(state.engines.is_empty());
    assert!(state.engine_editor.as_ref().unwrap().error.is_some());
}

#[test]
fn editing_keeps_id_position_key_and_fields_the_form_does_not_show() {
    let mut state = AppState::default();
    let mut draft = EngineDraft::new("A", "a");
    draft.options = vec![("Hash".into(), "64".into())];
    draft.multipv = 3;
    draft.low_priority = Some(true);
    draft.last_used = Some(42);
    let a = state.engines.add(draft).unwrap();
    let b = state.engines.add(EngineDraft::new("B", "b")).unwrap();
    send(&mut state, EngineMessage::SetHotkey(a, Some(Hotkey::F4)));
    send(&mut state, EngineMessage::Edit(a));
    editor(&mut state, EditorMessage::SetName("A2".into()));
    editor(&mut state, EditorMessage::SetProtocol(Protocol::XBoard));
    editor(&mut state, EditorMessage::SetElo("2800".into()));
    editor(
        &mut state,
        EditorMessage::SetUrl("  https://example.org ".into()),
    );
    editor(&mut state, EditorMessage::Save);
    let e = state.engines.get(a).unwrap();
    assert_eq!(e.name, "A2");
    assert_eq!(e.protocol, Protocol::XBoard);
    assert_eq!(e.elo, Some(2800));
    assert_eq!(e.url.as_deref(), Some("https://example.org"));
    assert_eq!(e.options, [("Hash".to_owned(), "64".to_owned())]);
    assert_eq!(
        (e.multipv, e.low_priority, e.last_used),
        (3, Some(true), Some(42))
    );
    assert_eq!(state.engines.engines()[0].id, a);
    assert_eq!(state.engines.hotkey_of(a), Some(Hotkey::F4));
    assert_eq!(state.engines.position(b), Some(1));
}

#[test]
fn editing_an_unknown_engine_warns_not_found() {
    let mut state = AppState::default();
    send(
        &mut state,
        EngineMessage::Edit(mychess_app::engines::EngineId(7)),
    );
    assert_eq!(notice(&state), Some(&EngineNotice::NotFound));
    assert!(state.engine_editor.is_none());
}

#[test]
fn name_duplicates_are_allowed_and_nonexistent_commands_do_not_block_saving() {
    let mut state = AppState::default();
    for _ in 0..2 {
        fill_new(&mut state, "Same", "/definitely/not/here/engine");
        editor(&mut state, EditorMessage::Save);
    }
    assert_eq!(state.engines.len(), 2);
}

#[test]
fn typing_a_letter_selects_the_next_engine_with_that_initial_ignoring_case_and_wrapping() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["Alpha", "beta", "Atlas"], &fake);
    send(&mut state, EngineMessage::OpenDialog);
    assert_eq!(state.engine_dialog.as_ref().unwrap().selected, Some(ids[0]));
    send(&mut state, EngineMessage::SelectInitial('a'));
    assert_eq!(state.engine_dialog.as_ref().unwrap().selected, Some(ids[2]));
    send(&mut state, EngineMessage::SelectInitial('A'));
    assert_eq!(state.engine_dialog.as_ref().unwrap().selected, Some(ids[0]));
    send(&mut state, EngineMessage::SelectInitial('B'));
    assert_eq!(state.engine_dialog.as_ref().unwrap().selected, Some(ids[1]));
    // No match: unchanged.
    send(&mut state, EngineMessage::SelectInitial('z'));
    assert_eq!(state.engine_dialog.as_ref().unwrap().selected, Some(ids[1]));
}

#[test]
fn selecting_an_unknown_engine_clears_the_selection_and_close_dialog_closes_the_editor() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    send(&mut state, EngineMessage::OpenDialog);
    send(
        &mut state,
        EngineMessage::Select(Some(mychess_app::engines::EngineId(99))),
    );
    assert_eq!(state.engine_dialog.as_ref().unwrap().selected, None);
    send(&mut state, EngineMessage::Edit(ids[0]));
    send(&mut state, EngineMessage::CloseDialog);
    assert!(state.engine_dialog.is_none() && state.engine_editor.is_none());
}

#[test]
fn changes_wait_while_the_file_is_being_read_and_the_answer_replaces_the_list() {
    let mut state = with_path();
    let fx = update(&mut state, Message::Startup);
    assert!(
        fx.iter()
            .any(|e| matches!(e, mychess_app::Effect::LoadEngines { .. }))
    );
    send(&mut state, EngineMessage::New);
    assert!(
        state.engine_editor.is_none(),
        "commands are ignored meanwhile"
    );
    let loaded = mychess_app::engines::load_engine_list(
        "version = 1\nnext_id = 5\n[[engine]]\nid = 4\nname = \"X\"\ncommand = \"x\"\n",
    )
    .unwrap();
    send(&mut state, EngineMessage::Loaded(Box::new(Ok(loaded))));
    assert_eq!(state.engines.len(), 1);
    send(&mut state, EngineMessage::New);
    assert!(state.engine_editor.is_some());
}

#[test]
fn a_failed_load_keeps_the_current_list_and_warns() {
    let mut state = with_path();
    let a = state.engines.add(EngineDraft::new("A", "a")).unwrap();
    update(&mut state, Message::Startup);
    send(
        &mut state,
        EngineMessage::Loaded(Box::new(Err(StoreError::Syntax))),
    );
    assert!(state.engines.get(a).is_some());
    assert_eq!(notice(&state), Some(&EngineNotice::FileUnreadable));
    // A newer version has its own notice.
    send(
        &mut state,
        EngineMessage::Loaded(Box::new(Err(StoreError::NewerVersion { found: 9 }))),
    );
    assert_eq!(notice(&state), Some(&EngineNotice::NewerVersion));
    // And the user can edit again (loading flag cleared).
    send(&mut state, EngineMessage::New);
    assert!(state.engine_editor.is_some());
}

#[test]
fn a_load_with_fixed_entries_reports_how_many_remarks() {
    let mut state = with_path();
    update(&mut state, Message::Startup);
    let loaded = mychess_app::engines::load_engine_list(
        "version = 1\n[[engine]]\nname = \"ok\"\ncommand = \"c\"\nelo = 99999\n[[engine]]\nname = \"\"\ncommand = \"d\"\n",
    )
    .unwrap();
    let n = loaded.issues.len();
    assert!(n >= 2);
    send(&mut state, EngineMessage::Loaded(Box::new(Ok(loaded))));
    assert_eq!(state.engines.len(), 1);
    assert_eq!(notice(&state), Some(&EngineNotice::FileFixed { count: n }));
}

#[test]
fn no_config_directory_means_no_effects_for_the_list_but_the_session_keeps_working() {
    let mut state = AppState::default();
    let fx = update(&mut state, Message::Startup);
    assert!(
        !fx.iter()
            .any(|e| matches!(e, mychess_app::Effect::LoadEngines { .. }))
    );
    fill_new(&mut state, "A", "a");
    let fx = editor(&mut state, EditorMessage::Save);
    assert!(saves(&fx).is_empty());
    assert_eq!(state.engines.len(), 1);
}

#[test]
fn new_form_then_cancel_changes_nothing() {
    let mut state = AppState::default();
    fill_new(&mut state, "A", "a");
    editor(&mut state, EditorMessage::Cancel);
    assert!(state.engine_editor.is_none() && state.engines.is_empty());
    // A form for an existing entry shows its args as one line that survives.
    let id = state
        .engines
        .add({
            let mut d = EngineDraft::new("A", "a");
            d.args = vec!["--x".into(), "a b".into(), String::new()];
            d
        })
        .unwrap();
    let ed = EngineEditor::for_entry(state.engines.get(id).unwrap(), None);
    assert_eq!(ed.draft().unwrap().args, ["--x", "a b", ""]);
}
