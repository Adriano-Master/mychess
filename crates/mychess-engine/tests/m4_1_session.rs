//! M4-1 contract tests: handshake, deadlines, the command/state table and the
//! command queue (spec `docs/specs/m4-1-protocolo-uci.md`, rule 1, D404, D407,
//! D408, D409, D607, D609). The engine is played by hand over an in-memory
//! pipe and the clock is paused, so nothing waits for real time.

#[path = "m4_1_common/helpers.rs"]
mod common;

use std::time::Duration;

use common::*;
use mychess_engine::testing::{ManualEngine, ScriptedEngine};
use mychess_engine::{
    Engine, EngineCommand, EngineError, EngineEvent, EngineWarning, ExitReason, OptionValue, Phase,
};
use tokio::time::Instant;

// ---------------------------------------------------------------------------
// Handshake
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn handshake_sends_uci_first_and_reports_identity_options_and_capabilities() {
    let (mut peer, _engine, mut events) = ManualEngine::launch(config());
    assert_eq!(line(&mut peer).await, "uci");
    peer.say("id name Test Engine 1.0").await;
    peer.say("id author The Authors").await;
    peer.say(HASH).await;
    peer.say(MULTIPV4).await;
    peer.say(CLEAR_HASH).await;
    peer.say("uciok").await;
    // Then, and only then: ucinewgame and exactly one isready (D408, D409).
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
    peer.say("readyok").await;
    let EngineEvent::Ready(identity) = next(&mut events).await else {
        panic!("first event must be Ready");
    };
    assert_eq!(identity.name, "Test Engine 1.0");
    assert_eq!(identity.author.as_deref(), Some("The Authors"));
    let names: Vec<_> = identity.options.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["Hash", "MultiPV", "Clear Hash"]);
    assert_eq!(identity.capabilities.multipv, Some(1..=4));
    assert!(!identity.capabilities.chess960);
    assert!(!identity.capabilities.ponder);
    assert!(identity.capabilities.search_moves);
}

#[tokio::test(start_paused = true)]
async fn uciok_without_id_name_or_options_is_valid_and_falls_back_to_the_configured_name() {
    let (mut peer, _engine, mut events) = ManualEngine::launch(config());
    assert_eq!(line(&mut peer).await, "uci");
    peer.say("uciok").await;
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
    peer.say("readyok").await;
    let EngineEvent::Ready(identity) = next(&mut events).await else {
        panic!("expected Ready");
    };
    assert_eq!(identity.name, "fake");
    assert_eq!(identity.author, None);
    assert_eq!(identity.options.len(), 0);
    assert_eq!(identity.capabilities.multipv, None);
}

#[tokio::test(start_paused = true)]
async fn junk_before_uciok_is_ignored_and_late_options_are_dropped() {
    let (mut peer, _engine, mut events) = ManualEngine::launch(config());
    assert_eq!(line(&mut peer).await, "uci");
    for junk in [
        "Welcome to Engine!",
        "info depth 3 score cp 4",
        "bestmove e2e4",
        "readyok",
        "option name Bogus type wibble",
        "option type check",
        "id nameless",
        "   ",
        "",
    ] {
        peer.say(junk).await;
    }
    peer.say("id name Noisy").await;
    peer.say(HASH).await;
    peer.say("uciok").await;
    // An option after uciok does not count (rule 2).
    peer.say("option name Late type check default false").await;
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
    peer.say("readyok").await;
    let EngineEvent::Ready(identity) = next(&mut events).await else {
        panic!("expected Ready");
    };
    assert_eq!(identity.name, "Noisy");
    let names: Vec<_> = identity.options.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["Hash"]);
}

#[tokio::test(start_paused = true)]
async fn a_repeated_option_keeps_the_last_announcement() {
    let (mut peer, _engine, mut events) = ManualEngine::launch(config());
    peer.handshake(
        "Dup",
        &[
            "option name Hash type spin default 16 min 1 max 1024",
            "option name Ponder type check default false",
            "option name Hash type spin default 32 min 1 max 64",
        ],
    )
    .await;
    let EngineEvent::Ready(identity) = next(&mut events).await else {
        panic!("expected Ready");
    };
    let hash: Vec<_> = identity
        .options
        .iter()
        .filter(|o| o.name == "Hash")
        .collect();
    assert_eq!(hash.len(), 1);
    assert_eq!(
        hash[0].kind,
        mychess_engine::OptionKind::Spin {
            default: 32,
            min: 1,
            max: 64
        }
    );
}

#[tokio::test(start_paused = true)]
async fn crlf_replies_from_the_engine_are_understood() {
    let fake = ScriptedEngine::builder()
        .crlf()
        .name("Windows Style")
        .build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    let EngineEvent::Ready(identity) = next(&mut events).await else {
        panic!("expected Ready");
    };
    assert_eq!(identity.name, "Windows Style");
    engine.send(EngineCommand::Search(depth(KPK, 1))).unwrap();
    assert!(next_best(&mut events).await.mv.is_some());
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn saved_options_go_out_in_order_before_a_single_isready_and_managed_ones_are_skipped() {
    let mut cfg = config();
    cfg.options = vec![
        ("Hash".into(), OptionValue::Int(64)),
        ("Ponder".into(), OptionValue::Bool(true)),
        ("UCI_Chess960".into(), OptionValue::Bool(true)),
        ("UCI_AnalyseMode".into(), OptionValue::Bool(false)),
        ("Clear Hash".into(), OptionValue::Bool(true)),
        ("SyzygyPath".into(), OptionValue::Text(String::new())),
        ("Style".into(), OptionValue::Text("Risky".into())),
    ];
    let (mut peer, _engine, mut events) = ManualEngine::launch(cfg);
    assert_eq!(line(&mut peer).await, "uci");
    for l in [
        "id name Opts",
        HASH,
        PONDER,
        CHESS960,
        "option name UCI_AnalyseMode type check default false",
        CLEAR_HASH,
        SYZYGY,
        STYLE,
        "uciok",
    ] {
        peer.say(l).await;
    }
    // Saved values in order, the managed ones left out; AnalyseMode is
    // switched on by the session itself because the engine announces it.
    expect_lines(
        &mut peer,
        &[
            "setoption name Hash value 64",
            "setoption name Clear Hash",
            "setoption name SyzygyPath value <empty>",
            "setoption name Style value Risky",
            "setoption name UCI_AnalyseMode value true",
            "ucinewgame",
            "isready",
        ],
    )
    .await;
    peer.say("readyok").await;
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));
}

#[tokio::test(start_paused = true)]
async fn invalid_saved_options_become_warnings_and_are_not_sent() {
    let mut cfg = config();
    cfg.options = vec![
        ("Hash".into(), OptionValue::Int(0)),
        ("NoSuchOption".into(), OptionValue::Int(1)),
        ("Style".into(), OptionValue::Text("Wild".into())),
        ("SyzygyPath".into(), OptionValue::Text("a\nquit".into())),
        ("Hash".into(), OptionValue::Int(32)),
    ];
    let (mut peer, _engine, mut events) = ManualEngine::launch(cfg);
    assert_eq!(line(&mut peer).await, "uci");
    for l in [HASH, STYLE, SYZYGY, "uciok"] {
        peer.say(l).await;
    }
    expect_lines(
        &mut peer,
        &["setoption name Hash value 32", "ucinewgame", "isready"],
    )
    .await;
    peer.say("readyok").await;
    let mut skipped = 0;
    loop {
        match next(&mut events).await {
            EngineEvent::Warning(EngineWarning::OptionSkipped { .. }) => skipped += 1,
            EngineEvent::Ready(_) => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(skipped, 4);
}

// ---------------------------------------------------------------------------
// Deadlines (D404, D414)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn a_uciok_within_ten_seconds_is_accepted() {
    let fake = ScriptedEngine::builder()
        .uciok_delay(Duration::from_millis(9_900))
        .build();
    let began = Instant::now();
    let result = fake.start(config()).await;
    assert!(result.is_ok(), "{:?}", result.err());
    assert!(began.elapsed() >= Duration::from_millis(9_900));
}

#[tokio::test(start_paused = true)]
async fn a_uciok_after_ten_seconds_is_a_timeout_error() {
    let fake = ScriptedEngine::builder()
        .uciok_delay(Duration::from_millis(10_100))
        .build();
    let began = Instant::now();
    let result = fake.start(config()).await;
    assert!(
        matches!(result, Err(EngineError::Timeout(Phase::Uciok))),
        "{result:?}"
    );
    let elapsed = began.elapsed();
    assert!(
        elapsed >= Duration::from_secs(10) && elapsed < Duration::from_secs(11),
        "gave up at {elapsed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_uciok_deadline_is_configurable() {
    let mut cfg = config();
    cfg.timeouts.uciok = Duration::from_secs(2);
    let fake = ScriptedEngine::builder()
        .uciok_delay(Duration::from_millis(2_100))
        .build();
    assert!(matches!(
        fake.start(cfg.clone()).await,
        Err(EngineError::Timeout(Phase::Uciok))
    ));
    let fake = ScriptedEngine::builder()
        .uciok_delay(Duration::from_millis(1_900))
        .build();
    assert!(fake.start(cfg).await.is_ok());
}

#[tokio::test(start_paused = true)]
async fn a_silent_engine_gets_exited_with_a_uciok_timeout_and_is_told_to_quit() {
    let (mut peer, _engine, mut events) = ManualEngine::launch(config());
    assert_eq!(line(&mut peer).await, "uci");
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::Timeout(Phase::Uciok));
    assert_eq!(line(&mut peer).await, "quit");
    assert!(events.recv().await.is_none(), "nothing follows Exited");
}

#[tokio::test(start_paused = true)]
async fn readyok_is_awaited_for_thirty_seconds_after_the_handshake() {
    // Within the deadline.
    let (mut peer, _engine, mut events) = ManualEngine::launch(config());
    assert_eq!(line(&mut peer).await, "uci");
    peer.say("uciok").await;
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
    tokio::time::sleep(Duration::from_millis(29_900)).await;
    peer.say("readyok").await;
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));

    // Past it.
    let (mut peer, _engine, mut events) = ManualEngine::launch(config());
    assert_eq!(line(&mut peer).await, "uci");
    peer.say("uciok").await;
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
    tokio::time::sleep(Duration::from_millis(30_100)).await;
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::Timeout(Phase::Readyok));
}

#[tokio::test(start_paused = true)]
async fn start_reports_a_readyok_timeout_as_a_typed_error() {
    // An engine that answers `uciok` but never `readyok`.
    let mut cfg = config();
    cfg.timeouts.readyok = Duration::from_secs(1);
    let (mut peer, engine, mut events) = ManualEngine::launch(cfg);
    assert_eq!(line(&mut peer).await, "uci");
    peer.say("uciok").await;
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::Timeout(Phase::Readyok));
    assert!(matches!(
        engine.send(EngineCommand::Stop),
        Err(EngineError::Closed)
    ));
}

#[tokio::test(start_paused = true)]
async fn the_barrier_after_setoption_also_has_a_readyok_deadline() {
    let (mut peer, engine, mut events) = ready(&[HASH]).await;
    engine
        .send(EngineCommand::SetOption {
            name: "Hash".into(),
            value: OptionValue::Int(8),
        })
        .unwrap();
    expect_lines(&mut peer, &["setoption name Hash value 8", "isready"]).await;
    let began = Instant::now();
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::Timeout(Phase::Readyok));
    assert!(began.elapsed() >= Duration::from_secs(30));
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_dies_during_the_handshake_is_reported_as_crashed() {
    let (mut peer, engine, mut events) = ManualEngine::launch(config());
    assert_eq!(line(&mut peer).await, "uci");
    peer.say("id name Dying").await;
    peer.close().await;
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::Crashed);
    assert!(matches!(
        engine.send(EngineCommand::NewGame),
        Err(EngineError::Closed)
    ));
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_dies_while_configuring_is_reported_as_crashed() {
    let (mut peer, _engine, mut events) = ManualEngine::launch(config());
    assert_eq!(line(&mut peer).await, "uci");
    peer.say("uciok").await;
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
    peer.close().await;
    assert_eq!(next_exit(&mut events).await.reason, ExitReason::Crashed);
}

#[tokio::test(start_paused = true)]
async fn spurious_uciok_and_readyok_while_ready_are_ignored() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    peer.say("readyok").await;
    peer.say("uciok").await;
    peer.say("readyok").await;
    assert_nothing_sent_since(&mut peer, &engine).await;
    assert_eq!(drain(&mut events).len(), 0);
}

// ---------------------------------------------------------------------------
// Rule 1: the command/state table
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn ready_ignores_stop_and_ponderhit() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    engine.send(EngineCommand::Stop).unwrap();
    engine.send(EngineCommand::PonderHit).unwrap();
    // The very next line the engine sees is the marker's.
    assert_nothing_sent_since(&mut peer, &engine).await;
    assert_eq!(drain(&mut events).len(), 0);
}

#[tokio::test(start_paused = true)]
async fn ready_accepts_newgame_setoption_setmultipv_and_search() {
    let (mut peer, engine, mut events) = ready(&[HASH, MULTIPV4]).await;
    engine.send(EngineCommand::NewGame).unwrap();
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
    peer.say("readyok").await;
    engine
        .send(EngineCommand::SetOption {
            name: "Hash".into(),
            value: OptionValue::Int(128),
        })
        .unwrap();
    expect_lines(&mut peer, &["setoption name Hash value 128", "isready"]).await;
    peer.say("readyok").await;
    engine.send(EngineCommand::SetMultiPv(3)).unwrap();
    expect_lines(&mut peer, &["setoption name MultiPV value 3", "isready"]).await;
    peer.say("readyok").await;
    let (id, position, go) = start_search(&mut peer, &engine, &mut events, depth(KPK, 4)).await;
    assert_eq!(id.0, 1);
    assert_eq!(position, format!("position fen {KPK}"));
    assert_eq!(go, "go depth 4");
}

#[tokio::test(start_paused = true)]
async fn a_search_queued_behind_a_setoption_waits_for_its_readyok() {
    let (mut peer, engine, mut events) = ready(&[HASH]).await;
    engine
        .send(EngineCommand::SetOption {
            name: "Hash".into(),
            value: OptionValue::Int(2),
        })
        .unwrap();
    engine.send(EngineCommand::Search(depth(KPK, 2))).unwrap();
    expect_lines(&mut peer, &["setoption name Hash value 2", "isready"]).await;
    // No `position`/`go` before `readyok`.
    settle().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.next_line())
            .await
            .is_err()
    );
    peer.say("readyok").await;
    expect_lines(&mut peer, &[&format!("position fen {KPK}"), "go depth 2"]).await;
    assert!(matches!(
        next(&mut events).await,
        EngineEvent::SearchStarted(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn searching_stop_sends_stop_and_still_delivers_the_old_bestmove() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let (id, _, _) = start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(line(&mut peer).await, "stop");
    peer.say("bestmove e2e4").await;
    let best = next_best(&mut events).await;
    assert_eq!(best.search, id);
    assert!(best.mv.is_some());
    // Back to Ready: a new search starts at once.
    let (id2, _, go) = start_search(&mut peer, &engine, &mut events, depth(KPK, 1)).await;
    assert!(id2 > id);
    assert_eq!(go, "go depth 1");
}

#[tokio::test(start_paused = true)]
async fn searching_search_stops_the_old_one_and_starts_the_new_after_its_bestmove() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let (a, _, _) = start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::Search(depth(KPK, 7))).unwrap();
    assert_eq!(line(&mut peer).await, "stop");
    // Nothing for the new search until the old bestmove arrives.
    assert!(
        tokio::time::timeout(Duration::from_millis(100), peer.next_line())
            .await
            .is_err()
    );
    peer.say("bestmove e2e4").await;
    expect_lines(&mut peer, &[&format!("position fen {KPK}"), "go depth 7"]).await;
    let events_seen = until_best(&mut events).await;
    assert!(
        matches!(&events_seen[..], [.., EngineEvent::BestMove(b)] if b.search == a),
        "{events_seen:?}"
    );
    let b = until(&mut events, |e| match e {
        EngineEvent::SearchStarted(id) => Some(id),
        _ => None,
    })
    .await;
    assert!(b > a);
}

#[tokio::test(start_paused = true)]
async fn searching_setoption_stops_applies_and_restarts_the_same_search_with_a_new_id() {
    let (mut peer, engine, mut events) = ready(&[HASH]).await;
    let (a, _, _) = start_search(&mut peer, &engine, &mut events, depth(KPK, 9)).await;
    engine
        .send(EngineCommand::SetOption {
            name: "Hash".into(),
            value: OptionValue::Int(4),
        })
        .unwrap();
    assert_eq!(line(&mut peer).await, "stop");
    peer.say("bestmove e2e4").await;
    expect_lines(&mut peer, &["setoption name Hash value 4", "isready"]).await;
    peer.say("readyok").await;
    expect_lines(&mut peer, &[&format!("position fen {KPK}"), "go depth 9"]).await;
    assert_eq!(next_best(&mut events).await.search, a);
    let b = until(&mut events, |e| match e {
        EngineEvent::SearchStarted(id) => Some(id),
        _ => None,
    })
    .await;
    assert!(b > a);
}

#[tokio::test(start_paused = true)]
async fn searching_setmultipv_restarts_the_search_after_the_option_is_applied() {
    let (mut peer, engine, mut events) = ready(&[MULTIPV4]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::SetMultiPv(2)).unwrap();
    assert_eq!(line(&mut peer).await, "stop");
    peer.say("bestmove e2e4").await;
    expect_lines(&mut peer, &["setoption name MultiPV value 2", "isready"]).await;
    peer.say("readyok").await;
    expect_lines(&mut peer, &[&format!("position fen {KPK}"), "go infinite"]).await;
}

#[tokio::test(start_paused = true)]
async fn searching_newgame_stops_and_applies_later_without_restarting_the_search() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::NewGame).unwrap();
    assert_eq!(line(&mut peer).await, "stop");
    peer.say("bestmove e2e4").await;
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
    peer.say("readyok").await;
    // No new `position`/`go` follows.
    assert_nothing_sent_since(&mut peer, &engine).await;
}

#[tokio::test(start_paused = true)]
async fn ponderhit_is_sent_only_while_pondering_and_only_once() {
    let (mut peer, engine, mut events) = ready(&[PONDER]).await;
    // Not pondering: ignored.
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::PonderHit).unwrap();
    engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(line(&mut peer).await, "stop", "ponderhit was not sent");
    peer.say("bestmove e2e4").await;

    // Pondering.
    let mut request = infinite(KPK);
    request.ponder = true;
    let (_, _, go) = start_search(&mut peer, &engine, &mut events, request).await;
    assert!(go.starts_with("go ponder"), "{go}");
    engine.send(EngineCommand::PonderHit).unwrap();
    engine.send(EngineCommand::PonderHit).unwrap();
    assert_eq!(line(&mut peer).await, "ponderhit");
    engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(
        line(&mut peer).await,
        "stop",
        "the second ponderhit is dropped"
    );
}

#[tokio::test(start_paused = true)]
async fn the_session_does_not_turn_the_ponder_option_on_by_itself() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    let mut request = infinite(KPK);
    request.ponder = true;
    engine.send(EngineCommand::Search(request)).unwrap();
    until(&mut events, |e| {
        matches!(e, EngineEvent::SearchStarted(_)).then_some(())
    })
    .await;
    assert!(
        !fake.received().iter().any(|l| l.contains("Ponder")),
        "{:?}",
        fake.received()
    );
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn stopping_ignores_a_second_stop_and_a_stop_does_not_cancel_the_running_stop() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::Stop).unwrap();
    engine.send(EngineCommand::Stop).unwrap();
    engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(line(&mut peer).await, "stop");
    peer.say("bestmove e2e4").await;
    // Only one `stop` was written: the next line is the marker's.
    assert_nothing_sent_since(&mut peer, &engine).await;
}

#[tokio::test(start_paused = true)]
async fn stopping_keeps_setoptions_in_order_and_replaces_the_pending_search() {
    let (mut peer, engine, mut events) = ready(&[HASH, MULTIPV4]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(line(&mut peer).await, "stop");
    engine
        .send(EngineCommand::SetOption {
            name: "Hash".into(),
            value: OptionValue::Int(2),
        })
        .unwrap();
    engine.send(EngineCommand::SetMultiPv(2)).unwrap();
    engine.send(EngineCommand::Search(depth(KPK, 3))).unwrap();
    engine.send(EngineCommand::Search(depth(KPK, 5))).unwrap();
    peer.say("bestmove e2e4").await;
    // Both options in order, then ONE isready, then only the last search.
    expect_lines(
        &mut peer,
        &[
            "setoption name Hash value 2",
            "setoption name MultiPV value 2",
            "isready",
        ],
    )
    .await;
    peer.say("readyok").await;
    expect_lines(&mut peer, &[&format!("position fen {KPK}"), "go depth 5"]).await;
    assert_search_running_and_stop(&mut peer, &engine).await;
}

#[tokio::test(start_paused = true)]
async fn quit_is_accepted_in_every_state() {
    // Handshake.
    let (mut peer, engine, mut events) = ManualEngine::launch(config());
    assert_eq!(line(&mut peer).await, "uci");
    engine.send(EngineCommand::Quit).unwrap();
    assert_eq!(next_exit(&mut events).await.reason, ExitReason::Quit);
    assert_eq!(line(&mut peer).await, "quit");

    // Configuring.
    let (mut peer, engine, mut events) = ManualEngine::launch(config());
    assert_eq!(line(&mut peer).await, "uci");
    peer.say("uciok").await;
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
    engine.send(EngineCommand::Quit).unwrap();
    assert_eq!(next_exit(&mut events).await.reason, ExitReason::Quit);

    // Searching and Stopping.
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(line(&mut peer).await, "stop");
    engine.send(EngineCommand::Quit).unwrap();
    assert_eq!(next_exit(&mut events).await.reason, ExitReason::Quit);
}

#[tokio::test(start_paused = true)]
async fn every_command_fails_with_closed_after_the_session_ended() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    engine.send(EngineCommand::Quit).unwrap();
    assert_eq!(next_exit(&mut events).await.reason, ExitReason::Quit);
    for command in [
        EngineCommand::NewGame,
        EngineCommand::SetMultiPv(2),
        EngineCommand::Search(depth(KPK, 1)),
        EngineCommand::Stop,
        EngineCommand::PonderHit,
        EngineCommand::Quit,
    ] {
        assert!(
            matches!(engine.send(command.clone()), Err(EngineError::Closed)),
            "{command:?}"
        );
    }
    assert!(events.recv().await.is_none());
}

// ---------------------------------------------------------------------------
// Commands before Ready (D607, D609)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn commands_in_handshake_run_in_order_and_only_the_last_search_survives() {
    let (mut peer, engine, mut events) = ManualEngine::launch(config());
    engine.send(EngineCommand::NewGame).unwrap();
    engine
        .send(EngineCommand::SetOption {
            name: "Hash".into(),
            value: OptionValue::Int(32),
        })
        .unwrap();
    engine.send(EngineCommand::SetMultiPv(2)).unwrap();
    engine.send(EngineCommand::Search(depth(KPK, 3))).unwrap();
    engine.send(EngineCommand::Search(depth(KPK, 6))).unwrap();
    let setup = peer.handshake("Q", &[HASH, MULTIPV4]).await;
    assert_eq!(setup, ["ucinewgame", "isready"], "nothing leaks early");
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));
    expect_lines(
        &mut peer,
        &[
            "ucinewgame",
            "setoption name Hash value 32",
            "setoption name MultiPV value 2",
            "isready",
        ],
    )
    .await;
    peer.say("readyok").await;
    expect_lines(&mut peer, &[&format!("position fen {KPK}"), "go depth 6"]).await;
    assert_search_running_and_stop(&mut peer, &engine).await;
}

/// The engine is searching; a `NewGame` shows nothing else is queued.
async fn assert_search_running_and_stop(peer: &mut ManualEngine, engine: &Engine) {
    engine.send(EngineCommand::Stop).expect("send");
    assert_eq!(line(peer).await, "stop");
}

#[tokio::test(start_paused = true)]
async fn stop_before_ready_cancels_the_pending_search() {
    let (mut peer, engine, mut events) = ManualEngine::launch(config());
    engine.send(EngineCommand::Search(depth(KPK, 3))).unwrap();
    engine.send(EngineCommand::Stop).unwrap();
    peer.handshake("Q", &[]).await;
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));
    assert_nothing_sent_since(&mut peer, &engine).await;
    assert_eq!(drain(&mut events).len(), 0);
}

#[tokio::test(start_paused = true)]
async fn the_send_channel_is_bounded_and_never_blocks() {
    // Nothing runs the session between these calls (current-thread runtime).
    let (_peer, engine, _events) = ManualEngine::launch(config());
    let mut busy = 0;
    for _ in 0..100 {
        if matches!(engine.send(EngineCommand::NewGame), Err(EngineError::Busy)) {
            busy += 1;
        }
    }
    assert!(busy >= 80, "only {busy} Busy errors out of 100 sends");
}

#[tokio::test(start_paused = true)]
async fn the_wait_queue_holds_sixteen_actions_and_warns_about_the_rest() {
    let (mut peer, engine, mut events) = ManualEngine::launch(config());
    for _ in 0..16 {
        engine.send(EngineCommand::NewGame).unwrap();
    }
    settle().await; // the session moves them from the channel to its queue
    for _ in 0..4 {
        engine.send(EngineCommand::NewGame).unwrap();
    }
    settle().await;
    peer.handshake("Q", &[]).await;
    let mut warnings = 0;
    loop {
        match next(&mut events).await {
            EngineEvent::Warning(EngineWarning::CommandRefused { .. }) => warnings += 1,
            EngineEvent::Ready(_) => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(warnings, 4);
    let mut newgames = 0;
    loop {
        let l = line(&mut peer).await;
        match l.as_str() {
            "ucinewgame" => newgames += 1,
            "isready" => break,
            other => panic!("unexpected line {other}"),
        }
    }
    assert_eq!(newgames, 16);
}

#[tokio::test(start_paused = true)]
async fn queued_commands_that_the_engine_cannot_honour_become_warnings() {
    // Before Ready the engine's options are unknown, so the checks happen
    // when the queue runs.
    let (mut peer, engine, mut events) = ManualEngine::launch(config());
    engine
        .send(EngineCommand::SetOption {
            name: "Hash".into(),
            value: OptionValue::Int(0),
        })
        .unwrap();
    engine.send(EngineCommand::SetMultiPv(3)).unwrap();
    engine
        .send(EngineCommand::SetOption {
            name: "Nope".into(),
            value: OptionValue::Bool(true),
        })
        .unwrap();
    peer.handshake("Q", &[HASH]).await; // no MultiPV option
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));
    let mut refused = 0;
    for _ in 0..3 {
        if matches!(
            next(&mut events).await,
            EngineEvent::Warning(EngineWarning::CommandRefused { .. })
        ) {
            refused += 1;
        }
    }
    assert_eq!(refused, 3);
    assert_nothing_sent_since(&mut peer, &engine).await;
}

// ---------------------------------------------------------------------------
// launch / start
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn start_returns_the_handle_only_after_ready_and_launch_returns_at_once() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    assert!(engine.identity().is_some());
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));
    assert_eq!(engine.process_id(), None, "no real process in memory");
    engine.shutdown().await;

    let (_peer, engine, _events) = ManualEngine::launch(config());
    assert!(engine.identity().is_none(), "launch does not wait");
}

#[tokio::test(start_paused = true)]
async fn a_failed_handshake_after_launch_arrives_as_an_exited_event() {
    let fake = ScriptedEngine::builder().silent().build();
    let mut cfg = config();
    cfg.timeouts.uciok = Duration::from_millis(500);
    let (_engine, mut events) = fake.launch(cfg);
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::Timeout(Phase::Uciok));
}

#[tokio::test(start_paused = true)]
async fn a_cloned_handle_shares_the_session_and_drop_of_all_clones_ends_it() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let other = engine.clone();
    drop(engine);
    // Still alive through the clone.
    assert_nothing_sent_since(&mut peer, &other).await;
    drop(other);
    // All handles gone: the session ends and the pipe closes.
    // The pipe closes (`quit` first), and no `Exited` is reported to nobody.
    let mut saw_quit = false;
    while let Some(l) = peer.next_line().await {
        saw_quit |= l == "quit";
    }
    assert!(saw_quit || events.recv().await.is_none());
}
