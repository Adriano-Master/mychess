//! M4-1 contract tests: real processes (the `mychess-fake-engine` binary and,
//! on Unix, tiny shell-script engines): launching without a shell, working
//! directory, spawn errors, deadlines, exit handling, no orphan processes,
//! stderr/stdin hazards, `probe_uci` and the pure `build_command`.

#[path = "m4_1_common/helpers.rs"]
mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{KPK, depth, infinite};
use mychess_engine::{
    CREATE_NO_WINDOW, Engine, EngineCommand, EngineConfig, EngineError, EngineEvent, EngineEvents,
    ExitInfo, ExitReason, IDLE_PRIORITY_CLASS, NICE_INCREMENT, OptionValue, Phase, build_command,
    build_command_with, probe_uci,
};

const FAKE: &str = env!("CARGO_BIN_EXE_mychess-fake-engine");

fn fake(args: &[&str]) -> EngineConfig {
    let mut config = EngineConfig::new(FAKE);
    config.args = args.iter().map(|a| (*a).to_owned()).collect();
    config
}

async fn wait_for<T>(
    events: &mut EngineEvents,
    mut pick: impl FnMut(EngineEvent) -> Option<T>,
) -> T {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let event = events.recv().await.expect("the session ended early");
            if let Some(found) = pick(event) {
                return found;
            }
        }
    })
    .await
    .expect("an event within 30 s")
}

async fn exit_of(events: &mut EngineEvents) -> ExitInfo {
    wait_for(events, |e| match e {
        EngineEvent::Exited(x) => Some(x),
        _ => None,
    })
    .await
}

async fn best_of(events: &mut EngineEvents) -> mychess_engine::BestMove {
    wait_for(events, |e| match e {
        EngineEvent::BestMove(b) => Some(b),
        _ => None,
    })
    .await
}

/// A unique marker for `--name`, so a process can be found by its command line.
fn marker(test: &str) -> String {
    format!("mychess-test-{test}-{}", std::process::id())
}

/// Pids of live (non-zombie) processes whose command line contains `token`.
#[cfg(target_os = "linux")]
fn processes_with(token: &str) -> Vec<u32> {
    let mut found = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return found;
    };
    for entry in dir.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        if let Ok(cmdline) = std::fs::read(entry.path().join("cmdline"))
            && String::from_utf8_lossy(&cmdline).contains(token)
        {
            found.push(pid);
        }
    }
    found
}

#[cfg(target_os = "linux")]
async fn assert_no_process_with(token: &str) {
    for _ in 0..100 {
        if processes_with(token).is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("processes left behind: {:?}", processes_with(token));
}

/// Known limitation: outside Linux there is no `/proc` to look for orphans,
/// so the orphan checks (`assert_no_process_with`, `assert_pid_gone`) only
/// pass trivially; the kill itself is covered by the timing assertions.
#[cfg(not(target_os = "linux"))]
async fn assert_no_process_with(_token: &str) {}

#[cfg(target_os = "linux")]
fn pid_alive(pid: u32) -> bool {
    // A zombie keeps its /proc entry but has an empty command line.
    std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| !c.is_empty())
}

#[cfg(target_os = "linux")]
async fn assert_pid_gone(pid: u32) {
    for _ in 0..100 {
        if !pid_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("process {pid} is still alive");
}

#[cfg(not(target_os = "linux"))]
async fn assert_pid_gone(_pid: u32) {}

// ---------------------------------------------------------------------------
// the happy path and shutdown
// ---------------------------------------------------------------------------

#[tokio::test]
async fn start_analyse_three_positions_and_shut_down_leaves_no_process() {
    let token = marker("happy");
    let (engine, mut events) = Engine::start(fake(&["--name", &token])).await.unwrap();
    let pid = engine.process_id().expect("a real pid");
    assert!(engine.identity().is_some_and(|i| i.name == token));
    for n in 1..=3 {
        engine.send(EngineCommand::Search(depth(KPK, n))).unwrap();
        assert!(best_of(&mut events).await.mv.is_some());
    }
    engine.shutdown().await;
    assert_pid_gone(pid).await;
    assert_no_process_with(&token).await;
}

#[tokio::test]
async fn shutdown_of_a_well_behaved_engine_is_prompt() {
    let (engine, _events) = Engine::start(fake(&[])).await.unwrap();
    let began = Instant::now();
    engine.shutdown().await;
    assert!(
        began.elapsed() < Duration::from_millis(1500),
        "{:?}",
        began.elapsed()
    );
}

#[tokio::test]
async fn an_engine_that_ignores_quit_is_killed_after_the_quit_wait() {
    let token = marker("ignorequit");
    let (engine, mut events) = Engine::start(fake(&["--ignore-quit", "--name", &token]))
        .await
        .unwrap();
    let pid = engine.process_id().unwrap();
    let began = Instant::now();
    engine.send(EngineCommand::Quit).unwrap();
    let exit = exit_of(&mut events).await;
    let waited = began.elapsed();
    assert_eq!(exit.reason, ExitReason::Quit);
    assert!(
        waited >= Duration::from_millis(400) && waited < Duration::from_millis(2500),
        "{waited:?}"
    );
    assert_pid_gone(pid).await;
    assert_no_process_with(&token).await;
}

#[tokio::test]
async fn the_quit_wait_comes_from_the_configuration() {
    let mut config = fake(&["--ignore-quit"]);
    config.timeouts.quit = Duration::from_millis(100);
    let (engine, mut events) = Engine::start(config).await.unwrap();
    let began = Instant::now();
    engine.send(EngineCommand::Quit).unwrap();
    exit_of(&mut events).await;
    assert!(
        began.elapsed() < Duration::from_millis(1500),
        "{:?}",
        began.elapsed()
    );
}

#[tokio::test]
async fn dropping_the_engine_and_its_events_kills_the_process() {
    let token = marker("drop");
    let (engine, events) = Engine::start(fake(&["--name", &token])).await.unwrap();
    let pid = engine.process_id().unwrap();
    drop(engine);
    drop(events);
    assert_pid_gone(pid).await;
    assert_no_process_with(&token).await;
}

#[tokio::test]
async fn dropping_a_stubborn_engine_still_kills_it() {
    let token = marker("dropstubborn");
    let (engine, events) = Engine::start(fake(&["--ignore-quit", "--name", &token]))
        .await
        .unwrap();
    let pid = engine.process_id().unwrap();
    drop(engine);
    drop(events);
    assert_pid_gone(pid).await;
}

#[tokio::test]
async fn dropping_only_the_events_also_ends_the_session() {
    let token = marker("dropevents");
    let (engine, events) = Engine::start(fake(&["--name", &token])).await.unwrap();
    let pid = engine.process_id().unwrap();
    drop(events);
    assert_pid_gone(pid).await;
    drop(engine);
}

#[tokio::test]
async fn clones_keep_the_engine_alive_until_the_last_one_is_dropped() {
    let (engine, events) = Engine::start(fake(&[])).await.unwrap();
    let pid = engine.process_id().unwrap();
    let clone = engine.clone();
    drop(engine);
    tokio::time::sleep(Duration::from_millis(200)).await;
    #[cfg(target_os = "linux")]
    assert!(pid_alive(pid), "a clone is still using the engine");
    clone.send(EngineCommand::NewGame).unwrap();
    drop(clone);
    drop(events);
    assert_pid_gone(pid).await;
}

#[tokio::test]
async fn a_search_in_progress_is_ended_by_shutdown_without_orphans() {
    let token = marker("shutdownsearch");
    let (engine, mut events) = Engine::start(fake(&["--name", &token])).await.unwrap();
    engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    wait_for(&mut events, |e| {
        matches!(e, EngineEvent::Info(_)).then_some(())
    })
    .await;
    engine.shutdown().await;
    assert_no_process_with(&token).await;
}

// ---------------------------------------------------------------------------
// failures
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_missing_executable_is_a_spawn_error_carrying_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("no-such-engine");
    let result = Engine::start(EngineConfig::new(&path)).await;
    match result {
        Err(EngineError::Spawn { path: p, source }) => {
            assert_eq!(p, path);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("{:?}", other.err()),
    }
    // `launch` fails at once, with the same error.
    assert!(matches!(
        Engine::launch(EngineConfig::new(&path)),
        Err(EngineError::Spawn { .. })
    ));
    // And so does the probe.
    assert!(matches!(
        probe_uci(EngineConfig::new(&path)).await,
        Err(EngineError::Spawn { .. })
    ));
}

#[tokio::test]
async fn a_directory_is_not_an_engine() {
    let dir = tempfile::tempdir().unwrap();
    let result = Engine::start(EngineConfig::new(dir.path())).await;
    assert!(
        matches!(result, Err(EngineError::Spawn { .. })),
        "{:?}",
        result.err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_file_without_execute_permission_is_a_spawn_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engine");
    std::fs::write(&path, "#!/bin/sh\necho hi\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    match Engine::start(EngineConfig::new(&path)).await {
        Err(EngineError::Spawn { source, .. }) => {
            assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
        }
        other => panic!("{:?}", other.err()),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_text_file_that_is_not_a_program_is_an_error_not_a_hang() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engine");
    std::fs::write(&path, "this is not a program").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let began = Instant::now();
    let result = Engine::start(EngineConfig::new(&path)).await;
    assert!(
        matches!(
            result,
            Err(EngineError::Spawn { .. } | EngineError::ExitedEarly { .. })
        ),
        "{:?}",
        result.err()
    );
    assert!(began.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn a_program_that_exits_at_once_is_exited_early_with_status_and_stderr() {
    // The fake engine rejects an unknown flag with exit code 2 and a message.
    let began = Instant::now();
    let result = Engine::start(fake(&["--definitely-not-a-flag"])).await;
    match result {
        Err(EngineError::ExitedEarly {
            status,
            stderr_tail,
        }) => {
            assert_eq!(status.and_then(|s| s.code()), Some(2));
            assert!(
                stderr_tail.iter().any(|l| l.contains("unknown flag")),
                "{stderr_tail:?}"
            );
        }
        other => panic!("{:?}", other.err()),
    }
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "must not wait for the deadline"
    );
}

#[tokio::test]
async fn a_silent_engine_times_out_at_uciok_and_leaves_no_process() {
    let token = marker("silent");
    let mut config = fake(&["--silent", "--name", &token]);
    config.timeouts.uciok = Duration::from_millis(300);
    let began = Instant::now();
    let result = Engine::start(config).await;
    assert!(
        matches!(result, Err(EngineError::Timeout(Phase::Uciok))),
        "{:?}",
        result.err()
    );
    assert!(began.elapsed() < Duration::from_secs(3));
    assert_no_process_with(&token).await;
}

#[tokio::test]
async fn a_slow_uciok_inside_the_deadline_is_accepted_and_outside_it_is_not() {
    let mut config = fake(&["--uciok-delay-ms", "250"]);
    config.timeouts.uciok = Duration::from_secs(5);
    assert!(Engine::start(config).await.is_ok());
    let mut config = fake(&["--uciok-delay-ms", "1500"]);
    config.timeouts.uciok = Duration::from_millis(300);
    assert!(matches!(
        Engine::start(config).await,
        Err(EngineError::Timeout(Phase::Uciok))
    ));
}

#[tokio::test]
async fn an_engine_that_dies_mid_search_is_reported_with_status_and_stderr_tail() {
    let token = marker("crash");
    let (engine, mut events) = Engine::start(fake(&[
        "--crash-after-go",
        "--exit-code",
        "7",
        "--name",
        &token,
    ]))
    .await
    .unwrap();
    let pid = engine.process_id().unwrap();
    engine.send(EngineCommand::Search(depth(KPK, 2))).unwrap();
    let exit = exit_of(&mut events).await;
    assert_eq!(exit.reason, ExitReason::Crashed);
    assert_eq!(exit.status.and_then(|s| s.code()), Some(7));
    assert!(
        exit.stderr_tail
            .iter()
            .any(|l| l.contains("crashing on purpose")),
        "{:?}",
        exit.stderr_tail
    );
    assert!(events.recv().await.is_none());
    assert!(matches!(
        engine.send(EngineCommand::Stop),
        Err(EngineError::Closed)
    ));
    assert_pid_gone(pid).await;
    // The handle survives the death: shutting down an already dead session is fine.
    engine.shutdown().await;
}

#[tokio::test]
async fn the_stderr_tail_keeps_only_the_last_ten_lines() {
    let (engine, mut events) = Engine::start(fake(&["--stderr-flood", "--crash-after-go"]))
        .await
        .unwrap();
    engine.send(EngineCommand::Search(depth(KPK, 1))).unwrap();
    let exit = exit_of(&mut events).await;
    assert!(exit.stderr_tail.len() <= 10, "{}", exit.stderr_tail.len());
    assert_ne!(exit.stderr_tail.len(), 0);
}

#[tokio::test]
async fn a_stderr_flood_neither_blocks_the_engine_nor_fills_memory_and_reaches_the_log() {
    let (engine, mut events) = Engine::start(fake(&["--stderr-flood"])).await.unwrap();
    for n in 1..=3 {
        engine.send(EngineCommand::Search(depth(KPK, n))).unwrap();
        assert!(best_of(&mut events).await.mv.is_some());
    }
    // Let the flood be read.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (entries, _) = engine.log().snapshot_since(0);
    assert!(entries.len() <= mychess_engine::DEFAULT_LOG_LINES);
    assert!(
        entries
            .iter()
            .any(|e| e.kind == mychess_engine::LogKind::Stderr),
        "stderr lines belong in the log"
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn garbage_output_with_huge_lines_and_bad_bytes_does_not_break_the_session() {
    let (engine, mut events) = Engine::start(fake(&["--garbage", "--crlf"])).await.unwrap();
    for n in 1..=3 {
        engine.send(EngineCommand::Search(depth(KPK, n))).unwrap();
        assert!(best_of(&mut events).await.mv.is_some());
    }
    let (entries, _) = engine.log().snapshot_since(0);
    assert!(entries.iter().all(|e| e.text.len() < 4096 + 64));
    engine.shutdown().await;
}

// ---------------------------------------------------------------------------
// probe_uci
// ---------------------------------------------------------------------------

#[tokio::test]
async fn probe_learns_the_identity_and_ends_the_process() {
    let token = marker("probe");
    let identity = probe_uci(fake(&["--name", &token])).await.unwrap();
    assert_eq!(identity.name, token);
    assert!(identity.options.iter().any(|o| o.name == "Hash"));
    assert!(identity.options.iter().any(|o| o.name == "Clear Hash"));
    assert_eq!(identity.capabilities.multipv, Some(1..=255));
    assert_no_process_with(&token).await;
}

#[tokio::test]
async fn probe_does_not_send_the_saved_options() {
    let mut config = fake(&[]);
    config.options = vec![("Hash".into(), OptionValue::Int(64))];
    // Nothing observable from the fake over a pipe; the in-memory suite checks
    // the lines. Here: the probe still succeeds and returns the defaults.
    let identity = probe_uci(config).await.unwrap();
    assert!(identity.options.iter().any(|o| o.name == "Hash"));
}

#[tokio::test]
async fn probe_of_a_silent_engine_times_out_and_leaves_no_process() {
    let token = marker("probesilent");
    let mut config = fake(&["--silent", "--name", &token]);
    config.timeouts.uciok = Duration::from_millis(300);
    let result = probe_uci(config).await;
    assert!(
        matches!(result, Err(EngineError::Timeout(Phase::Uciok))),
        "{result:?}"
    );
    assert_no_process_with(&token).await;
}

#[tokio::test]
async fn probe_of_a_program_that_exits_is_exited_early() {
    let result = probe_uci(fake(&["--definitely-not-a-flag"])).await;
    assert!(
        matches!(result, Err(EngineError::ExitedEarly { .. })),
        "{result:?}"
    );
}

#[tokio::test]
async fn probe_of_an_engine_that_ignores_quit_kills_it() {
    let token = marker("probestubborn");
    let began = Instant::now();
    let identity = probe_uci(fake(&["--ignore-quit", "--name", &token]))
        .await
        .unwrap();
    assert_eq!(identity.name, token);
    assert!(
        began.elapsed() < Duration::from_secs(3),
        "{:?}",
        began.elapsed()
    );
    assert_no_process_with(&token).await;
}

// ---------------------------------------------------------------------------
// shell-script engines (Unix): arguments, working directory, a stuck stdin
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod scripts {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Writes an executable script into `dir` and returns its path.
    pub(super) fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    /// Answers the handshake with `id name <name_expr>` (a shell expression).
    pub(super) fn engine_script(name_expr: &str, extra: &str) -> String {
        format!(
            r#"while IFS= read -r line; do
  case "$line" in
    uci) echo "id name {name_expr}"; echo "option name Big type string default x"; echo uciok;;
    isready) echo readyok;;
    quit) exit 0;;
  esac
  {extra}
done"#
        )
    }

    /// Starts a script engine, retrying while the kernel says the file is busy
    /// (`ETXTBSY` happens when another test forks while we hold it open).
    pub(super) async fn start(config: EngineConfig) -> (Engine, EngineEvents) {
        for _ in 0..20 {
            match Engine::start(config.clone()).await {
                Ok(pair) => return pair,
                Err(EngineError::Spawn { source, .. }) if source.raw_os_error() == Some(26) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(e) => panic!("{e}"),
            }
        }
        panic!("script stayed busy");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn arguments_with_spaces_quotes_and_metacharacters_arrive_intact_without_a_shell() {
    let dir = tempfile::tempdir().unwrap();
    let path = scripts::script(
        dir.path(),
        "args-engine",
        &scripts::engine_script("$# [$1] [$2] [$3]", ""),
    );
    let mut config = EngineConfig::new(&path);
    config.args = vec![
        "two words".to_owned(),
        r#"it's "quoted""#.to_owned(),
        "$HOME; echo pwned `id` *".to_owned(),
    ];
    let (engine, _events) = scripts::start(config).await;
    assert_eq!(
        engine.identity().unwrap().name,
        r#"3 [two words] [it's "quoted"] [$HOME; echo pwned `id` *]"#
    );
    engine.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn the_working_directory_defaults_to_the_directory_of_the_executable() {
    let dir = tempfile::tempdir().unwrap();
    let path = scripts::script(
        dir.path(),
        "cwd-engine",
        &scripts::engine_script("$(pwd -P)", ""),
    );
    let (engine, _events) = scripts::start(EngineConfig::new(&path)).await;
    let reported = PathBuf::from(&engine.identity().unwrap().name);
    assert_eq!(reported, dir.path().canonicalize().unwrap());
    engine.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn an_explicit_working_directory_wins() {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let path = scripts::script(
        dir.path(),
        "cwd-engine",
        &scripts::engine_script("$(pwd -P)", ""),
    );
    let mut config = EngineConfig::new(&path);
    config.working_dir = Some(work.path().to_path_buf());
    let (engine, _events) = scripts::start(config).await;
    assert_eq!(
        PathBuf::from(&engine.identity().unwrap().name),
        work.path().canonicalize().unwrap()
    );
    engine.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn an_engine_that_stops_reading_its_input_does_not_freeze_the_session() {
    let dir = tempfile::tempdir().unwrap();
    // After the handshake this engine sleeps and never reads again.
    let body = r#"while IFS= read -r line; do
  case "$line" in
    uci) echo "id name Deaf"; echo "option name Big type string default x"; echo uciok;;
    isready) echo readyok; break;;
  esac
done
sleep 30"#;
    let path = scripts::script(dir.path(), "deaf-engine", body);
    let mut config = EngineConfig::new(&path);
    config.timeouts.readyok = Duration::from_secs(2);
    config.timeouts.quit = Duration::from_millis(300);
    let (engine, mut events) = scripts::start(config).await;
    let pid = engine.process_id().unwrap();
    // Far more than a pipe holds: the writer task blocks, the session must not.
    let big = "x".repeat(300_000);
    for _ in 0..3 {
        drop(engine.send(EngineCommand::SetOption {
            name: "Big".into(),
            value: OptionValue::Text(big.clone()),
        }));
    }
    let began = Instant::now();
    engine.send(EngineCommand::Quit).unwrap();
    let exit = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(EngineEvent::Exited(x)) = events.recv().await {
                return x;
            }
        }
    })
    .await
    .expect("the session must still answer Quit");
    assert_eq!(exit.reason, ExitReason::Quit);
    assert!(
        began.elapsed() < Duration::from_secs(3),
        "{:?}",
        began.elapsed()
    );
    assert_pid_gone(pid).await;
}

#[cfg(unix)]
#[tokio::test]
async fn probe_sends_uci_and_never_the_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("received.txt");
    let body = format!(
        r#"while IFS= read -r line; do
  echo "$line" >> "{}"
  case "$line" in
    uci) echo "id name Logger"; echo "option name Hash type spin default 16 min 1 max 1024"; echo uciok;;
    isready) echo readyok;;
    quit) exit 0;;
  esac
done"#,
        log.display()
    );
    let path = scripts::script(dir.path(), "logger-engine", &body);
    let mut config = EngineConfig::new(&path);
    config.options = vec![("Hash".into(), OptionValue::Int(64))];
    let mut identity = None;
    for _ in 0..20 {
        match probe_uci(config.clone()).await {
            Ok(i) => {
                identity = Some(i);
                break;
            }
            Err(EngineError::Spawn { .. }) => tokio::time::sleep(Duration::from_millis(50)).await,
            Err(e) => panic!("{e}"),
        }
    }
    assert_eq!(identity.unwrap().name, "Logger");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let received = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = received.lines().collect();
    assert_eq!(lines.first(), Some(&"uci"));
    assert!(
        lines.iter().all(|l| matches!(*l, "uci" | "quit")),
        "the probe sent more than uci/quit: {lines:?}"
    );
}

// ---------------------------------------------------------------------------
// build_command (pure; the three platforms)
// ---------------------------------------------------------------------------

fn plan_config(command: impl Into<PathBuf>) -> EngineConfig {
    let mut c = EngineConfig::new(command);
    c.args = vec![
        "--threads".into(),
        "2 with space".into(),
        r#"quo"te"#.into(),
    ];
    c
}

#[test]
fn the_plan_keeps_program_and_arguments_separate() {
    let exe = std::env::temp_dir().join("engines dir").join("stockfish");
    let plan = build_command_with(&plan_config(&exe), None);
    assert_eq!(plan.program, exe);
    assert_eq!(
        plan.args,
        [
            OsString::from("--threads"),
            OsString::from("2 with space"),
            OsString::from(r#"quo"te"#)
        ]
    );
}

#[test]
fn the_plan_runs_in_the_engine_directory_unless_told_otherwise() {
    let exe = std::env::temp_dir().join("engines").join("stockfish");
    let plan = build_command_with(&plan_config(&exe), None);
    assert_eq!(plan.current_dir.as_deref(), exe.parent());
    let mut config = plan_config(&exe);
    let work = std::env::temp_dir().join("work");
    config.working_dir = Some(work.clone());
    assert_eq!(build_command_with(&config, None).current_dir, Some(work));
}

#[test]
fn a_bare_program_name_is_left_for_the_path_lookup() {
    let plan = build_command_with(&plan_config("stockfish"), None);
    assert_eq!(plan.program, Path::new("stockfish"));
    assert_eq!(plan.current_dir, None);
}

#[test]
fn a_relative_path_with_directories_becomes_absolute() {
    let relative = Path::new("engines").join("stockfish");
    let plan = build_command_with(&plan_config(&relative), None);
    assert!(plan.program.is_absolute(), "{}", plan.program.display());
    assert!(plan.program.ends_with(&relative));
    let dir = plan.current_dir.expect("the engine's directory");
    assert!(dir.is_absolute());
    assert_eq!(Some(dir.as_path()), plan.program.parent());
}

#[test]
fn windows_flags_exist_only_on_windows() {
    let plan = build_command_with(&plan_config("engine.exe"), None);
    if cfg!(windows) {
        assert_eq!(plan.creation_flags, CREATE_NO_WINDOW);
    } else {
        assert_eq!(plan.creation_flags, 0);
    }
    assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
    assert_eq!(IDLE_PRIORITY_CLASS, 0x0000_0040);
}

#[test]
fn low_priority_is_idle_class_on_windows_and_nice_on_unix() {
    let mut config = plan_config("stockfish");
    config.low_priority = true;
    if cfg!(windows) {
        let plan = build_command_with(&config, None);
        assert_eq!(plan.creation_flags, CREATE_NO_WINDOW | IDLE_PRIORITY_CLASS);
        assert_eq!(plan.program, Path::new("stockfish"));
    } else {
        let nice = Path::new("/usr/bin/nice");
        let plan = build_command_with(&config, Some(nice));
        assert_eq!(plan.program, nice);
        assert_eq!(NICE_INCREMENT, "15");
        assert_eq!(
            plan.args[..3],
            [
                OsString::from("-n"),
                OsString::from("15"),
                OsString::from("stockfish")
            ]
        );
        assert_eq!(plan.args.len(), 3 + 3, "the engine's own arguments follow");
        assert_eq!(plan.args[3], OsString::from("--threads"));
        assert_eq!(plan.creation_flags, 0);
        assert!(plan.note.is_none());
        // Without `nice`: runs normally and leaves a note for the log.
        let plain = build_command_with(&config, None);
        assert_eq!(plain.program, Path::new("stockfish"));
        assert_eq!(plain.args.len(), 3);
        assert!(plain.note.is_some());
    }
}

#[test]
fn normal_priority_never_wraps_the_engine() {
    let config = plan_config("stockfish");
    let plan = build_command_with(&config, Some(Path::new("/usr/bin/nice")));
    assert_eq!(plan.program, Path::new("stockfish"));
    assert_eq!(plan.args.len(), 3);
    assert!(plan.note.is_none());
}

#[test]
fn build_command_is_deterministic() {
    let mut config = plan_config("stockfish");
    config.low_priority = true;
    assert_eq!(build_command(&config), build_command(&config));
}

#[cfg(unix)]
#[tokio::test]
async fn a_low_priority_engine_starts_and_works() {
    let mut config = fake(&[]);
    config.low_priority = true;
    let (engine, mut events) = Engine::start(config).await.unwrap();
    engine.send(EngineCommand::Search(depth(KPK, 1))).unwrap();
    assert!(best_of(&mut events).await.mv.is_some());
    engine.shutdown().await;
}
