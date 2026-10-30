//! M4-1 contract tests: `position`/`go` commands, search ids, stale events,
//! stop recovery (D407, D610), `bestmove` validation (D411), score point of
//! view (D405, D611), MultiPV (D618) and Chess960 (D413, D617).

#[path = "m4_1_common/helpers.rs"]
mod common;

use std::fmt::Write as _;
use std::time::Duration;

use common::*;
use mychess_core::Position;
use mychess_engine::testing::{ManualEngine, ScriptedEngine};
use mychess_engine::{
    Bound, Clock, EngineCommand, EngineError, EngineEvent, EngineWarning, ExitReason, OptionValue,
    Score, SearchLimits, SearchPosition, SearchRequest, Wdl,
};
use tokio::time::Instant;

// ---------------------------------------------------------------------------
// position / go
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn the_standard_start_position_is_sent_as_startpos_with_moves() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let start = Position::new();
    let e4 = mv(&start, "e2e4");
    let after = start.after(e4).unwrap();
    let e5 = mv(&after, "e7e5");
    let nf3 = mv(&after.after(e5).unwrap(), "g1f3");
    let request = SearchRequest::infinite(SearchPosition {
        start,
        moves: vec![e4, e5, nf3],
    });
    let (_, position, go) = start_search(&mut peer, &engine, &mut events, request).await;
    assert_eq!(position, "position startpos moves e2e4 e7e5 g1f3");
    assert_eq!(go, "go infinite");
}

#[tokio::test(start_paused = true)]
async fn another_start_is_sent_as_a_fen_and_castling_and_promotion_keep_their_notation() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let fen = "r3k2r/8/8/8/8/8/p7/RN2K2R w KQkq - 0 1";
    let start = pos(fen);
    let castle = mv(&start, "e1g1");
    let after = start.after(castle).unwrap();
    let promo_black = mv(&after, "a2b1q");
    let request = SearchRequest::infinite(SearchPosition {
        start,
        moves: vec![castle, promo_black],
    });
    let (_, position, _) = start_search(&mut peer, &engine, &mut events, request).await;
    assert_eq!(position, format!("position fen {fen} moves e1g1 a2b1q"));
}

#[tokio::test(start_paused = true)]
async fn the_start_position_with_other_counters_is_sent_as_a_fen() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 3 17";
    let (_, position, _) = start_search(&mut peer, &engine, &mut events, infinite(fen)).await;
    assert_eq!(position, format!("position fen {fen}"));
}

#[tokio::test(start_paused = true)]
async fn a_null_move_in_the_path_sends_the_current_fen_without_moves() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let start = Position::new();
    let e4 = mv(&start, "e2e4");
    let request = SearchRequest::infinite(SearchPosition {
        start: start.clone(),
        moves: vec![e4, mychess_core::Move::Null],
    });
    let (_, position, _) = start_search(&mut peer, &engine, &mut events, request).await;
    let mut expected = start.after(e4).unwrap();
    expected.play(mychess_core::Move::Null).unwrap();
    assert!(position.starts_with("position fen "), "{position}");
    assert!(!position.contains(" moves "), "{position}");
    assert_eq!(position, format!("position fen {}", expected.fen()));
}

#[tokio::test(start_paused = true)]
async fn each_limit_has_its_own_go_form() {
    let secs = Duration::from_secs;
    let ms = Duration::from_millis;
    let cases: Vec<(SearchLimits, &str)> = vec![
        (SearchLimits::default(), "go infinite"),
        (
            SearchLimits {
                infinite: true,
                ..SearchLimits::default()
            },
            "go infinite",
        ),
        (
            SearchLimits {
                depth: Some(12),
                ..SearchLimits::default()
            },
            "go depth 12",
        ),
        (
            SearchLimits {
                nodes: Some(u64::MAX),
                ..SearchLimits::default()
            },
            "go nodes 18446744073709551615",
        ),
        (
            SearchLimits {
                movetime: Some(ms(1500)),
                ..SearchLimits::default()
            },
            "go movetime 1500",
        ),
        (
            SearchLimits {
                mate: Some(3),
                ..SearchLimits::default()
            },
            "go mate 3",
        ),
        (
            SearchLimits {
                clock: Some(Clock {
                    white_time: secs(300),
                    black_time: secs(299),
                    white_increment: secs(2),
                    black_increment: ms(500),
                    moves_to_go: None,
                }),
                ..SearchLimits::default()
            },
            "go wtime 300000 btime 299000 winc 2000 binc 500",
        ),
        (
            SearchLimits {
                clock: Some(Clock {
                    white_time: ms(0),
                    black_time: secs(60),
                    white_increment: ms(0),
                    black_increment: ms(0),
                    moves_to_go: Some(40),
                }),
                ..SearchLimits::default()
            },
            "go wtime 0 btime 60000 winc 0 binc 0 movestogo 40",
        ),
    ];
    for (limits_under_test, expected) in cases {
        let (mut peer, engine, mut events) = ready(&[]).await;
        let (_, _, go) = start_search(
            &mut peer,
            &engine,
            &mut events,
            limits(KPK, limits_under_test.clone()),
        )
        .await;
        assert_eq!(go, expected, "{limits_under_test:?}");
    }
}

#[tokio::test(start_paused = true)]
async fn searchmoves_goes_last_and_an_empty_list_means_no_restriction() {
    let start = pos(KPK);
    let e3 = mv(&start, "e2e3");
    let e4 = mv(&start, "e2e4");
    for (limit, expected) in [
        (
            SearchLimits {
                depth: Some(6),
                ..SearchLimits::default()
            },
            "go depth 6 searchmoves e2e3 e2e4",
        ),
        (SearchLimits::default(), "go infinite searchmoves e2e3 e2e4"),
        (
            SearchLimits {
                movetime: Some(Duration::from_millis(100)),
                ..SearchLimits::default()
            },
            "go movetime 100 searchmoves e2e3 e2e4",
        ),
    ] {
        let (mut peer, engine, mut events) = ready(&[]).await;
        let mut request = limits(KPK, limit);
        request.search_moves = vec![e3, e4];
        let (_, _, go) = start_search(&mut peer, &engine, &mut events, request).await;
        assert_eq!(go, expected);
    }
    let (mut peer, engine, mut events) = ready(&[]).await;
    let (_, _, go) = start_search(&mut peer, &engine, &mut events, depth(KPK, 6)).await;
    assert!(!go.contains("searchmoves"), "{go}");
}

#[tokio::test(start_paused = true)]
async fn go_ponder_comes_before_the_limits() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let mut request = depth(KPK, 5);
    request.ponder = true;
    let (_, _, go) = start_search(&mut peer, &engine, &mut events, request).await;
    assert_eq!(go, "go ponder depth 5");
}

#[tokio::test(start_paused = true)]
async fn search_ids_grow_and_every_event_carries_the_id_of_its_search() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));
    let mut ids = Vec::new();
    for n in 1..=3 {
        engine.send(EngineCommand::Search(depth(KPK, n))).unwrap();
        let seen = until_best(&mut events).await;
        let started = seen
            .iter()
            .find_map(|e| match e {
                EngineEvent::SearchStarted(id) => Some(*id),
                _ => None,
            })
            .expect("SearchStarted");
        for e in &seen {
            match e {
                EngineEvent::Info(i) => assert_eq!(i.search, started),
                EngineEvent::BestMove(b) => assert_eq!(b.search, started),
                _ => {}
            }
        }
        ids.push(started);
    }
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn search_started_comes_before_any_info_of_that_search() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    engine.send(EngineCommand::Search(depth(KPK, 3))).unwrap();
    let seen = until_best(&mut events).await;
    let started = seen
        .iter()
        .position(|e| matches!(e, EngineEvent::SearchStarted(_)))
        .unwrap();
    let first_info = seen.iter().position(|e| matches!(e, EngineEvent::Info(_)));
    assert!(first_info.is_none_or(|i| started < i), "{seen:?}");
    engine.shutdown().await;
}

// ---------------------------------------------------------------------------
// stale reports (D407, D610, D619)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn the_old_bestmove_is_delivered_with_the_old_id_and_stale_infos_are_dropped() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let (a, _, _) = start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    peer.say("info depth 3 score cp 11 pv e2e4").await;
    let before_stop = next_info(&mut events).await;
    assert_eq!(before_stop.search, a);

    engine.send(EngineCommand::Search(depth(KPK, 2))).unwrap();
    assert_eq!(line(&mut peer).await, "stop");
    // Everything the old search says after `stop` is stale (D610).
    peer.say("info depth 4 score cp 22 pv e2e4").await;
    peer.say("info depth 5 score cp 33 pv e2e4").await;
    peer.say("bestmove e2e4").await;
    expect_lines(&mut peer, &[&format!("position fen {KPK}"), "go depth 2"]).await;
    let (b_started, b_infos) = {
        let mut seen = Vec::new();
        let old_best = next_best(&mut events).await;
        assert_eq!(old_best.search, a);
        peer.say("info depth 1 score cp 7 pv e2e4").await;
        peer.say("bestmove e2e3").await;
        loop {
            let e = next(&mut events).await;
            let done = matches!(e, EngineEvent::BestMove(_));
            seen.push(e);
            if done {
                break;
            }
        }
        let started = seen
            .iter()
            .find_map(|e| match e {
                EngineEvent::SearchStarted(id) => Some(*id),
                _ => None,
            })
            .unwrap();
        let infos: Vec<_> = seen
            .iter()
            .filter_map(|e| match e {
                EngineEvent::Info(i) => Some(i.clone()),
                _ => None,
            })
            .collect();
        (started, infos)
    };
    assert!(b_started > a);
    assert_eq!(b_infos.len(), 1);
    assert_eq!(b_infos[0].search, b_started);
    assert_eq!(b_infos[0].depth, Some(1));
}

#[tokio::test(start_paused = true)]
async fn infos_after_the_bestmove_or_without_a_search_are_ignored() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    peer.say("info depth 3 score cp 4 pv e2e4").await;
    peer.say("bestmove e2e4").await;
    start_search(&mut peer, &engine, &mut events, depth(KPK, 1)).await;
    peer.say("bestmove e2e4").await;
    let best = next_best(&mut events).await;
    assert_eq!(best.search.0, 1);
    peer.say("info depth 9 score cp 4 pv e2e4").await;
    peer.say("bestmove e2e4").await;
    assert_nothing_sent_since(&mut peer, &engine).await;
    // Neither the stray info nor the second bestmove surfaced.
    let rest = drain(&mut events);
    assert!(rest.is_empty(), "{rest:?}");
}

#[tokio::test(start_paused = true)]
async fn a_hanging_stop_is_recovered_through_isready_within_three_seconds() {
    let fake = ScriptedEngine::builder().hang_on_stop().build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    let a = until(&mut events, |e| match e {
        EngineEvent::SearchStarted(id) => Some(id),
        _ => None,
    })
    .await;
    next_info(&mut events).await;
    let began = Instant::now();
    engine.send(EngineCommand::Stop).unwrap();
    let seen = until_best(&mut events).await;
    assert!(
        began.elapsed() <= Duration::from_secs(3),
        "{:?}",
        began.elapsed()
    );
    let EngineEvent::BestMove(best) = seen.last().unwrap() else {
        unreachable!()
    };
    assert_eq!((best.search, best.mv), (a, None));
    assert!(
        seen.iter()
            .any(|e| matches!(e, EngineEvent::Warning(EngineWarning::Misbehaved(_)))),
        "{seen:?}"
    );
    let sent = fake.received();
    assert!(sent.iter().any(|l| l == "stop"));
    assert!(sent.iter().filter(|l| *l == "isready").count() >= 2);
}

#[tokio::test(start_paused = true)]
async fn a_missing_bestmove_after_stop_ends_the_old_search_and_closes_the_session() {
    let fake = ScriptedEngine::builder().no_bestmove_after_stop().build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    let a = until(&mut events, |e| match e {
        EngineEvent::SearchStarted(id) => Some(id),
        _ => None,
    })
    .await;
    next_info(&mut events).await;
    let began = Instant::now();
    engine.send(EngineCommand::Stop).unwrap();
    let best = next_best(&mut events).await;
    assert_eq!((best.search, best.mv), (a, None));
    assert!(began.elapsed() <= Duration::from_secs(3));
    // Spec m4-1, D619: a late `bestmove` of the old search would be read as the
    // next one's, so an engine that skipped it is given up after the recovery.
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::StopTimeout);
    assert!(engine.send(EngineCommand::Search(depth(KPK, 2))).is_err());
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_answers_neither_bestmove_nor_isready_is_ended_within_three_seconds() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let (a, _, _) = start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let began = Instant::now();
    engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(line(&mut peer).await, "stop");
    assert_eq!(line(&mut peer).await, "isready");
    // D619: the search ends in a `BestMove` even though no `readyok` came.
    let best = next_best(&mut events).await;
    assert_eq!((best.search, best.mv), (a, None));
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::StopTimeout);
    let waited = began.elapsed();
    assert!(
        waited >= Duration::from_millis(2_900) && waited <= Duration::from_millis(3_600),
        "{waited:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_stop_deadlines_come_from_the_configuration() {
    let mut cfg = config();
    cfg.timeouts.stop = Duration::from_millis(200);
    cfg.timeouts.stop_recovery = Duration::from_millis(300);
    let (mut peer, engine, mut events) = ready_with(cfg, &[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let began = Instant::now();
    engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(next_exit(&mut events).await.reason, ExitReason::StopTimeout);
    // 200 ms + 300 ms, then the quit wait before the exit is reported.
    let waited = began.elapsed();
    assert!(
        waited >= Duration::from_millis(500) && waited <= Duration::from_millis(1_100),
        "{waited:?}"
    );
}

// ---------------------------------------------------------------------------
// bestmove validation (D411)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn an_illegal_bestmove_becomes_a_warning_and_no_move() {
    for bad in ["e2e5", "e7e5", "a1a2", "e2e4q", "zzzz", "e2e", "e2e4e5"] {
        let (mut peer, engine, mut events) = ready(&[]).await;
        start_search(&mut peer, &engine, &mut events, depth(KPK, 1)).await;
        peer.say(&format!("bestmove {bad}")).await;
        let seen = until_best(&mut events).await;
        let EngineEvent::BestMove(best) = seen.last().unwrap() else {
            unreachable!()
        };
        assert_eq!(best.mv, None, "{bad}");
        assert!(
            seen.iter()
                .any(|e| matches!(e, EngineEvent::Warning(EngineWarning::IllegalBestMove(_)))),
            "{bad}: {seen:?}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn none_and_0000_are_no_move_without_a_warning() {
    for text in [
        "bestmove (none)",
        "bestmove 0000",
        "bestmove (none) ponder e7e5",
    ] {
        let (mut peer, engine, mut events) = ready(&[]).await;
        start_search(&mut peer, &engine, &mut events, depth(KPK, 1)).await;
        peer.say(text).await;
        let seen = until_best(&mut events).await;
        let EngineEvent::BestMove(best) = seen.last().unwrap() else {
            unreachable!()
        };
        assert_eq!((best.mv, best.ponder), (None, None), "{text}");
        assert!(
            !seen.iter().any(|e| matches!(e, EngineEvent::Warning(_))),
            "{text}: {seen:?}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn bestmove_with_a_legal_ponder_move_delivers_both_and_an_illegal_ponder_is_dropped() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let root = Position::new();
    start_search(&mut peer, &engine, &mut events, infinite(&root.fen())).await;
    peer.say("bestmove e2e4 ponder e7e5").await;
    let best = next_best(&mut events).await;
    let after = root.after(best.mv.unwrap()).unwrap();
    assert_eq!(best.mv.map(|m| root.uci(m)).as_deref(), Some("e2e4"));
    assert_eq!(best.ponder.map(|m| after.uci(m)).as_deref(), Some("e7e5"));

    start_search(&mut peer, &engine, &mut events, infinite(&root.fen())).await;
    peer.say("bestmove e2e4 ponder e2e4").await;
    let best = next_best(&mut events).await;
    assert!(best.mv.is_some());
    assert_eq!(best.ponder, None);
}

#[tokio::test(start_paused = true)]
async fn a_bestmove_for_a_promotion_and_for_an_en_passant_capture_is_understood() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(
        &mut peer,
        &engine,
        &mut events,
        infinite("4k3/P7/8/8/8/8/8/4K3 w - - 0 1"),
    )
    .await;
    peer.say("bestmove a7a8q").await;
    assert!(next_best(&mut events).await.mv.is_some());
    start_search(
        &mut peer,
        &engine,
        &mut events,
        infinite("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1"),
    )
    .await;
    peer.say("bestmove e5d6").await;
    assert!(next_best(&mut events).await.mv.is_some());
}

#[tokio::test(start_paused = true)]
async fn a_promotion_without_the_piece_letter_is_an_illegal_bestmove() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(
        &mut peer,
        &engine,
        &mut events,
        infinite("4k3/P7/8/8/8/8/8/4K3 w - - 0 1"),
    )
    .await;
    peer.say("bestmove a7a8").await;
    let seen = until_best(&mut events).await;
    let EngineEvent::BestMove(best) = seen.last().unwrap() else {
        unreachable!()
    };
    assert_eq!(best.mv, None);
}

// ---------------------------------------------------------------------------
// terminal positions and illegal requests (rule 7)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn mate_and_stalemate_answer_bestmove_none_without_talking_to_the_engine() {
    for fen in [WHITE_MATED, BLACK_STALEMATED] {
        let fake = ScriptedEngine::builder().build();
        let (engine, mut events) = fake.start(config()).await.unwrap();
        let before = fake.received();
        engine.send(EngineCommand::Search(infinite(fen))).unwrap();
        let seen = until_best(&mut events).await;
        let EngineEvent::BestMove(best) = seen.last().unwrap() else {
            unreachable!()
        };
        assert_eq!((best.mv, best.ponder), (None, None));
        let started = seen
            .iter()
            .position(|e| matches!(e, EngineEvent::SearchStarted(_)))
            .expect("SearchStarted");
        assert_eq!(started + 1, seen.len() - 1, "{seen:?}");
        assert_eq!(fake.received(), before, "{fen}: nothing reaches the engine");
        // The engine still works afterwards.
        engine.send(EngineCommand::Search(depth(KPK, 1))).unwrap();
        assert!(next_best(&mut events).await.mv.is_some());
        engine.shutdown().await;
    }
}

#[tokio::test(start_paused = true)]
async fn illegal_moves_in_the_path_refuse_the_search_and_send_nothing() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    let before = fake.received();
    // A move that is legal in another position, not in the start position.
    let other = pos("4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
    let alien = mv(&other, "a1a8");
    let request = SearchRequest::infinite(SearchPosition {
        start: Position::new(),
        moves: vec![alien],
    });
    let sent = engine.send(EngineCommand::Search(request));
    let refused = match sent {
        Err(_) => true,
        Ok(()) => {
            until(&mut events, |e| match e {
                EngineEvent::Warning(EngineWarning::CommandRefused { .. }) => Some(true),
                EngineEvent::SearchStarted(_) | EngineEvent::BestMove(_) => Some(false),
                _ => None,
            })
            .await
        }
    };
    assert!(
        refused,
        "an illegal path must be refused with an error or a warning"
    );
    assert_eq!(fake.received(), before, "nothing reaches the engine");
    // And the session is intact.
    engine.send(EngineCommand::Search(depth(KPK, 1))).unwrap();
    assert!(next_best(&mut events).await.mv.is_some());
    engine.shutdown().await;
}

// ---------------------------------------------------------------------------
// score point of view (D405, D611)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn scores_are_converted_to_white_point_of_view() {
    // (fen, engine line, expected score, expected bound)
    let cases = [
        (
            KPK,
            "info depth 5 score cp 30 pv e2e4",
            Score::Cp(30),
            Bound::Exact,
        ),
        (
            BLACK_WINS_QUEEN,
            "info depth 5 score cp 30 pv a3a1",
            Score::Cp(-30),
            Bound::Exact,
        ),
        (
            KPK,
            "info depth 5 score mate 3 pv e2e4",
            Score::Mate(3),
            Bound::Exact,
        ),
        (
            KPK,
            "info depth 5 score mate -3 pv e2e4",
            Score::Mate(-3),
            Bound::Exact,
        ),
        (
            BLACK_WINS_QUEEN,
            "info depth 5 score mate 2 pv a3a1",
            Score::Mate(-2),
            Bound::Exact,
        ),
        (
            BLACK_WINS_QUEEN,
            "info depth 5 score mate -2 pv a3a1",
            Score::Mate(2),
            Bound::Exact,
        ),
        (
            KPK,
            "info depth 5 score cp 12 lowerbound pv e2e4",
            Score::Cp(12),
            Bound::Lower,
        ),
        (
            KPK,
            "info depth 5 score cp 12 upperbound pv e2e4",
            Score::Cp(12),
            Bound::Upper,
        ),
        (
            BLACK_WINS_QUEEN,
            "info depth 5 score cp 12 lowerbound pv a3a1",
            Score::Cp(-12),
            Bound::Upper,
        ),
        (
            BLACK_WINS_QUEEN,
            "info depth 5 score cp 12 upperbound pv a3a1",
            Score::Cp(-12),
            Bound::Lower,
        ),
        (
            KPK,
            "info depth 5 score cp 0 pv e2e4",
            Score::Cp(0),
            Bound::Exact,
        ),
        (
            BLACK_WINS_QUEEN,
            "info depth 5 score cp -2147483648 pv a3a1",
            Score::Cp(i32::MAX),
            Bound::Exact,
        ),
        // `mate 0`: the side to move is checkmated (D611).
        (
            KPK,
            "info depth 0 score mate 0",
            Score::Cp(-32_750),
            Bound::Exact,
        ),
        (
            BLACK_WINS_QUEEN,
            "info depth 0 score mate 0",
            Score::Cp(32_750),
            Bound::Exact,
        ),
    ];
    for (fen, text, score, bound) in cases {
        let (mut peer, engine, mut events) = ready(&[]).await;
        start_search(&mut peer, &engine, &mut events, infinite(fen)).await;
        peer.say(text).await;
        let info = next_info(&mut events).await;
        assert_eq!(info.score, Some((score, bound)), "{fen} / {text}");
    }
}

#[tokio::test(start_paused = true)]
async fn wdl_is_swapped_for_black_and_normalised_to_one_thousand() {
    let cases = [
        (
            KPK,
            "info depth 5 wdl 100 300 600 pv e2e4",
            Wdl {
                win: 100,
                draw: 300,
                loss: 600,
            },
        ),
        (
            BLACK_WINS_QUEEN,
            "info depth 5 wdl 100 300 600 pv a3a1",
            Wdl {
                win: 600,
                draw: 300,
                loss: 100,
            },
        ),
        // Sum below 1000: the rest goes to the largest value.
        (
            KPK,
            "info depth 5 wdl 400 400 100",
            Wdl {
                win: 500,
                draw: 400,
                loss: 100,
            },
        ),
    ];
    for (fen, text, expected) in cases {
        let (mut peer, engine, mut events) = ready(&[]).await;
        start_search(&mut peer, &engine, &mut events, infinite(fen)).await;
        peer.say(text).await;
        let info = next_info(&mut events).await;
        assert_eq!(info.wdl, Some(expected), "{text}");
    }
    // Always sums to 1000, whatever the engine says.
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    for text in [
        "info wdl 1 1 1",
        "info wdl 999 999 999",
        "info wdl 4000000000 1 1",
        "info wdl 333 333 333",
    ] {
        peer.say(text).await;
        let wdl = next_info(&mut events).await.wdl.expect("a wdl");
        assert_eq!(wdl.win + wdl.draw + wdl.loss, 1000, "{text}: {wdl:?}");
    }
    peer.say("info wdl 0 0 0").await;
    assert_eq!(next_info(&mut events).await.wdl, None);
}

#[tokio::test(start_paused = true)]
async fn the_pv_is_validated_and_cut_at_the_first_illegal_move() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(
        &mut peer,
        &engine,
        &mut events,
        infinite(&Position::new().fen()),
    )
    .await;
    peer.say("info depth 3 score cp 1 pv e2e4 e7e5 g1f3").await;
    let ok = next_info(&mut events).await;
    assert_eq!(ok.pv.len(), 3);
    assert!(!ok.pv_truncated);
    peer.say("info depth 3 score cp 1 pv e2e4 e7e5 e5e3 g1f3")
        .await;
    let cut = next_info(&mut events).await;
    assert_eq!(cut.pv.len(), 2);
    assert!(cut.pv_truncated);
    peer.say("info depth 3 score cp 1 pv e2e5").await;
    let none = next_info(&mut events).await;
    assert!(none.pv.is_empty() && none.pv_truncated);
    peer.say("info depth 3 score cp 1 pv e2e4 0000 e7e5").await;
    let nul = next_info(&mut events).await;
    assert_eq!(nul.pv.len(), 1);
    assert!(nul.pv_truncated);
}

#[tokio::test(start_paused = true)]
async fn an_info_without_score_or_pv_still_updates_the_statistics() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(
        &mut peer,
        &engine,
        &mut events,
        infinite(&Position::new().fen()),
    )
    .await;
    peer.say("info depth 12 nodes 123456 nps 1000000 hashfull 77 tbhits 3 cpuload 900 time 120")
        .await;
    let info = next_info(&mut events).await;
    assert_eq!(info.depth, Some(12));
    assert_eq!(info.nodes, Some(123_456));
    assert_eq!(info.nps, Some(1_000_000));
    assert_eq!(info.hashfull_permille, Some(77));
    assert_eq!(info.tbhits, Some(3));
    assert_eq!(info.cpuload_permille, Some(900));
    assert_eq!(info.time_ms, Some(120));
    assert_eq!(info.score, None);
    assert_eq!(info.pv.len(), 0);
    assert_eq!(info.multipv, 1, "an absent multipv is line 1");
    peer.say("info depth 12 currmove e2e4 currmovenumber 3")
        .await;
    let info = next_info(&mut events).await;
    assert_eq!(info.currmovenumber, Some(3));
    assert!(info.currmove.is_some());
    peer.say("info string NNUE evaluation using nn.nnue").await;
    let info = next_info(&mut events).await;
    assert_eq!(
        info.string.as_deref(),
        Some("NNUE evaluation using nn.nnue")
    );
    assert_eq!(info.pv.len(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_bad_field_loses_only_that_field() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(
        &mut peer,
        &engine,
        &mut events,
        infinite(&Position::new().fen()),
    )
    .await;
    peer.say("info depth abc score foo 12 nodes 99 hashfull 5000 multipv 0 pv e2e4")
        .await;
    let info = next_info(&mut events).await;
    assert_eq!(info.depth, None);
    assert_eq!(
        info.score, None,
        "a score without cp|mate (Prodeo) is skipped"
    );
    assert_eq!(info.nodes, Some(99));
    assert_eq!(info.hashfull_permille, None);
    assert_eq!(info.multipv, 1);
    assert_eq!(info.pv.len(), 1);
}

// ---------------------------------------------------------------------------
// hostile bytes through the whole session (D411, rule 10)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn invalid_utf8_nul_and_crlf_do_not_disturb_the_session() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, depth(KPK, 1)).await;
    peer.say_raw(b"info string \xff\xfe broken \x00 bytes\r\n")
        .await;
    peer.say_raw(b"info depth 2 sco\x00re cp 9 pv e2e4\r\n")
        .await;
    peer.say_raw(b"\r\n\r\n   \r\n").await;
    peer.say_raw(b"bestmove e2e4\r\n").await;
    let best = next_best(&mut events).await;
    assert!(best.mv.is_some());
}

#[tokio::test(start_paused = true)]
async fn a_line_of_two_mebibytes_is_cut_and_the_next_line_is_clean() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, depth(KPK, 1)).await;
    let mut big = b"info string ".to_vec();
    big.extend(std::iter::repeat_n(b'A', 2 * 1024 * 1024));
    big.push(b'\n');
    big.extend_from_slice(b"info depth 7 score cp 3 pv e2e4\nbestmove e2e4\n");
    peer.say_raw(&big).await;
    let seen = until_best(&mut events).await;
    let depth7 = seen
        .iter()
        .any(|e| matches!(e, EngineEvent::Info(i) if i.depth == Some(7)));
    assert!(depth7, "the line after the huge one was lost");
    // The log noted the cut and holds only a short entry for the huge line.
    let (entries, _) = engine.log().snapshot_since(0);
    assert!(entries.iter().all(|e| e.text.len() < 5000));
    assert!(entries.iter().any(|e| e.text.contains("1 MiB")));
}

#[tokio::test(start_paused = true)]
async fn one_hundred_thousand_lines_in_a_row_leave_the_session_responsive() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let mut blob = String::new();
    for i in 0..100_000_u32 {
        writeln!(
            blob,
            "info depth {} score cp {} pv e2e4",
            i % 60 + 1,
            i % 50
        )
        .unwrap();
    }
    blob.push_str("bestmove e2e4\n");
    peer.say_raw(blob.as_bytes()).await;
    let seen = until_best(&mut events).await;
    assert!(seen.len() < 5_000, "{} events for 100000 lines", seen.len());
    assert!(engine.log().len() <= 5000, "the log ring is bounded");
    assert_nothing_sent_since(&mut peer, &engine).await;
}

// ---------------------------------------------------------------------------
// MultiPV (D618)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn multipv_values_are_clamped_to_the_announced_range() {
    let (mut peer, engine, _events) = ready(&[MULTIPV4]).await;
    for (asked, sent) in [(0_u8, 1), (1, 1), (4, 4), (5, 4), (255, 4)] {
        engine.send(EngineCommand::SetMultiPv(asked)).unwrap();
        expect_lines(
            &mut peer,
            &[&format!("setoption name MultiPV value {sent}"), "isready"],
        )
        .await;
        peer.say("readyok").await;
    }
}

#[tokio::test(start_paused = true)]
async fn the_multipv_capability_is_cut_to_one_to_two_hundred_fifty_five() {
    let fake = ScriptedEngine::builder().build();
    let (engine, _events) = fake.start(config()).await.unwrap();
    assert_eq!(
        engine.identity().unwrap().capabilities.multipv,
        Some(1..=255)
    );
    engine.send(EngineCommand::SetMultiPv(255)).unwrap();
    assert!(fake.received().iter().all(|l| !l.contains("value 256")));
    // 256 is a valid engine value but the u8 command cannot ask for it, and
    // the generic path refuses what the spin range cannot hold.
    assert!(matches!(
        engine.send(EngineCommand::SetOption {
            name: "MultiPV".into(),
            value: OptionValue::Int(257)
        }),
        Err(EngineError::InvalidOption(_))
    ));
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn an_engine_without_multipv_refuses_setmultipv() {
    let fake = ScriptedEngine::builder().no_multipv().build();
    let (engine, _events) = fake.start(config()).await.unwrap();
    assert_eq!(engine.identity().unwrap().capabilities.multipv, None);
    assert!(matches!(
        engine.send(EngineCommand::SetMultiPv(2)),
        Err(EngineError::Unsupported(_))
    ));
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn multipv_lines_come_ordered_by_depth_and_line_from_the_scripted_engine() {
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    engine.send(EngineCommand::SetMultiPv(3)).unwrap();
    engine
        .send(EngineCommand::Search(depth(&Position::new().fen(), 3)))
        .unwrap();
    let mut cfg_events = Vec::new();
    loop {
        let e = next(&mut events).await;
        let done = matches!(e, EngineEvent::BestMove(_));
        if let EngineEvent::Info(i) = &e {
            cfg_events.push((i.depth.unwrap(), i.multipv));
        }
        if done {
            break;
        }
    }
    assert!(cfg_events.iter().all(|(_, mpv)| (1..=3).contains(mpv)));
    for depth in 1..=3 {
        let lines: Vec<u8> = cfg_events
            .iter()
            .filter(|(d, _)| *d == depth)
            .map(|(_, m)| *m)
            .collect();
        // Coalescing may drop a depth, never reorder the lines within it.
        assert!(
            lines.windows(2).all(|w| w[0] < w[1]),
            "depth {depth}: {lines:?}"
        );
    }
    engine.shutdown().await;
}

// ---------------------------------------------------------------------------
// Chess960 (D413, D617)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn chess960_turns_the_option_on_waits_and_sends_king_takes_rook() {
    let (mut peer, engine, mut events) = ready(&[CHESS960]).await;
    let start = pos(FRC_CASTLE);
    let castle = mv(&start, "e1d1");
    let request = SearchRequest::infinite(SearchPosition {
        start,
        moves: vec![castle],
    });
    engine.send(EngineCommand::Search(request)).unwrap();
    expect_lines(
        &mut peer,
        &["setoption name UCI_Chess960 value true", "isready"],
    )
    .await;
    // `position` waits for readyok.
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.next_line())
            .await
            .is_err()
    );
    peer.say("readyok").await;
    let position = line(&mut peer).await;
    assert_eq!(
        position,
        format!(
            "position fen {} moves e1d1",
            "4k3/8/8/8/8/8/8/1R1RK3 w D - 0 1"
        )
    );
    assert_eq!(line(&mut peer).await, "go infinite");
    until(&mut events, |e| {
        matches!(e, EngineEvent::SearchStarted(_)).then_some(())
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn the_chess960_option_follows_the_rules_of_each_search() {
    let (mut peer, engine, mut events) = ready(&[CHESS960]).await;
    // 960 twice: the option is set once.
    for _ in 0..2 {
        engine
            .send(EngineCommand::Search(depth(FRC_CASTLE, 1)))
            .unwrap();
        let first = line(&mut peer).await;
        if first.starts_with("setoption") {
            assert_eq!(first, "setoption name UCI_Chess960 value true");
            expect_lines(&mut peer, &["isready"]).await;
            peer.say("readyok").await;
            line(&mut peer).await; // position
        }
        assert_eq!(line(&mut peer).await, "go depth 1");
        peer.say("bestmove e1d1").await;
        next_best(&mut events).await;
    }
    // Back to the standard rules: turned off first.
    engine.send(EngineCommand::Search(depth(KPK, 1))).unwrap();
    expect_lines(
        &mut peer,
        &["setoption name UCI_Chess960 value false", "isready"],
    )
    .await;
    peer.say("readyok").await;
    expect_lines(&mut peer, &[&format!("position fen {KPK}"), "go depth 1"]).await;
}

#[tokio::test(start_paused = true)]
async fn the_saved_chess960_option_is_never_sent_with_the_saved_values() {
    let mut cfg = config();
    cfg.options = vec![("UCI_Chess960".into(), OptionValue::Bool(true))];
    let (mut peer, _engine, _events) = ManualEngine::launch(cfg);
    assert_eq!(line(&mut peer).await, "uci");
    peer.say(CHESS960).await;
    peer.say("uciok").await;
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
}

#[tokio::test(start_paused = true)]
async fn castling_in_pv_and_bestmove_is_read_in_both_notations_in_chess960() {
    let (mut peer, engine, mut events) = ready(&[CHESS960]).await;
    engine
        .send(EngineCommand::Search(infinite(FRC_CASTLE)))
        .unwrap();
    expect_lines(
        &mut peer,
        &["setoption name UCI_Chess960 value true", "isready"],
    )
    .await;
    peer.say("readyok").await;
    line(&mut peer).await;
    line(&mut peer).await;
    peer.say("info depth 2 score cp 5 pv e1d1").await;
    let info = next_info(&mut events).await;
    assert_eq!(info.pv.len(), 1);
    assert!(info.pv[0].is_castle(), "king takes rook is a castling move");
    peer.say("info depth 3 score cp 5 pv e1c1").await;
    let info = next_info(&mut events).await;
    assert!(info.pv.first().is_some_and(|m| m.is_castle()), "{info:?}");
    peer.say("bestmove e1d1").await;
    let best = next_best(&mut events).await;
    assert!(best.mv.is_some_and(mychess_core::Move::is_castle));
}

#[tokio::test(start_paused = true)]
async fn standard_castling_keeps_the_classic_notation() {
    let (mut peer, engine, mut events) = ready(&[CHESS960]).await;
    let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
    let start = pos(fen);
    let castle = mv(&start, "e1g1");
    let request = SearchRequest::infinite(SearchPosition {
        start,
        moves: vec![castle],
    });
    let (_, position, _) = start_search(&mut peer, &engine, &mut events, request).await;
    assert_eq!(position, format!("position fen {fen} moves e1g1"));
}

#[tokio::test(start_paused = true)]
async fn an_engine_without_chess960_refuses_a_chess960_game() {
    let fake = ScriptedEngine::builder().chess960(false).build();
    let (engine, mut events) = fake.start(config()).await.unwrap();
    assert!(!engine.identity().unwrap().capabilities.chess960);
    let before = fake.received();
    assert!(matches!(
        engine.send(EngineCommand::Search(infinite(FRC_CASTLE))),
        Err(EngineError::UnsupportedVariant)
    ));
    assert_eq!(fake.received(), before);
    // Standard games are fine.
    engine.send(EngineCommand::Search(depth(KPK, 1))).unwrap();
    assert!(next_best(&mut events).await.mv.is_some());
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn the_capability_comes_from_the_uci_chess960_option() {
    let with = ScriptedEngine::builder().chess960(true).build();
    let (engine, _e) = with.start(config()).await.unwrap();
    assert!(engine.identity().unwrap().capabilities.chess960);
    engine.shutdown().await;
}
