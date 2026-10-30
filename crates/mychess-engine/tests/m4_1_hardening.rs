//! M4-1 contract tests for the hardening round: memory caps (options, `var`s,
//! PV, text, lines, event queue), `Quit` and `shutdown` in every wait, the
//! `isready` counter (only the last `readyok` counts), D407/D619 revised
//! (an engine that ignores `stop` is given up), the `spin` repair and
//! session-level `proptest`s with hostile input.

#[path = "m4_1_common/helpers.rs"]
mod common;

use std::fmt::Write as _;
use std::time::Duration;

use common::*;
use mychess_engine::testing::ManualEngine;
use mychess_engine::{
    EngineCommand, EngineEvent, EngineWarning, ExitReason, LogKind, OptionKind, OptionValue,
};
use proptest::prelude::*;
use tokio::time::Instant;

fn info_line(depth: u32) -> String {
    format!("info depth {depth} multipv 1 score cp {depth} pv e2e4")
}

/// Starts a session and plays the handshake with `option_lines`; returns the
/// identity from `Ready`.
async fn ready_identity(
    option_lines: &[String],
) -> (
    ManualEngine,
    mychess_engine::Engine,
    mychess_engine::EngineEvents,
    mychess_engine::EngineIdentity,
) {
    let (mut peer, engine, mut events) = ManualEngine::launch(config());
    let refs: Vec<&str> = option_lines.iter().map(String::as_str).collect();
    peer.handshake("Manual", &refs).await;
    let identity = until(&mut events, |e| match e {
        EngineEvent::Ready(id) => Some(id),
        _ => None,
    })
    .await;
    (peer, engine, events, identity)
}

fn log_notes(engine: &mychess_engine::Engine) -> Vec<String> {
    engine
        .log()
        .snapshot_since(0)
        .0
        .into_iter()
        .filter(|e| e.kind == LogKind::Note)
        .map(|e| e.text)
        .collect()
}

// ---------------------------------------------------------------------------
// Option caps and the spin repair
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn an_engine_announcing_thousands_of_options_is_capped_and_still_becomes_ready() {
    let lines: Vec<String> = (0..5000)
        .map(|i| format!("option name Opt{i} type check default false"))
        .collect();
    let (_peer, engine, _events, identity) = ready_identity(&lines).await;
    let n = identity.options.len();
    assert!((100..=1024).contains(&n), "{n} options kept");
    assert_eq!(identity.options[0].name, "Opt0", "the first ones are kept");
    assert!(!log_notes(&engine).is_empty(), "the cut leaves a note");
}

#[tokio::test(start_paused = true)]
async fn re_announcing_an_option_beyond_the_cap_still_replaces_it() {
    let mut lines: Vec<String> = (0..2000)
        .map(|i| format!("option name Opt{i} type check default false"))
        .collect();
    lines.push("option name Opt0 type check default true".to_owned());
    let (_peer, _engine, _events, identity) = ready_identity(&lines).await;
    let first = identity
        .options
        .iter()
        .find(|o| o.name == "Opt0")
        .expect("Opt0");
    assert_eq!(first.kind, OptionKind::Check { default: true });
    let count = identity.options.iter().filter(|o| o.name == "Opt0").count();
    assert_eq!(count, 1);
}

#[tokio::test(start_paused = true)]
async fn a_combo_with_a_huge_var_list_is_bounded() {
    let mut line = String::from("option name Big type combo default v0");
    for i in 0..20_000 {
        write!(line, " var v{i}").expect("string write");
    }
    let (_peer, _engine, _events, identity) = ready_identity(&[line]).await;
    let big = identity.options.iter().find(|o| o.name == "Big");
    if let Some(big) = big {
        let OptionKind::Combo { vars, .. } = &big.kind else {
            panic!("{big:?}")
        };
        assert!(vars.len() <= 1024, "{} vars", vars.len());
        assert_eq!(vars[0], "v0");
    }
}

#[tokio::test(start_paused = true)]
async fn an_option_with_an_enormous_name_is_not_kept_whole() {
    let long = "N".repeat(10_000);
    let lines = vec![
        format!("option name {long} type check default false"),
        "option name Fine type check default false".to_owned(),
    ];
    let (_peer, _engine, _events, identity) = ready_identity(&lines).await;
    assert!(identity.options.iter().all(|o| o.name.len() <= 4096));
    assert!(identity.options.iter().any(|o| o.name == "Fine"));
}

#[tokio::test(start_paused = true)]
async fn a_spin_default_outside_its_range_is_accepted_clamped_and_noted() {
    let lines = vec![
        "option name Low type spin default 0 min 5 max 10".to_owned(),
        "option name High type spin default 99 min 5 max 10".to_owned(),
        "option name AtMin type spin default 5 min 5 max 10".to_owned(),
        "option name AtMax type spin default 10 min 5 max 10".to_owned(),
        "option name Neg type spin default -100 min -50 max -10".to_owned(),
    ];
    let (_peer, engine, _events, identity) = ready_identity(&lines).await;
    let get = |name: &str| {
        identity
            .options
            .iter()
            .find(|o| o.name == name)
            .unwrap_or_else(|| panic!("{name} missing"))
            .kind
            .clone()
    };
    assert_eq!(
        get("Low"),
        OptionKind::Spin {
            default: 5,
            min: 5,
            max: 10
        }
    );
    assert_eq!(
        get("High"),
        OptionKind::Spin {
            default: 10,
            min: 5,
            max: 10
        }
    );
    assert_eq!(
        get("Neg"),
        OptionKind::Spin {
            default: -50,
            min: -50,
            max: -10
        }
    );
    // The repaired ones leave a note; the in-range ones do not.
    let notes = log_notes(&engine).join("\n");
    assert!(notes.contains("Low"), "{notes}");
    assert!(notes.contains("High"), "{notes}");
    assert!(
        !notes.contains("AtMin") && !notes.contains("AtMax"),
        "{notes}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_spin_with_min_above_max_is_rejected_and_the_rest_survives() {
    let lines = vec![
        "option name Broken type spin default 5 min 10 max 1".to_owned(),
        "option name Fine type spin default 5 min 1 max 10".to_owned(),
    ];
    let (_peer, engine, _events, identity) = ready_identity(&lines).await;
    assert!(identity.options.iter().all(|o| o.name != "Broken"));
    assert!(identity.options.iter().any(|o| o.name == "Fine"));
    assert!(log_notes(&engine).join("\n").contains("Broken"));
}

#[tokio::test(start_paused = true)]
async fn a_repaired_spin_is_a_normal_option_afterwards() {
    let lines = vec!["option name Skill type spin default 50 min 0 max 20".to_owned()];
    let (mut peer, engine, events, _identity) = ready_identity(&lines).await;
    engine
        .send(EngineCommand::SetOption {
            name: "Skill".into(),
            value: OptionValue::Int(20),
        })
        .unwrap();
    expect_lines(&mut peer, &["setoption name Skill value 20", "isready"]).await;
    peer.say("readyok").await;
    // The repaired range 0..=20 is enforced: 21 is refused.
    assert!(
        engine
            .send(EngineCommand::SetOption {
                name: "Skill".into(),
                value: OptionValue::Int(21),
            })
            .is_err()
    );
    drop(events);
}

// ---------------------------------------------------------------------------
// Hostile `info` / text / lines
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn a_pv_of_a_hundred_thousand_legal_moves_keeps_at_most_256() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let mut line = String::from("info depth 1 score cp 1 pv");
    for _ in 0..25_000 {
        line.push_str(" e1d1 e8d8 d1e1 d8e8");
    }
    peer.say(&line).await;
    let info = next_info(&mut events).await;
    assert_eq!(info.pv.len(), 256);
    assert_eq!(info.depth, Some(1));
}

#[tokio::test(start_paused = true)]
async fn a_huge_info_string_is_bounded_and_the_line_survives() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let text = "x".repeat(500_000);
    peer.say(&format!("info depth 3 string {text}")).await;
    let info = next_info(&mut events).await;
    assert_eq!(info.depth, Some(3));
    assert!(info.string.as_deref().is_none_or(|s| s.len() <= 8192));
}

#[tokio::test(start_paused = true)]
async fn out_of_range_numbers_lose_only_their_field() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    peer.say(
        "info depth 7 multipv 99999999999 nodes -5 nps 18446744073709551616 \
         score cp 99999999999999 hashfull 4294967297 time 12",
    )
    .await;
    let info = next_info(&mut events).await;
    assert_eq!(info.depth, Some(7));
    assert_eq!(info.time_ms, Some(12));
    assert_eq!(info.nodes, None);
    assert_eq!(info.nps, None);
    assert_eq!(info.score, None);
    assert!(info.multipv >= 1, "multipv {}", info.multipv);
}

#[tokio::test(start_paused = true)]
async fn many_oversized_lines_keep_the_log_and_the_session_bounded() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let big = vec![b'z'; 2 * 1024 * 1024];
    for _ in 0..20 {
        peer.say_raw(&big).await;
        peer.say_raw(b"\n").await;
    }
    peer.say(&info_line(4)).await;
    assert_eq!(next_info(&mut events).await.depth, Some(4));
    let (entries, _) = engine.log().snapshot_since(0);
    assert!(
        entries.iter().all(|e| e.text.len() <= 4096 + 64),
        "an entry of {} bytes",
        entries.iter().map(|e| e.text.len()).max().unwrap_or(0)
    );
    assert!(
        entries
            .iter()
            .any(|e| e.kind == LogKind::Note && e.text.contains("1 MiB")),
        "no note about the cut line"
    );
}

#[tokio::test(start_paused = true)]
async fn a_line_just_under_one_mebibyte_is_not_cut() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let pad = "y".repeat(1_040_000);
    peer.say(&format!("info depth 9 string {pad}")).await;
    peer.say(&info_line(10)).await;
    let mut depths = Vec::new();
    while depths.last() != Some(&10) {
        depths.extend(next_info(&mut events).await.depth);
    }
    assert!(depths.contains(&9) || depths == [10], "{depths:?}");
    assert!(
        !log_notes(&engine).iter().any(|t| t.contains("1 MiB")),
        "a line below the cap must not be reported as cut"
    );
}

// ---------------------------------------------------------------------------
// Quit / shutdown in every wait
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn shutdown_during_the_handshake_returns_promptly() {
    let (mut peer, engine, events) = ManualEngine::launch(config());
    assert_eq!(line(&mut peer).await, "uci");
    let began = Instant::now();
    tokio::time::timeout(Duration::from_secs(5), engine.shutdown())
        .await
        .expect("shutdown returns");
    assert!(began.elapsed() < Duration::from_secs(2));
    drop(events);
}

/// `shutdown` is the polite way out: whatever the scheduling, the app must
/// see `Exited(Quit)` and the engine must get the full quit wait.
#[tokio::test(start_paused = true)]
async fn shutdown_always_reports_exited_quit_as_the_last_event() {
    let mut wrong = Vec::new();
    for round in 0..64 {
        let (_peer, engine, mut events) = ready(&[]).await;
        engine.shutdown().await;
        let mut last = None;
        while let Some(e) = events.recv().await {
            last = Some(e);
        }
        match last {
            Some(EngineEvent::Exited(x)) if x.reason == ExitReason::Quit => {}
            other => wrong.push((round, other)),
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of 64 rounds: {:?}",
        wrong.len(),
        wrong.first()
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_always_gives_a_stubborn_engine_the_whole_quit_wait_before_the_kill() {
    use mychess_engine::testing::ScriptedEngine;
    let mut short = Vec::new();
    for round in 0..32 {
        let fake = ScriptedEngine::builder().ignore_quit().build();
        let (engine, _events) = fake.start(config()).await.unwrap();
        let began = Instant::now();
        engine.shutdown().await;
        if began.elapsed() < Duration::from_millis(450) {
            short.push((round, began.elapsed()));
        }
    }
    assert!(
        short.is_empty(),
        "{} of 32: {:?}",
        short.len(),
        short.first()
    );
}

#[tokio::test(start_paused = true)]
async fn quit_during_a_setoption_barrier_ends_the_session_with_quit() {
    let (mut peer, engine, mut events) = ready(&[MULTIPV4]).await;
    engine.send(EngineCommand::SetMultiPv(2)).unwrap();
    expect_lines(&mut peer, &["setoption name MultiPV value 2", "isready"]).await;
    engine.send(EngineCommand::Quit).unwrap();
    assert_eq!(next_exit(&mut events).await.reason, ExitReason::Quit);
    assert_eq!(line(&mut peer).await, "quit");
}

#[tokio::test(start_paused = true)]
async fn quit_while_probing_a_stop_ends_with_quit_not_stop_timeout() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::Stop).unwrap();
    expect_lines(&mut peer, &["stop", "isready"]).await;
    engine.send(EngineCommand::Quit).unwrap();
    assert_eq!(next_exit(&mut events).await.reason, ExitReason::Quit);
}

#[tokio::test(start_paused = true)]
async fn quit_and_shutdown_work_while_the_event_queue_is_full_and_nobody_reads() {
    let mut cfg = config();
    cfg.info_interval = Duration::ZERO;
    let (mut peer, engine, mut events) = ready_with(cfg, &[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let mut blob = String::new();
    for depth in 1..=5000_u32 {
        blob.push_str(&info_line(depth));
        blob.push('\n');
    }
    peer.say_raw(blob.as_bytes()).await;
    peer.say("bestmove e2e4").await; // blocks in `emit`: the queue is full
    settle().await;
    let began = Instant::now();
    engine.send(EngineCommand::Quit).unwrap();
    let mut saw_quit = false;
    while let Some(l) = tokio::time::timeout(Duration::from_secs(5), peer.next_line())
        .await
        .expect("the pipe closes without the consumer reading")
    {
        saw_quit |= l == "quit";
    }
    assert!(saw_quit);
    assert!(began.elapsed() < Duration::from_secs(4));
    // The consumer, late, still reaches the end of the stream.
    while tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("the stream ends")
        .is_some()
    {}
}

#[tokio::test(start_paused = true)]
async fn commands_sent_while_the_session_waits_for_queue_room_are_not_lost() {
    let mut cfg = config();
    cfg.info_interval = Duration::ZERO;
    let (mut peer, engine, mut events) = ready_with(cfg, &[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    let mut blob = String::new();
    for depth in 1..=5000_u32 {
        blob.push_str(&info_line(depth));
        blob.push('\n');
    }
    peer.say_raw(blob.as_bytes()).await;
    peer.say("bestmove e2e4").await;
    settle().await;
    engine.send(EngineCommand::NewGame).unwrap();
    settle().await;
    // The consumer wakes up and drains everything.
    next_best(&mut events).await;
    expect_lines(&mut peer, &["ucinewgame", "isready"]).await;
    peer.say("readyok").await;
    engine.shutdown().await;
}

// ---------------------------------------------------------------------------
// The isready counter
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn an_old_readyok_does_not_close_a_newer_barrier() {
    let (mut peer, engine, mut events) = ready(&[HASH]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::Stop).unwrap();
    expect_lines(&mut peer, &["stop"]).await;
    // No bestmove in 1.5 s: probe (isready #1).
    assert_eq!(line(&mut peer).await, "isready");
    // The bestmove limps in after all; the barrier of #1 is gone.
    peer.say("bestmove e2e4").await;
    next_best(&mut events).await;
    engine
        .send(EngineCommand::SetOption {
            name: "Hash".into(),
            value: OptionValue::Int(32),
        })
        .unwrap();
    engine.send(EngineCommand::Search(depth(KPK, 3))).unwrap();
    expect_lines(&mut peer, &["setoption name Hash value 32", "isready"]).await;
    // The answer to the probe (#1) arrives now: it must not release #2.
    peer.say("readyok").await;
    let early = tokio::time::timeout(Duration::from_millis(200), peer.next_line()).await;
    assert!(
        early.is_err(),
        "search started behind a stale readyok: {early:?}"
    );
    peer.say("readyok").await;
    let position = line(&mut peer).await;
    assert!(position.starts_with("position "), "{position}");
    assert_eq!(line(&mut peer).await, "go depth 3");
}

#[tokio::test(start_paused = true)]
async fn a_bestmove_during_the_probe_is_the_normal_path_and_the_late_readyok_is_harmless() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let (a, _, _) = start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::Stop).unwrap();
    expect_lines(&mut peer, &["stop", "isready"]).await;
    peer.say("bestmove e2e4").await;
    let seen = until_best(&mut events).await;
    let Some(EngineEvent::BestMove(best)) = seen.last() else {
        unreachable!()
    };
    assert_eq!((best.search, best.mv.is_some()), (a, true));
    assert!(
        !seen
            .iter()
            .any(|e| matches!(e, EngineEvent::Warning(EngineWarning::Misbehaved(_))))
    );
    // A new search goes out at once, and the late readyok does not disturb it.
    let (b, position, go) = start_search(&mut peer, &engine, &mut events, depth(KPK, 5)).await;
    assert!(b > a);
    assert!(position.starts_with("position "), "{position}");
    assert_eq!(go, "go depth 5");
    peer.say("readyok").await;
    settle().await;
    assert!(
        !drain(&mut events)
            .iter()
            .any(|e| matches!(e, EngineEvent::Exited(_) | EngineEvent::BestMove(_)))
    );
    engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(line(&mut peer).await, "stop");
}

// ---------------------------------------------------------------------------
// D619 (revised)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn d619_outcome_is_warning_then_old_bestmove_none_then_exit_and_the_queued_search_dies() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    let (a, _, _) = start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    // A new search arrives while stopping; the engine ignores `stop`.
    engine.send(EngineCommand::Search(depth(KPK, 4))).unwrap();
    assert_eq!(line(&mut peer).await, "stop");
    assert_eq!(line(&mut peer).await, "isready");
    let began = Instant::now();
    peer.say("readyok").await; // alive, but it ignored `stop`
    let mut seen = Vec::new();
    let exit = loop {
        match next(&mut events).await {
            EngineEvent::Exited(x) => break x,
            other => seen.push(other),
        }
    };
    assert_eq!(exit.reason, ExitReason::StopTimeout);
    // The answer came at 1.5 s; the verdict does not wait for the 3 s mark.
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "{:?}",
        began.elapsed()
    );
    let warn_at = seen
        .iter()
        .position(|e| matches!(e, EngineEvent::Warning(EngineWarning::Misbehaved(_))))
        .expect("Warning(Misbehaved)");
    let best_at = seen
        .iter()
        .position(|e| matches!(e, EngineEvent::BestMove(b) if b.search == a && b.mv.is_none()))
        .expect("BestMove(None) of the old search");
    assert!(warn_at < best_at, "{seen:?}");
    // The queued search never reached the engine nor the app.
    assert!(
        !seen
            .iter()
            .any(|e| matches!(e, EngineEvent::SearchStarted(_)))
    );
    let mut rest = Vec::new();
    while let Some(l) = peer.next_line().await {
        rest.push(l);
    }
    assert!(
        rest.iter().all(|l| l == "quit"),
        "the engine kept receiving commands: {rest:?}"
    );
    assert!(engine.send(EngineCommand::Stop).is_err());
}

#[tokio::test(start_paused = true)]
async fn d619_a_second_stop_while_probing_changes_nothing() {
    let (mut peer, engine, mut events) = ready(&[]).await;
    start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
    engine.send(EngineCommand::Stop).unwrap();
    expect_lines(&mut peer, &["stop", "isready"]).await;
    engine.send(EngineCommand::Stop).unwrap();
    settle().await;
    let began = Instant::now();
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::StopTimeout);
    assert!(began.elapsed() >= Duration::from_millis(1400));
}

// ---------------------------------------------------------------------------
// proptest: hostile sessions never panic and keep their invariants
// ---------------------------------------------------------------------------

const VOCAB: &[&str] = &[
    "info",
    "depth",
    "seldepth",
    "multipv",
    "score",
    "cp",
    "mate",
    "lowerbound",
    "upperbound",
    "wdl",
    "nodes",
    "nps",
    "pv",
    "currmove",
    "currmovenumber",
    "string",
    "hashfull",
    "tbhits",
    "cpuload",
    "time",
    "refutation",
    "currline",
    "bestmove",
    "ponder",
    "option",
    "name",
    "type",
    "spin",
    "check",
    "combo",
    "button",
    "default",
    "min",
    "max",
    "var",
    "e2e4",
    "e1d1",
    "e8d8",
    "e2e3",
    "a1a1",
    "0000",
    "(none)",
    "e7e8q",
    "0",
    "1",
    "-1",
    "42",
    "99999999999999999999",
    "-99999999999",
    "4294967296",
    "-",
    "<empty>",
    "stop",
    "go",
    "position",
    "quit",
    "ucinewgame",
];

fn token() -> impl Strategy<Value = String> {
    prop_oneof![
        8 => proptest::sample::select(VOCAB).prop_map(str::to_owned),
        2 => "[ -~]{0,12}",
        1 => any::<i64>().prop_map(|n| n.to_string()),
    ]
}

fn junk_line() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        8 => proptest::collection::vec(token(), 0..40).prop_map(|t| t.join(" ").into_bytes()),
        2 => proptest::collection::vec(any::<u8>(), 0..300),
    ]
    .prop_map(|mut bytes| {
        // One physical line per entry: strip separators; keep NUL/invalid UTF-8.
        bytes.retain(|b| *b != b'\n' && *b != b'\r');
        bytes
    })
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .expect("runtime")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn junk_during_a_search_never_kills_the_session_and_events_respect_the_caps(
        lines in proptest::collection::vec(junk_line(), 0..60),
    ) {
        runtime().block_on(async {
            let (mut peer, engine, mut events) = ready(&[MULTIPV4]).await;
            start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
            for l in &lines {
                // Lines that would legitimately end the engine's session are
                // not interesting; the rest is.
                peer.say_raw(l).await;
                peer.say_raw(b"\n").await;
            }
            settle().await;
            engine.send(EngineCommand::Quit).expect("send");
            let mut exited = None;
            while let Some(e) = events.recv().await {
                match e {
                    EngineEvent::Info(i) => {
                        prop_assert!(i.pv.len() <= 256);
                        prop_assert!(i.string.as_ref().is_none_or(|s| s.len() <= 8192));
                    }
                    EngineEvent::Exited(x) => exited = Some(x),
                    _ => {}
                }
            }
            let exit = exited.expect("a session that panicked would not say Exited");
            prop_assert_eq!(exit.reason, ExitReason::Quit);
            Ok(())
        })?;
    }

    #[test]
    fn junk_in_the_handshake_still_ends_in_ready_with_sane_options(
        lines in proptest::collection::vec(junk_line(), 0..40),
        options in proptest::collection::vec(
            proptest::collection::vec(token(), 0..25)
                .prop_map(|t| format!("option {}", t.join(" "))),
            0..40,
        ),
    ) {
        runtime().block_on(async {
            let (mut peer, engine, mut events) = ManualEngine::launch(config());
            assert_eq!(line(&mut peer).await, "uci");
            for l in &lines {
                peer.say_raw(l).await;
                peer.say_raw(b"\n").await;
            }
            for o in &options {
                peer.say(o).await;
            }
            peer.say("uciok").await;
            // The junk may contain a lone `uciok`-like token only by chance
            // (it is not in the vocabulary), so Ready must come.
            while let Some(l) = peer.next_line().await {
                if l == "isready" {
                    break;
                }
            }
            peer.say("readyok").await;
            let identity = until(&mut events, |e| match e {
                EngineEvent::Ready(id) => Some(id),
                _ => None,
            })
            .await;
            prop_assert!(identity.options.len() <= 1024);
            for o in &identity.options {
                prop_assert!(!o.name.is_empty() && o.name.len() <= 4096);
                match &o.kind {
                    OptionKind::Spin { default, min, max } => {
                        prop_assert!(min <= max);
                        prop_assert!((min..=max).contains(&default));
                    }
                    OptionKind::Combo { vars, .. } => prop_assert!(vars.len() <= 1024),
                    _ => {}
                }
            }
            engine.send(EngineCommand::Quit).expect("send");
            Ok(())
        })?;
    }

    #[test]
    fn any_bestmove_text_ends_the_search_exactly_once_with_a_legal_move_or_none(
        tokens in proptest::collection::vec(token(), 1..6)
            .prop_filter("a bare `bestmove` has no defined meaning", |t| {
                t.iter().any(|w| !w.trim().is_empty())
            }),
    ) {
        runtime().block_on(async {
            let (mut peer, engine, mut events) = ready(&[]).await;
            let (id, _, _) = start_search(&mut peer, &engine, &mut events, infinite(KPK)).await;
            peer.say(&format!("bestmove {}", tokens.join(" "))).await;
            settle().await;
            let seen = drain(&mut events);
            let bests: Vec<_> = seen
                .iter()
                .filter_map(|e| match e {
                    EngineEvent::BestMove(b) => Some(*b),
                    _ => None,
                })
                .collect();
            prop_assert_eq!(bests.len(), 1, "{:?}", seen);
            prop_assert_eq!(bests[0].search, id);
            if let Some(m) = bests[0].mv {
                prop_assert!(pos(KPK).legal_moves().contains(&m));
            }
            engine.send(EngineCommand::Quit).expect("send");
            Ok(())
        })?;
    }
}
