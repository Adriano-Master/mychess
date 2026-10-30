//! Shared helpers of the `m8_1d_*` contract tests (spec
//! `docs/specs/m8-1-jogo-contra-engine.md`, section "M8-1d": ponder and
//! coach): the helpers of M8-1a plus events on a chosen execution, a second
//! (coach) engine and the search filters.
//!
//! Only the public API of `mychess-app` is used.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

#[path = "../m8_1_common/mod.rs"]
pub mod base;

pub use base::*;
use mychess_app::engines::{EngineDraft, EngineMessage, StartMode};
use mychess_app::{AppState, CoachConfig, ColorChoice, Effect, Message, PlayConfig, update};
use mychess_core::{Move, Position};
use mychess_engine::{BestMove, EngineCommand, EngineEvent, SearchId, SearchRequest};

use mychess_app::engines::{EngineId, RunId};

/// A rig whose engine declares the `Ponder` option.
pub fn ponder_rig() -> Rig {
    Rig::with_options(vec![check("Ponder")])
}

/// The configuration of a game with ponder on.
pub fn ponder_config(r: &Rig, color: ColorChoice, mode: mychess_app::TimeMode) -> PlayConfig {
    let mut c = r.config(color, mode);
    c.ponder = true;
    c
}

/// The move `uci` after `line` (UCI texts) from the standard position.
pub fn mv_after(line: &[&str], uci: &str) -> Move {
    let mut pos = Position::new();
    for text in line {
        let m = pos.parse_uci(text).expect("uci");
        pos.play(m).expect("legal");
    }
    pos.parse_uci(uci).expect("uci")
}

/// An event of `run`.
pub fn event_on(state: &mut AppState, run: RunId, event: EngineEvent) -> Vec<Effect> {
    update(
        state,
        Message::Engine(EngineMessage::Event {
            run,
            event: Box::new(event),
        }),
    )
}

/// The engine of `run` starts the search `tag` with the id `search`.
pub fn begin_search(state: &mut AppState, run: RunId, search: u64, tag: u64) {
    event_on(
        state,
        run,
        EngineEvent::SearchTagged {
            search: SearchId(search),
            tag,
        },
    );
    event_on(state, run, EngineEvent::SearchStarted(SearchId(search)));
}

pub fn best_event(search: u64, mv: Option<Move>, ponder: Option<Move>) -> EngineEvent {
    EngineEvent::BestMove(BestMove {
        search: SearchId(search),
        mv,
        ponder,
    })
}

/// The commands of `effects` addressed to `run`.
pub fn commands_for(effects: &[Effect], run: RunId) -> Vec<EngineCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand {
                run: r, command, ..
            } if *r == run => Some((**command).clone()),
            _ => None,
        })
        .collect()
}

/// The searches of `effects` (all runs) with the flag `ponder`.
pub fn ponder_searches(effects: &[Effect], ponder: bool) -> Vec<SearchRequest> {
    commands(effects)
        .into_iter()
        .filter_map(|c| match c {
            EngineCommand::Search(r) if r.ponder == ponder => Some(r),
            _ => None,
        })
        .collect()
}

pub fn count(effects: &[Effect], wanted: &EngineCommand) -> usize {
    commands(effects).iter().filter(|c| *c == wanted).count()
}

/// A second engine (the coach) that finished its handshake, in `rig`'s state.
pub fn add_engine(
    rig: &mut Rig,
    name: &str,
    options: Vec<mychess_engine::EngineOption>,
) -> (EngineId, RunId) {
    let engine = rig
        .state
        .engines
        .add(EngineDraft::new(name, format!("/{name}")))
        .expect("add engine");
    let fx = update(
        &mut rig.state,
        Message::Engine(EngineMessage::Start(engine, StartMode::Analyze)),
    );
    let run = fx
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("StartEngine");
    let mut id = identity(options);
    name.clone_into(&mut id.name);
    update(
        &mut rig.state,
        Message::Engine(EngineMessage::Started {
            run,
            identity: Box::new(id),
            at: 1_700_000_000,
        }),
    );
    (engine, run)
}

/// A second engine in the list that has not been started.
pub fn add_engine_entry(rig: &mut Rig, name: &str) -> EngineId {
    rig.state
        .engines
        .add(EngineDraft::new(name, format!("/{name}")))
        .expect("add engine")
}

/// The handshake of `run` ends.
pub fn finish_handshake(
    rig: &mut Rig,
    run: RunId,
    name: &str,
    options: Vec<mychess_engine::EngineOption>,
) -> Vec<Effect> {
    let mut id = identity(options);
    name.clone_into(&mut id.name);
    update(
        &mut rig.state,
        Message::Engine(EngineMessage::Started {
            run,
            identity: Box::new(id),
            at: 1_700_000_000,
        }),
    )
}

/// Whether `tag` is one of a search of the coach.
pub fn is_coach_tag(tag: u64) -> bool {
    tag >> 48 == 0x504D
}

/// The searches of `effects` sent to `run`.
pub fn searches_for(effects: &[Effect], run: RunId) -> Vec<SearchRequest> {
    commands_for(effects, run)
        .into_iter()
        .filter_map(|c| match c {
            EngineCommand::Search(r) => Some(r),
            _ => None,
        })
        .collect()
}

pub fn coach_config(coach: EngineId) -> CoachConfig {
    let mut c = CoachConfig::new(coach);
    c.depth = 8;
    c.seconds = 2;
    c
}

/// The coach answers the search `request` (id `search`) with `best` and the
/// exact `score` (White's point of view).
pub fn coach_answer(
    rig: &mut Rig,
    run: RunId,
    request: &SearchRequest,
    search: u64,
    best: Move,
    score: mychess_engine::Score,
) -> Vec<Effect> {
    begin_search(&mut rig.state, run, search, request.tag);
    event_on(
        &mut rig.state,
        run,
        EngineEvent::Info(mychess_engine::SearchInfo {
            search: SearchId(search),
            multipv: 1,
            score: Some((score, mychess_engine::Bound::Exact)),
            ..mychess_engine::SearchInfo::default()
        }),
    );
    event_on(&mut rig.state, run, best_event(search, Some(best), None))
}

/// Like [`coach_answer`] with the depth the last exact line reached.
pub fn coach_answer_at(
    rig: &mut Rig,
    run: RunId,
    request: &SearchRequest,
    search: u64,
    best: Move,
    score: mychess_engine::Score,
    depth: u32,
) -> Vec<Effect> {
    begin_search(&mut rig.state, run, search, request.tag);
    event_on(
        &mut rig.state,
        run,
        EngineEvent::Info(mychess_engine::SearchInfo {
            search: SearchId(search),
            multipv: 1,
            depth: Some(depth),
            score: Some((score, mychess_engine::Bound::Exact)),
            ..mychess_engine::SearchInfo::default()
        }),
    );
    event_on(&mut rig.state, run, best_event(search, Some(best), None))
}
