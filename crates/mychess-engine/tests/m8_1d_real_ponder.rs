//! M8-1d tests against a real UCI engine (spec D2438): pondering (`go
//! ponder`, `ponderhit` on a hit, a new search on a miss) and the two
//! searches of the coach. Ignored by default; run with
//! `MYCHESS_TEST_ENGINE=/path/to/engine cargo test -p mychess-engine --test
//! m8_1d_real_ponder -- --ignored`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use mychess_core::{Move, Position};
use mychess_engine::{
    BestMove, Bound, Engine, EngineCommand, EngineConfig, EngineEvent, EngineEvents, OptionValue,
    SearchLimits, SearchPosition, SearchRequest,
};

fn engine_path() -> Option<String> {
    std::env::var("MYCHESS_TEST_ENGINE")
        .ok()
        .filter(|p| !p.is_empty())
}

async fn next(events: &mut EngineEvents) -> EngineEvent {
    tokio::time::timeout(Duration::from_secs(60), events.recv())
        .await
        .expect("an event in time")
        .expect("alive")
}

async fn best_of(events: &mut EngineEvents) -> BestMove {
    loop {
        if let EngineEvent::BestMove(b) = next(events).await {
            return b;
        }
    }
}

fn depth_request(start: &Position, moves: &[Move], depth: u32, ponder: bool) -> SearchRequest {
    SearchRequest {
        limits: SearchLimits {
            depth: Some(depth),
            ..SearchLimits::default()
        },
        ponder,
        ..SearchRequest::infinite(SearchPosition {
            start: start.clone(),
            moves: moves.to_vec(),
        })
    }
}

async fn start(path: String) -> (Engine, EngineEvents) {
    let (engine, events) = Engine::start(EngineConfig::new(path)).await.unwrap();
    let has_ponder = engine
        .identity()
        .unwrap()
        .options
        .iter()
        .any(|o| o.name == "Ponder");
    assert!(has_ponder, "the engine must declare Ponder");
    engine.send(EngineCommand::SetPonder(true)).unwrap();
    (engine, events)
}

/// The engine's move and the reply it expects, from the start position.
async fn first_move(engine: &Engine, events: &mut EngineEvents, start: &Position) -> (Move, Move) {
    engine
        .send(EngineCommand::Search(depth_request(start, &[], 10, false)))
        .unwrap();
    let best = best_of(events).await;
    (
        best.mv.expect("a move"),
        best.ponder.expect("a ponder move at depth 10"),
    )
}

#[tokio::test]
#[ignore = "requires MYCHESS_TEST_ENGINE"]
async fn real_engine_ponderhit_ends_the_ponder_search_with_a_move() {
    let Some(path) = engine_path() else {
        return;
    };
    let (engine, mut events) = start(path).await;
    let start = Position::new();
    let (mv, reply) = first_move(&engine, &mut events, &start).await;
    // Ponder on the expected reply; no bestmove may come before ponderhit.
    engine
        .send(EngineCommand::Search(depth_request(
            &start,
            &[mv, reply],
            6,
            true,
        )))
        .unwrap();
    let quiet = tokio::time::timeout(Duration::from_millis(800), best_of(&mut events)).await;
    assert!(quiet.is_err(), "a pondering engine does not move by itself");
    engine.send(EngineCommand::PonderHit).unwrap();
    assert!(best_of(&mut events).await.mv.is_some());
}

#[tokio::test]
#[ignore = "requires MYCHESS_TEST_ENGINE"]
async fn real_engine_a_new_search_replaces_a_missed_ponder_and_the_old_answer_has_another_id() {
    let Some(path) = engine_path() else {
        return;
    };
    let (engine, mut events) = start(path).await;
    let start = Position::new();
    let (mv, reply) = first_move(&engine, &mut events, &start).await;
    engine
        .send(EngineCommand::Search(depth_request(
            &start,
            &[mv, reply],
            6,
            true,
        )))
        .unwrap();
    let ponder_id = loop {
        if let EngineEvent::SearchStarted(id) = next(&mut events).await {
            break id;
        }
    };
    // The player played something else: the real search replaces it.
    let after = start.after(mv).unwrap();
    let other = after
        .legal_moves()
        .into_iter()
        .find(|m| *m != reply)
        .expect("another move");
    engine
        .send(EngineCommand::Search(depth_request(
            &start,
            &[mv, other],
            6,
            false,
        )))
        .unwrap();
    let mut seen = Vec::new();
    while seen.len() < 2 {
        if let EngineEvent::BestMove(b) = next(&mut events).await {
            seen.push(b.search);
        }
    }
    assert_eq!(seen[0], ponder_id, "the stopped ponder reports first");
    assert_ne!(seen[1], ponder_id, "and the real search has its own id");
}

#[tokio::test]
#[ignore = "requires MYCHESS_TEST_ENGINE"]
async fn real_engine_the_two_coach_searches_reach_the_same_depth() {
    let Some(path) = engine_path() else {
        return;
    };
    let (engine, mut events) = Engine::start(EngineConfig::new(path)).await.unwrap();
    engine
        .send(EngineCommand::SetOption {
            name: "Threads".into(),
            value: OptionValue::Int(1),
        })
        .ok();
    let start = Position::new();
    engine
        .send(EngineCommand::Search(depth_request(&start, &[], 8, false)))
        .unwrap();
    let mut d1 = 0;
    let best;
    loop {
        match next(&mut events).await {
            EngineEvent::Info(i)
                if i.multipv <= 1 && matches!(i.score, Some((_, Bound::Exact))) =>
            {
                d1 = i.depth.unwrap_or(d1);
            }
            EngineEvent::BestMove(b) => {
                best = b.mv;
                break;
            }
            _ => {}
        }
    }
    assert!(best.is_some());
    let played = start
        .legal_moves()
        .into_iter()
        .find(|m| Some(*m) != best)
        .unwrap();
    let mut restricted = depth_request(&start, &[], 8, false);
    restricted.search_moves = vec![played];
    engine.send(EngineCommand::Search(restricted)).unwrap();
    let mut d2 = 0;
    loop {
        match next(&mut events).await {
            EngineEvent::Info(i)
                if i.multipv <= 1 && matches!(i.score, Some((_, Bound::Exact))) =>
            {
                d2 = i.depth.unwrap_or(d2);
            }
            EngineEvent::BestMove(b) => {
                assert_eq!(b.mv, Some(played), "searchmoves restricts the answer");
                break;
            }
            _ => {}
        }
    }
    assert!(d1.abs_diff(d2) <= 1, "depths {d1} and {d2}");
}
