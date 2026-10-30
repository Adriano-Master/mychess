//! M4-1 contract tests: event throughput and ordering (D412): the bounded
//! channel, `info` coalescing on a virtual clock, never dropping `BestMove`,
//! `Warning` and `Exited`, slow consumers and `try_recv`.

#[path = "m4_1_common/helpers.rs"]
mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use common::*;
use mychess_engine::testing::ScriptedEngine;
use mychess_engine::{EngineCommand, EngineEvent, EngineWarning, ExitReason};
use tokio::time::Instant;

fn info_line(multipv: u8, depth: u32) -> String {
    format!("info depth {depth} multipv {multipv} score cp {depth} pv e2e4")
}

#[tokio::test(start_paused = true)]
async fn a_thousand_infos_in_ten_milliseconds_collapse_and_the_bestmove_survives() {
    let mut cfg = config();
    cfg.info_interval = Duration::from_millis(50);
    let (mut peer, engine, mut events) = ready_with(cfg, &[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    for depth in 1..=1000_u32 {
        peer.say(&info_line(1, depth)).await;
        if depth % 100 == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
    peer.say("bestmove e2e4").await;
    let seen = until_best(&mut events).await;
    let infos: Vec<_> = seen
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Info(i) => Some(i),
            _ => None,
        })
        .collect();
    assert!(
        !infos.is_empty() && infos.len() <= 2,
        "{} infos in a 10 ms window",
        infos.len()
    );
    assert_eq!(
        infos.last().unwrap().depth,
        Some(1000),
        "the newest report is delivered before the bestmove"
    );
    assert!(matches!(seen.last(), Some(EngineEvent::BestMove(_))));
}

#[tokio::test(start_paused = true)]
async fn each_multipv_line_is_limited_independently_to_one_info_per_interval() {
    let mut cfg = config();
    cfg.info_interval = Duration::from_millis(50);
    let (mut peer, engine, mut events) = ready_with(cfg, &[MULTIPV4]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let mut stamps: BTreeMap<u8, Vec<Instant>> = BTreeMap::new();
    for ms in 0..1000_u32 {
        for line in 1..=3_u8 {
            peer.say(&info_line(line, ms + 1)).await;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
        for e in drain(&mut events) {
            if let EngineEvent::Info(i) = e {
                stamps.entry(i.multipv).or_default().push(Instant::now());
            }
        }
    }
    assert_eq!(stamps.keys().copied().collect::<Vec<_>>(), [1, 2, 3]);
    for (line, times) in &stamps {
        // 1 s / 50 ms = 20 intervals; the first report passes immediately.
        assert!(
            (15..=22).contains(&times.len()),
            "line {line}: {} reports in one second",
            times.len()
        );
        for pair in times.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                gap >= Duration::from_millis(48),
                "line {line}: two reports {gap:?} apart"
            );
        }
    }
}

#[tokio::test(start_paused = true)]
async fn the_last_value_of_each_line_wins_and_a_search_change_flushes_the_old_one() {
    let mut cfg = config();
    cfg.info_interval = Duration::from_millis(50);
    let (mut peer, engine, mut events) = ready_with(cfg, &[MULTIPV4]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    for line in 1..=3_u8 {
        peer.say(&info_line(line, 1)).await;
    }
    for line in [3_u8, 1, 2] {
        peer.say(&info_line(line, 2)).await;
        peer.say(&info_line(line, 3)).await;
    }
    peer.say("bestmove e2e4").await;
    let seen = until_best(&mut events).await;
    let mut last: BTreeMap<u8, u32> = BTreeMap::new();
    for e in &seen {
        if let EngineEvent::Info(i) = e {
            last.insert(i.multipv, i.depth.unwrap());
        }
    }
    assert_eq!(last, BTreeMap::from([(1, 3), (2, 3), (3, 3)]));
}

#[tokio::test(start_paused = true)]
async fn the_raw_log_keeps_every_line_even_when_the_events_are_coalesced() {
    let (mut peer, engine, mut events) = ready_with(config(), &[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let before = engine.log().snapshot_since(0).1;
    for depth in 1..=300_u32 {
        peer.say(&info_line(1, depth)).await;
    }
    peer.say("bestmove e2e4").await;
    until_best(&mut events).await;
    let (entries, _) = engine.log().snapshot_since(before);
    let received = entries
        .iter()
        .filter(|e| e.text.starts_with("info depth "))
        .count();
    assert_eq!(received, 300);
}

#[tokio::test(start_paused = true)]
async fn warning_ordering_and_infos_flush_before_the_bestmove() {
    let (mut peer, engine, mut events) = ready_with(config(), &[]).await;
    start_search(&mut peer, &engine, &mut events, depth(KPK, 3)).await;
    peer.say("info depth 1 score cp 1 pv e2e4").await;
    peer.say("info depth 2 score cp 2 pv e2e4").await;
    peer.say("bestmove e2e9").await;
    let seen = until_best(&mut events).await;
    let kinds: Vec<&str> = seen
        .iter()
        .map(|e| match e {
            EngineEvent::Info(_) => "info",
            EngineEvent::Warning(_) => "warning",
            EngineEvent::BestMove(_) => "best",
            _ => "other",
        })
        .collect();
    // infos first, then the warning about the move, then the bestmove.
    let warning = kinds.iter().position(|k| *k == "warning").unwrap();
    let best = kinds.iter().position(|k| *k == "best").unwrap();
    assert!(kinds[..warning].iter().filter(|k| **k == "info").count() >= 1);
    assert_eq!(best, warning + 1, "{kinds:?}");
}

#[tokio::test(start_paused = true)]
async fn ready_is_the_first_event_and_exited_the_last() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    engine.send(EngineCommand::Search(depth(KPK, 2))).unwrap();
    engine.send(EngineCommand::Quit).unwrap();
    let mut all = Vec::new();
    while let Some(e) = events.recv().await {
        all.push(e);
    }
    assert!(
        matches!(all.first(), Some(EngineEvent::Ready(_))),
        "{all:?}"
    );
    assert!(
        matches!(all.last(), Some(EngineEvent::Exited(_))),
        "{all:?}"
    );
    assert_eq!(
        all.iter()
            .filter(|e| matches!(e, EngineEvent::Exited(_)))
            .count(),
        1
    );
    assert!(events.recv().await.is_none(), "nothing after Exited");
}

#[tokio::test(start_paused = true)]
async fn try_recv_never_blocks_and_returns_what_is_waiting() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    // Ready is already queued by `start`.
    assert!(matches!(events.try_recv(), Some(EngineEvent::Ready(_))));
    assert!(events.try_recv().is_none());
    engine.send(EngineCommand::Search(depth(KPK, 1))).unwrap();
    settle().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut got_best = false;
    while let Some(e) = events.try_recv() {
        got_best |= matches!(e, EngineEvent::BestMove(_));
    }
    assert!(got_best);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn a_consumer_that_reads_late_still_gets_the_bestmove_and_the_session_survives() {
    // Every report passes (interval 0); the consumer does not read at all
    // while 20 000 reports and a bestmove go by.
    let mut cfg = config();
    cfg.info_interval = Duration::ZERO;
    let (mut peer, engine, mut events) = ready_with(cfg, &[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let mut blob = String::new();
    for depth in 1..=20_000_u32 {
        blob.push_str(&info_line(1, depth));
        blob.push('\n');
    }
    peer.say_raw(blob.as_bytes()).await;
    settle().await;
    peer.say("bestmove e2e4").await;
    settle().await;
    // The channel holds at most 1024 events.
    let mut infos = 0;
    loop {
        match tokio::time::timeout(Duration::from_secs(5), events.recv()).await {
            Ok(Some(EngineEvent::Info(_))) => infos += 1,
            Ok(Some(EngineEvent::BestMove(_))) => {
                break;
            }
            Ok(Some(_)) => {}
            other => panic!("the bestmove never came: {other:?}"),
        }
    }
    assert!(
        infos <= 1030,
        "{infos} infos queued: the channel is not bounded"
    );
    // The session answers commands afterwards.
    assert_nothing_sent_since(&mut peer, &engine).await;
}

#[tokio::test(start_paused = true)]
async fn dropping_the_receiver_ends_the_session_and_closes_the_pipe() {
    let (mut peer, engine, events) = ready_with(config(), &[]).await;
    drop(events);
    let mut lines = Vec::new();
    while let Some(l) = tokio::time::timeout(Duration::from_secs(5), peer.next_line())
        .await
        .expect("the session closes the pipe")
    {
        lines.push(l);
    }
    // Dropping is the impatient way out (spec rule 12): `quit` is optional,
    // the pipe must close.
    assert!(lines.iter().all(|l| l == "quit"), "{lines:?}");
    assert!(
        engine.send(EngineCommand::NewGame).is_err() || {
            settle().await;
            engine.send(EngineCommand::NewGame).is_err()
        }
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_returns_even_when_the_consumer_never_reads() {
    let mut cfg = config();
    cfg.info_interval = Duration::ZERO;
    let fake = ScriptedEngine::builder().build();
    let (engine, events) = fake.start(cfg).await.unwrap();
    engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    // 20 virtual seconds of 5 ms reports into a channel nobody reads.
    tokio::time::sleep(Duration::from_secs(20)).await;
    let began = Instant::now();
    engine.shutdown().await;
    assert!(
        began.elapsed() <= Duration::from_secs(3),
        "shutdown took {:?}",
        began.elapsed()
    );
    drop(events);
}

#[tokio::test(start_paused = true)]
async fn a_warning_event_is_never_lost_to_a_flood_of_infos() {
    let mut cfg = config();
    cfg.info_interval = Duration::ZERO;
    let (mut peer, engine, mut events) = ready_with(cfg, &[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    for depth in 1..=500_u32 {
        peer.say(&info_line(1, depth)).await;
    }
    peer.say("bestmove a1a1").await;
    let seen = until_best(&mut events).await;
    assert!(
        seen.iter()
            .any(|e| matches!(e, EngineEvent::Warning(EngineWarning::IllegalBestMove(_)))),
        "the warning disappeared among {} events",
        seen.len()
    );
}

#[tokio::test(start_paused = true)]
async fn exited_is_delivered_after_a_crash_in_the_middle_of_a_search() {
    let (mut peer, engine, mut events) = ready_with(config(), &[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    peer.say(&info_line(1, 5)).await;
    peer.close().await;
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::Crashed);
    assert!(events.recv().await.is_none());
    assert!(engine.send(EngineCommand::Stop).is_err());
}
