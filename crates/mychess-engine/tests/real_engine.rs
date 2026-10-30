//! Tests against a real UCI engine (Stockfish or similar). Ignored by
//! default; run with `MYCHESS_TEST_ENGINE=/path/to/engine cargo test -p
//! mychess-engine --test real_engine -- --ignored`.

use std::time::Duration;

use mychess_core::Position;
use mychess_engine::{
    Engine, EngineCommand, EngineConfig, EngineEvent, SearchLimits, SearchPosition, SearchRequest,
};

fn engine_path() -> Option<String> {
    std::env::var("MYCHESS_TEST_ENGINE")
        .ok()
        .filter(|p| !p.is_empty())
}

#[tokio::test]
#[ignore = "needs MYCHESS_TEST_ENGINE"]
async fn real_engine_depth_search_and_stop() {
    let Some(path) = engine_path() else {
        return;
    };
    let (engine, mut events) = Engine::start(EngineConfig::new(path)).await.unwrap();
    let identity = engine.identity().unwrap().clone();
    assert!(identity.options.iter().any(|o| o.name == "Hash"));

    let request = SearchRequest {
        limits: SearchLimits {
            depth: Some(8),
            ..SearchLimits::default()
        },
        ..SearchRequest::infinite(SearchPosition::new(Position::new()))
    };
    engine.send(EngineCommand::Search(request)).unwrap();
    let mut last_score = None;
    let best = loop {
        match tokio::time::timeout(Duration::from_secs(30), events.recv())
            .await
            .unwrap()
            .unwrap()
        {
            EngineEvent::Info(i) if i.score.is_some() => last_score = i.score,
            EngineEvent::BestMove(b) => break b,
            _ => {}
        }
    };
    assert!(best.mv.is_some());
    let (score, _) = last_score.expect("a score");
    assert!(score.as_centipawns().abs() < 100, "{score:?}");

    engine
        .send(EngineCommand::Search(SearchRequest::infinite(
            SearchPosition::new(Position::new()),
        )))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let began = std::time::Instant::now();
    engine.send(EngineCommand::Stop).unwrap();
    loop {
        if let EngineEvent::BestMove(_) =
            tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
                .unwrap()
        {
            break;
        }
    }
    assert!(began.elapsed() < Duration::from_secs(1));
    engine.shutdown().await;
}
