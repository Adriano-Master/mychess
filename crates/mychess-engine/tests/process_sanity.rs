//! Sanity tests of the process handling with the fake-engine binary. The
//! complete contract suite is written separately.

use std::time::{Duration, Instant};

use mychess_core::Position;
use mychess_engine::{
    Engine, EngineCommand, EngineConfig, EngineError, EngineEvent, EngineEvents, ExitReason,
    SearchLimits, SearchPosition, SearchRequest, probe_uci,
};

fn fake(args: &[&str]) -> EngineConfig {
    let mut config = EngineConfig::new(env!("CARGO_BIN_EXE_mychess-fake-engine"));
    config.args = args.iter().map(|a| (*a).to_owned()).collect();
    config
}

fn depth(n: u32) -> SearchRequest {
    SearchRequest {
        limits: SearchLimits {
            depth: Some(n),
            ..SearchLimits::default()
        },
        ..SearchRequest::infinite(SearchPosition::new(Position::new()))
    }
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
async fn starts_analyses_three_positions_and_quits() {
    let (engine, mut events) = Engine::start(fake(&[])).await.unwrap();
    let pid = engine.process_id().expect("a real process");
    for n in 1..=3 {
        engine.send(EngineCommand::Search(depth(n))).unwrap();
        let best = wait_for(&mut events, |e| match e {
            EngineEvent::BestMove(b) => Some(b),
            _ => None,
        })
        .await;
        assert!(best.mv.is_some());
    }
    engine.shutdown().await;
    #[cfg(target_os = "linux")]
    {
        std::thread::sleep(Duration::from_millis(100));
        assert!(!process_exists(pid), "engine process {pid} was left behind");
    }
    let _ = pid;
}

#[tokio::test]
async fn missing_executable_is_a_spawn_error() {
    let dir = tempfile::tempdir().unwrap();
    let result = Engine::start(EngineConfig::new(dir.path().join("nope"))).await;
    assert!(matches!(result, Err(EngineError::Spawn { .. })));
}

#[tokio::test]
async fn an_engine_that_ignores_quit_is_killed_quickly() {
    let (engine, mut events) = Engine::start(fake(&["--ignore-quit"])).await.unwrap();
    let pid = engine.process_id().unwrap();
    let began = Instant::now();
    engine.send(EngineCommand::Quit).unwrap();
    let exit = wait_for(&mut events, |e| match e {
        EngineEvent::Exited(x) => Some(x),
        _ => None,
    })
    .await;
    assert_eq!(exit.reason, ExitReason::Quit);
    assert!(
        began.elapsed() < Duration::from_millis(5000),
        "{:?}",
        began.elapsed()
    );
    #[cfg(target_os = "linux")]
    {
        std::thread::sleep(Duration::from_millis(100));
        assert!(!process_exists(pid));
    }
    let _ = pid;
}

#[tokio::test]
async fn a_crash_reports_status_and_stderr() {
    let (engine, mut events) = Engine::start(fake(&["--crash-after-go", "--exit-code", "7"]))
        .await
        .unwrap();
    engine.send(EngineCommand::Search(depth(2))).unwrap();
    let exit = wait_for(&mut events, |e| match e {
        EngineEvent::Exited(x) => Some(x),
        _ => None,
    })
    .await;
    assert_eq!(exit.reason, ExitReason::Crashed);
    assert_eq!(exit.status.and_then(|s| s.code()), Some(7));
    assert!(
        exit.stderr_tail
            .iter()
            .any(|l| l.contains("crashing on purpose")),
        "{:?}",
        exit.stderr_tail
    );
}

#[tokio::test]
async fn a_silent_engine_times_out() {
    let mut config = fake(&["--silent"]);
    config.timeouts.uciok = Duration::from_millis(300);
    let result = Engine::start(config).await;
    assert!(matches!(result, Err(EngineError::Timeout(_))), "{result:?}");
}

#[tokio::test]
async fn stderr_flood_and_garbage_do_not_block() {
    let (engine, mut events) = Engine::start(fake(&["--stderr-flood", "--garbage", "--crlf"]))
        .await
        .unwrap();
    engine.send(EngineCommand::Search(depth(3))).unwrap();
    wait_for(&mut events, |e| {
        matches!(e, EngineEvent::BestMove(_)).then_some(())
    })
    .await;
    engine.shutdown().await;
}

#[tokio::test]
async fn probe_learns_the_options_and_ends_the_engine() {
    let identity = probe_uci(fake(&["--name", "Probed"])).await.unwrap();
    assert_eq!(identity.name, "Probed");
    assert!(identity.options.iter().any(|o| o.name == "Hash"));
}
