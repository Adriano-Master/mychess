//! M4-3 phase 2 contract tests: executions (`Starting`/`Running`/`Failed`/
//! `Stopping`), F2-F4, main engine, position sent to the engines, quit
//! (spec D433-D436, D721-D723; rules 5, 6, 7, 13). No process is involved:
//! the answers of the effects are fed by hand, or through the in-memory fake.
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
    EngineFailure, EngineMessage, EngineNotice, EngineRunState, FailureKind, Hotkey, MANY_RUNS,
    RunId, StartMode,
};
use mychess_app::{
    Action, AppState, Command, DiscardChoice, Effect, GameMessage, Message, Nav, update,
};
use mychess_engine::testing::ScriptedEngine;
use mychess_engine::{EngineCommand, EngineEvent, ExitInfo, ExitReason};

fn start_run(state: &mut AppState, id: mychess_app::engines::EngineId, mode: StartMode) -> RunId {
    let fx = send(state, EngineMessage::Start(id, mode));
    let runs = starts(&fx);
    assert_eq!(runs.len(), 1, "one StartEngine expected, got {fx:?}");
    runs[0]
}

fn running(state: &mut AppState, id: mychess_app::engines::EngineId) -> RunId {
    let run = start_run(state, id, StartMode::Analyze);
    started(state, run);
    run
}

fn state_of(state: &AppState, run: RunId) -> Option<EngineRunState> {
    state
        .engine_runs()
        .iter()
        .find(|r| r.id == run)
        .map(|r| r.state.clone())
}

fn failure() -> EngineFailure {
    EngineFailure {
        kind: FailureKind::ExitedEarly,
        exit_code: Some(3),
        stderr_tail: vec!["bad".into()],
    }
}

fn exited(reason: ExitReason) -> Box<EngineEvent> {
    Box::new(EngineEvent::Exited(ExitInfo {
        reason,
        status: None,
        stderr_tail: vec!["last words".into()],
    }))
}

// ---- lifecycle ----

#[test]
fn start_creates_a_starting_run_and_started_makes_it_running_and_records_last_used() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    assert_eq!(state_of(&state, run), Some(EngineRunState::Starting));
    assert_eq!(state.main_engine(), None, "only Running engines are main");
    let fx = started(&mut state, run);
    assert_eq!(state_of(&state, run), Some(EngineRunState::Running));
    assert_eq!(state.main_engine(), Some(run));
    assert_eq!(
        state.engines.get(ids[0]).unwrap().last_used,
        Some(1_700_000_000)
    );
    // Analysis is on by default: the position shown is sent once it is ready.
    assert_eq!(search_runs(&fx), [run]);
}

#[test]
fn start_without_analysis_leaves_the_engine_ready_and_idle_until_space_turns_it_on() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Idle);
    let fx = started(&mut state, run);
    assert!(search_runs(&fx).is_empty(), "nostart: no search");
    assert_eq!(state.main_engine(), None, "not analysing: not main");
    // Space turns the analysis on: the next update sends the position.
    let fx = send(&mut state, EngineMessage::ToggleMainAnalysis);
    assert_eq!(search_runs(&fx), [run]);
    assert_eq!(state.main_engine(), Some(run));
    // Space again turns it off and stops the search.
    let fx = send(&mut state, EngineMessage::ToggleMainAnalysis);
    assert!(
        commands(&fx)
            .iter()
            .any(|(r, c)| *r == run && *c == EngineCommand::Stop)
    );
    assert!(search_runs(&fx).is_empty());
    assert_eq!(state.main_engine(), None);
}

#[test]
fn starting_an_engine_that_runs_ends_it_and_a_second_run_of_the_same_engine_never_exists() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = running(&mut state, ids[0]);
    let fx = send(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze));
    assert!(starts(&fx).is_empty());
    assert_eq!(stops(&fx), [run]);
    assert_eq!(state_of(&state, run), Some(EngineRunState::Stopping));
    // A third press while it is stopping does nothing.
    let fx = send(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze));
    assert!(starts(&fx).is_empty() && stops(&fx).is_empty());
    assert_eq!(state.engine_runs().len(), 1);
    send(&mut state, EngineMessage::Stopped(run));
    assert!(state.engine_runs().is_empty());
}

#[test]
fn stopping_a_run_that_is_still_starting_ends_the_process_when_started_arrives_late() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    send(&mut state, EngineMessage::Stop(run));
    assert_eq!(state_of(&state, run), Some(EngineRunState::Stopping));
    let fx = started(&mut state, run);
    assert_eq!(stops(&fx), [run], "the late process must be ended");
    assert_eq!(state_of(&state, run), Some(EngineRunState::Stopping));
    assert!(search_runs(&fx).is_empty());
    assert_eq!(state.main_engine(), None);
}

#[test]
fn started_for_an_unknown_run_ends_that_process() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, _) = state_with(&["A"], &fake);
    let fx = started(&mut state, RunId(4242));
    assert_eq!(stops(&fx), [RunId(4242)]);
    assert!(state.engine_runs().is_empty());
}

#[test]
fn a_second_started_for_a_running_run_is_ignored() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = running(&mut state, ids[0]);
    let fx = started(&mut state, run);
    assert!(stops(&fx).is_empty());
    assert_eq!(state_of(&state, run), Some(EngineRunState::Running));
}

#[test]
fn start_failure_keeps_a_failed_run_with_the_failure_and_warns() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["Broken"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    send(
        &mut state,
        EngineMessage::StartFailed {
            run,
            failure: failure(),
        },
    );
    assert_eq!(
        state_of(&state, run),
        Some(EngineRunState::Failed(failure()))
    );
    assert_eq!(
        notice(&state),
        Some(&EngineNotice::StartFailed {
            name: "Broken".into(),
            failure: failure()
        })
    );
    assert_eq!(state.main_engine(), None);
    // Restart: a new run id (never reused), the failed run is gone.
    let fx = send(&mut state, EngineMessage::Restart(run));
    let new = starts(&fx);
    assert_eq!(new.len(), 1);
    assert!(new[0] > run);
    assert_eq!(run_ids(&state), new);
}

#[test]
fn start_failure_of_a_run_being_stopped_just_drops_it_without_a_warning() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    send(&mut state, EngineMessage::Stop(run));
    send(
        &mut state,
        EngineMessage::StartFailed {
            run,
            failure: failure(),
        },
    );
    assert!(state.engine_runs().is_empty());
    assert!(notice(&state).is_none());
}

#[test]
fn late_start_failure_for_unknown_or_running_runs_changes_nothing() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = running(&mut state, ids[0]);
    send(
        &mut state,
        EngineMessage::StartFailed {
            run,
            failure: failure(),
        },
    );
    assert_eq!(state_of(&state, run), Some(EngineRunState::Running));
    send(
        &mut state,
        EngineMessage::StartFailed {
            run: RunId(999),
            failure: failure(),
        },
    );
    assert_eq!(state.engine_runs().len(), 1);
}

#[test]
fn stopping_a_failed_run_removes_it_without_an_effect_and_start_of_a_failed_engine_retries() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    send(
        &mut state,
        EngineMessage::StartFailed {
            run,
            failure: failure(),
        },
    );
    let fx = send(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze));
    assert_eq!(starts(&fx).len(), 1, "a failed engine is started again");
    assert_eq!(state.engine_runs().len(), 1);
    // Stop on a failed run: no process to end.
    let run2 = starts(&fx)[0];
    send(
        &mut state,
        EngineMessage::StartFailed {
            run: run2,
            failure: failure(),
        },
    );
    let fx = send(&mut state, EngineMessage::Stop(run2));
    assert!(fx.is_empty());
    assert!(state.engine_runs().is_empty());
}

#[test]
fn a_crash_event_marks_the_run_failed_with_exit_data_and_a_clean_quit_drops_it() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B"], &fake);
    let a = running(&mut state, ids[0]);
    let b = running(&mut state, ids[1]);
    send(
        &mut state,
        EngineMessage::Event {
            run: a,
            event: exited(ExitReason::Crashed),
        },
    );
    match state_of(&state, a) {
        Some(EngineRunState::Failed(f)) => {
            assert_eq!(f.kind, FailureKind::Crashed);
            assert_eq!(f.stderr_tail, ["last words"]);
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(matches!(notice(&state), Some(EngineNotice::Crashed { name, .. }) if name == "A"));
    assert_eq!(
        state.main_engine(),
        Some(b),
        "the crashed engine is no longer main"
    );
    send(
        &mut state,
        EngineMessage::Event {
            run: b,
            event: exited(ExitReason::Quit),
        },
    );
    assert_eq!(run_ids(&state), [a]);
    // An exit event of an unknown run is ignored.
    send(
        &mut state,
        EngineMessage::Event {
            run: RunId(777),
            event: exited(ExitReason::Crashed),
        },
    );
    assert_eq!(state.engine_runs().len(), 1);
}

#[test]
fn an_exit_event_of_a_run_being_stopped_does_not_warn() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = running(&mut state, ids[0]);
    send(&mut state, EngineMessage::Stop(run));
    send(
        &mut state,
        EngineMessage::Event {
            run,
            event: exited(ExitReason::Crashed),
        },
    );
    assert!(notice(&state).is_none());
    assert_eq!(state_of(&state, run), Some(EngineRunState::Stopping));
}

#[test]
fn the_refusal_of_a_command_warns_but_a_busy_queue_does_not() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = running(&mut state, ids[0]);
    send(
        &mut state,
        EngineMessage::CommandDone {
            run,
            result: Err(mychess_app::engines::CommandError::Busy),
        },
    );
    assert!(notice(&state).is_none());
    send(
        &mut state,
        EngineMessage::CommandDone {
            run,
            result: Err(mychess_app::engines::CommandError::Refused),
        },
    );
    assert_eq!(notice(&state), Some(&EngineNotice::CommandRefused));
}

// ---- many runs, main engine ----

#[test]
fn from_four_runs_on_the_interface_warns_about_the_load() {
    let fake = ScriptedEngine::builder().build();
    let names = ["A", "B", "C", "D", "E"];
    let (mut state, ids) = state_with(&names, &fake);
    assert_eq!(MANY_RUNS, 4);
    for (i, id) in ids.iter().enumerate() {
        state.notice = None;
        running(&mut state, *id);
        let n = i + 1;
        if n >= MANY_RUNS {
            assert_eq!(
                notice(&state),
                Some(&EngineNotice::ManyRunning { count: n }),
                "with {n} runs"
            );
        } else {
            assert!(notice(&state).is_none(), "with {n} runs");
        }
    }
    assert_eq!(state.engine_runs().len(), 5, "there is no hard limit");
}

#[test]
fn main_engine_follows_the_order_of_the_list_not_the_order_of_starting() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B", "C"], &fake);
    let c = running(&mut state, ids[2]);
    assert_eq!(state.main_engine(), Some(c));
    let a = running(&mut state, ids[0]);
    let b = running(&mut state, ids[1]);
    assert_eq!(state.main_engine(), Some(a));
    // Reordering recalculates it: C above A.
    send(&mut state, EngineMessage::MoveUp(ids[2]));
    send(&mut state, EngineMessage::MoveUp(ids[2]));
    assert_eq!(state.engines.engines()[0].id, ids[2]);
    assert_eq!(state.main_engine(), Some(c));
    // A run that is not analysing is skipped; one that starts is skipped.
    send(&mut state, EngineMessage::Stop(c));
    assert_eq!(state.main_engine(), Some(a));
    send(&mut state, EngineMessage::Stop(a));
    assert_eq!(state.main_engine(), Some(b));
}

#[test]
fn main_engine_ignores_a_starting_failed_or_idle_run_even_if_higher_in_the_list() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B", "C", "D"], &fake);
    let _starting = start_run(&mut state, ids[0], StartMode::Analyze);
    let failed = start_run(&mut state, ids[1], StartMode::Analyze);
    send(
        &mut state,
        EngineMessage::StartFailed {
            run: failed,
            failure: failure(),
        },
    );
    let idle = start_run(&mut state, ids[2], StartMode::Idle);
    started(&mut state, idle);
    assert_eq!(state.main_engine(), None);
    let d = running(&mut state, ids[3]);
    assert_eq!(state.main_engine(), Some(d));
}

#[test]
fn space_without_a_running_engine_is_a_no_op_with_a_warning() {
    let mut state = AppState::default();
    let fx = send(&mut state, EngineMessage::ToggleMainAnalysis);
    assert!(fx.is_empty());
    assert_eq!(notice(&state), Some(&EngineNotice::NoEngineRunning));
    // A run that is only Starting does not count as running.
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    state.notice = None;
    start_run(&mut state, ids[0], StartMode::Analyze);
    send(&mut state, EngineMessage::ToggleMainAnalysis);
    assert_eq!(notice(&state), Some(&EngineNotice::NoEngineRunning));
}

#[test]
fn space_toggles_the_first_running_engine_in_list_order() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B"], &fake);
    let b = running(&mut state, ids[1]);
    let a = running(&mut state, ids[0]);
    let fx = send(&mut state, EngineMessage::ToggleMainAnalysis);
    assert!(
        commands(&fx)
            .iter()
            .any(|(r, c)| *r == a && *c == EngineCommand::Stop)
    );
    assert!(!commands(&fx).iter().any(|(r, _)| *r == b));
    let a_run = state.engine_runs().iter().find(|r| r.id == a).unwrap();
    assert!(!a_run.analyzing);
}

// ---- hotkeys ----

#[test]
fn f2_f3_f4_start_the_engine_of_the_key_toggle_it_and_warn_when_the_key_is_empty() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B"], &fake);
    send(
        &mut state,
        EngineMessage::SetHotkey(ids[0], Some(Hotkey::F2)),
    );
    send(
        &mut state,
        EngineMessage::SetHotkey(ids[1], Some(Hotkey::F4)),
    );
    let fx = update(&mut state, Message::Action(Action::StartEngine(1)));
    let a = starts(&fx);
    assert_eq!(a.len(), 1);
    assert_eq!(state.engine_runs()[0].engine, ids[0]);
    // F3 has no engine.
    state.notice = None;
    let fx = update(&mut state, Message::Action(Action::StartEngine(2)));
    assert!(starts(&fx).is_empty());
    assert_eq!(
        notice(&state),
        Some(&EngineNotice::NoEngineOnKey(Hotkey::F3))
    );
    // F4 starts the other; F2 again ends the first.
    update(&mut state, Message::Action(Action::StartEngine(3)));
    assert_eq!(state.engine_runs()[1].engine, ids[1]);
    started(&mut state, a[0]);
    let fx = update(&mut state, Message::Action(Action::StartEngine(1)));
    assert_eq!(stops(&fx), a);
}

#[test]
fn engine_key_numbers_out_of_range_do_nothing() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    send(
        &mut state,
        EngineMessage::SetHotkey(ids[0], Some(Hotkey::F2)),
    );
    for n in [0u8, 4, 255] {
        let fx = update(&mut state, Message::Action(Action::StartEngine(n)));
        assert!(fx.is_empty(), "key number {n}");
    }
    assert!(state.engine_runs().is_empty());
}

#[test]
fn hotkeys_hold_ids_so_deleting_another_engine_does_not_change_which_one_f3_starts() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B", "C"], &fake);
    for (id, key) in ids.iter().zip(Hotkey::ALL) {
        send(&mut state, EngineMessage::SetHotkey(*id, Some(key)));
    }
    send(&mut state, EngineMessage::OpenDialog);
    send(&mut state, EngineMessage::Delete(ids[0]));
    send(&mut state, EngineMessage::ConfirmDelete(ids[0]));
    send(&mut state, EngineMessage::MoveUp(ids[2]));
    update(&mut state, Message::Action(Action::StartEngine(2)));
    assert_eq!(state.engine_runs()[0].engine, ids[1]);
}

// ---- delete / edit while running ----

#[test]
fn deleting_a_running_engine_ends_its_run_first() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B"], &fake);
    let run = running(&mut state, ids[0]);
    send(&mut state, EngineMessage::OpenDialog);
    send(&mut state, EngineMessage::Delete(ids[0]));
    let fx = send(&mut state, EngineMessage::ConfirmDelete(ids[0]));
    assert_eq!(stops(&fx), [run]);
    assert!(state.engines.get(ids[0]).is_none());
    assert_eq!(state_of(&state, run), Some(EngineRunState::Stopping));
    assert_eq!(state.main_engine(), None);
}

#[test]
fn deleting_an_engine_while_its_start_is_in_flight_does_not_leave_an_orphan() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    send(&mut state, EngineMessage::OpenDialog);
    send(&mut state, EngineMessage::Delete(ids[0]));
    send(&mut state, EngineMessage::ConfirmDelete(ids[0]));
    let fx = started(&mut state, run);
    assert_eq!(stops(&fx), [run]);
    assert!(search_runs(&fx).is_empty());
}

#[test]
fn editing_the_command_of_a_running_engine_says_to_restart_and_keeps_the_run() {
    use mychess_app::engines::EditorMessage;
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = running(&mut state, ids[0]);
    send(&mut state, EngineMessage::Edit(ids[0]));
    send(
        &mut state,
        EngineMessage::Editor(EditorMessage::SetArgs("--new".into())),
    );
    send(&mut state, EngineMessage::Editor(EditorMessage::Save));
    assert_eq!(notice(&state), Some(&EngineNotice::RestartToApply));
    assert_eq!(state_of(&state, run), Some(EngineRunState::Running));
    // Changing only the Elo does not ask for a restart.
    state.notice = None;
    send(&mut state, EngineMessage::Edit(ids[0]));
    send(
        &mut state,
        EngineMessage::Editor(EditorMessage::SetElo("2000".into())),
    );
    send(&mut state, EngineMessage::Editor(EditorMessage::Save));
    assert!(notice(&state).is_none());
}

#[test]
fn saved_options_reach_a_running_engine_at_once_and_multipv_is_sent_at_each_start() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    // No option command before Running.
    let fx = send(
        &mut state,
        EngineMessage::SetOption {
            engine: ids[0],
            name: "Hash".into(),
            value: "32".into(),
        },
    );
    assert!(commands(&fx).is_empty());
    state
        .engines
        .edit(ids[0], {
            let mut d = state.engines.get(ids[0]).unwrap().to_draft();
            d.multipv = 3;
            d
        })
        .unwrap();
    let fx = started(&mut state, run);
    assert!(
        commands(&fx)
            .iter()
            .any(|(r, c)| *r == run && *c == EngineCommand::SetMultiPv(3))
    );
    let fx = send(
        &mut state,
        EngineMessage::SetOption {
            engine: ids[0],
            name: "Hash".into(),
            value: "64".into(),
        },
    );
    let setopt = commands(&fx)
        .into_iter()
        .filter(|(_, c)| matches!(c, EngineCommand::SetOption { .. }))
        .count();
    assert_eq!(setopt, 1);
    // The value was replaced, not duplicated.
    assert_eq!(
        state.engines.get(ids[0]).unwrap().options,
        [("Hash".to_owned(), "64".to_owned())]
    );
    // An unknown engine is a notice, not a panic.
    send(
        &mut state,
        EngineMessage::SetOption {
            engine: mychess_app::engines::EngineId(404),
            name: "Hash".into(),
            value: "1".into(),
        },
    );
    assert_eq!(notice(&state), Some(&EngineNotice::NotFound));
}

// ---- position sent (D436, D722) ----

#[test]
fn every_active_run_gets_the_same_position_when_a_move_is_played() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B", "C"], &fake);
    let a = running(&mut state, ids[0]);
    let b = running(&mut state, ids[1]);
    let _idle = {
        let r = start_run(&mut state, ids[2], StartMode::Idle);
        started(&mut state, r);
        r
    };
    let fx = play(&mut state, "e2e4");
    let s = searches(&fx);
    assert_eq!(s.len(), 2, "idle run gets none: {s:?}");
    for (run, _, moves) in &s {
        assert!([a, b].contains(run));
        assert_eq!(moves, &["e2e4"]);
    }
}

#[test]
fn an_unchanged_position_is_not_sent_again() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    running(&mut state, ids[0]);
    play(&mut state, "e2e4");
    // Messages that do not change the position: nothing is sent.
    for m in [
        Message::Game(GameMessage::Navigate(Nav::End)),
        Message::Game(GameMessage::Navigate(Nav::Forward(1))),
        Message::Engine(EngineMessage::DismissReports),
        Message::Engine(EngineMessage::OpenDialog),
    ] {
        let fx = update(&mut state, m);
        assert!(searches(&fx).is_empty(), "{fx:?}");
    }
    // Going back and forth does send.
    let fx = game(&mut state, GameMessage::Navigate(Nav::Back(1)));
    assert_eq!(searches(&fx)[0].2, Vec::<String>::new());
    let fx = game(&mut state, GameMessage::Navigate(Nav::Forward(1)));
    assert_eq!(searches(&fx)[0].2, ["e2e4"]);
}

#[test]
fn the_path_to_the_cursor_includes_the_variation_moves() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    running(&mut state, ids[0]);
    play(&mut state, "e2e4");
    play(&mut state, "e7e5");
    play(&mut state, "g1f3");
    game(&mut state, GameMessage::Navigate(Nav::Back(2)));
    // After 1.e4, a variation 1...c5.
    let mv = state.session.position().parse_uci("c7c5").unwrap();
    let fx = update(
        &mut state,
        Message::Game(GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions {
                force_variation: true,
            },
        }),
    );
    let s = searches(&fx);
    assert_eq!(s.len(), 1, "{fx:?}");
    assert_eq!(s[0].2, ["e2e4", "c7c5"]);
    // Deeper into the variation.
    let fx = play(&mut state, "g1f3");
    assert_eq!(searches(&fx)[0].2, ["e2e4", "c7c5", "g1f3"]);
    // Leaving the variation returns to the position it starts from.
    let fx = game(&mut state, GameMessage::Navigate(Nav::ExitVariation));
    assert_eq!(searches(&fx)[0].2, ["e2e4"]);
}

#[test]
fn trial_mode_moves_are_analysed_and_leaving_trial_returns_to_the_real_position() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    running(&mut state, ids[0]);
    play(&mut state, "e2e4");
    let fx = game(&mut state, GameMessage::EnterTrial { null_move: false });
    assert!(searches(&fx).len() <= 1);
    let fx = play(&mut state, "e7e5");
    assert_eq!(searches(&fx)[0].2, ["e2e4", "e7e5"]);
    let fx = game(&mut state, GameMessage::ExitTrial);
    assert_eq!(
        searches(&fx).last().map(|s| s.2.clone()),
        Some(vec!["e2e4".to_owned()])
    );
}

#[test]
fn a_null_move_in_trial_mode_does_not_panic_the_target() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    running(&mut state, ids[0]);
    play(&mut state, "e2e4");
    game(&mut state, GameMessage::EnterTrial { null_move: true });
    game(&mut state, GameMessage::ExitTrial);
    assert_eq!(state.engine_runs().len(), 1);
}

#[test]
fn undo_sends_the_previous_position() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    running(&mut state, ids[0]);
    play(&mut state, "d2d4");
    play(&mut state, "d7d5");
    let fx = game(&mut state, GameMessage::Undo);
    assert_eq!(searches(&fx)[0].2, ["d2d4"]);
}

#[test]
fn a_new_game_sends_new_game_before_the_search_and_a_search_even_if_the_position_is_equal() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = running(&mut state, ids[0]);
    play(&mut state, "e2e4");
    let fx = update(&mut state, Message::Command(Command::NewGame));
    assert!(fx.is_empty() || searches(&fx).is_empty(), "asks first");
    let fx = update(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave));
    let cmds = commands(&fx);
    let new_game = cmds
        .iter()
        .position(|(r, c)| *r == run && *c == EngineCommand::NewGame);
    let search = cmds
        .iter()
        .position(|(_, c)| matches!(c, EngineCommand::Search(_)));
    assert!(new_game.is_some(), "NewGame expected: {cmds:?}");
    assert!(search.is_some(), "Search expected: {cmds:?}");
    assert!(new_game < search);
    assert_eq!(searches(&fx)[0].2, Vec::<String>::new());
    // Navigation does not send NewGame.
    let fx = play(&mut state, "d2d4");
    assert!(!has_new_game(&fx, run));
}

#[test]
fn starting_an_engine_in_the_middle_of_a_game_sends_the_current_position_only() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    play(&mut state, "e2e4");
    play(&mut state, "c7c5");
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    let fx = started(&mut state, run);
    let s = searches(&fx);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].2, ["e2e4", "c7c5"]);
    assert!(!has_new_game(&fx, run), "no NewGame for the first position");
}

#[test]
fn a_run_that_becomes_active_later_gets_the_position_exactly_once() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A", "B"], &fake);
    let a = running(&mut state, ids[0]);
    play(&mut state, "e2e4");
    let b = running(&mut state, ids[1]);
    // B got the current position at its Started; A is not resent.
    let fx = game(&mut state, GameMessage::Navigate(Nav::Back(1)));
    let mut got = search_runs(&fx);
    got.sort();
    assert_eq!(got, [a, b]);
}

#[test]
fn hostile_messages_with_unknown_ids_do_not_panic() {
    let mut state = AppState::default();
    let ghost = mychess_app::engines::EngineId(31337);
    for m in [
        EngineMessage::Start(ghost, StartMode::Analyze),
        EngineMessage::Stop(RunId(1)),
        EngineMessage::Restart(RunId(1)),
        EngineMessage::Stopped(RunId(1)),
        EngineMessage::Copy(ghost),
        EngineMessage::MoveUp(ghost),
        EngineMessage::MoveDown(ghost),
        EngineMessage::SetHotkey(ghost, Some(Hotkey::F2)),
        EngineMessage::ConfirmDelete(ghost),
        EngineMessage::ToggleHotkey(Hotkey::F4),
        EngineMessage::CommandDone {
            run: RunId(5),
            result: Err(mychess_app::engines::CommandError::NotRunning),
        },
        EngineMessage::ShutdownDone,
        EngineMessage::Saved(Ok(())),
    ] {
        update(&mut state, Message::Engine(m));
    }
    assert!(state.engine_runs().is_empty());
}

// ---- quit (D723) ----

#[test]
fn quit_without_runs_does_not_touch_the_engines() {
    let mut state = AppState::with_preferences_path(Some(PathBuf::from("p.toml")));
    let fx = update(&mut state, Message::Quit);
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::ShutdownEngines { .. }))
    );
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::SavePreferences { .. }))
    );
}

fn quit_state(
    names: &[&str],
    fake: &ScriptedEngine,
) -> (AppState, Vec<mychess_app::engines::EngineId>) {
    let mut state = AppState::with_preferences_path(Some(PathBuf::from("cfg").join("p.toml")));
    state.engine_hub = hub_with(fake);
    let ids = names
        .iter()
        .map(|n| {
            state
                .engines
                .add(mychess_app::engines::EngineDraft::new(*n, *n))
                .unwrap()
        })
        .collect();
    (state, ids)
}

fn shutdowns(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::ShutdownEngines { .. }))
        .count()
}

#[test]
fn quit_ends_every_engine_before_saving_preferences_and_exiting() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = quit_state(&["A", "B"], &fake);
    running(&mut state, ids[0]);
    running(&mut state, ids[1]);
    // Let the saves triggered by the starts finish.
    while !state.engine_runs().is_empty() && saves_pending(&mut state) {}
    let fx = update(&mut state, Message::Quit);
    assert_eq!(shutdowns(&fx), 1);
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::SavePreferences { .. } | Effect::Exit))
    );
    // A second quit while waiting does not ask again nor exit.
    let fx = update(&mut state, Message::Quit);
    assert_eq!(shutdowns(&fx), 0);
    assert!(!fx.iter().any(|e| matches!(e, Effect::Exit)));
    let fx = send(&mut state, EngineMessage::ShutdownDone);
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::SavePreferences { .. }))
    );
    assert!(state.engine_runs().is_empty());
    let fx = update(&mut state, Message::PreferencesSaved(Ok(())));
    assert_eq!(fx, [Effect::Exit]);
}

fn saves_pending(state: &mut AppState) -> bool {
    // Answers the save of `engines.toml` that a start asked for, if any.
    let fx = send(state, EngineMessage::Saved(Ok(())));
    !saves(&fx).is_empty()
}

#[test]
fn quit_waits_for_the_engines_toml_save_in_flight_and_then_shuts_the_engines_down() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = quit_state(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    // `Started` asks to save `last_used`: the save is in flight.
    let fx = started(&mut state, run);
    assert_eq!(saves(&fx).len(), 1);
    let fx = update(&mut state, Message::Quit);
    assert!(fx.is_empty(), "waits for the save: {fx:?}");
    let fx = send(&mut state, EngineMessage::Saved(Ok(())));
    assert_eq!(shutdowns(&fx), 1);
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::SavePreferences { .. } | Effect::Exit))
    );
}

#[test]
fn quit_with_a_resave_pending_saves_the_newest_list_before_shutting_down() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = quit_state(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    started(&mut state, run);
    // A change while the first save is in flight.
    send(
        &mut state,
        EngineMessage::SetHotkey(ids[0], Some(Hotkey::F3)),
    );
    let fx = update(&mut state, Message::Quit);
    assert!(fx.is_empty());
    let fx = send(&mut state, EngineMessage::Saved(Ok(())));
    assert_eq!(saves(&fx).len(), 1, "the newest list is written first");
    assert_eq!(shutdowns(&fx), 0);
    let fx = send(&mut state, EngineMessage::Saved(Ok(())));
    assert_eq!(shutdowns(&fx), 1);
}

#[test]
fn quit_with_a_failed_save_still_proceeds() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = quit_state(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    started(&mut state, run);
    update(&mut state, Message::Quit);
    let fx = send(
        &mut state,
        EngineMessage::Saved(Err(mychess_app::engines::StoreError::Syntax)),
    );
    assert_eq!(shutdowns(&fx), 1);
}

#[test]
fn quit_with_only_failed_runs_does_not_shut_anything_down() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = quit_state(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    send(
        &mut state,
        EngineMessage::StartFailed {
            run,
            failure: failure(),
        },
    );
    let fx = update(&mut state, Message::Quit);
    assert_eq!(shutdowns(&fx), 0);
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::SavePreferences { .. }))
    );
}

#[test]
fn quit_while_an_engine_is_still_starting_shuts_down_and_a_late_started_is_ended() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = quit_state(&["A"], &fake);
    let run = start_run(&mut state, ids[0], StartMode::Analyze);
    let fx = update(&mut state, Message::Quit);
    assert_eq!(shutdowns(&fx), 1);
    send(&mut state, EngineMessage::ShutdownDone);
    let fx = started(&mut state, run);
    assert_eq!(stops(&fx), [run], "no orphan process after the quit");
}

#[test]
fn quit_asks_about_unsaved_games_before_touching_the_engines() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = quit_state(&["A"], &fake);
    running(&mut state, ids[0]);
    play(&mut state, "e2e4");
    let fx = update(&mut state, Message::Quit);
    assert_eq!(shutdowns(&fx), 0);
    assert!(state.discard_prompt.is_some());
}
