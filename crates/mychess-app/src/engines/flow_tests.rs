//! Tests of the engine management flows: `update` with the messages of the
//! user and `perform` with the in-memory scripted engine and real files
//! (spec M4-3, phase 2).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mychess_engine::EngineEvent;
use mychess_engine::testing::ScriptedEngine;

use super::*;
use crate::effect::{Effect, perform};
use crate::error::RequestId;
use crate::flow::AppNotice;
use crate::message::Message;
use crate::prefs::AnalysisPrefs;
use crate::session::{GameMessage, PlayOptions};
use crate::shortcuts::{Action, Key, KeyInput, Modifiers, NamedKey};
use crate::state::{AppState, update};

fn hub_with(fake: &ScriptedEngine) -> EngineHub {
    let fake = fake.clone();
    EngineHub::with_launcher(Arc::new(move |config| {
        let fake = fake.clone();
        Box::pin(async move { fake.start(config).await })
    }))
}

/// A state with `names` in the list and engines that start the fake.
fn state_with(names: &[&str], fake: &ScriptedEngine) -> (AppState, Vec<EngineId>) {
    let mut state = AppState::default();
    state.engine_hub = hub_with(fake);
    let ids = names
        .iter()
        .map(|n| {
            state
                .engines
                .add(EngineDraft::new(*n, format!("/fake/{n}")))
                .expect("add")
        })
        .collect();
    (state, ids)
}

fn send(state: &mut AppState, message: EngineMessage) -> Vec<Effect> {
    update(state, Message::Engine(message))
}

/// Runs the effects of `effects` and of their answers; returns the effects
/// run, in order.
async fn settle(state: &mut AppState, effects: Vec<Effect>) -> Vec<Effect> {
    let mut seen = Vec::new();
    let mut queue: VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        seen.push(effect.clone());
        let answer = perform(effect).await;
        queue.extend(update(state, answer));
    }
    seen
}

async fn drive(state: &mut AppState, message: EngineMessage) -> Vec<Effect> {
    let effects = send(state, message);
    settle(state, effects).await
}

fn play(state: &mut AppState, uci: &str) -> Vec<Effect> {
    let mv = state.session.position().parse_uci(uci).expect("uci");
    update(
        state,
        Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    )
}

fn commands(effects: &[Effect]) -> Vec<(RunId, mychess_engine::EngineCommand)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand { run, command, .. } => Some((*run, (**command).clone())),
            _ => None,
        })
        .collect()
}

fn searches(effects: &[Effect]) -> Vec<RunId> {
    commands(effects)
        .into_iter()
        .filter(|(_, c)| matches!(c, mychess_engine::EngineCommand::Search(_)))
        .map(|(r, _)| r)
        .collect()
}

fn has_save(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::SaveEngines { .. }))
}

fn editor_msg(state: &mut AppState, message: EditorMessage) -> Vec<Effect> {
    send(state, EngineMessage::Editor(message))
}

fn notice(state: &AppState) -> Option<&EngineNotice> {
    match state.notice.as_ref() {
        Some(AppNotice::Engine(n)) => Some(n),
        _ => None,
    }
}

// ---- the list ----

#[test]
fn new_then_save_adds_the_engine_asks_for_a_save_and_gives_it_f2() {
    let mut state = AppState::default();
    send(&mut state, EngineMessage::OpenDialog);
    send(&mut state, EngineMessage::New);
    editor_msg(&mut state, EditorMessage::SetName("Stockfish".into()));
    editor_msg(&mut state, EditorMessage::SetCommand("stockfish".into()));
    // No path to the preferences: nothing is saved but the list changes.
    editor_msg(&mut state, EditorMessage::Save);
    assert!(state.engine_editor.is_none());
    assert_eq!(state.engines.len(), 1);
    let id = state.engines.engines()[0].id;
    assert_eq!(state.engines.hotkey_of(id), Some(Hotkey::F2));
    assert_eq!(
        state.engine_dialog.as_ref().and_then(|d| d.selected),
        Some(id)
    );

    let mut with_path =
        AppState::with_preferences_path(Some(PathBuf::from("cfg").join("preferences.toml")));
    send(&mut with_path, EngineMessage::New);
    editor_msg(&mut with_path, EditorMessage::SetName("A".into()));
    editor_msg(&mut with_path, EditorMessage::SetCommand("a".into()));
    let effects = editor_msg(&mut with_path, EditorMessage::Save);
    let Some(Effect::SaveEngines { path, list }) = effects.first() else {
        panic!("expected a save: {effects:?}");
    };
    assert_eq!(path, &PathBuf::from("cfg").join("engines.toml"));
    assert_eq!(list.len(), 1);
}

#[test]
fn an_invalid_form_stays_open_with_its_error() {
    let mut state = AppState::default();
    send(&mut state, EngineMessage::New);
    let effects = editor_msg(&mut state, EditorMessage::Save);
    assert!(effects.is_empty());
    let editor = state.engine_editor.as_ref().expect("still open");
    assert_eq!(
        editor.error,
        Some(EditorError::Invalid(ValidationError::EmptyName))
    );
    assert_eq!(
        editor.error.as_ref().map(EditorError::message_key),
        Some("engines-error-empty-name")
    );
}

#[test]
fn giving_a_key_to_another_engine_takes_it_from_the_first() {
    let (mut state, ids) = state_with(&["A", "B"], &ScriptedEngine::builder().build());
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
}

#[test]
fn copy_adds_new_suffix_and_move_and_delete_ask_for_saves() {
    let mut state =
        AppState::with_preferences_path(Some(PathBuf::from("p").join("preferences.toml")));
    let id = state.engines.add(EngineDraft::new("A", "a")).expect("add");
    state.engines.add(EngineDraft::new("B", "b")).expect("add");
    send(&mut state, EngineMessage::OpenDialog);
    let effects = send(&mut state, EngineMessage::Copy(id));
    assert!(has_save(&effects));
    assert_eq!(state.engines.engines()[2].name, "A-new");
    assert!(
        !has_save(&send(&mut state, EngineMessage::MoveUp(id))),
        "a save is already in flight"
    );
    // Answer the save, then the next change saves again.
    send(&mut state, EngineMessage::Saved(Ok(())));
    assert!(
        !has_save(&send(&mut state, EngineMessage::MoveUp(id))),
        "already first: nothing changes"
    );
    assert!(has_save(&send(&mut state, EngineMessage::MoveDown(id))));
    assert_eq!(state.engines.position(id), Some(1));
}

#[test]
fn saves_are_one_in_flight_and_a_resave_carries_the_newest_list() {
    let mut state =
        AppState::with_preferences_path(Some(PathBuf::from("p").join("preferences.toml")));
    let id = state.engines.add(EngineDraft::new("A", "a")).expect("add");
    let first = send(&mut state, EngineMessage::SetHotkey(id, Some(Hotkey::F3)));
    assert!(has_save(&first));
    assert!(send(&mut state, EngineMessage::SetHotkey(id, Some(Hotkey::F4))).is_empty());
    let again = send(&mut state, EngineMessage::Saved(Ok(())));
    let Some(Effect::SaveEngines { list, .. }) = again.first() else {
        panic!("expected a resave");
    };
    assert_eq!(list.hotkey_of(id), Some(Hotkey::F4));
    assert!(send(&mut state, EngineMessage::Saved(Err(StoreError::Syntax))).is_empty());
    assert_eq!(notice(&state), Some(&EngineNotice::SaveFailed));
}

#[test]
fn delete_asks_for_confirmation_and_selection_moves_to_a_neighbour() {
    let (mut state, ids) = state_with(&["A", "B", "C"], &ScriptedEngine::builder().build());
    send(&mut state, EngineMessage::OpenDialog);
    send(&mut state, EngineMessage::Select(Some(ids[1])));
    send(&mut state, EngineMessage::Delete(ids[1]));
    assert_eq!(
        state.engine_dialog.as_ref().and_then(|d| d.confirm_delete),
        Some(ids[1])
    );
    assert_eq!(
        state.engines.len(),
        3,
        "nothing is deleted before the answer"
    );
    send(&mut state, EngineMessage::CancelDelete);
    assert_eq!(state.engines.len(), 3);
    send(&mut state, EngineMessage::ConfirmDelete(ids[1]));
    assert_eq!(
        state.engines.len(),
        3,
        "a confirmation nobody asked for is ignored"
    );
    send(&mut state, EngineMessage::Delete(ids[1]));
    send(&mut state, EngineMessage::ConfirmDelete(ids[1]));
    assert_eq!(state.engines.len(), 2);
    assert_eq!(
        state.engine_dialog.as_ref().and_then(|d| d.selected),
        Some(ids[2])
    );
}

#[test]
fn typing_a_letter_selects_the_next_engine_with_that_initial() {
    let (mut state, ids) = state_with(
        &["Alpha", "Beta", "Atlas"],
        &ScriptedEngine::builder().build(),
    );
    send(&mut state, EngineMessage::OpenDialog);
    send(&mut state, EngineMessage::SelectInitial('a'));
    assert_eq!(
        state.engine_dialog.as_ref().and_then(|d| d.selected),
        Some(ids[2])
    );
    send(&mut state, EngineMessage::SelectInitial('b'));
    assert_eq!(
        state.engine_dialog.as_ref().and_then(|d| d.selected),
        Some(ids[1])
    );
}

#[test]
fn changes_wait_while_the_list_is_being_read() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    let effects = update(&mut state, Message::Startup);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadEngines { .. }))
    );
    assert!(send(&mut state, EngineMessage::New).is_empty());
    assert!(
        state.engine_editor.is_none(),
        "the list is not editable yet"
    );
}

// ---- the file, through perform ----

#[tokio::test]
async fn the_list_is_saved_and_read_back_at_startup() {
    let dir = tempfile::tempdir().expect("dir");
    let prefs = dir.path().join("cfg").join("preferences.toml");
    let mut first = AppState::with_preferences_path(Some(prefs.clone()));
    drive(&mut first, EngineMessage::New).await;
    editor_msg(&mut first, EditorMessage::SetName("Crafty".into()));
    editor_msg(&mut first, EditorMessage::SetCommand("crafty".into()));
    editor_msg(&mut first, EditorMessage::SetArgs("-x \"a b\"".into()));
    let effects = editor_msg(&mut first, EditorMessage::Save);
    settle(&mut first, effects).await;
    assert!(dir.path().join("cfg").join("engines.toml").exists());
    let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("cfg"))
        .expect("dir")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
        .collect();
    assert!(leftovers.is_empty(), "no temporary file is left");

    let mut second = AppState::with_preferences_path(Some(prefs));
    let effects = update(&mut second, Message::Startup);
    settle(&mut second, effects).await;
    assert_eq!(second.engines.len(), 1);
    let entry = &second.engines.engines()[0];
    assert_eq!(
        (entry.name.as_str(), entry.args.clone()),
        ("Crafty", vec!["-x".to_owned(), "a b".to_owned()])
    );
    assert_eq!(second.engines.hotkey_of(entry.id), Some(Hotkey::F2));
}

#[tokio::test]
async fn an_unreadable_file_leaves_an_empty_list_a_copy_and_a_notice() {
    let dir = tempfile::tempdir().expect("dir");
    let prefs = dir.path().join("preferences.toml");
    std::fs::write(dir.path().join("engines.toml"), "this is = = not toml [[").expect("write");
    let mut state = AppState::with_preferences_path(Some(prefs));
    let effects = update(&mut state, Message::Startup);
    settle(&mut state, effects).await;
    assert!(state.engines.is_empty());
    assert_eq!(notice(&state), Some(&EngineNotice::FileUnreadable));
    assert!(dir.path().join("engines.corrupt").exists());
}

#[tokio::test]
async fn a_file_with_a_bad_field_loads_and_reports_the_fix() {
    let dir = tempfile::tempdir().expect("dir");
    std::fs::write(
        dir.path().join("engines.toml"),
        "version = 1\n[[engine]]\nid = 1\nname = \"A\"\ncommand = \"a\"\nelo = \"high\"\n[[engine]]\nid = 2\ncommand = \"nameless\"\n",
    )
    .expect("write");
    let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    let effects = update(&mut state, Message::Startup);
    settle(&mut state, effects).await;
    assert_eq!(state.engines.len(), 1);
    assert_eq!(state.engines.engines()[0].elo, None);
    assert_eq!(notice(&state), Some(&EngineNotice::FileFixed { count: 2 }));
    assert_eq!(state.engine_issues.len(), 2);
}

// ---- executions ----

#[tokio::test]
async fn starting_an_engine_reaches_running_and_records_the_last_use() {
    let fake = ScriptedEngine::builder().name("Fake 1").build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let effects = send(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze));
    assert_eq!(state.engine_runs()[0].state, EngineRunState::Starting);
    settle(&mut state, effects).await;
    let run = &state.engine_runs()[0];
    assert_eq!(run.state, EngineRunState::Running);
    assert_eq!(
        run.identity.as_ref().map(|i| i.name.as_str()),
        Some("Fake 1")
    );
    assert!(
        state
            .engines
            .get(ids[0])
            .and_then(|e| e.last_used)
            .is_some_and(|t| t > 1_600_000_000)
    );
    assert_eq!(state.main_engine(), Some(run.id));
    state.engine_hub.shutdown_all(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn starting_a_running_engine_ends_it() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    assert_eq!(state.engine_hub.alive(), 1);
    let effects = send(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::StopEngine { .. }))
    );
    assert_eq!(state.engine_runs()[0].state, EngineRunState::Stopping);
    // A third press while stopping does nothing.
    assert!(send(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).is_empty());
    settle(&mut state, effects).await;
    assert!(state.engine_runs().is_empty());
    assert_eq!(state.engine_hub.alive(), 0);
}

#[tokio::test]
async fn a_program_that_does_not_exist_fails_the_run_and_can_be_restarted() {
    let mut state = AppState::default();
    let id = state
        .engines
        .add(EngineDraft::new(
            "Ghost",
            "/nonexistent/mychess-ghost-engine",
        ))
        .expect("add");
    drive(&mut state, EngineMessage::Start(id, StartMode::Analyze)).await;
    let run = state.engine_runs()[0].clone();
    let EngineRunState::Failed(failure) = &run.state else {
        panic!("expected a failure: {:?}", run.state);
    };
    assert_eq!(failure.kind, FailureKind::CannotStart);
    assert!(
        matches!(notice(&state), Some(EngineNotice::StartFailed { name, .. }) if name == "Ghost")
    );
    assert_eq!(state.main_engine(), None);
    // Restart replaces the failed run with a new one.
    let effects = send(&mut state, EngineMessage::Restart(run.id));
    assert_eq!(state.engine_runs().len(), 1);
    assert_ne!(state.engine_runs()[0].id, run.id);
    assert!(matches!(effects.first(), Some(Effect::StartEngine { .. })));
}

#[tokio::test]
async fn the_main_engine_is_the_first_running_in_list_order_not_start_order() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B", "C"], &fake);
    for id in [ids[2], ids[1], ids[0]] {
        drive(&mut state, EngineMessage::Start(id, StartMode::Analyze)).await;
    }
    let of = |state: &AppState, id| state.engine_runs.of_engine(id).map(|r| r.id);
    assert_eq!(state.main_engine(), of(&state, ids[0]));
    // Turn A's analysis off: B becomes the main engine; C is still later.
    let run_a = of(&state, ids[0]).expect("run");
    state.engine_runs.get_mut(run_a).expect("run").analyzing = false;
    assert_eq!(state.main_engine(), of(&state, ids[1]));
    // Moving C to the front changes the answer.
    send(&mut state, EngineMessage::MoveUp(ids[2]));
    send(&mut state, EngineMessage::MoveUp(ids[2]));
    assert_eq!(state.main_engine(), of(&state, ids[2]));
    state.engine_hub.shutdown_all(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn deleting_a_running_engine_stops_it_first() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    send(&mut state, EngineMessage::OpenDialog);
    send(&mut state, EngineMessage::Delete(ids[0]));
    let effects = send(&mut state, EngineMessage::ConfirmDelete(ids[0]));
    assert!(matches!(effects.first(), Some(Effect::StopEngine { .. })));
    assert!(state.engines.is_empty());
    settle(&mut state, effects).await;
    assert_eq!(state.engine_hub.alive(), 0);
}

#[tokio::test]
async fn f2_f3_start_the_engine_of_the_key_and_an_empty_key_warns() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B"], &fake);
    state.engines.assign_default_hotkeys();
    let table = state.shortcuts.clone();
    let key = |n| KeyInput {
        key: Key::Named(NamedKey::F(n)),
        modifiers: Modifiers::NONE,
    };
    assert_eq!(table.lookup(&key(2)), Some(Action::StartEngine(1)));
    assert_eq!(table.lookup(&key(4)), Some(Action::StartEngine(3)));
    assert_eq!(
        Action::all()
            .filter(|a| matches!(a, Action::StartEngine(_)))
            .count(),
        0
    );
    let effects = update(&mut state, Message::Action(Action::StartEngine(2)));
    assert_eq!(
        state.engine_runs.of_engine(ids[1]).map(|r| &r.state),
        Some(&EngineRunState::Starting)
    );
    settle(&mut state, effects).await;
    assert!(update(&mut state, Message::Action(Action::StartEngine(3))).is_empty());
    assert_eq!(
        notice(&state),
        Some(&EngineNotice::NoEngineOnKey(Hotkey::F4))
    );
    state.engine_hub.shutdown_all(Duration::from_secs(2)).await;
}

#[test]
fn space_without_a_running_engine_only_warns() {
    let mut state = AppState::default();
    assert!(update(&mut state, Message::Action(Action::ToggleEngineAnalysis)).is_empty());
    assert_eq!(notice(&state), Some(&EngineNotice::NoEngineRunning));
}

#[tokio::test]
async fn space_turns_the_analysis_of_the_first_running_engine_off_and_on() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    let run = state.engine_runs()[0].id;
    let off = send(&mut state, EngineMessage::ToggleMainAnalysis);
    assert_eq!(commands(&off), [(run, mychess_engine::EngineCommand::Stop)]);
    assert_eq!(state.main_engine(), None);
    let on = send(&mut state, EngineMessage::ToggleMainAnalysis);
    assert_eq!(searches(&on), [run], "the position is sent again");
    assert_eq!(state.main_engine(), Some(run));
    state.engine_hub.shutdown_all(Duration::from_secs(2)).await;
}

// ---- the position (D436) ----

#[tokio::test]
async fn a_new_position_goes_to_the_running_analysing_engines_only() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B", "C"], &fake);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    drive(&mut state, EngineMessage::Start(ids[1], StartMode::Idle)).await;
    drive(&mut state, EngineMessage::Start(ids[2], StartMode::Analyze)).await;
    let run = |state: &AppState, id| state.engine_runs.of_engine(id).expect("run").id;
    state
        .engine_runs
        .get_mut(run(&state, ids[2]))
        .expect("run")
        .locked = true;
    let effects = play(&mut state, "e2e4");
    assert_eq!(searches(&effects), [run(&state, ids[0])]);
    state.engine_hub.shutdown_all(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn an_unchanged_position_does_not_repeat_the_search_and_a_new_game_is_announced() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    let run = state.engine_runs()[0].id;
    // The start position was sent when the engine became ready.
    assert!(update(&mut state, Message::FlushPreferences).is_empty());
    let moved = play(&mut state, "e2e4");
    assert_eq!(searches(&moved), [run]);
    // Going back and forth is a change each time; a no-op message is not.
    let again = update(
        &mut state,
        Message::Game(GameMessage::Navigate(crate::session::Nav::Forward(1))),
    );
    assert!(
        searches(&again).is_empty(),
        "already at the end: same position"
    );
    let back = update(
        &mut state,
        Message::Game(GameMessage::Navigate(crate::session::Nav::Start)),
    );
    assert_eq!(searches(&back), [run]);
    assert!(
        !commands(&back)
            .iter()
            .any(|(_, c)| *c == mychess_engine::EngineCommand::NewGame)
    );
    // Another game: `NewGame` first, then the search.
    // The game was altered: the replacement asks first.
    update(&mut state, Message::Command(crate::flow::Command::NewGame));
    let fresh = update(
        &mut state,
        Message::AnswerDiscard(crate::flow::DiscardChoice::DontSave),
    );
    let sent = commands(&fresh);
    assert_eq!(
        sent.first().map(|(_, c)| c.clone()),
        Some(mychess_engine::EngineCommand::NewGame)
    );
    assert_eq!(searches(&fresh), [run]);
    state.engine_hub.shutdown_all(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn every_running_engine_receives_the_search_and_the_fake_sees_it() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B"], &fake);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    drive(&mut state, EngineMessage::Start(ids[1], StartMode::Analyze)).await;
    let effects = play(&mut state, "d2d4");
    assert_eq!(searches(&effects).len(), 2);
    settle(&mut state, effects).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let received = fake.received();
    let go = received.iter().filter(|l| l.starts_with("go")).count();
    assert!(go >= 2, "both engines were told to search: {received:?}");
    assert!(
        received.iter().any(|l| l.contains("moves d2d4")),
        "{received:?}"
    );
    state.engine_hub.shutdown_all(Duration::from_secs(2)).await;
}

// ---- events and the end of the session ----

#[tokio::test]
async fn events_reach_the_front_end_through_the_single_channel() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let mut events = state.engine_hub.events().expect("receiver");
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    let run = state.engine_runs()[0].id;
    let (from, event) = tokio::time::timeout(Duration::from_secs(5), events.next())
        .await
        .expect("an event")
        .expect("open");
    assert_eq!(from, run);
    assert!(matches!(event, EngineEvent::Ready(_)));
    state.engine_hub.shutdown_all(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn an_engine_that_dies_becomes_failed_with_its_exit_information() {
    let fake = ScriptedEngine::builder().crash_after_go().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let mut events = state.engine_hub.events().expect("receiver");
    let effects = send(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze));
    let effects = settle(&mut state, effects).await;
    assert!(!searches(&effects).is_empty());
    let run = state.engine_runs()[0].id;
    loop {
        let (from, event) = tokio::time::timeout(Duration::from_secs(5), events.next())
            .await
            .expect("an event")
            .expect("open");
        let done = matches!(event, EngineEvent::Exited(_));
        update(
            &mut state,
            Message::Engine(EngineMessage::Event {
                run: from,
                event: Box::new(event),
            }),
        );
        if done {
            break;
        }
    }
    assert!(matches!(
        state.engine_runs.get(run).map(|r| &r.state),
        Some(EngineRunState::Failed(_))
    ));
    assert!(matches!(notice(&state), Some(EngineNotice::Crashed { .. })));
    assert_eq!(state.main_engine(), None);
}

#[tokio::test]
async fn quitting_ends_every_engine_and_only_then_exits() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B"], &fake);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    drive(&mut state, EngineMessage::Start(ids[1], StartMode::Analyze)).await;
    assert_eq!(state.engine_hub.alive(), 2);
    let effects = update(&mut state, Message::Quit);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::ShutdownEngines { .. }))
    );
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::Exit)),
        "waits for the engines"
    );
    let seen = settle(&mut state, effects).await;
    assert_eq!(state.engine_hub.alive(), 0);
    assert!(state.engine_runs().is_empty());
    assert!(
        seen.iter()
            .any(|e| matches!(e, Effect::ShutdownEngines { .. }))
    );
}

#[tokio::test]
async fn quit_requests_exit_after_the_engines_answer() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    let effects = update(&mut state, Message::Quit);
    let mut exit = false;
    let mut queue: VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        if matches!(effect, Effect::Exit) {
            exit = true;
            continue;
        }
        let answer = perform(effect).await;
        queue.extend(update(&mut state, answer));
    }
    assert!(exit, "the program exits once the engines ended");
    assert_eq!(state.engine_hub.alive(), 0);
}

/// Counts its own drops: tells a launch future was dropped.
struct Guard(Arc<std::sync::atomic::AtomicUsize>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn quit_cancels_an_engine_that_is_still_starting() {
    let dropped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut state = AppState::default();
    let counter = Arc::clone(&dropped);
    state.engine_hub = EngineHub::with_launcher(Arc::new(move |_| {
        let guard = Guard(Arc::clone(&counter));
        Box::pin(async move {
            let _guard = guard;
            std::future::pending::<LaunchResult>().await
        })
    }));
    let id = state
        .engines
        .add(EngineDraft::new("Slow", "/fake/slow"))
        .expect("add");
    let effects = send(&mut state, EngineMessage::Start(id, StartMode::Analyze));
    let start = effects
        .into_iter()
        .find(|e| matches!(e, Effect::StartEngine { .. }))
        .expect("start effect");
    let starting = tokio::spawn(perform(start));
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(state.engine_hub.launching(), 1);
    let effects = update(&mut state, Message::Quit);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::ShutdownEngines { .. }))
    );
    settle(&mut state, effects).await;
    assert_eq!(dropped.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(state.engine_runs().is_empty());
    // The late answer of the cancelled start changes nothing.
    let late = starting.await.expect("task");
    assert!(update(&mut state, late).is_empty());
    assert!(state.engine_runs().is_empty());
}

#[tokio::test]
async fn a_second_quit_ends_an_engine_started_after_the_first_one_finished() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    let effects = update(&mut state, Message::Quit);
    settle(&mut state, effects).await;
    // The quit did not go through (say, the user cancelled); an engine runs again.
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    assert_eq!(state.engine_hub.alive(), 1);
    let effects = update(&mut state, Message::Quit);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::ShutdownEngines { .. })),
        "{effects:?}"
    );
    settle(&mut state, effects).await;
    assert_eq!(state.engine_hub.alive(), 0);
}

#[test]
fn quit_with_no_engine_does_not_wait() {
    let mut state = AppState::default();
    let effects = update(&mut state, Message::Quit);
    assert!(effects.iter().any(|e| matches!(e, Effect::Exit)));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::ShutdownEngines { .. }))
    );
}

#[test]
fn quit_waits_for_a_save_of_the_list_in_flight() {
    let mut state =
        AppState::with_preferences_path(Some(PathBuf::from("p").join("preferences.toml")));
    let id = state.engines.add(EngineDraft::new("A", "a")).expect("add");
    send(&mut state, EngineMessage::SetHotkey(id, Some(Hotkey::F2)));
    let effects = update(&mut state, Message::Quit);
    assert!(effects.is_empty(), "{effects:?}");
    let after = send(&mut state, EngineMessage::Saved(Ok(())));
    assert!(
        after
            .iter()
            .any(|e| matches!(e, Effect::SavePreferences { .. } | Effect::Exit))
    );
}

// ---- editor: check, test, detect ----

#[tokio::test]
async fn the_editor_checks_the_command_on_disk_and_tests_a_missing_program() {
    let mut state = AppState::default();
    send(&mut state, EngineMessage::New);
    editor_msg(&mut state, EditorMessage::SetName("Ghost".into()));
    editor_msg(
        &mut state,
        EditorMessage::SetCommand("/nonexistent/mychess-ghost".into()),
    );
    let effects = editor_msg(&mut state, EditorMessage::Check);
    settle(&mut state, effects).await;
    let editor = state.engine_editor.as_ref().expect("editor");
    assert_eq!(editor.warnings, [CommandWarning::CommandNotFound]);
    let effects = editor_msg(&mut state, EditorMessage::Test);
    assert_eq!(
        state.engine_editor.as_ref().and_then(|e| e.test.clone()),
        Some(TestState::Running)
    );
    settle(&mut state, effects).await;
    assert!(matches!(
        state.engine_editor.as_ref().and_then(|e| e.test.clone()),
        Some(TestState::Failed(f)) if f.kind == FailureKind::CannotStart
    ));
    // A warning does not stop the save (D429).
    editor_msg(&mut state, EditorMessage::Save);
    assert_eq!(state.engines.len(), 1);
}

#[test]
fn a_stale_answer_of_the_editor_is_dropped_and_detection_is_a_suggestion() {
    let mut state = AppState::default();
    send(&mut state, EngineMessage::New);
    editor_msg(&mut state, EditorMessage::SetCommand("x".into()));
    let effects = editor_msg(&mut state, EditorMessage::Detect);
    let Some(Effect::DetectEngine { request, .. }) = effects.first() else {
        panic!("expected detection");
    };
    let request = *request;
    send(
        &mut state,
        EngineMessage::Detected {
            request: RequestId(999),
            result: Ok(Protocol::XBoard),
        },
    );
    assert_eq!(
        state.engine_editor.as_ref().map(|e| e.protocol),
        Some(Protocol::Uci)
    );
    send(
        &mut state,
        EngineMessage::Detected {
            request,
            result: Ok(Protocol::XBoard),
        },
    );
    assert_eq!(
        state.engine_editor.as_ref().map(|e| e.protocol),
        Some(Protocol::Uci),
        "only a suggestion"
    );
    editor_msg(&mut state, EditorMessage::UseDetected);
    assert_eq!(
        state.engine_editor.as_ref().map(|e| e.protocol),
        Some(Protocol::XBoard)
    );
}

#[test]
fn editing_the_command_of_a_running_engine_says_to_restart() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    state.engine_runs.begin(ids[0], true);
    send(&mut state, EngineMessage::Edit(ids[0]));
    editor_msg(&mut state, EditorMessage::SetCommand("/other".into()));
    editor_msg(&mut state, EditorMessage::Save);
    assert_eq!(notice(&state), Some(&EngineNotice::RestartToApply));
    assert_eq!(
        state.engines.get(ids[0]).map(|e| e.command.clone()),
        Some(PathBuf::from("/other"))
    );
}

#[tokio::test]
async fn setting_an_option_saves_it_and_applies_it_to_the_running_engine() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    let effects = send(
        &mut state,
        EngineMessage::SetOption {
            engine: ids[0],
            name: "Hash".into(),
            value: "64".into(),
        },
    );
    assert_eq!(
        state.engines.get(ids[0]).map(|e| e.options.clone()),
        Some(vec![("Hash".to_owned(), "64".to_owned())])
    );
    assert!(commands(&effects).iter().any(|(_, c)| matches!(c, mychess_engine::EngineCommand::SetOption { name, .. } if name == "Hash")));
    state.engine_hub.shutdown_all(Duration::from_secs(2)).await;
}

// ---- finding and importing ----

#[tokio::test]
async fn found_engines_are_only_offered_and_added_after_a_probe() {
    let mut state = AppState::default();
    send(&mut state, EngineMessage::OpenDialog);
    let effects = send(&mut state, EngineMessage::FindInstalled);
    assert!(matches!(
        effects.first(),
        Some(Effect::FindInstalledEngines { .. })
    ));
    assert!(state.engine_dialog.as_ref().is_some_and(|d| d.finding));
    let Some(Effect::FindInstalledEngines { request }) = effects.first() else {
        return;
    };
    let found = FoundEngine {
        name: "stockfish".into(),
        path: PathBuf::from("/usr/games/stockfish"),
    };
    send(
        &mut state,
        EngineMessage::Found {
            request: *request,
            engines: vec![found.clone()],
        },
    );
    assert_eq!(
        state.engine_dialog.as_ref().map(|d| d.found.clone()),
        Some(vec![found.clone()])
    );
    assert!(state.engines.is_empty(), "nothing enters the list unmarked");
    let effects = send(&mut state, EngineMessage::AddFound(found.path.clone()));
    let Some(Effect::ProbeFoundEngine { request, .. }) = effects.first() else {
        panic!("expected a probe");
    };
    let request = *request;
    send(
        &mut state,
        EngineMessage::FoundProbed {
            request,
            path: found.path.clone(),
            result: Err(EngineFailure {
                kind: FailureKind::NotAnEngine,
                exit_code: None,
                stderr_tail: vec![],
            }),
        },
    );
    assert!(state.engines.is_empty(), "a failed probe adds nothing");
    assert!(matches!(
        notice(&state),
        Some(EngineNotice::FoundRejected { .. })
    ));
    let effects = send(&mut state, EngineMessage::AddFound(found.path.clone()));
    let Some(Effect::ProbeFoundEngine { request, .. }) = effects.first() else {
        panic!("expected a probe");
    };
    send(
        &mut state,
        EngineMessage::FoundProbed {
            request: *request,
            path: found.path,
            result: Ok((Protocol::Uci, "Stockfish 17".into())),
        },
    );
    assert_eq!(state.engines.engines()[0].name, "Stockfish 17");
    assert_eq!(
        state.engines.hotkey_of(state.engines.engines()[0].id),
        Some(Hotkey::F2)
    );
    assert!(
        state
            .engine_dialog
            .as_ref()
            .is_some_and(|d| d.found.is_empty() && d.probing == 0)
    );
}

#[tokio::test]
async fn importing_an_engines_dat_adds_the_engines_once_and_reports() {
    let dir = tempfile::tempdir().expect("dir");
    let dat = dir.path().join("engines.dat");
    std::fs::write(
        &dat,
        "# Scid engines\nengine {\n  Name Crafty\n  Cmd crafty\n  Args {-x \"a b\"}\n  Dir .\n  Elo 2500\n  UCI 0\n}\nengine {\n  Name Bad\n}\n",
    )
    .expect("write");
    let mut state = AppState::default();
    drive(&mut state, EngineMessage::ImportScid(dat.clone())).await;
    assert_eq!(state.engines.len(), 1);
    assert_eq!(state.engines.engines()[0].elo, Some(2500));
    assert_eq!(
        notice(&state),
        Some(&EngineNotice::Imported {
            added: 1,
            duplicates: 0,
            invalid: 1
        })
    );
    let report = state
        .engine_dialog
        .as_ref()
        .and_then(|d| d.import.clone())
        .expect("report");
    assert_eq!(report.added.len(), 1);
    assert!(!report.warnings.is_empty(), "the relative path is remarked");
    drive(&mut state, EngineMessage::ImportScid(dat)).await;
    assert_eq!(state.engines.len(), 1, "duplicates are skipped");
    assert_eq!(
        notice(&state),
        Some(&EngineNotice::Imported {
            added: 0,
            duplicates: 1,
            invalid: 1
        })
    );
    drive(
        &mut state,
        EngineMessage::ImportScid(dir.path().join("missing.dat")),
    )
    .await;
    assert!(matches!(
        notice(&state),
        Some(EngineNotice::ImportFailed(_))
    ));
}

#[tokio::test]
async fn importing_hostile_text_never_evaluates_it() {
    let dir = tempfile::tempdir().expect("dir");
    let marker = dir.path().join("pwned");
    let dat = dir.path().join("engines.dat");
    std::fs::write(
        &dat,
        format!(
            "exec touch {m}\nengine {{ Name [exec touch {m}] Cmd $env(HOME) Dir . UCI 1 }}\n",
            m = marker.display()
        ),
    )
    .expect("write");
    let mut state = AppState::default();
    drive(&mut state, EngineMessage::ImportScid(dat)).await;
    assert!(!marker.exists());
    assert_eq!(state.engines.len(), 1);
    assert!(state.engines.engines()[0].name.contains("[exec"));
}

#[tokio::test]
async fn a_file_without_engine_blocks_reports_nothing_to_import() {
    let dir = tempfile::tempdir().expect("dir");
    let dat = dir.path().join("engines.dat");
    std::fs::write(&dat, "set x 1\n").expect("write");
    let mut state = AppState::default();
    drive(&mut state, EngineMessage::ImportScid(dat)).await;
    assert_eq!(notice(&state), Some(&EngineNotice::NothingToImport));
}

// ---- preferences ----

#[test]
fn analysis_preferences_change_through_the_message_and_feed_new_runs() {
    let mut state = AppState::default();
    let prefs = AnalysisPrefs {
        low_priority: true,
        log_lines: 7,
        ..AnalysisPrefs::default()
    };
    send(&mut state, EngineMessage::SetAnalysisPrefs(prefs));
    assert_eq!(*state.prefs.analysis, prefs);
    let id = state.engines.add(EngineDraft::new("A", "a")).expect("add");
    let effects = send(&mut state, EngineMessage::Start(id, StartMode::Analyze));
    let Some(Effect::StartEngine { defaults, .. }) = effects.first() else {
        panic!("expected a start");
    };
    assert_eq!((defaults.low_priority, defaults.log_lines), (true, 7));
}

#[test]
fn notices_have_message_keys() {
    let n = AppNotice::Engine(EngineNotice::NoEngineOnKey(Hotkey::F2));
    assert_eq!(n.message_key(), "engines-notice-no-engine-on-key");
    let failure = EngineFailure {
        kind: FailureKind::Crashed,
        exit_code: Some(1),
        stderr_tail: vec![],
    };
    assert_eq!(
        AppNotice::Engine(EngineNotice::Crashed {
            name: "A".into(),
            failure
        })
        .message_key(),
        "engines-failure-crashed"
    );
}

#[tokio::test]
async fn delete_without_the_dialog_open_is_ignored() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    send(&mut state, EngineMessage::Delete(ids[0]));
    assert!(state.engine_dialog.is_none());
}

#[test]
fn second_engine_added_gets_f3_when_the_first_holds_f2() {
    let mut list = EngineList::default();
    let a = list.add(EngineDraft::new("A", "a")).unwrap();
    assert!(list.assign_default_hotkeys());
    let b = list.add(EngineDraft::new("B", "b")).unwrap();
    assert!(list.assign_default_hotkeys());
    assert_eq!(list.hotkey_of(a), Some(Hotkey::ALL[0]));
    assert_eq!(list.hotkey_of(b), Some(Hotkey::ALL[1]));
    let c = list.add(EngineDraft::new("C", "c")).unwrap();
    assert!(!list.assign_default_hotkeys());
    assert_eq!(list.hotkey_of(c), None);
}

// ---- the UCI options dialog (D430, D740-D742) ----

fn options_identity() -> EngineIdentity {
    EngineIdentity {
        name: "Fake".to_owned(),
        author: None,
        options: vec![
            EngineOption {
                name: "Hash".to_owned(),
                kind: OptionKind::Spin {
                    default: 16,
                    min: 1,
                    max: 1024,
                },
            },
            EngineOption {
                name: "Clear Hash".to_owned(),
                kind: OptionKind::Button,
            },
        ],
        capabilities: Capabilities::default(),
    }
}

fn opened_options(state: &mut AppState, id: EngineId) -> RequestId {
    send(state, EngineMessage::OpenDialog);
    let effects = send(state, EngineMessage::OpenOptions(id));
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::TestEngine { request, .. } => Some(*request),
            _ => None,
        })
        .expect("a probe is asked for");
    send(
        state,
        EngineMessage::Tested {
            request,
            result: Box::new(Ok(options_identity())),
        },
    );
    request
}

#[tokio::test]
async fn options_open_with_a_probe_and_save_only_what_differs() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    state.flow.prefs_path = Some(PathBuf::from("p").join("preferences.toml"));
    opened_options(&mut state, ids[0]);
    let set = |name: &str, value: &str| {
        EngineMessage::Options(OptionsMessage::Set {
            name: name.to_owned(),
            value: value.to_owned(),
        })
    };
    send(&mut state, set("Hash", "0"));
    let effects = send(&mut state, EngineMessage::Options(OptionsMessage::Save));
    assert!(!has_save(&effects), "an out of range value blocks the save");
    let dialog = state.engine_dialog.as_ref().expect("dialog");
    assert!(
        dialog
            .options
            .as_ref()
            .expect("options")
            .error_of("Hash")
            .is_some()
    );
    send(&mut state, set("Hash", "64"));
    let effects = send(&mut state, EngineMessage::Options(OptionsMessage::Save));
    assert!(has_save(&effects));
    assert!(
        state
            .engine_dialog
            .as_ref()
            .expect("dialog")
            .options
            .is_none()
    );
    assert_eq!(
        state.engines.get(ids[0]).expect("entry").options,
        [("Hash".to_owned(), "64".to_owned())]
    );
    // Back to the default: nothing is saved for it.
    opened_options(&mut state, ids[0]);
    send(&mut state, EngineMessage::Options(OptionsMessage::Reset));
    send(&mut state, EngineMessage::Options(OptionsMessage::Save));
    assert!(state.engines.get(ids[0]).expect("entry").options.is_empty());
}

#[tokio::test]
async fn options_of_a_running_engine_apply_at_once_and_buttons_need_it_running() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    opened_options(&mut state, ids[0]);
    let effects = send(
        &mut state,
        EngineMessage::Options(OptionsMessage::Press("Clear Hash".to_owned())),
    );
    assert!(
        commands(&effects).is_empty(),
        "not running: nothing to send"
    );
    send(&mut state, EngineMessage::Options(OptionsMessage::Close));
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Idle)).await;
    send(&mut state, EngineMessage::OpenDialog);
    let effects = send(&mut state, EngineMessage::OpenOptions(ids[0]));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::TestEngine { .. })),
        "a running engine already told its options"
    );
    let options = state
        .engine_dialog
        .as_ref()
        .and_then(|d| d.options.as_ref())
        .expect("options");
    assert!(options.identity.is_some());
}

#[tokio::test]
async fn a_failed_probe_is_shown_and_a_late_answer_is_ignored() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    send(&mut state, EngineMessage::OpenDialog);
    let effects = send(&mut state, EngineMessage::OpenOptions(ids[0]));
    let Some(Effect::TestEngine { request, .. }) = effects.first().cloned() else {
        panic!("probe");
    };
    send(
        &mut state,
        EngineMessage::Tested {
            request: RequestId(request.0 + 99),
            result: Box::new(Ok(options_identity())),
        },
    );
    let status = |s: &AppState| {
        s.engine_dialog
            .as_ref()
            .and_then(|d| d.options.as_ref())
            .map(|o| o.status.clone())
    };
    assert_eq!(status(&state), Some(OptionsStatus::Probing));
    let failure = EngineFailure {
        kind: FailureKind::CannotStart,
        exit_code: None,
        stderr_tail: vec![],
    };
    send(
        &mut state,
        EngineMessage::Tested {
            request,
            result: Box::new(Err(failure.clone())),
        },
    );
    assert_eq!(status(&state), Some(OptionsStatus::Failed(failure)));
}
