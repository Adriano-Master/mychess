//! M4-2 contract tests with the fake-engine *binary*: `probe_xboard`,
//! `detect_protocol` and a real-process search (spec D416, behavior 5).

use std::time::Duration;

use mychess_core::Position;
use mychess_engine::{
    Engine, EngineCommand, EngineConfig, EngineError, EngineEvent, EngineEvents, Protocol,
    SearchLimits, SearchPosition, SearchRequest, detect_protocol, probe_xboard,
};

fn fake(args: &[&str]) -> EngineConfig {
    let mut config = EngineConfig::new(env!("CARGO_BIN_EXE_mychess-fake-engine"));
    config.args = args.iter().map(|a| (*a).to_owned()).collect();
    config
}

fn xboard(args: &[&str]) -> EngineConfig {
    let mut all = vec!["--protocol", "xboard"];
    all.extend_from_slice(args);
    let mut config = fake(&all);
    config.protocol = Protocol::XBoard;
    config.info_interval = Duration::ZERO;
    config
}

async fn until<T>(events: &mut EngineEvents, mut pick: impl FnMut(EngineEvent) -> Option<T>) -> T {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let e = events.recv().await.expect("session alive");
            if let Some(x) = pick(e) {
                return x;
            }
        }
    })
    .await
    .expect("event in 20 s")
}

#[tokio::test]
async fn probe_xboard_reports_the_name_and_no_options_for_several_dialects() {
    for args in [
        vec!["--name", "A"],
        vec!["--name", "A", "--features", "done=1"],
        vec!["--name", "A", "--features", "setboard=1 usermove=1 done=1"],
        vec!["--name", "A", "--slow-feature", "300"],
    ] {
        let id = probe_xboard(xboard(&args))
            .await
            .unwrap_or_else(|e| panic!("{args:?}: {e}"));
        assert_eq!(id.name, "A", "{args:?}");
        assert_eq!(id.options.len(), 0);
        assert_eq!(id.capabilities.multipv, None);
    }
}

#[tokio::test]
async fn probe_xboard_of_a_missing_program_is_a_spawn_error() {
    let mut config = EngineConfig::new("definitely-not-an-engine-binary-m42");
    config.protocol = Protocol::XBoard;
    let error = probe_xboard(config).await.unwrap_err();
    assert!(matches!(error, EngineError::Spawn { .. }), "{error:?}");
}

#[tokio::test]
async fn detect_protocol_of_a_missing_program_is_a_spawn_error() {
    let config = EngineConfig::new("definitely-not-an-engine-binary-m42");
    let error = detect_protocol(config).await.unwrap_err();
    assert!(matches!(error, EngineError::Spawn { .. }), "{error:?}");
}

#[tokio::test]
async fn detect_protocol_tries_uci_first_and_accepts_xboard_with_features() {
    assert_eq!(detect_protocol(fake(&[])).await.unwrap(), Protocol::Uci);
    assert_eq!(
        detect_protocol(fake(&["--protocol", "xboard", "--features", "done=1"]))
            .await
            .unwrap(),
        Protocol::XBoard
    );
}

#[tokio::test]
async fn detect_protocol_rejects_an_xboard_engine_that_never_sends_a_feature() {
    // protover 1 engines have no `feature` line, so they cannot be told from
    // a program that speaks nothing (spec behavior 5).
    let t0 = std::time::Instant::now();
    let error = detect_protocol(fake(&["--protocol", "xboard", "--legacy"]))
        .await
        .unwrap_err();
    assert!(matches!(error, EngineError::Unrecognised), "{error:?}");
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
}

#[tokio::test]
async fn detect_protocol_rejects_a_silent_xboard_engine() {
    let t0 = std::time::Instant::now();
    let error = detect_protocol(fake(&["--protocol", "xboard", "--silent"]))
        .await
        .unwrap_err();
    assert!(matches!(error, EngineError::Unrecognised), "{error:?}");
    assert!(t0.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn real_process_go_search_ends_with_a_legal_move_and_the_process_exits() {
    let (engine, mut events) = Engine::start(xboard(&["--coord-pv", "--white-pov"]))
        .await
        .unwrap();
    let pid = engine.process_id().expect("a real process");
    let start = Position::new();
    let request = SearchRequest {
        limits: SearchLimits {
            depth: Some(3),
            ..SearchLimits::default()
        },
        ..SearchRequest::infinite(SearchPosition::new(start.clone()))
    };
    engine.send(EngineCommand::Search(request)).unwrap();
    let best = until(&mut events, |e| match e {
        EngineEvent::BestMove(b) => Some(b),
        _ => None,
    })
    .await;
    assert!(start.is_legal(best.mv.expect("a move")));
    engine.shutdown().await;
    #[cfg(target_os = "linux")]
    {
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "orphan engine"
        );
    }
    #[cfg(not(target_os = "linux"))]
    let _ = pid;
}

#[tokio::test]
async fn real_process_dies_in_the_middle_of_a_search() {
    let (engine, mut events) = Engine::start(xboard(&["--crash-after-go"])).await.unwrap();
    engine
        .send(EngineCommand::Search(SearchRequest::infinite(
            SearchPosition::new(Position::new()),
        )))
        .unwrap();
    let exit = until(&mut events, |e| match e {
        EngineEvent::Exited(x) => Some(x),
        _ => None,
    })
    .await;
    assert_eq!(exit.reason, mychess_engine::ExitReason::Crashed);
}
