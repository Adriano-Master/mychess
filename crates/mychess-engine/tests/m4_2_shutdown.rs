//! M4-2 shutdown tests (the XBoard twin of the shutdown tests of
//! `m4_1_hardening.rs`): `Engine::shutdown` must always end in
//! `Exited(Quit)`, give a stubborn engine the whole quit wait, and leave
//! analyze mode (`exit`) before `quit`. Regression for the race between the
//! quit flag and the closing of the command channel.

#[path = "m4_1_common/helpers.rs"]
mod common;
#[path = "m4_2_common/xb.rs"]
mod xb;

use std::time::Duration;

use common::*;
use mychess_engine::testing::{ScriptedEngine, XBoardFake};
use mychess_engine::{EngineCommand, EngineEvent, ExitReason};
use tokio::time::Instant;
use xb::*;

#[tokio::test(start_paused = true)]
async fn shutdown_always_reports_exited_quit_as_the_last_event() {
    let mut wrong = Vec::new();
    for round in 0..64 {
        let r = full().await;
        let (_peer, engine, mut events) = (r.peer, r.engine, r.events);
        engine.shutdown().await;
        let mut last = None;
        while let Some(e) = events.recv().await {
            last = Some(e);
        }
        match last {
            Some(EngineEvent::Exited(x)) if x.reason == ExitReason::Quit => {}
            other => wrong.push((round, other)),
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of 64 rounds: {:?}",
        wrong.len(),
        wrong.first()
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_always_gives_a_stubborn_engine_the_whole_quit_wait_before_the_kill() {
    let mut short = Vec::new();
    for round in 0..32 {
        let fake = ScriptedEngine::builder()
            .xboard(XBoardFake::default())
            .ignore_quit()
            .build();
        let (engine, _events) = fake.start(xconfig()).await.expect("start");
        let began = Instant::now();
        engine.shutdown().await;
        if began.elapsed() < Duration::from_millis(450) {
            short.push((round, began.elapsed()));
        }
    }
    assert!(
        short.is_empty(),
        "{} of 32: {:?}",
        short.len(),
        short.first()
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_in_analyze_mode_sends_exit_before_quit_every_time() {
    let mut wrong = Vec::new();
    for round in 0..16 {
        let fake = ScriptedEngine::builder()
            .xboard(XBoardFake::default())
            .build();
        let (engine, mut events) = fake.start(xconfig()).await.expect("start");
        engine
            .send(EngineCommand::Search(infinite(KPK)))
            .expect("send");
        next_info(&mut events).await;
        engine.shutdown().await;
        let sent = fake.received();
        let exit = sent.iter().position(|l| l == "exit");
        let quit = sent.iter().position(|l| l == "quit");
        match (exit, quit) {
            (Some(e), Some(q)) if e < q => {}
            other => wrong.push((round, other)),
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}
