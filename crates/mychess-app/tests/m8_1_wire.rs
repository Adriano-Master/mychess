//! M8-1a contract tests of what the engine actually receives (spec D1238,
//! D1250): the app hands `NewGame`/`SetOption`/`Search` to a real engine
//! session whose peer is played by hand, and the lines on the wire are read:
//! `readyok` is awaited before `position`, and `go` carries the limit of the
//! mode in milliseconds.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1_common;

use std::time::Duration;

use m8_1_common::*;
use mychess_app::{PlayOutput, TimeMode};
use mychess_core::Color;
use mychess_engine::testing::ManualEngine;
use mychess_engine::{EngineCommand, EngineConfig, EngineEvent, EngineEvents, OptionValue};

async fn ready_session() -> (ManualEngine, mychess_engine::Engine, EngineEvents) {
    let mut cfg = EngineConfig::new("fake");
    cfg.info_interval = Duration::ZERO;
    let (mut peer, engine, mut events) = ManualEngine::launch(cfg);
    peer.handshake("Manual", &["option name Clear Hash type button"])
        .await;
    loop {
        let event = tokio::time::timeout(Duration::from_secs(60), events.recv())
            .await
            .expect("an event in time")
            .expect("alive");
        if matches!(event, EngineEvent::Ready(_)) {
            break;
        }
    }
    (peer, engine, events)
}

async fn line(peer: &mut ManualEngine) -> String {
    tokio::time::timeout(Duration::from_secs(5), peer.next_line())
        .await
        .expect("a line in time")
        .expect("the session is writing")
}

/// The think request the controller makes for `mode` after 1.e4 (the player
/// is White, 4 s used on his first move).
fn think_after_e4(mode: TimeMode) -> mychess_app::Think {
    let (mut c, _, t0) = begin(setup(Color::White, mode));
    let e4 = uci(&c, "e2e4");
    let out = c.player_move(e4, t0 + 4 * SEC);
    let mut thinks = out.into_iter().filter_map(|o| match o {
        PlayOutput::Think(t) => Some(t),
        _ => None,
    });
    thinks.next().expect("the engine is asked")
}

#[tokio::test(start_paused = true)]
async fn the_new_game_barrier_and_the_position_wait_for_readyok() {
    let (mut peer, engine, _events) = ready_session().await;
    let think = think_after_e4(TimeMode::Depth(3));
    engine.send(EngineCommand::NewGame).unwrap();
    engine
        .send(EngineCommand::Search(
            think.to_request(0x504C_0000_0000_0001),
        ))
        .unwrap();
    assert_eq!(line(&mut peer).await, "ucinewgame");
    assert_eq!(line(&mut peer).await, "isready");
    // Until readyok arrives the engine must not see `position` or `go`.
    let early = tokio::time::timeout(Duration::from_millis(200), peer.next_line()).await;
    assert!(early.is_err(), "nothing may follow isready: {early:?}");
    peer.say("readyok").await;
    assert_eq!(line(&mut peer).await, "position startpos moves e2e4");
    assert_eq!(line(&mut peer).await, "go depth 3");
}

#[tokio::test(start_paused = true)]
async fn options_are_applied_and_confirmed_before_the_position() {
    let (mut peer, engine, _events) = ready_session().await;
    let think = think_after_e4(TimeMode::Depth(2));
    engine.send(EngineCommand::NewGame).unwrap();
    engine
        .send(EngineCommand::SetOption {
            name: "Clear Hash".to_owned(),
            value: OptionValue::Bool(true),
        })
        .unwrap();
    engine
        .send(EngineCommand::Search(
            think.to_request(0x504C_0000_0000_0002),
        ))
        .unwrap();
    let mut seen = Vec::new();
    loop {
        let l = line(&mut peer).await;
        if l == "isready" {
            peer.say("readyok").await;
        }
        let done = l.starts_with("go ");
        if l.starts_with("position") {
            assert!(
                seen.iter()
                    .any(|s: &String| s.starts_with("setoption name Clear Hash")),
                "the option goes before the position: {seen:?}"
            );
        }
        seen.push(l);
        if done {
            break;
        }
    }
    assert!(
        seen.iter().position(|l| l == "ucinewgame").unwrap()
            < seen.iter().position(|l| l.starts_with("position")).unwrap()
    );
}

#[tokio::test(start_paused = true)]
async fn every_mode_sends_its_go_in_the_units_of_uci() {
    let cases: [(TimeMode, &str); 4] = [
        (TimeMode::Depth(7), "go depth 7"),
        (TimeMode::Nodes(1500), "go nodes 1500"),
        (TimeMode::MoveTime(6), "go movetime 6000"),
        (
            timebonus(5, 3, 5, 3),
            // 300 s - 4 s + 3 s for White, 300 s for Black untouched.
            "go wtime 299000 btime 300000 winc 3000 binc 3000",
        ),
    ];
    for (mode, expected) in cases {
        let (mut peer, engine, _events) = ready_session().await;
        let think = think_after_e4(mode);
        engine
            .send(EngineCommand::Search(
                think.to_request(0x504C_0000_0000_0003),
            ))
            .unwrap();
        let mut got = String::new();
        for _ in 0..8 {
            let l = line(&mut peer).await;
            if l == "isready" {
                peer.say("readyok").await;
            }
            if l.starts_with("go ") {
                got = l;
                break;
            }
        }
        assert_eq!(got, expected, "{mode:?}");
    }
}

#[tokio::test(start_paused = true)]
async fn asymmetric_budgets_reach_the_right_fields() {
    let (mut peer, engine, _events) = ready_session().await;
    let think = think_after_e4(timebonus(10, 5, 3, 2));
    engine
        .send(EngineCommand::Search(
            think.to_request(0x504C_0000_0000_0004),
        ))
        .unwrap();
    let mut got = String::new();
    for _ in 0..8 {
        let l = line(&mut peer).await;
        if l == "isready" {
            peer.say("readyok").await;
        }
        if l.starts_with("go ") {
            got = l;
            break;
        }
    }
    // White: 600 - 4 + 5; Black: 180; increments 5 and 2.
    assert_eq!(got, "go wtime 601000 btime 180000 winc 5000 binc 2000");
}
