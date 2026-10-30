//! M4-2 contract tests: transitions between modes and states (replacing a
//! search, stop/quit during the handshake, stale engine output), D419, D423
//! and the "every `SearchStarted` ends in a `BestMove`" contract.

#[path = "m4_1_common/helpers.rs"]
mod common;
#[path = "m4_2_common/xb.rs"]
mod xb;

use common::*;
use mychess_engine::testing::ManualEngine;
use mychess_engine::{EngineCommand, EngineEvent, ExitReason};
use xb::*;

#[tokio::test(start_paused = true)]
async fn analyze_replaced_by_a_depth_limited_search_switches_to_go_mode() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    r.engine.send(EngineCommand::Search(depth(KPK, 6))).unwrap();
    let lines = r.read_search_lines().await;
    assert_eq!(
        strs(&lines),
        [
            "exit",
            "new",
            "force",
            &format!("setboard {}", pos(KPK).fen()),
            "st 120000",
            "sd 6",
            "post",
            "ping 2",
            "go"
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn go_replaced_by_an_analysis_stops_with_a_question_mark_and_waits_for_the_move() {
    let mut r = full().await;
    let (first, _) = r.start(depth(KPK, 6)).await;
    r.engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    assert_eq!(r.next_line().await, "?");
    settle().await;
    quiet(&mut r.events);
    r.peer.say("move e2e4").await;
    let best = next_best(&mut r.events).await;
    assert_eq!(best.search, first);
    let lines = r.read_search_lines().await;
    assert_eq!(
        lines[0], "force",
        "the engine already left thinking: no `exit`: {lines:?}"
    );
    assert!(lines.contains(&"analyze".to_owned()));
    let second = r.started().await;
    assert_ne!(second, first);
}

#[tokio::test(start_paused = true)]
async fn the_old_go_move_arriving_after_the_new_search_started_is_dropped() {
    let mut r = full().await;
    let (first, _) = r.start(depth(KPK, 6)).await;
    r.engine
        .send(EngineCommand::Search(depth(BLACK_WINS_QUEEN, 6)))
        .unwrap();
    assert_eq!(r.next_line().await, "?");
    r.peer.say("move e2e4").await;
    let best = next_best(&mut r.events).await;
    assert_eq!(best.search, first);
    let lines = r.read_search_lines().await; // new search, ping not yet answered
    let second = r.started().await;
    // A duplicate/late move line of the previous search must not end the new one.
    r.peer.say("move a3a1").await;
    settle().await;
    assert!(
        drain(&mut r.events)
            .iter()
            .all(|e| !matches!(e, EngineEvent::BestMove(_)))
    );
    r.pong(&lines).await;
    r.peer.say("move a3a1").await;
    let best = next_best(&mut r.events).await;
    assert_eq!(best.search, second);
}

#[tokio::test(start_paused = true)]
async fn only_the_last_of_several_searches_sent_while_stopping_a_go_search_runs() {
    let mut r = full().await;
    r.start(depth(KPK, 6)).await;
    r.engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    assert_eq!(r.next_line().await, "?");
    r.engine
        .send(EngineCommand::Search(infinite(BLACK_WINS_QUEEN)))
        .unwrap();
    settle().await;
    r.peer.say("move e2e4").await;
    let lines = r.read_search_lines().await;
    assert!(
        lines.contains(&format!("setboard {}", pos(BLACK_WINS_QUEEN).fen())),
        "{lines:?}"
    );
    assert!(!lines.contains(&format!("setboard {}", pos(KPK).fen())));
}

#[tokio::test(start_paused = true)]
async fn stop_while_a_go_search_is_stopping_cancels_the_pending_search() {
    let mut r = full().await;
    r.start(depth(KPK, 6)).await;
    r.engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    assert_eq!(r.next_line().await, "?");
    r.engine.send(EngineCommand::Stop).unwrap();
    settle().await;
    r.peer.say("move e2e4").await;
    next_best(&mut r.events).await;
    settle().await;
    assert!(
        drain(&mut r.events)
            .iter()
            .all(|e| !matches!(e, EngineEvent::SearchStarted(_))),
        "the replaced search was cancelled by Stop"
    );
}

#[tokio::test(start_paused = true)]
async fn stop_during_the_handshake_cancels_the_pending_search() {
    let (mut peer, engine, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    engine
        .send(EngineCommand::Search(infinite(START_FEN)))
        .unwrap();
    engine.send(EngineCommand::Stop).unwrap();
    peer.say(NO_PING).await;
    loop {
        if line(&mut peer).await == "force" {
            break;
        }
    }
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));
    settle().await;
    quiet(&mut events);
}

#[tokio::test(start_paused = true)]
async fn quit_during_the_handshake_ends_with_quit() {
    let (mut peer, engine, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    engine.send(EngineCommand::Quit).unwrap();
    assert_eq!(line(&mut peer).await, "quit");
    assert_eq!(next_exit(&mut events).await.reason, ExitReason::Quit);
}

#[tokio::test(start_paused = true)]
async fn every_started_search_ends_in_one_best_move_across_many_replacements() {
    let mut r = full().await;
    let mut started = 0;
    for fen in [KPK, BLACK_WINS_QUEEN, KPK, WHITE_MATED, KPK] {
        r.engine.send(EngineCommand::Search(infinite(fen))).unwrap();
        started += 1;
        if fen == WHITE_MATED {
            continue; // answered by the adapter alone
        }
        let lines = r.read_search_lines().await;
        r.pong(&lines).await;
    }
    r.engine.send(EngineCommand::Stop).unwrap();
    let mut best = 0;
    let mut starts = 0;
    let mut last_was_best = false;
    // Stop at the final `exit`; then collect what is queued.
    loop {
        let l = r.next_line().await;
        if l == "exit" {
            break;
        }
    }
    settle().await;
    for e in drain(&mut r.events) {
        match e {
            EngineEvent::SearchStarted(_) => {
                starts += 1;
                last_was_best = false;
            }
            EngineEvent::BestMove(_) => {
                best += 1;
                last_was_best = true;
            }
            _ => {}
        }
    }
    assert_eq!((starts, best), (started, started));
    assert!(last_was_best);
}

#[tokio::test(start_paused = true)]
async fn infos_always_carry_the_id_of_the_current_search() {
    let mut r = full().await;
    let (a, _) = r.start(infinite(KPK)).await;
    let i = r.think(" 3. +1 1 1 e2e4").await;
    assert_eq!(i.search, a);
    r.engine
        .send(EngineCommand::Search(infinite(BLACK_WINS_QUEEN)))
        .unwrap();
    let lines = r.read_search_lines().await;
    let b = until(&mut r.events, |e| match e {
        EngineEvent::SearchStarted(id) => Some(id),
        _ => None,
    })
    .await;
    r.peer.say(" 30. +1 1 1 e2e4").await; // old search, before the pong
    r.pong(&lines).await;
    let i = r.think(" 1. -3 1 1 a3a1").await;
    assert_eq!((i.search, i.depth), (b, Some(1)));
}
