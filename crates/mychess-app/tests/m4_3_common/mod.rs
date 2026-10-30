//! Shared helpers of the `m4_3_*` contract tests (spec
//! `m4-3-gerencia-de-engines.md`, phase 2: the app side).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mychess_app::engines::{
    EngineDraft, EngineHub, EngineId, EngineMessage, EngineNotice, RunId, StartMode,
};
use mychess_app::{
    Action, AppNotice, AppState, Effect, GameMessage, Message, PlayOptions, perform, update,
};
use mychess_engine::testing::ScriptedEngine;
use mychess_engine::{
    Capabilities, EngineCommand, EngineConfig, EngineError, EngineIdentity, SearchPosition,
};

/// A hub whose engines are the in-memory scripted fake.
pub fn hub_with(fake: &ScriptedEngine) -> EngineHub {
    let fake = fake.clone();
    EngineHub::with_launcher(Arc::new(move |config: EngineConfig| {
        let fake = fake.clone();
        Box::pin(async move { fake.start(config).await })
    }))
}

/// A hub whose launcher always fails with "exited early" and a stderr line.
pub fn failing_hub() -> EngineHub {
    EngineHub::with_launcher(Arc::new(|_config: EngineConfig| {
        Box::pin(async move {
            Err(EngineError::ExitedEarly {
                status: None,
                stderr_tail: vec!["boom".to_owned()],
            })
        })
    }))
}

/// A state with `names` in the list (no keys) and engines that start `fake`.
pub fn state_with(names: &[&str], fake: &ScriptedEngine) -> (AppState, Vec<EngineId>) {
    let mut state = AppState::default();
    state.engine_hub = hub_with(fake);
    let ids = names
        .iter()
        .map(|n| {
            state
                .engines
                .add(EngineDraft::new(*n, format!("fake-{n}")))
                .expect("add")
        })
        .collect();
    (state, ids)
}

pub fn send(state: &mut AppState, message: EngineMessage) -> Vec<Effect> {
    update(state, Message::Engine(message))
}

/// Runs the non-host effects and feeds their answers back; returns every
/// effect that was run, in order.
pub async fn settle(state: &mut AppState, effects: Vec<Effect>) -> Vec<Effect> {
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

pub async fn drive(state: &mut AppState, message: EngineMessage) -> Vec<Effect> {
    let effects = send(state, message);
    settle(state, effects).await
}

pub async fn drive_msg(state: &mut AppState, message: Message) -> Vec<Effect> {
    let effects = update(state, message);
    settle(state, effects).await
}

pub fn play(state: &mut AppState, uci: &str) -> Vec<Effect> {
    let mv = state.session.position().parse_uci(uci).expect("uci");
    update(
        state,
        Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    )
}

pub fn game(state: &mut AppState, message: GameMessage) -> Vec<Effect> {
    update(state, Message::Game(message))
}

pub fn commands(effects: &[Effect]) -> Vec<(RunId, EngineCommand)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand { run, command, .. } => Some((*run, (**command).clone())),
            _ => None,
        })
        .collect()
}

/// The runs that got a `Search`, with the position as `(start FEN, UCI moves)`.
pub fn searches(effects: &[Effect]) -> Vec<(RunId, String, Vec<String>)> {
    commands(effects)
        .into_iter()
        .filter_map(|(run, c)| match c {
            EngineCommand::Search(request) => {
                let (fen, moves) = describe(&request.position);
                Some((run, fen, moves))
            }
            _ => None,
        })
        .collect()
}

pub fn describe(position: &SearchPosition) -> (String, Vec<String>) {
    let mut at = position.start.clone();
    let mut moves = Vec::new();
    for mv in &position.moves {
        moves.push(at.uci(*mv));
        at.play(*mv).expect("legal move");
    }
    (position.start.fen(), moves)
}

pub fn search_runs(effects: &[Effect]) -> Vec<RunId> {
    searches(effects).into_iter().map(|(r, _, _)| r).collect()
}

pub fn has_new_game(effects: &[Effect], run: RunId) -> bool {
    commands(effects)
        .iter()
        .any(|(r, c)| *r == run && *c == EngineCommand::NewGame)
}

pub fn saves(effects: &[Effect]) -> Vec<&Effect> {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::SaveEngines { .. }))
        .collect()
}

pub fn stops(effects: &[Effect]) -> Vec<RunId> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::StopEngine { run, .. } => Some(*run),
            _ => None,
        })
        .collect()
}

pub fn starts(effects: &[Effect]) -> Vec<RunId> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .collect()
}

pub fn notice(state: &AppState) -> Option<&EngineNotice> {
    match state.notice.as_ref() {
        Some(AppNotice::Engine(n)) => Some(n),
        _ => None,
    }
}

pub fn identity(name: &str) -> EngineIdentity {
    EngineIdentity {
        name: name.to_owned(),
        author: None,
        options: Vec::new(),
        capabilities: Capabilities::default(),
    }
}

/// Answers a `StartEngine` by hand, as if the process had finished its
/// handshake (no process involved).
pub fn started(state: &mut AppState, run: RunId) -> Vec<Effect> {
    send(
        state,
        EngineMessage::Started {
            run,
            identity: Box::new(identity("Fake")),
            at: 1_700_000_000,
        },
    )
}

pub fn run_ids(state: &AppState) -> Vec<RunId> {
    state.engine_runs().iter().map(|r| r.id).collect()
}

/// The `mychess-fake-engine` binary built by
/// `cargo build -p mychess-engine --features testing --bin mychess-fake-engine`
/// (next to the test executable's `deps` directory), if present. With
/// `MYCHESS_REQUIRE_FAKE_ENGINE` set (`cargo xtask test` does) a missing
/// binary fails the test instead of skipping it.
pub fn fake_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = format!("mychess-fake-engine{}", std::env::consts::EXE_SUFFIX);
    let path = exe.parent()?.parent()?.join(name);
    let found = path.is_file().then_some(path);
    assert!(
        found.is_some() || std::env::var_os("MYCHESS_REQUIRE_FAKE_ENGINE").is_none(),
        "mychess-fake-engine is not built (cargo build -p mychess-engine --features testing --bin mychess-fake-engine)"
    );
    found
}

pub fn temp_prefs_path(dir: &Path) -> PathBuf {
    dir.join("cfg").join("preferences.toml")
}
