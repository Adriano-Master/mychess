//! M8-6b (spec D2307, D2308, D2310): the engine jobs of the exercise, with
//! the fake engine of the project (deterministic: material-greedy, MultiPV
//! announced). The whole exercise is run for real at the end.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::print_stderr
)]

mod m4_3_common;

use std::collections::VecDeque;
use std::time::Duration;

use mychess_app::calvar::{
    Assessment, CalvarConfig, CalvarEffect, CalvarEngine, CalvarMessage, CalvarPhase,
    EngineFailure, LinesJob, RootJob, analyze_lines, analyze_root,
};
use mychess_app::engines::EngineDraft;
use mychess_app::tactics::Cancel;
use mychess_app::{AppState, Effect, Message, perform, update};
use mychess_core::Position;
use mychess_engine::EngineConfig;

/// The binary of the fake engine. Without it a test says so and skips,
/// except in CI (`CI` set) or with `MYCHESS_REQUIRE_FAKE_ENGINE`, where the
/// missing binary is a failure: a silent pass would hide the lack of the
/// check (D2363).
fn fake() -> Option<std::path::PathBuf> {
    let found = m4_3_common::fake_binary();
    if found.is_none() {
        assert!(
            std::env::var_os("CI").is_none(),
            "mychess-fake-engine is not built and this is CI"
        );
        eprintln!(
            "SKIPPED: build it with `cargo build -p mychess-engine --features testing --bin mychess-fake-engine`"
        );
    }
    found
}

fn engine() -> Option<CalvarEngine> {
    fake().map(|binary| CalvarEngine(EngineConfig::new(binary)))
}

const CAPTURE: &str = "4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1";

#[tokio::test]
async fn the_first_stage_gives_the_lines_of_the_engine_best_first() {
    let Some(engine) = engine() else {
        return;
    };
    let position = Position::from_fen(CAPTURE).unwrap();
    let job = RootJob {
        engine,
        position: position.clone(),
        multipv: 3,
        think: Duration::from_millis(300),
        cancel: Cancel::new(),
    };
    let lines = analyze_root(&job).await.expect("lines");
    assert!(!lines.is_empty() && lines.len() <= 3, "{lines:?}");
    assert_eq!(lines[0].multipv, 1);
    assert_eq!(
        position.uci(lines[0].pv[0]),
        "e4d5",
        "the greedy engine takes the pawn"
    );
    assert!(lines.windows(2).all(|w| w[0].multipv < w[1].multipv));
    assert!(lines.iter().all(|l| !l.pv.is_empty()));
}

#[tokio::test]
async fn the_second_stage_searches_after_the_first_move_and_at_the_end() {
    let Some(engine) = engine() else {
        return;
    };
    let start = Position::new();
    let line = |text: &str| mychess_app::calvar::read_moves(&start, text).moves;
    let lines = vec![line("e4"), line("d4 d5 c4")];
    let job = LinesJob {
        engine,
        start,
        lines,
        per_search: Duration::from_millis(300),
        cancel: Cancel::new(),
    };
    assert_eq!(job.searches(), 3);
    let probes = analyze_lines(&job).await.expect("probes");
    assert_eq!(probes.len(), 2);
    assert!(probes[0].first.is_some());
    assert!(
        probes[0].end.is_none(),
        "a one-move line has no second search"
    );
    assert!(probes[1].first.is_some() && probes[1].end.is_some());
}

#[tokio::test]
async fn a_line_that_ends_the_game_needs_no_search() {
    let Some(engine) = engine() else {
        return;
    };
    let start = Position::new();
    let mate = mychess_app::calvar::read_moves(&start, "f3 e5 g4 Qh4#").moves;
    assert_eq!(mate.len(), 4);
    let job = LinesJob {
        engine,
        start,
        lines: vec![mate],
        per_search: Duration::from_millis(300),
        cancel: Cancel::new(),
    };
    let probes = analyze_lines(&job).await.expect("probes");
    let end = probes[0].end.as_ref().expect("the mate");
    assert_eq!(end.depth, 0);
    assert!(end.eval.cp < -30_000, "Black mated White: {:?}", end.eval);
}

#[tokio::test]
async fn a_cancelled_job_ends_at_once_and_says_so() {
    let Some(engine) = engine() else {
        return;
    };
    let cancel = Cancel::new();
    cancel.cancel();
    let job = RootJob {
        engine: engine.clone(),
        position: Position::new(),
        multipv: 3,
        think: Duration::from_secs(60),
        cancel: cancel.clone(),
    };
    let started = std::time::Instant::now();
    assert_eq!(analyze_root(&job).await, Err(EngineFailure::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(30));
    // Cancelled while the engine thinks.
    let cancel = Cancel::new();
    let job = RootJob {
        cancel: cancel.clone(),
        ..job
    };
    let raise = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(800)).await;
        cancel.cancel();
    });
    let started = std::time::Instant::now();
    assert_eq!(analyze_root(&job).await, Err(EngineFailure::Cancelled));
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "did not wait the 60 s"
    );
    raise.await.unwrap();
}

#[tokio::test]
async fn an_engine_that_is_not_there_is_a_failure_not_a_panic() {
    let job = RootJob {
        engine: CalvarEngine(EngineConfig::new("/nonexistent/for/the/test")),
        position: Position::new(),
        multipv: 3,
        think: Duration::from_millis(200),
        cancel: Cancel::new(),
    };
    assert_eq!(analyze_root(&job).await, Err(EngineFailure::NotStarted));
}

#[tokio::test]
async fn the_whole_exercise_runs_against_the_fake_engine() {
    let Some(binary) = fake() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    state
        .engines
        .add(EngineDraft::new("Fake", binary))
        .expect("engine");
    let mut queue: VecDeque<Effect> = VecDeque::new();
    let send = |state: &mut AppState, queue: &mut VecDeque<Effect>, m: CalvarMessage| {
        queue.extend(update(state, Message::Calvar(m)));
    };
    send(
        &mut state,
        &mut queue,
        CalvarMessage::Start {
            config: Box::new(CalvarConfig {
                lines_required: 2,
                multipv: 3,
                root_ms: 300,
                line_ms: 300,
                ..CalvarConfig::default()
            }),
            now: 1,
        },
    );
    // Run the first stage (the engine thinks while the user "calculates").
    let mut ran = 0;
    while let Some(effect) = queue.pop_front() {
        if matches!(effect, Effect::Calvar(CalvarEffect::AnalyzeRoot { .. })) {
            ran += 1;
        }
        let answer = perform(effect).await;
        queue.extend(update(&mut state, answer));
    }
    assert_eq!(ran, 1);
    assert!(state.calvar_view().root_ready);
    for (text, a) in [
        ("e4 e5 Nf3", Assessment::Equal),
        ("d4 d5", Assessment::Equal),
    ] {
        send(
            &mut state,
            &mut queue,
            CalvarMessage::EnterText(text.into()),
        );
        send(&mut state, &mut queue, CalvarMessage::EndLine(a));
    }
    send(&mut state, &mut queue, CalvarMessage::Done);
    while let Some(effect) = queue.pop_front() {
        let answer = perform(effect).await;
        queue.extend(update(&mut state, answer));
    }
    let view = state.calvar_view();
    let CalvarPhase::Reported(score) = view.phase else {
        panic!("{:?} {:?}", view.phase, view.notice);
    };
    let report = view.report.expect("report");
    assert_eq!(report.lines.len(), 2);
    assert!(report.lines.iter().all(|l| l.first.is_some()));
    assert!(report.lines[0].end.is_some());
    assert_eq!(report.score, score);
    assert!(report.good_total >= 1);
    assert_eq!(view.stats.done, 1);
}
