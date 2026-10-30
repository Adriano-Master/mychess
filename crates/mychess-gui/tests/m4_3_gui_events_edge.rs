//! Engine events through the shell, remarks in the status bar and the
//! limits of the dialogs (spec M4-3 rules 5, 6, D721, D730, D745). Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m4_3_gui_common;

use m4_3_gui_common::*;
use mychess_app::engines::{
    Capabilities, EditorMessage as E, EngineDraft, EngineEvent, EngineIdentity, EngineMessage,
    EngineNotice, EngineRunState, EngineWarning, FailureKind, RunId, StartMode,
};
use mychess_app::{AppNotice, Effect};

fn notice(rig: &Rig) -> Option<AppNotice> {
    rig.shell
        .ui
        .status_notice
        .clone()
        .or_else(|| rig.shell.app.notice.clone())
}

fn warning_event(run: RunId) -> EngineMessage {
    EngineMessage::Event {
        run,
        event: Box::new(EngineEvent::Warning(EngineWarning::IllegalBestMove(
            "x".to_owned(),
        ))),
    }
}

fn identity() -> EngineIdentity {
    EngineIdentity {
        name: "Fake".to_owned(),
        author: None,
        options: vec![],
        capabilities: Capabilities::default(),
    }
}

#[test]
fn an_engine_event_does_not_erase_the_remark_in_the_status_bar_but_a_user_command_does() {
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::ToggleHotkey(Hotkey::F3));
    assert_eq!(
        notice(&rig),
        Some(AppNotice::Engine(EngineNotice::NoEngineOnKey(Hotkey::F3)))
    );
    let run = start(&mut rig, ids[0]);
    // `Start` is a user command: the old remark is gone.
    assert_ne!(
        notice(&rig),
        Some(AppNotice::Engine(EngineNotice::NoEngineOnKey(Hotkey::F3)))
    );
    send(&mut rig, EngineMessage::ToggleHotkey(Hotkey::F3));
    let before = notice(&rig);
    assert!(before.is_some());
    send(&mut rig, warning_event(run));
    send(&mut rig, warning_event(run));
    assert_eq!(notice(&rig), before, "events leave the remark alone");
}

use mychess_app::engines::Hotkey;

#[test]
fn events_of_a_run_that_no_longer_exists_are_ignored() {
    let (mut rig, ids) = trio();
    let run = start(&mut rig, ids[0]);
    send(&mut rig, EngineMessage::Stop(run));
    send(&mut rig, EngineMessage::Stopped(run));
    for n in 0..3 {
        send(&mut rig, warning_event(run));
        send(&mut rig, warning_event(RunId(10_000 + n)));
    }
    assert_eq!(rig.shell.app.engine_runs.len(), 0);
}

#[test]
fn many_events_in_a_row_keep_the_shell_alive_and_the_dialog_drawable() {
    let (mut rig, ids) = trio();
    let run = start(&mut rig, ids[0]);
    for _ in 0..2000 {
        send(&mut rig, warning_event(run));
    }
    assert!(has(&rig, "Stockfish  [starting...]"));
}

#[test]
fn a_start_failure_shows_the_name_the_reason_and_the_restart_button_works() {
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::Select(Some(ids[1])));
    let run = start(&mut rig, ids[1]);
    let mut f = failure(FailureKind::ExitedEarly);
    f.exit_code = Some(2);
    f.stderr_tail = vec!["bad option".to_owned()];
    send(&mut rig, EngineMessage::StartFailed { run, failure: f });
    assert!(has(&rig, "Lc0  [failed]"));
    assert!(has(&rig, "Restart"));
    let sent = click(&mut rig, "Restart");
    assert_eq!(sent, [EngineMessage::Restart(run)]);
    let new = rig.shell.app.engine_runs.of_engine(ids[1]).unwrap();
    assert_ne!(new.id, run, "a fresh execution");
    assert_eq!(new.state, EngineRunState::Starting);
    assert!(has(&rig, "Lc0  [starting...]"));
}

#[test]
fn stop_on_a_failed_run_removes_it_from_the_list() {
    let (mut rig, ids) = trio();
    let run = start(&mut rig, ids[0]);
    send(
        &mut rig,
        EngineMessage::StartFailed {
            run,
            failure: failure(FailureKind::CannotStart),
        },
    );
    send(&mut rig, EngineMessage::Stop(run));
    assert!(rig.shell.app.engine_runs.is_empty());
    assert!(has(&rig, "Start"));
}

#[test]
fn a_started_answer_for_a_run_stopped_meanwhile_ends_the_process() {
    let (mut rig, ids) = trio();
    let run = start(&mut rig, ids[0]);
    send(&mut rig, EngineMessage::Stop(run));
    let effects = update(
        &mut rig,
        EngineMessage::Started {
            run,
            identity: Box::new(identity()),
            at: 5,
        },
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::StopEngine { run: r, .. } if *r == run)),
        "{effects:?}"
    );
    assert_ne!(
        rig.shell.app.engine_runs.get(run).map(|r| r.state.clone()),
        Some(EngineRunState::Running)
    );
}

#[test]
fn a_started_run_updates_the_last_use_shown_in_the_list() {
    let (mut rig, ids) = trio();
    let run = start(&mut rig, ids[0]);
    assert!(!has(&rig, "2023-11-14"));
    send(
        &mut rig,
        EngineMessage::Started {
            run,
            identity: Box::new(identity()),
            at: 1_700_000_000,
        },
    );
    assert!(has(&rig, "Stockfish  [running]"));
    assert!(has(&rig, "2023-11-14"));
}

// ---- limits ----

#[test]
fn saving_a_new_engine_on_a_full_list_says_the_list_is_full_and_adds_nothing() {
    let mut rig = Rig::new();
    for n in 0..256 {
        rig.shell
            .app
            .engines
            .add(EngineDraft::new(format!("E{n}"), "/x"))
            .unwrap();
    }
    send(&mut rig, EngineMessage::OpenDialog);
    send(&mut rig, EngineMessage::New);
    editor(&mut rig, E::SetName("One more".to_owned()));
    editor(&mut rig, E::SetCommand("/y".to_owned()));
    editor(&mut rig, E::Save);
    assert_eq!(rig.shell.app.engines.len(), 256);
    assert_eq!(
        notice(&rig),
        Some(AppNotice::Engine(EngineNotice::ListFull))
    );
}

#[test]
fn copying_an_engine_whose_name_would_pass_the_limit_gives_the_invalid_entry_remark() {
    let (mut rig, ids) = rig_with(&[(&"n".repeat(4096), "/x")]);
    send(&mut rig, EngineMessage::Copy(ids[0]));
    assert_eq!(rig.shell.app.engines.len(), 1);
    assert_eq!(
        notice(&rig),
        Some(AppNotice::Engine(EngineNotice::EntryInvalid))
    );
}

#[test]
fn engine_commands_of_the_dialog_do_nothing_while_the_dialog_is_closed() {
    let mut rig = Rig::new();
    let id = rig
        .shell
        .app
        .engines
        .add(EngineDraft::new("A", "/a"))
        .unwrap();
    send(&mut rig, EngineMessage::Delete(id));
    send(&mut rig, EngineMessage::ConfirmDelete(id));
    assert_eq!(rig.shell.app.engines.len(), 1, "no dialog, no deletion");
}

#[test]
fn a_stale_deletion_of_an_engine_removed_meanwhile_is_harmless() {
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::Delete(ids[1]));
    rig.shell.app.engines.remove(ids[1]).unwrap();
    send(&mut rig, EngineMessage::ConfirmDelete(ids[1]));
    assert_eq!(names(&rig), ["Stockfish", "Crafty"]);
    // The question for an engine that is gone draws nothing and does not panic.
    let _ = &rig.shell.view();
}

#[test]
fn start_in_idle_mode_leaves_the_run_not_analysing() {
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::Start(ids[0], StartMode::Idle));
    assert!(
        !rig.shell
            .app
            .engine_runs
            .of_engine(ids[0])
            .unwrap()
            .analyzing
    );
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::Start(ids[0], StartMode::Analyze));
    assert!(
        rig.shell
            .app
            .engine_runs
            .of_engine(ids[0])
            .unwrap()
            .analyzing
    );
}

#[test]
fn the_dialog_reflects_a_list_replaced_by_a_late_read_of_engines_toml() {
    let (mut rig, _) = trio();
    let loaded = mychess_app::engines::load_engine_list(
        "version = 1\n[[engine]]\nname = \"From disk\"\ncommand = \"/d\"\n",
    )
    .unwrap();
    send(&mut rig, EngineMessage::Loaded(Box::new(Ok(loaded))));
    assert_eq!(names(&rig), ["From disk"]);
    assert!(has(&rig, "From disk"));
    assert!(!has(&rig, "Stockfish"));
}
