//! M8-1d contract tests of `EngineCommand::SetPonder` and of pondering on
//! the session (spec D2402): the option is sent with the real name, behind
//! the usual `isready` barrier; it is refused without the option and on
//! XBoard; a fake engine with `Ponder` on answers `bestmove ... ponder`,
//! and `go ponder` ends with `ponderhit`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "m4_1_common/helpers.rs"]
mod common;

use std::time::Duration;

use common::*;
use mychess_core::Position;
use mychess_engine::testing::{ManualEngine, ScriptedEngine, XBoardFake};
use mychess_engine::{
    EngineCommand, EngineConfig, EngineError, EngineEvent, OptionValue, Protocol, SearchLimits,
    SearchPosition, SearchRequest,
};

async fn line(peer: &mut ManualEngine) -> String {
    tokio::time::timeout(Duration::from_secs(5), peer.next_line())
        .await
        .expect("a line in time")
        .expect("the session is writing")
}

async fn ready(
    options: &[&str],
) -> (
    ManualEngine,
    mychess_engine::Engine,
    mychess_engine::EngineEvents,
) {
    let mut cfg = EngineConfig::new("fake");
    cfg.info_interval = Duration::ZERO;
    let (mut peer, engine, mut events) = ManualEngine::launch(cfg);
    peer.handshake("Manual", options).await;
    loop {
        if matches!(next(&mut events).await, EngineEvent::Ready(_)) {
            break;
        }
    }
    (peer, engine, events)
}

#[tokio::test(start_paused = true)]
async fn set_ponder_sends_the_option_with_its_real_name_and_waits_for_readyok() {
    let (mut peer, engine, _events) = ready(&["option name Ponder type check default false"]).await;
    engine.send(EngineCommand::SetPonder(true)).unwrap();
    assert_eq!(line(&mut peer).await, "setoption name Ponder value true");
    assert_eq!(line(&mut peer).await, "isready");
    peer.say("readyok").await;
    engine.send(EngineCommand::SetPonder(false)).unwrap();
    assert_eq!(line(&mut peer).await, "setoption name Ponder value false");
}

#[tokio::test(start_paused = true)]
async fn set_option_still_refuses_the_managed_ponder_option() {
    let (_peer, engine, _events) = ready(&["option name Ponder type check default false"]).await;
    let err = engine
        .send(EngineCommand::SetOption {
            name: "Ponder".into(),
            value: OptionValue::Bool(true),
        })
        .unwrap_err();
    assert!(matches!(err, EngineError::InvalidOption(_)), "{err:?}");
}

#[tokio::test(start_paused = true)]
async fn set_ponder_is_refused_without_the_option() {
    let (_peer, engine, _events) =
        ready(&["option name Hash type spin default 16 min 1 max 64"]).await;
    assert!(matches!(
        engine.send(EngineCommand::SetPonder(true)),
        Err(EngineError::Unsupported("Ponder"))
    ));
}

#[tokio::test(start_paused = true)]
async fn go_ponder_is_written_and_ponderhit_follows() {
    let (mut peer, engine, _events) = ready(&["option name Ponder type check default false"]).await;
    let mut request = depth("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", 5);
    request.ponder = true;
    engine.send(EngineCommand::Search(request)).unwrap();
    let mut seen = Vec::new();
    loop {
        let l = line(&mut peer).await;
        if l == "isready" {
            peer.say("readyok").await;
        }
        let done = l.starts_with("go ");
        seen.push(l);
        if done {
            break;
        }
    }
    assert_eq!(
        seen.last().map(String::as_str),
        Some("go ponder depth 5"),
        "{seen:?}"
    );
    engine.send(EngineCommand::PonderHit).unwrap();
    assert_eq!(line(&mut peer).await, "ponderhit");
}

#[tokio::test(start_paused = true)]
async fn the_fake_engine_gives_a_ponder_move_once_ponder_is_on_and_ends_on_ponderhit() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake
        .start(EngineConfig::new("fake"))
        .await
        .expect("started");
    // Off: the bestmove carries no ponder move.
    let start = Position::new();
    let request = SearchRequest {
        limits: SearchLimits {
            depth: Some(3),
            ..SearchLimits::default()
        },
        ..SearchRequest::infinite(SearchPosition::new(start.clone()))
    };
    engine.send(EngineCommand::Search(request.clone())).unwrap();
    let best = next_best(&mut events).await;
    assert!(best.mv.is_some() && best.ponder.is_none());
    // On: it does.
    engine.send(EngineCommand::SetPonder(true)).unwrap();
    engine.send(EngineCommand::Search(request)).unwrap();
    let best = next_best(&mut events).await;
    assert!(best.mv.is_some());
    let ponder = best.ponder.expect("a ponder move");
    // The ponder move is legal after the best move.
    let after = start.after(best.mv.unwrap()).unwrap();
    assert!(after.is_legal(ponder));
    // `go ponder` on that position runs until `ponderhit`.
    let ponder_request = SearchRequest {
        ponder: true,
        limits: SearchLimits {
            depth: Some(3),
            ..SearchLimits::default()
        },
        ..SearchRequest::infinite(SearchPosition {
            start,
            moves: vec![best.mv.unwrap(), ponder],
        })
    };
    engine.send(EngineCommand::Search(ponder_request)).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(300), next_best(&mut events))
            .await
            .is_err(),
        "no bestmove while pondering"
    );
    engine.send(EngineCommand::PonderHit).unwrap();
    assert!(next_best(&mut events).await.mv.is_some());
}

#[tokio::test(start_paused = true)]
async fn set_ponder_is_refused_on_xboard() {
    let fake = ScriptedEngine::builder()
        .xboard(XBoardFake::default())
        .build();
    let mut config = EngineConfig::new("fake");
    config.protocol = Protocol::XBoard;
    let (engine, _events) = fake.start(config).await.expect("started");
    assert!(matches!(
        engine.send(EngineCommand::SetPonder(true)),
        Err(EngineError::Unsupported("Ponder"))
    ));
}
