//! M4-3 contract tests of `perform` with engines: the in-memory fake (several
//! runs, events, shutdown within 1.5 s of virtual time, handshake timeouts)
//! and, when `mychess-fake-engine` is built, real processes (probe, detect,
//! found engines, no orphans). Spec D435, D720-D723, D727; rules 5, 13;
//! acceptance criteria 4 and 7.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string
)]

mod m4_3_common;

use std::sync::Arc;
use std::time::Duration;

use m4_3_common::*;
use mychess_app::engines::{
    DetectState, EditorMessage, EngineDraft, EngineHub, EngineMessage, EngineNotice,
    EngineRunState, FailureKind, RunId, SHUTDOWN_WAIT, StartMode, TestState,
};
use mychess_app::{AppState, Effect, Message, perform, update};
use mychess_engine::testing::ScriptedEngine;
use mychess_engine::{EngineConfig, EngineEvent, ExitReason, Phase, Protocol};

/// A hub that picks the fake by the engine's command (`fake-<name>`).
fn hub_by_command(fakes: Vec<(&'static str, ScriptedEngine)>) -> EngineHub {
    EngineHub::with_launcher(Arc::new(move |config: EngineConfig| {
        let name = config.command.to_string_lossy().into_owned();
        let fake = fakes
            .iter()
            .find(|(n, _)| name.ends_with(n))
            .map(|(_, f)| f.clone())
            .expect("a fake for the command");
        Box::pin(async move { fake.start(config).await })
    }))
}

fn state_for(
    names: &[&'static str],
    fakes: Vec<(&'static str, ScriptedEngine)>,
) -> (AppState, Vec<mychess_app::engines::EngineId>) {
    let mut state = AppState::default();
    state.engine_hub = hub_by_command(fakes);
    let ids = names
        .iter()
        .map(|n| {
            state
                .engines
                .add(EngineDraft::new(*n, format!("fake-{n}")))
                .unwrap()
        })
        .collect();
    (state, ids)
}

fn received(fake: &ScriptedEngine, needle: &str) -> usize {
    fake.received()
        .iter()
        .filter(|l| l.contains(needle))
        .count()
}

async fn until<F: FnMut() -> bool>(mut cond: F) {
    for _ in 0..2000 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition not reached");
}

#[tokio::test]
async fn start_effect_runs_the_handshake_and_the_run_becomes_running_with_identity() {
    let fake = ScriptedEngine::builder().name("Alpha").build();
    let (mut state, ids) = state_for(&["A"], vec![("fake-A", fake.clone())]);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    let run = &state.engine_runs()[0];
    assert_eq!(run.state, EngineRunState::Running);
    assert_eq!(run.identity.as_ref().unwrap().name, "Alpha");
    assert_eq!(state.engine_hub.alive(), 1);
    assert!(state.engines.get(ids[0]).unwrap().last_used.is_some());
    // The first position was sent by the same settle: a "go" reached the engine.
    until(|| received(&fake, "go") >= 1).await;
}

#[tokio::test]
async fn several_runs_each_receive_the_position_change_and_stop_independently() {
    let fa = ScriptedEngine::builder().name("A").build();
    let fb = ScriptedEngine::builder().name("B").build();
    let fc = ScriptedEngine::builder().name("C").build();
    let (mut state, ids) = state_for(
        &["A", "B", "C"],
        vec![
            ("fake-A", fa.clone()),
            ("fake-B", fb.clone()),
            ("fake-C", fc.clone()),
        ],
    );
    for id in &ids {
        drive(&mut state, EngineMessage::Start(*id, StartMode::Analyze)).await;
    }
    assert_eq!(state.engine_hub.alive(), 3);
    let fx = play(&mut state, "e2e4");
    settle(&mut state, fx).await;
    for f in [&fa, &fb, &fc] {
        until(|| received(f, "position startpos moves e2e4") >= 1).await;
    }
    // Stopping B leaves the others alive and still receiving.
    let b = state.engine_runs()[1].id;
    drive(&mut state, EngineMessage::Stop(b)).await;
    assert_eq!(state.engine_hub.alive(), 2);
    assert_eq!(state.engine_runs().len(), 2);
    let fx = play(&mut state, "e7e5");
    settle(&mut state, fx).await;
    until(|| received(&fa, "moves e2e4 e7e5") >= 1).await;
    until(|| received(&fc, "moves e2e4 e7e5") >= 1).await;
    assert_eq!(
        received(&fb, "moves e2e4 e7e5"),
        0,
        "a stopped engine gets nothing"
    );
}

#[tokio::test]
async fn start_failure_effect_gives_a_failed_run_with_exit_data_and_the_engine_can_restart() {
    let mut state = AppState::default();
    state.engine_hub = failing_hub();
    let id = state.engines.add(EngineDraft::new("Bad", "bad")).unwrap();
    drive(&mut state, EngineMessage::Start(id, StartMode::Analyze)).await;
    let run = state.engine_runs()[0].clone();
    match run.state {
        EngineRunState::Failed(f) => {
            assert_eq!(f.kind, FailureKind::ExitedEarly);
            assert_eq!(f.stderr_tail, ["boom"]);
        }
        other => panic!("expected failure, got {other:?}"),
    }
    assert!(matches!(
        notice(&state),
        Some(EngineNotice::StartFailed { name, .. }) if name == "Bad"
    ));
    assert_eq!(state.engine_hub.alive(), 0);
    drive(&mut state, EngineMessage::Restart(run.id)).await;
    assert_eq!(state.engine_runs().len(), 1);
    assert_ne!(state.engine_runs()[0].id, run.id);
}

#[tokio::test(start_paused = true)]
async fn a_silent_engine_times_out_the_handshake_with_a_translatable_failure() {
    let fake = ScriptedEngine::builder().silent().build();
    let (mut state, ids) = state_for(&["S"], vec![("fake-S", fake)]);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    match &state.engine_runs()[0].state {
        EngineRunState::Failed(f) => {
            assert_eq!(f.kind, FailureKind::Timeout(Phase::Uciok));
            assert_eq!(f.message_key(), "engines-failure-timeout");
        }
        other => panic!("expected timeout, got {other:?}"),
    }
    assert_eq!(state.engine_hub.alive(), 0);
    assert_eq!(state.main_engine(), None);
}

#[tokio::test(start_paused = true)]
async fn shutdown_ends_all_engines_within_the_total_wait_even_if_they_ignore_quit() {
    let stubborn = ScriptedEngine::builder().ignore_quit().build();
    let polite = ScriptedEngine::builder().build();
    let (mut state, ids) = state_for(
        &["A", "B", "C"],
        vec![
            ("fake-A", stubborn.clone()),
            ("fake-B", polite),
            ("fake-C", stubborn),
        ],
    );
    for id in &ids {
        drive(&mut state, EngineMessage::Start(*id, StartMode::Idle)).await;
    }
    assert_eq!(state.engine_hub.alive(), 3);
    let before = tokio::time::Instant::now();
    let answer = perform(Effect::ShutdownEngines {
        hub: state.engine_hub.clone(),
    })
    .await;
    let elapsed = before.elapsed();
    assert_eq!(answer, Message::Engine(EngineMessage::ShutdownDone));
    assert_eq!(state.engine_hub.alive(), 0);
    assert!(
        elapsed <= SHUTDOWN_WAIT + Duration::from_millis(50),
        "took {elapsed:?} of virtual time"
    );
    assert_eq!(SHUTDOWN_WAIT, Duration::from_millis(1500));
}

#[tokio::test]
async fn full_quit_flow_with_running_engines_ends_in_exit_and_empties_the_hub() {
    let fa = ScriptedEngine::builder().build();
    let fb = ScriptedEngine::builder().build();
    let dir = tempfile::tempdir().unwrap();
    let (mut state, ids) = state_for(&["A", "B"], vec![("fake-A", fa), ("fake-B", fb)]);
    state.flow_set_prefs_path(&temp_prefs_path(dir.path()));
    for id in &ids {
        drive(&mut state, EngineMessage::Start(*id, StartMode::Analyze)).await;
    }
    let effects = update(&mut state, Message::Quit);
    let seen = settle(&mut state, effects).await;
    assert!(
        seen.iter()
            .any(|e| matches!(e, Effect::ShutdownEngines { .. }))
    );
    assert_eq!(state.engine_hub.alive(), 0);
    assert!(state.engine_runs().is_empty());
}

trait PrefsPath {
    fn flow_set_prefs_path(&mut self, path: &std::path::Path);
}
impl PrefsPath for AppState {
    fn flow_set_prefs_path(&mut self, path: &std::path::Path) {
        let hub = self.engine_hub.clone();
        let engines = self.engines.clone();
        *self = AppState::with_preferences_path(Some(path.to_path_buf()));
        self.engine_hub = hub;
        self.engines = engines;
    }
}

#[tokio::test]
async fn events_arrive_through_the_single_stream_exactly_once_and_a_crash_marks_the_run_failed() {
    let fake = ScriptedEngine::builder().crash_after_go().build();
    let (mut state, ids) = state_for(&["X"], vec![("fake-X", fake)]);
    let mut stream = state
        .engine_hub
        .events()
        .expect("first call gets the stream");
    assert!(
        state.engine_hub.events().is_none(),
        "the receiver is handed out once"
    );
    let hub_clone = state.engine_hub.clone();
    assert!(hub_clone.events().is_none());
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze)).await;
    let run = state.engine_runs()[0].id;
    // The search sent after the start makes the fake die.
    let exited = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let (r, event) = stream.next().await.expect("hub alive");
            assert_eq!(r, run);
            if matches!(event, EngineEvent::Exited(_)) {
                return event;
            }
            update(
                &mut state,
                Message::Engine(EngineMessage::Event {
                    run: r,
                    event: Box::new(event),
                }),
            );
        }
    })
    .await
    .expect("an exit event");
    let EngineEvent::Exited(info) = &exited else {
        unreachable!()
    };
    assert!(matches!(info.reason, ExitReason::Crashed));
    update(
        &mut state,
        Message::Engine(EngineMessage::Event {
            run,
            event: Box::new(exited),
        }),
    );
    assert!(matches!(
        state.engine_runs()[0].state,
        EngineRunState::Failed(_)
    ));
    assert!(matches!(notice(&state), Some(EngineNotice::Crashed { .. })));
    // The hub forgets the dead engine by itself.
    until(|| state.engine_hub.alive() == 0).await;
}

#[tokio::test]
async fn commands_to_a_run_that_ended_answer_not_running_without_panicking() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_for(&["A"], vec![("fake-A", fake)]);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Idle)).await;
    let run = state.engine_runs()[0].id;
    drive(&mut state, EngineMessage::Stop(run)).await;
    let answer = perform(Effect::EngineCommand {
        run,
        command: Box::new(mychess_engine::EngineCommand::Stop),
        hub: state.engine_hub.clone(),
    })
    .await;
    assert_eq!(
        answer,
        Message::Engine(EngineMessage::CommandDone {
            run,
            result: Err(mychess_app::engines::CommandError::NotRunning)
        })
    );
    // Stopping an unknown run is harmless too.
    let answer = perform(Effect::StopEngine {
        run: RunId(9999),
        hub: state.engine_hub.clone(),
    })
    .await;
    assert_eq!(answer, Message::Engine(EngineMessage::Stopped(RunId(9999))));
}

#[tokio::test]
async fn changing_an_unsupported_option_is_refused_with_a_notice_not_a_crash() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_for(&["A"], vec![("fake-A", fake)]);
    drive(&mut state, EngineMessage::Start(ids[0], StartMode::Idle)).await;
    drive(
        &mut state,
        EngineMessage::SetOption {
            engine: ids[0],
            name: "NoSuchOption".into(),
            value: "1".into(),
        },
    )
    .await;
    assert_eq!(notice(&state), Some(&EngineNotice::CommandRefused));
    assert_eq!(state.engine_runs()[0].state, EngineRunState::Running);
}

// ---- real processes ----

macro_rules! need_fake {
    () => {
        match fake_binary() {
            Some(p) => p,
            None => {
                eprintln!(
                    "mychess-fake-engine not built (cargo build -p mychess-engine --features testing --bin mychess-fake-engine): skipping"
                );
                return;
            }
        }
    };
}

fn draft_for(binary: &std::path::Path, args: &[&str]) -> EngineDraft {
    let mut d = EngineDraft::new("Fake", binary);
    d.args = args.iter().map(|a| (*a).to_owned()).collect();
    d
}

async fn tested(state: &mut AppState) -> TestState {
    let fx = update(
        state,
        Message::Engine(EngineMessage::Editor(EditorMessage::Test)),
    );
    assert_eq!(fx.len(), 1, "{fx:?}");
    settle(state, fx).await;
    state.engine_editor.as_ref().unwrap().test.clone().unwrap()
}

fn editor_for(state: &mut AppState, command: &std::path::Path, args: &str, protocol: Protocol) {
    send(state, EngineMessage::New);
    let e = |s: &mut AppState, m| {
        send(s, EngineMessage::Editor(m));
    };
    e(
        state,
        EditorMessage::SetCommand(command.to_string_lossy().into_owned()),
    );
    e(state, EditorMessage::SetArgs(args.to_owned()));
    e(state, EditorMessage::SetProtocol(protocol));
}

#[tokio::test]
async fn test_engine_with_the_real_fake_reports_identity_and_options_for_uci_and_xboard() {
    let bin = need_fake!();
    let mut state = AppState::default();
    editor_for(&mut state, &bin, "--name FakeFish", Protocol::Uci);
    match tested(&mut state).await {
        TestState::Passed(identity) => {
            assert_eq!(identity.name, "FakeFish");
            assert!(!identity.options.is_empty());
        }
        other => panic!("expected pass, got {other:?}"),
    }
    // The name is not required to test (the form has none yet).
    assert!(state.engine_editor.as_ref().unwrap().error.is_none());
    editor_for(
        &mut state,
        &bin,
        "--protocol xboard --name XFish",
        Protocol::XBoard,
    );
    match tested(&mut state).await {
        TestState::Passed(identity) => assert_eq!(identity.name, "XFish"),
        other => panic!("expected pass, got {other:?}"),
    }
}

#[tokio::test]
async fn test_engine_with_a_missing_executable_fails_with_cannot_start() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    editor_for(
        &mut state,
        &dir.path().join("no-such-engine"),
        "",
        Protocol::Uci,
    );
    match tested(&mut state).await {
        TestState::Failed(f) => {
            assert_eq!(f.kind, FailureKind::CannotStart);
            assert_eq!(f.message_key(), "engines-failure-cannot-start");
        }
        other => panic!("expected failure, got {other:?}"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_engine_with_a_program_that_dies_reports_exit_code_and_stderr() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("dies.sh");
    std::fs::write(&script, "#!/bin/sh\necho 'oops it broke' >&2\nexit 7\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut state = AppState::default();
    editor_for(&mut state, &script, "", Protocol::Uci);
    match tested(&mut state).await {
        TestState::Failed(f) => {
            assert_eq!(f.kind, FailureKind::ExitedEarly);
            assert_eq!(f.exit_code, Some(7));
            assert!(
                f.stderr_tail.iter().any(|l| l.contains("oops it broke")),
                "{f:?}"
            );
        }
        other => panic!("expected failure, got {other:?}"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_non_executable_file_fails_the_test_without_hanging() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.txt");
    std::fs::write(&file, "not a program").unwrap();
    let mut state = AppState::default();
    editor_for(&mut state, &file, "", Protocol::Uci);
    assert!(matches!(tested(&mut state).await, TestState::Failed(_)));
}

#[tokio::test]
async fn detect_protocol_finds_uci_with_the_real_fake_and_it_is_only_a_suggestion() {
    let bin = need_fake!();
    let mut state = AppState::default();
    editor_for(&mut state, &bin, "", Protocol::XBoard);
    let fx = update(
        &mut state,
        Message::Engine(EngineMessage::Editor(EditorMessage::Detect)),
    );
    settle(&mut state, fx).await;
    let editor = state.engine_editor.as_ref().unwrap();
    assert_eq!(editor.detect, Some(DetectState::Found(Protocol::Uci)));
    assert_eq!(
        editor.protocol,
        Protocol::XBoard,
        "the form is not changed by itself"
    );
    send(
        &mut state,
        EngineMessage::Editor(EditorMessage::UseDetected),
    );
    assert_eq!(
        state.engine_editor.as_ref().unwrap().protocol,
        Protocol::Uci
    );
}

#[tokio::test]
#[ignore = "slow: waits the ~5 s UCI deadline before trying XBoard"]
async fn detect_protocol_falls_back_to_xboard_after_the_uci_deadline() {
    let bin = need_fake!();
    let mut state = AppState::default();
    editor_for(&mut state, &bin, "--protocol xboard", Protocol::Uci);
    let fx = update(
        &mut state,
        Message::Engine(EngineMessage::Editor(EditorMessage::Detect)),
    );
    settle(&mut state, fx).await;
    assert_eq!(
        state.engine_editor.as_ref().unwrap().detect,
        Some(DetectState::Found(Protocol::XBoard))
    );
}

#[tokio::test]
async fn detect_protocol_of_a_missing_program_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    editor_for(&mut state, &dir.path().join("none"), "", Protocol::Uci);
    let fx = update(
        &mut state,
        Message::Engine(EngineMessage::Editor(EditorMessage::Detect)),
    );
    settle(&mut state, fx).await;
    assert!(matches!(
        state.engine_editor.as_ref().unwrap().detect,
        Some(DetectState::Failed(_))
    ));
}

#[tokio::test]
async fn a_found_engine_is_probed_then_added_with_the_name_it_gives_and_default_keys() {
    let bin = need_fake!();
    let mut state = AppState::default();
    send(&mut state, EngineMessage::OpenDialog);
    drive(&mut state, EngineMessage::AddFound(bin.clone())).await;
    assert_eq!(state.engines.len(), 1);
    let e = &state.engines.engines()[0];
    assert_eq!(e.name, "FakeFish");
    assert_eq!(e.command, bin);
    assert_eq!(e.protocol, Protocol::Uci);
    assert_eq!(
        state.engines.hotkey_of(e.id),
        Some(mychess_app::engines::Hotkey::F2)
    );
    assert_eq!(state.engine_dialog.as_ref().unwrap().probing, 0);
    assert!(
        state.engine_runs().is_empty(),
        "probing leaves no run behind"
    );
    assert_eq!(state.engine_hub.alive(), 0);
}

#[tokio::test]
async fn a_found_engine_that_does_not_answer_is_rejected_and_not_added() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    send(&mut state, EngineMessage::OpenDialog);
    drive(
        &mut state,
        EngineMessage::AddFound(dir.path().join("ghost")),
    )
    .await;
    assert!(state.engines.is_empty());
    assert!(matches!(
        notice(&state),
        Some(EngineNotice::FoundRejected { .. })
    ));
    assert_eq!(state.engine_dialog.as_ref().unwrap().probing, 0);
}

#[tokio::test]
async fn the_find_effect_answers_with_a_list_and_runs_nothing() {
    let mut state = AppState::default();
    send(&mut state, EngineMessage::OpenDialog);
    let fx = send(&mut state, EngineMessage::FindInstalled);
    assert!(state.engine_dialog.as_ref().unwrap().finding);
    settle(&mut state, fx).await;
    assert!(!state.engine_dialog.as_ref().unwrap().finding);
    assert!(state.engine_runs().is_empty());
}

#[tokio::test]
async fn a_real_process_runs_receives_the_position_and_leaves_no_orphan_after_shutdown() {
    let bin = need_fake!();
    let marker = format!(
        "mychess-m43-{}-{}",
        std::process::id(),
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    );
    let mut state = AppState::default();
    let mut d = draft_for(&bin, &["--name", &marker]);
    d.name = "Real".into();
    let id = state.engines.add(d).unwrap();
    let d2 = {
        let mut d = draft_for(&bin, &["--name", &format!("{marker}-2")]);
        d.name = "Real2".into();
        d
    };
    let id2 = state.engines.add(d2).unwrap();
    drive(&mut state, EngineMessage::Start(id, StartMode::Analyze)).await;
    drive(&mut state, EngineMessage::Start(id2, StartMode::Analyze)).await;
    assert_eq!(state.engine_hub.alive(), 2);
    let fx = play(&mut state, "e2e4");
    settle(&mut state, fx).await;
    #[cfg(target_os = "linux")]
    assert_eq!(processes_with(&marker), 2, "two engine processes are alive");
    let answer = perform(Effect::ShutdownEngines {
        hub: state.engine_hub.clone(),
    })
    .await;
    assert_eq!(answer, Message::Engine(EngineMessage::ShutdownDone));
    assert_eq!(state.engine_hub.alive(), 0);
    #[cfg(target_os = "linux")]
    {
        // Allow the OS a moment to reap.
        for _ in 0..100 {
            if processes_with(&marker) == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(processes_with(&marker), 0, "no orphan process may survive");
    }
}

#[cfg(target_os = "linux")]
fn processes_with(marker: &str) -> usize {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_ascii_digit())
        })
        .filter(|e| {
            std::fs::read(e.path().join("cmdline")).is_ok_and(|b| {
                let text = String::from_utf8_lossy(&b).replace('\0', " ");
                text.contains(marker) && text.contains("mychess-fake-engine")
            })
        })
        .count()
}

#[tokio::test]
async fn starting_and_stopping_a_real_process_twice_in_a_row_is_clean() {
    let bin = need_fake!();
    let mut state = AppState::default();
    let id = state.engines.add(draft_for(&bin, &[])).unwrap();
    for _ in 0..2 {
        drive(&mut state, EngineMessage::Start(id, StartMode::Idle)).await;
        assert_eq!(state.engine_runs()[0].state, EngineRunState::Running);
        assert_eq!(state.engine_hub.alive(), 1);
        drive(&mut state, EngineMessage::Start(id, StartMode::Idle)).await;
        assert!(state.engine_runs().is_empty());
        assert_eq!(state.engine_hub.alive(), 0);
    }
}
