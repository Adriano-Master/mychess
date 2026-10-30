//! Helpers shared by the `m4_1_*` contract tests.
//!
//! Included with `#[path = "m4_1_common/helpers.rs"] mod common;` (the
//! workspace lint `mod_module_files` forbids a `mod.rs`).
#![allow(dead_code)]

use std::time::Duration;

use mychess_core::{Move, Position};
use mychess_engine::testing::ManualEngine;
use mychess_engine::{
    BestMove, Engine, EngineCommand, EngineConfig, EngineEvent, EngineEvents, ExitInfo, SearchId,
    SearchInfo, SearchLimits, SearchPosition, SearchRequest,
};

/// A FEN with a lone pawn per side: tiny, with several legal moves.
pub(crate) const KPK: &str = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
/// Black to move in a position where Black can win a queen.
pub(crate) const BLACK_WINS_QUEEN: &str = "4k3/8/8/8/8/q7/8/Q3K3 b - - 0 1";
/// White is checkmated (fool's mate).
pub(crate) const WHITE_MATED: &str =
    "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3";
/// Black is stalemated.
pub(crate) const BLACK_STALEMATED: &str = "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1";
/// A Chess960 start where `e1d1` is castling (king takes own rook).
pub(crate) const FRC_CASTLE: &str = "4k3/8/8/8/8/8/8/1R1RK3 w D - 0 1";

pub(crate) fn config() -> EngineConfig {
    EngineConfig::new("fake")
}

pub(crate) fn pos(fen: &str) -> Position {
    Position::from_fen(fen).expect("valid test FEN")
}

pub(crate) fn mv(position: &Position, text: &str) -> Move {
    position.parse_uci(text).expect("legal test move")
}

pub(crate) fn infinite(fen: &str) -> SearchRequest {
    SearchRequest::infinite(SearchPosition::new(pos(fen)))
}

pub(crate) fn limits(fen: &str, limits: SearchLimits) -> SearchRequest {
    SearchRequest {
        limits,
        ..infinite(fen)
    }
}

pub(crate) fn depth(fen: &str, n: u32) -> SearchRequest {
    limits(
        fen,
        SearchLimits {
            depth: Some(n),
            ..SearchLimits::default()
        },
    )
}

/// The next event, failing after 60 virtual (or real) seconds.
pub(crate) async fn next(events: &mut EngineEvents) -> EngineEvent {
    tokio::time::timeout(Duration::from_secs(60), events.recv())
        .await
        .expect("an event in time")
        .expect("the session is alive")
}

/// Reads events until `pick` returns `Some`.
pub(crate) async fn until<T>(
    events: &mut EngineEvents,
    mut pick: impl FnMut(EngineEvent) -> Option<T>,
) -> T {
    loop {
        if let Some(found) = pick(next(events).await) {
            return found;
        }
    }
}

pub(crate) async fn next_best(events: &mut EngineEvents) -> BestMove {
    until(events, |e| match e {
        EngineEvent::BestMove(b) => Some(b),
        _ => None,
    })
    .await
}

pub(crate) async fn next_exit(events: &mut EngineEvents) -> ExitInfo {
    until(events, |e| match e {
        EngineEvent::Exited(x) => Some(x),
        _ => None,
    })
    .await
}

pub(crate) async fn next_info(events: &mut EngineEvents) -> SearchInfo {
    until(events, |e| match e {
        EngineEvent::Info(i) => Some(i),
        _ => None,
    })
    .await
}

/// Everything that is already waiting in the channel.
pub(crate) fn drain(events: &mut EngineEvents) -> Vec<EngineEvent> {
    let mut out = Vec::new();
    while let Some(e) = events.try_recv() {
        out.push(e);
    }
    out
}

/// Collects events up to and including the next `BestMove`.
pub(crate) async fn until_best(events: &mut EngineEvents) -> Vec<EngineEvent> {
    let mut out = Vec::new();
    loop {
        let e = next(events).await;
        let done = matches!(e, EngineEvent::BestMove(_));
        out.push(e);
        if done {
            return out;
        }
    }
}

/// Lets every ready task run (for paused-clock tests).
pub(crate) async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

pub(crate) const MULTIPV4: &str = "option name MultiPV type spin default 1 min 1 max 4";
pub(crate) const HASH: &str = "option name Hash type spin default 16 min 1 max 1024";
pub(crate) const PONDER: &str = "option name Ponder type check default false";
pub(crate) const CHESS960: &str = "option name UCI_Chess960 type check default false";
pub(crate) const SYZYGY: &str = "option name SyzygyPath type string default <empty>";
pub(crate) const CLEAR_HASH: &str = "option name Clear Hash type button";
pub(crate) const STYLE: &str =
    "option name Style type combo default Normal var Solid var Normal var Risky";

/// The next line the session wrote, failing after 5 (virtual) seconds.
pub(crate) async fn line(peer: &mut ManualEngine) -> String {
    tokio::time::timeout(Duration::from_secs(5), peer.next_line())
        .await
        .expect("a line in time")
        .expect("the session is still writing")
}

/// Asserts the next lines written by the session, in order.
pub(crate) async fn expect_lines(peer: &mut ManualEngine, want: &[&str]) {
    for w in want {
        assert_eq!(line(peer).await, *w);
    }
}

/// A session whose engine is played by hand, past the handshake and `Ready`.
pub(crate) async fn ready_with(
    config: EngineConfig,
    options: &[&str],
) -> (ManualEngine, Engine, EngineEvents) {
    let (mut peer, engine, mut events) = ManualEngine::launch(config);
    peer.handshake("Manual", options).await;
    match next(&mut events).await {
        EngineEvent::Ready(_) => {}
        other => panic!("expected Ready, got {other:?}"),
    }
    (peer, engine, events)
}

pub(crate) async fn ready(options: &[&str]) -> (ManualEngine, Engine, EngineEvents) {
    let mut cfg = config();
    cfg.info_interval = Duration::ZERO;
    ready_with(cfg, options).await
}

/// Sends a `NewGame` and checks it is the very next thing the engine sees
/// (so nothing was written in between), then answers the barrier.
pub(crate) async fn assert_nothing_sent_since(peer: &mut ManualEngine, engine: &Engine) {
    engine.send(EngineCommand::NewGame).expect("send");
    expect_lines(peer, &["ucinewgame", "isready"]).await;
    peer.say("readyok").await;
}

/// Starts `request` on a ready session and returns the id and the two lines
/// (`position`, `go`) the engine received.
pub(crate) async fn start_search(
    peer: &mut ManualEngine,
    engine: &Engine,
    events: &mut EngineEvents,
    request: SearchRequest,
) -> (SearchId, String, String) {
    engine.send(EngineCommand::Search(request)).expect("send");
    let position = line(peer).await;
    let go = line(peer).await;
    let id = until(events, |e| match e {
        EngineEvent::SearchStarted(id) => Some(id),
        _ => None,
    })
    .await;
    (id, position, go)
}
