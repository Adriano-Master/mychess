//! Shared helpers of the `m4_5_*` contract tests (spec
//! `m4-5-barra-setas-semaforo.md`, app side): runs that are ready and
//! analysing, synthetic engine reports and state shortcuts.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use mychess_app::engines::{EngineDraft, EngineId, EngineMessage, RunId, StartMode};
use mychess_app::{
    Action, AnalysisMessage, AppState, ArrowMode, ArrowPrefs, Effect, EvalBarMessage, EvalBarPrefs,
    GameMessage, Message, Nav, PlayOptions, PrefChange, PrefsMessage, TrafficLightPrefs, update,
};
use mychess_core::{Move, Position};
use mychess_engine::{
    Bound, Capabilities, EngineCommand, EngineEvent, EngineIdentity, Score, SearchId, SearchInfo,
};

pub fn identity(name: &str) -> EngineIdentity {
    EngineIdentity {
        name: name.to_owned(),
        author: None,
        options: Vec::new(),
        capabilities: Capabilities {
            multipv: Some(1..=5),
            search_moves: true,
            ..Capabilities::default()
        },
    }
}

/// Starts the engine `id` in analysis mode and finishes its handshake.
pub fn start_run(state: &mut AppState, id: EngineId, name: &str) -> RunId {
    let fx = update(
        state,
        Message::Engine(EngineMessage::Start(id, StartMode::Analyze)),
    );
    let run = fx
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("StartEngine");
    update(
        state,
        Message::Engine(EngineMessage::Started {
            run,
            identity: Box::new(identity(name)),
            at: 1_700_000_000,
        }),
    );
    run
}

/// One engine running and analysing; no search reported yet.
pub fn analysing() -> (AppState, RunId) {
    let mut state = AppState::default();
    let id = state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("add");
    let run = start_run(&mut state, id, "Fake");
    (state, run)
}

/// Two engines running, in list order: `(state, first, second)`.
pub fn two_runs() -> (AppState, RunId, RunId) {
    let mut state = AppState::default();
    let a = state
        .engines
        .add(EngineDraft::new("A", "/a"))
        .expect("add a");
    let b = state
        .engines
        .add(EngineDraft::new("B", "/b"))
        .expect("add b");
    let ra = start_run(&mut state, a, "A");
    let rb = start_run(&mut state, b, "B");
    (state, ra, rb)
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

pub fn started(state: &mut AppState, run: RunId, search: u64) {
    event(state, run, EngineEvent::SearchStarted(SearchId(search)));
}

pub fn info(state: &mut AppState, run: RunId, info: SearchInfo) {
    event(state, run, EngineEvent::Info(info));
}

pub fn msg(state: &mut AppState, m: impl Into<Message>) -> Vec<Effect> {
    update(state, m.into())
}

pub fn bar(state: &mut AppState, m: EvalBarMessage) {
    update(state, Message::EvalBar(m));
}

pub fn pref(state: &mut AppState, change: PrefChange) {
    update(state, Message::Prefs(PrefsMessage::Set(change)));
}

pub fn game(state: &mut AppState, m: GameMessage) {
    update(state, Message::Game(m));
}

pub fn nav(state: &mut AppState, n: Nav) {
    game(state, GameMessage::Navigate(n));
}

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

/// Sets the comment after the move at the cursor.
pub fn comment(state: &mut AppState, text: &str) {
    let node = state.session.cursor();
    game(
        state,
        GameMessage::SetComment {
            node,
            text: text.to_owned(),
        },
    );
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

pub fn report_score(
    position: &Position,
    search: u64,
    multipv: u8,
    depth: u32,
    score: Score,
    bound: Bound,
    pv: &[&str],
) -> SearchInfo {
    SearchInfo {
        search: SearchId(search),
        multipv,
        depth: Some(depth),
        score: Some((score, bound)),
        pv: pv_of(position, pv),
        time_ms: Some(1000),
        nodes: Some(1_000_000),
        ..SearchInfo::default()
    }
}

/// A report in centipawns, White's point of view, exact.
pub fn report(
    position: &Position,
    search: u64,
    multipv: u8,
    depth: u32,
    cp: i32,
    pv: &[&str],
) -> SearchInfo {
    report_score(
        position,
        search,
        multipv,
        depth,
        Score::Cp(cp),
        Bound::Exact,
        pv,
    )
}

/// The position at the cursor.
pub fn here(state: &AppState) -> Position {
    state.session.position().clone()
}

/// Reports one line of the cursor position for search `search`.
pub fn line(state: &mut AppState, run: RunId, search: u64, n: u8, cp: i32, pv: &[&str]) {
    let pos = here(state);
    info(state, run, report(&pos, search, n, 12, cp, pv));
}

/// A state with the bar, the arrows and the lights wanted, the engine
/// analysing with `lines` MultiPV lines of the start position (search 1).
pub fn with_lines(multipv: u8, firsts: &[(i32, &str)]) -> (AppState, RunId) {
    let (mut state, run) = analysing();
    msg(&mut state, AnalysisMessage::SetMultiPv(run, multipv));
    started(&mut state, run, 1);
    for (i, (cp, uci)) in firsts.iter().enumerate() {
        let n = u8::try_from(i + 1).expect("small");
        line(&mut state, run, 1, n, *cp, &[uci]);
    }
    (state, run)
}

pub fn enable_all(state: &mut AppState) {
    pref(
        state,
        PrefChange::EvalBar(EvalBarPrefs {
            show: true,
            ..EvalBarPrefs::default()
        }),
    );
    pref(
        state,
        PrefChange::Arrows(ArrowPrefs {
            enabled: true,
            ..ArrowPrefs::default()
        }),
    );
    pref(
        state,
        PrefChange::TrafficLights(TrafficLightPrefs { show: true }),
    );
}

pub fn eval_prefs(state: &AppState) -> EvalBarPrefs {
    state.prefs.engine_view.eval_bar
}

pub fn arrow_prefs(state: &AppState) -> ArrowPrefs {
    state.prefs.engine_view.arrows
}

pub fn set_eval(state: &mut AppState, f: impl FnOnce(&mut EvalBarPrefs)) {
    let mut p = eval_prefs(state);
    f(&mut p);
    pref(state, PrefChange::EvalBar(p));
}

pub fn set_arrows(state: &mut AppState, f: impl FnOnce(&mut ArrowPrefs)) {
    let mut p = arrow_prefs(state);
    f(&mut p);
    pref(state, PrefChange::Arrows(p));
}

/// `"e2e4"`-style text of each arrow.
pub fn arrow_texts(state: &AppState) -> Vec<String> {
    state
        .engine_arrows()
        .iter()
        .map(|a| format!("{}{}", a.from, a.to))
        .collect()
}

/// Reports line `n` whose PV is the first legal move of the cursor position.
pub fn line_any(state: &mut AppState, run: RunId, search: u64, n: u8, cp: i32) {
    let pos = here(state);
    let mv = pos.legal_moves().first().copied();
    let mut r = report(&pos, search, n, 12, cp, &[]);
    r.pv = mv.into_iter().collect();
    info(state, run, r);
}
