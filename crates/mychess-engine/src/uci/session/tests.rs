//! Sanity tests of the session over in-memory pipes. The full contract suite
//! lives in `tests/`.

use std::time::Duration;

use mychess_core::Position;

use crate::engine::{
    BestMove, COMMAND_QUEUE, Engine, EngineCommand, EngineConfig, EngineError, EngineEvent,
    EngineEvents, EngineWarning, ExitReason, Phase,
};
use crate::request::{SearchLimits, SearchPosition, SearchRequest};
use crate::testing::{ManualEngine, ScriptedEngine};

async fn next(events: &mut EngineEvents) -> EngineEvent {
    tokio::time::timeout(Duration::from_secs(60), events.recv())
        .await
        .expect("event in time")
        .expect("session alive")
}

/// Reads events until one satisfies `pick`.
async fn until<T>(events: &mut EngineEvents, mut pick: impl FnMut(EngineEvent) -> Option<T>) -> T {
    loop {
        if let Some(found) = pick(next(events).await) {
            return found;
        }
    }
}

fn depth_request(depth: u32) -> SearchRequest {
    SearchRequest {
        limits: SearchLimits {
            depth: Some(depth),
            ..SearchLimits::default()
        },
        ..SearchRequest::infinite(SearchPosition::new(Position::new()))
    }
}

fn search_on(fen: &str) -> SearchRequest {
    SearchRequest::infinite(SearchPosition::new(Position::from_fen(fen).unwrap()))
}

#[tokio::test(start_paused = true)]
async fn handshake_reports_identity_and_capabilities() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(EngineConfig::new("fake")).await.unwrap();
    let EngineEvent::Ready(identity) = next(&mut events).await else {
        panic!("first event must be Ready");
    };
    assert_eq!(identity.name, "FakeFish");
    assert_eq!(identity.capabilities.multipv, Some(1..=255));
    assert!(identity.capabilities.chess960 && identity.capabilities.wdl);
    assert!(identity.options.iter().any(|o| o.name == "Skill Level"));
    assert!(identity.options.iter().any(|o| o.name == "Clear Hash"));
    assert_eq!(engine.identity(), Some(&identity));
    // `uci`, then `ucinewgame` + one `isready`; analyse mode switched on.
    assert_eq!(
        fake.received(),
        [
            "uci",
            "setoption name UCI_AnalyseMode value true",
            "ucinewgame",
            "isready"
        ]
    );
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn saved_options_are_sent_before_one_isready_and_managed_ones_are_skipped() {
    use crate::option::OptionValue;
    let fake = ScriptedEngine::builder().build();
    let mut config = EngineConfig::new("fake");
    config.options = vec![
        ("Hash".into(), OptionValue::Int(64)),
        ("Clear Hash".into(), OptionValue::Bool(true)),
        ("UCI_Chess960".into(), OptionValue::Bool(true)),
        ("Hash2".into(), OptionValue::Int(1)),
        ("Skill Level".into(), OptionValue::Int(99)),
    ];
    let (engine, mut events) = fake.start(config).await.unwrap();
    let received = fake.received();
    assert_eq!(received[1], "setoption name Hash value 64");
    assert_eq!(received[2], "setoption name Clear Hash");
    assert_eq!(received.iter().filter(|l| *l == "isready").count(), 1);
    assert!(!received.iter().any(|l| l.contains("UCI_Chess960")));
    let mut warnings = 0;
    while let Some(event) = events.try_recv() {
        warnings += usize::from(matches!(event, EngineEvent::Warning(_)));
    }
    assert_eq!(warnings, 2, "unknown option and out-of-range value");
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn depth_search_sends_position_and_go_and_returns_a_legal_bestmove() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(EngineConfig::new("fake")).await.unwrap();
    engine
        .send(EngineCommand::Search(depth_request(3)))
        .unwrap();
    let id = until(&mut events, |e| match e {
        EngineEvent::SearchStarted(id) => Some(id),
        _ => None,
    })
    .await;
    let best = until(&mut events, |e| match e {
        EngineEvent::BestMove(b) => Some(b),
        _ => None,
    })
    .await;
    assert_eq!(best.search, id);
    assert!(best.mv.is_some());
    let received = fake.received();
    assert!(received.contains(&"position startpos".to_owned()));
    assert!(received.contains(&"go depth 3".to_owned()));
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn infos_are_white_point_of_view_and_stop_delivers_the_bestmove() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(EngineConfig::new("fake")).await.unwrap();
    // Black to move, Black can win a queen: the engine says +900, White sees -900.
    let request = search_on("4k3/8/8/8/8/q7/8/Q3K3 b - - 0 1");
    engine.send(EngineCommand::Search(request)).unwrap();
    let info = until(&mut events, |e| match e {
        EngineEvent::Info(i) if i.score.is_some() && !i.pv.is_empty() => Some(i),
        _ => None,
    })
    .await;
    let (score, _) = info.score.unwrap();
    assert!(score.as_centipawns() <= 0, "{score:?}");
    engine.send(EngineCommand::Stop).unwrap();
    let best = until(&mut events, |e| match e {
        EngineEvent::BestMove(b) => Some(b),
        _ => None,
    })
    .await;
    assert_eq!(best.search, info.search);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn terminal_position_answers_without_talking_to_the_engine() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(EngineConfig::new("fake")).await.unwrap();
    let before = fake.received().len();
    // Fool's mate: White is checkmated.
    let request = search_on("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
    engine.send(EngineCommand::Search(request)).unwrap();
    let best = until(&mut events, |e| match e {
        EngineEvent::BestMove(b) => Some(b),
        _ => None,
    })
    .await;
    assert_eq!(best.mv, None);
    assert_eq!(fake.received().len(), before);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn searches_requested_while_stopping_collapse_into_the_last_one() {
    let (mut peer, engine, mut events) = ManualEngine::launch(EngineConfig::new("manual"));
    let setup = peer.handshake("Manual", &[]).await;
    assert!(setup.contains(&"isready".to_owned()));
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));

    let a = search_on("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1");
    let b = search_on("4k3/8/8/8/8/8/3P4/4K3 w - - 0 1");
    let c = search_on("4k3/8/8/8/8/8/2P5/4K3 w - - 0 1");
    engine.send(EngineCommand::Search(a)).unwrap();
    assert_eq!(
        peer.next_line().await.unwrap(),
        "position fen 4k3/8/8/8/8/8/4P3/4K3 w - - 0 1"
    );
    assert_eq!(peer.next_line().await.unwrap(), "go infinite");
    engine.send(EngineCommand::Search(b)).unwrap();
    assert_eq!(peer.next_line().await.unwrap(), "stop");
    engine.send(EngineCommand::Search(c)).unwrap();
    // Stale report of the old search, then its bestmove.
    peer.say("info depth 5 score cp 10 pv e2e4").await;
    peer.say("bestmove e2e4").await;
    assert_eq!(
        peer.next_line().await.unwrap(),
        "position fen 4k3/8/8/8/8/8/2P5/4K3 w - - 0 1"
    );
    assert_eq!(peer.next_line().await.unwrap(), "go infinite");
    peer.say("info depth 7 score cp 20 pv c2c4").await;
    peer.say("bestmove c2c4").await;

    let mut seen = Vec::new();
    while seen
        .iter()
        .filter(|e| matches!(e, EngineEvent::BestMove(_)))
        .count()
        < 2
    {
        seen.push(next(&mut events).await);
    }
    let infos: Vec<_> = seen
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Info(i) => Some(i.search),
            _ => None,
        })
        .collect();
    assert_eq!(infos.len(), 1, "the stale info is dropped: {seen:?}");
    assert_eq!(infos[0].0, 2);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn only_the_request_that_starts_reports_its_tag_right_before_started() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(EngineConfig::new("fake")).await.unwrap();
    let mut tagged = depth_request(2);
    tagged.tag = 7;
    engine.send(EngineCommand::Search(tagged)).unwrap();
    let mut order = Vec::new();
    while !order.iter().any(|e| matches!(e, EngineEvent::BestMove(_))) {
        order.push(next(&mut events).await);
    }
    let at = order
        .iter()
        .position(|e| matches!(e, EngineEvent::SearchTagged { .. }))
        .expect("the tag is reported");
    assert!(matches!(
        (&order[at], &order[at + 1]),
        (EngineEvent::SearchTagged { search, tag: 7 }, EngineEvent::SearchStarted(id))
            if search == id
    ));
    // Without a tag nothing is added.
    engine
        .send(EngineCommand::Search(depth_request(2)))
        .unwrap();
    let mut untagged = Vec::new();
    while !untagged
        .iter()
        .any(|e| matches!(e, EngineEvent::BestMove(_)))
    {
        untagged.push(next(&mut events).await);
    }
    assert!(
        !untagged
            .iter()
            .any(|e| matches!(e, EngineEvent::SearchTagged { .. }))
    );
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn a_refusal_for_a_full_queue_names_the_real_command() {
    // Before the handshake ends nothing runs, so the commands pile up.
    let (_peer, engine, mut events) = ManualEngine::launch(EngineConfig::new("manual"));
    for _ in 0..=COMMAND_QUEUE {
        engine.send(EngineCommand::NewGame).unwrap();
        tokio::task::yield_now().await;
    }
    let warning = until(&mut events, |e| match e {
        EngineEvent::Warning(w) => Some(w),
        _ => None,
    })
    .await;
    assert!(
        matches!(
            &warning,
            EngineWarning::CommandRefused {
                command: "ucinewgame",
                ..
            }
        ),
        "{warning:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_bestmove_without_a_move_ends_the_search_with_a_warning() {
    // D411: `bestmove` alone is a violation, but the search must still end.
    let (mut peer, engine, mut events) = ManualEngine::launch(EngineConfig::new("manual"));
    peer.handshake("Manual", &[]).await;
    engine
        .send(EngineCommand::Search(depth_request(5)))
        .unwrap();
    assert_eq!(peer.next_line().await.unwrap(), "position startpos");
    assert_eq!(peer.next_line().await.unwrap(), "go depth 5");
    peer.say("bestmove").await;
    let mut seen = Vec::new();
    let best = loop {
        match next(&mut events).await {
            EngineEvent::BestMove(b) => break b,
            other => seen.push(other),
        }
    };
    assert_eq!(best.mv, None);
    assert!(
        seen.iter()
            .any(|e| matches!(e, EngineEvent::Warning(EngineWarning::Misbehaved(_)))),
        "{seen:?}"
    );
    // The session goes on: a new search works.
    engine
        .send(EngineCommand::Search(depth_request(1)))
        .unwrap();
    assert_eq!(peer.next_line().await.unwrap(), "position startpos");
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn missing_bestmove_after_stop_ends_the_old_search_and_the_session() {
    let fake = ScriptedEngine::builder().no_bestmove_after_stop().build();
    let (engine, mut events) = fake.start(EngineConfig::new("fake")).await.unwrap();
    engine
        .send(EngineCommand::Search(search_on(
            "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1",
        )))
        .unwrap();
    until(&mut events, |e| {
        matches!(e, EngineEvent::Info(_)).then_some(())
    })
    .await;
    engine.send(EngineCommand::Stop).unwrap();
    let best = until(&mut events, |e| match e {
        EngineEvent::BestMove(b) => Some(b),
        _ => None,
    })
    .await;
    assert_eq!(best.mv, None);
    // The engine answered `isready` but never `stop`: a late `bestmove` could
    // be taken for the next search's, so the session closes (spec m4-1, D619).
    let exit = until(&mut events, |e| match e {
        EngineEvent::Exited(info) => Some(info),
        _ => None,
    })
    .await;
    assert_eq!(exit.reason, ExitReason::StopTimeout);
    assert!(
        engine
            .send(EngineCommand::Search(depth_request(1)))
            .is_err()
    );
}

#[tokio::test(start_paused = true)]
async fn engine_that_answers_nothing_after_stop_is_ended() {
    let (mut peer, engine, mut events) = ManualEngine::launch(EngineConfig::new("manual"));
    peer.handshake("Manual", &[]).await;
    engine
        .send(EngineCommand::Search(depth_request(5)))
        .unwrap();
    assert_eq!(peer.next_line().await.unwrap(), "position startpos");
    assert_eq!(peer.next_line().await.unwrap(), "go depth 5");
    engine.send(EngineCommand::Stop).unwrap();
    // D619: even without any `readyok`, the search ends in a `BestMove`
    // (after a warning) before the exit notice.
    let mut seen = Vec::new();
    let exit = loop {
        match events.recv().await.expect("the session ends with Exited") {
            EngineEvent::Exited(x) => break x,
            other => seen.push(other),
        }
    };
    assert!(
        matches!(
            seen.as_slice(),
            [
                EngineEvent::Ready(_),
                EngineEvent::SearchStarted(_),
                EngineEvent::Warning(EngineWarning::Misbehaved(_)),
                EngineEvent::BestMove(BestMove { mv: None, .. })
            ]
        ),
        "{seen:?}"
    );
    assert_eq!(exit.reason, ExitReason::StopTimeout);
    assert!(matches!(
        engine.send(EngineCommand::Stop),
        Err(EngineError::Closed)
    ));
}

#[tokio::test(start_paused = true)]
async fn silent_engine_times_out_at_uciok() {
    let fake = ScriptedEngine::builder().silent().build();
    let mut config = EngineConfig::new("fake");
    config.timeouts.uciok = Duration::from_millis(300);
    let result = fake.start(config).await;
    assert!(
        matches!(result, Err(EngineError::Timeout(Phase::Uciok))),
        "{result:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn engine_that_dies_during_the_handshake_is_reported() {
    let (mut peer, engine, mut events) = ManualEngine::launch(EngineConfig::new("manual"));
    assert_eq!(peer.next_line().await.unwrap(), "uci");
    peer.close().await;
    let exit = until(&mut events, |e| match e {
        EngineEvent::Exited(x) => Some(x),
        _ => None,
    })
    .await;
    assert_eq!(exit.reason, ExitReason::Crashed);
    drop(engine);
}

#[tokio::test(start_paused = true)]
async fn chess960_switches_the_option_and_sends_king_takes_rook() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(EngineConfig::new("fake")).await.unwrap();
    let start = Position::from_fen("4k3/8/8/8/8/8/8/1R1RK3 w D - 0 1").unwrap();
    let castle = start.parse_uci("e1d1").unwrap();
    let request = SearchRequest {
        limits: SearchLimits {
            depth: Some(1),
            ..SearchLimits::default()
        },
        ..SearchRequest::infinite(SearchPosition {
            start,
            moves: vec![castle],
        })
    };
    engine.send(EngineCommand::Search(request)).unwrap();
    until(&mut events, |e| {
        matches!(e, EngineEvent::BestMove(_)).then_some(())
    })
    .await;
    let received = fake.received();
    let switch = received
        .iter()
        .position(|l| l == "setoption name UCI_Chess960 value true")
        .expect("option switched on");
    let position = received
        .iter()
        .position(|l| l.starts_with("position fen") && l.ends_with("moves e1d1"))
        .expect("castling as king takes rook");
    assert!(switch < position);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn chess960_is_refused_by_an_engine_without_the_option() {
    let fake = ScriptedEngine::builder().chess960(false).build();
    let (engine, _events) = fake.start(EngineConfig::new("fake")).await.unwrap();
    let start = Position::from_fen("4k3/8/8/8/8/8/8/1R1RK3 w D - 0 1").unwrap();
    let result = engine.send(EngineCommand::Search(SearchRequest::infinite(
        SearchPosition::new(start),
    )));
    assert!(matches!(result, Err(EngineError::UnsupportedVariant)));
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn multipv_option_and_validation_in_send() {
    use crate::option::OptionValue;
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(EngineConfig::new("fake")).await.unwrap();
    assert!(matches!(
        engine.send(EngineCommand::SetOption {
            name: "Hash".into(),
            value: OptionValue::Int(0)
        }),
        Err(EngineError::InvalidOption(_))
    ));
    assert!(matches!(
        engine.send(EngineCommand::SetOption {
            name: "Ponder".into(),
            value: OptionValue::Bool(true)
        }),
        Err(EngineError::InvalidOption(_))
    ));
    engine.send(EngineCommand::SetMultiPv(3)).unwrap();
    engine
        .send(EngineCommand::Search(depth_request(2)))
        .unwrap();
    let mut lines = std::collections::BTreeSet::new();
    loop {
        match next(&mut events).await {
            EngineEvent::Info(i) => {
                lines.insert(i.multipv);
            }
            EngineEvent::BestMove(_) => break,
            _ => {}
        }
    }
    assert_eq!(lines.into_iter().collect::<Vec<_>>(), [1, 2, 3]);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn hostile_output_does_not_break_the_session() {
    let fake = ScriptedEngine::builder().garbage().build();
    let (engine, mut events) = fake.start(EngineConfig::new("fake")).await.unwrap();
    engine
        .send(EngineCommand::Search(depth_request(3)))
        .unwrap();
    until(&mut events, |e| {
        matches!(e, EngineEvent::BestMove(_)).then_some(())
    })
    .await;
    assert!(engine.log().len() > 10);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn commands_during_the_handshake_wait_for_ready() {
    let (mut peer, engine, mut events) = ManualEngine::launch(EngineConfig::new("manual"));
    engine.send(EngineCommand::SetMultiPv(2)).unwrap();
    engine
        .send(EngineCommand::Search(depth_request(1)))
        .unwrap();
    // Nothing but the handshake reaches the engine yet.
    let setup = peer
        .handshake(
            "Manual",
            &["option name MultiPV type spin default 1 min 1 max 4"],
        )
        .await;
    assert!(
        !setup
            .iter()
            .any(|l| l.starts_with("go") || l.contains("MultiPV"))
    );
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));
    assert_eq!(
        peer.next_line().await.unwrap(),
        "setoption name MultiPV value 2"
    );
    assert_eq!(peer.next_line().await.unwrap(), "isready");
    peer.say("readyok").await;
    assert_eq!(peer.next_line().await.unwrap(), "position startpos");
    assert_eq!(peer.next_line().await.unwrap(), "go depth 1");
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn many_info_lines_are_coalesced_and_the_bestmove_is_not_lost() {
    let (mut peer, engine, mut events) = ManualEngine::launch(EngineConfig::new("manual"));
    peer.handshake("Manual", &[]).await;
    engine
        .send(EngineCommand::Search(depth_request(9)))
        .unwrap();
    peer.next_line().await;
    peer.next_line().await;
    for depth in 1..=1000 {
        peer.say(&format!("info depth {depth} score cp 5 pv e2e4"))
            .await;
    }
    peer.say("bestmove e2e4").await;
    let mut infos = Vec::new();
    loop {
        match next(&mut events).await {
            EngineEvent::Info(i) => infos.push(i),
            EngineEvent::BestMove(_) => break,
            _ => {}
        }
    }
    assert!(
        !infos.is_empty() && infos.len() < 50,
        "{} infos",
        infos.len()
    );
    assert_eq!(
        infos.last().unwrap().depth,
        Some(1000),
        "the newest report is flushed before bestmove"
    );
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn quit_ignored_by_the_engine_still_ends_the_session() {
    let fake = ScriptedEngine::builder().ignore_quit().build();
    let (engine, mut events) = fake.start(EngineConfig::new("fake")).await.unwrap();
    engine.send(EngineCommand::Quit).unwrap();
    let exit = until(&mut events, |e| match e {
        EngineEvent::Exited(x) => Some(x),
        _ => None,
    })
    .await;
    assert_eq!(exit.reason, ExitReason::Quit);
}

#[tokio::test(start_paused = true)]
async fn start_fails_for_a_missing_executable() {
    let dir = tempfile::tempdir().unwrap();
    let result = Engine::start(EngineConfig::new(dir.path().join("no-such-engine"))).await;
    assert!(
        matches!(result, Err(EngineError::Spawn { .. })),
        "{result:?}"
    );
}
