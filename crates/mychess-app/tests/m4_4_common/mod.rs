//! Shared helpers of the `m4_4_*` contract tests (spec
//! `m4-4-janela-de-analise.md`, app side): a run that is ready and
//! analysing, synthetic engine reports and accessors for the results.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use mychess_app::engines::{EngineDraft, EngineId, EngineMessage, RunId, StartMode};
use mychess_app::{
    AnalysisMessage, AnalysisNotice, AnalysisView, AppNotice, AppState, Effect, GameMessage,
    Message, Nav, PlayOptions, update,
};
use mychess_core::{Move, NodeId, Position};
use mychess_engine::{
    BestMove, Bound, Capabilities, EngineCommand, EngineEvent, EngineIdentity, Score, SearchId,
    SearchInfo, SearchRequest,
};

pub fn identity(multipv: bool, search_moves: bool) -> EngineIdentity {
    EngineIdentity {
        name: "Fake".to_owned(),
        author: None,
        options: Vec::new(),
        capabilities: Capabilities {
            multipv: multipv.then_some(1..=5),
            search_moves,
            ..Capabilities::default()
        },
    }
}

/// A state with one UCI engine in the list; no run yet.
pub fn state_with_engine() -> (AppState, EngineId) {
    let mut state = AppState::default();
    let id = state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("add");
    (state, id)
}

/// A run that finished its handshake with `identity`, analysing. Returns
/// the effects of the `Started` answer (the first search).
pub fn ready(identity: EngineIdentity) -> (AppState, RunId, Vec<Effect>) {
    let (mut state, id) = state_with_engine();
    let fx = update(
        &mut state,
        Message::Engine(EngineMessage::Start(id, StartMode::Analyze)),
    );
    let run = fx
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("StartEngine");
    let fx = update(
        &mut state,
        Message::Engine(EngineMessage::Started {
            run,
            identity: Box::new(identity),
            at: 1_700_000_000,
        }),
    );
    (state, run, fx)
}

pub fn analysing(multipv: bool, search_moves: bool) -> (AppState, RunId, Vec<Effect>) {
    ready(identity(multipv, search_moves))
}

pub fn event(state: &mut AppState, run: RunId, event: EngineEvent) -> Vec<Effect> {
    update(
        state,
        Message::Engine(EngineMessage::Event {
            run,
            event: Box::new(event),
        }),
    )
}

pub fn info(state: &mut AppState, run: RunId, info: SearchInfo) -> Vec<Effect> {
    event(state, run, EngineEvent::Info(info))
}

pub fn msg(state: &mut AppState, message: AnalysisMessage) -> Vec<Effect> {
    update(state, Message::Analysis(message))
}

pub fn game(state: &mut AppState, message: GameMessage) -> Vec<Effect> {
    update(state, Message::Game(message))
}

pub fn nav(state: &mut AppState, nav: Nav) -> Vec<Effect> {
    game(state, GameMessage::Navigate(nav))
}

/// Plays `uci` at the cursor (extends or, if it exists, follows).
pub fn play(state: &mut AppState, uci: &str) {
    let mv = state.session.position().parse_uci(uci).expect("uci");
    game(
        state,
        GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        },
    );
}

pub fn play_all(state: &mut AppState, ucis: &[&str]) {
    for u in ucis {
        play(state, u);
    }
}

pub fn started(state: &mut AppState, run: RunId, search: u64) {
    event(state, run, EngineEvent::SearchStarted(SearchId(search)));
}

pub fn commands(effects: &[Effect]) -> Vec<EngineCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand { command, .. } => Some((**command).clone()),
            _ => None,
        })
        .collect()
}

pub fn searches(effects: &[Effect]) -> Vec<SearchRequest> {
    commands(effects)
        .into_iter()
        .filter_map(|c| match c {
            EngineCommand::Search(r) => Some(r),
            _ => None,
        })
        .collect()
}

pub fn count_stops(effects: &[Effect]) -> usize {
    commands(effects)
        .iter()
        .filter(|c| **c == EngineCommand::Stop)
        .count()
}

/// UCI moves played from `position`; panics on an illegal move.
pub fn pv_of(position: &Position, ucis: &[&str]) -> Vec<Move> {
    let mut pos = position.clone();
    ucis.iter()
        .map(|u| {
            let m = pos.parse_uci(u).expect("uci");
            pos.play(m).expect("play");
            m
        })
        .collect()
}

pub fn report(
    position: &Position,
    search: u64,
    multipv: u8,
    depth: u32,
    cp: i32,
    pv: &[&str],
) -> SearchInfo {
    SearchInfo {
        search: SearchId(search),
        multipv,
        depth: Some(depth),
        score: Some((Score::Cp(cp), Bound::Exact)),
        pv: pv_of(position, pv),
        time_ms: Some(1000),
        nodes: Some(1_000_000),
        ..SearchInfo::default()
    }
}

pub fn view(state: &AppState, run: RunId) -> &AnalysisView {
    state.analysis_view(run).expect("view")
}

/// `(multipv, text)` of every line shown.
pub fn lines_of(state: &AppState, run: RunId) -> Vec<(u8, String)> {
    view(state, run)
        .lines
        .iter()
        .map(|l| (l.multipv, l.text.clone()))
        .collect()
}

pub fn analysis_notice(state: &AppState) -> Option<&AnalysisNotice> {
    match state.notice.as_ref() {
        Some(AppNotice::Analysis(n)) => Some(n),
        _ => None,
    }
}

/// Analysing three lines of the start position with search 1 started:
/// 1.e4, 1.d4, 1.Nf3 (depth 12).
pub fn with_three_lines() -> (AppState, RunId, Position) {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    let start = Position::new();
    started(&mut state, run, 1);
    for (n, uci) in [(1u8, "e2e4"), (2, "d2d4"), (3, "g1f3")] {
        info(
            &mut state,
            run,
            report(&start, 1, n, 12, 40 - i32::from(n) * 10, &[uci]),
        );
    }
    (state, run, start)
}

/// The mainline of the game as UCI moves.
pub fn mainline_ucis(state: &AppState) -> Vec<String> {
    let game = state.session.game();
    let mut pos = game.start_position().clone();
    game.mainline()
        .filter(|id| *id != game.root())
        .map(|id| {
            let mv = game.mv(id).expect("mv");
            let u = pos.uci(mv);
            pos.play(mv).expect("legal");
            u
        })
        .collect()
}
