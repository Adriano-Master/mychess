//! Process-level sanity tests of the XBoard adapter with the fake-engine
//! binary in XBoard mode, and of the protocol detection. The full contract
//! suite is written separately.

use std::time::Duration;

use mychess_core::Position;
use mychess_engine::{
    Engine, EngineCommand, EngineConfig, EngineError, EngineEvent, EngineEvents, Protocol,
    SearchPosition, SearchRequest, detect_protocol, probe_xboard,
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
    config
}

async fn wait_for<T>(
    events: &mut EngineEvents,
    mut pick: impl FnMut(EngineEvent) -> Option<T>,
) -> T {
    let found = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let event = events.recv().await.expect("session ended early");
            if let Some(found) = pick(event) {
                return found;
            }
        }
    })
    .await;
    found.expect("event within 20 s")
}

#[cfg(target_os = "linux")]
fn process_exists(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[tokio::test]
async fn xboard_engine_starts_analyses_stops_and_quits() {
    let (engine, mut events) = Engine::start(xboard(&[])).await.unwrap();
    let pid = engine.process_id().expect("a real process");
    let identity = engine.identity().expect("ready").clone();
    assert_eq!(identity.name, "FakeFish");
    assert!(identity.capabilities.multipv.is_none());
    let request = SearchRequest::infinite(SearchPosition::new(Position::new()));
    engine.send(EngineCommand::Search(request)).unwrap();
    let info = wait_for(&mut events, |e| match e {
        EngineEvent::Info(i) => Some(i),
        _ => None,
    })
    .await;
    assert_ne!(info.pv.len(), 0);
    engine.send(EngineCommand::Stop).unwrap();
    let best = wait_for(&mut events, |e| match e {
        EngineEvent::BestMove(b) => Some(b),
        _ => None,
    })
    .await;
    assert!(best.mv.is_some());
    engine.shutdown().await;
    #[cfg(target_os = "linux")]
    {
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!process_exists(pid), "the engine process is still alive");
    }
    #[cfg(not(target_os = "linux"))]
    let _ = pid;
}

#[tokio::test]
async fn probe_xboard_reads_the_name() {
    let identity = probe_xboard(xboard(&["--name", "Probed"])).await.unwrap();
    assert_eq!(identity.name, "Probed");
    assert_eq!(identity.options.len(), 0);
}

#[tokio::test]
async fn detect_protocol_tells_uci_from_xboard() {
    assert_eq!(detect_protocol(fake(&[])).await.unwrap(), Protocol::Uci);
    assert_eq!(
        detect_protocol(fake(&["--protocol", "xboard"]))
            .await
            .unwrap(),
        Protocol::XBoard
    );
}

#[tokio::test]
async fn detect_protocol_rejects_a_silent_engine() {
    let started = std::time::Instant::now();
    let error = detect_protocol(fake(&["--silent"])).await.unwrap_err();
    assert!(matches!(error, EngineError::Unrecognised), "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// Needs an XBoard engine installed by the user (for example
/// `gnuchess --xboard` or `crafty`): run with
/// `MYCHESS_TEST_XBOARD_ENGINE=/path/to/engine cargo test -- --ignored`.
#[tokio::test]
#[ignore = "requires MYCHESS_TEST_XBOARD_ENGINE"]
async fn real_xboard_engine_analyses_for_a_second() {
    let Some(path) = std::env::var_os("MYCHESS_TEST_XBOARD_ENGINE") else {
        return;
    };
    let mut config = EngineConfig::new(path);
    config.protocol = Protocol::XBoard;
    let (engine, mut events) = Engine::start(config).await.unwrap();
    let request = SearchRequest::infinite(SearchPosition::new(Position::new()));
    engine.send(EngineCommand::Search(request)).unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    engine.send(EngineCommand::Stop).unwrap();
    let best = wait_for(&mut events, |e| match e {
        EngineEvent::BestMove(b) => Some(b),
        _ => None,
    })
    .await;
    assert!(best.mv.is_some());
    engine.shutdown().await;
}
