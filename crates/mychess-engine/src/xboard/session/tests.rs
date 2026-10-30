//! Sanity tests of the XBoard session over in-memory pipes with the fake
//! engine. The full contract suite lives in `tests/`.

use std::time::Duration;

use mychess_core::Position;

use crate::engine::{
    Engine, EngineCommand, EngineConfig, EngineError, EngineEvent, EngineEvents, ExitReason, Phase,
};
use crate::protocol::{Protocol, Score};
use crate::request::{SearchLimits, SearchPosition, SearchRequest};
use crate::testing::{ScriptedEngine, XBoardFake};

/// Lines written before the first search with the default dialect:
/// `xboard`, `protover 2`, five answers, `easy`, `post`, `force`.
const INIT_LINES: usize = 10;

fn config() -> EngineConfig {
    let mut config = EngineConfig::new("fake");
    config.protocol = Protocol::XBoard;
    config
}

fn dialect(f: impl FnOnce(&mut XBoardFake)) -> ScriptedEngine {
    let mut d = XBoardFake::default();
    f(&mut d);
    ScriptedEngine::builder().xboard(d).build()
}

async fn next(events: &mut EngineEvents) -> EngineEvent {
    tokio::time::timeout(Duration::from_secs(60), events.recv())
        .await
        .expect("event in time")
        .expect("session alive")
}

async fn until<T>(events: &mut EngineEvents, mut pick: impl FnMut(EngineEvent) -> Option<T>) -> T {
    loop {
        if let Some(found) = pick(next(events).await) {
            return found;
        }
    }
}

async fn best_move(events: &mut EngineEvents) -> crate::engine::BestMove {
    until(events, |e| match e {
        EngineEvent::BestMove(b) => Some(b),
        _ => None,
    })
    .await
}

async fn started(events: &mut EngineEvents) {
    until(events, |e| {
        matches!(e, EngineEvent::SearchStarted(_)).then_some(())
    })
    .await;
}

const BLACK_CAPTURES: &str = "4k3/8/8/3p4/4P3/8/8/4K3 b - - 0 1";

fn search_fen(fen: &str) -> SearchRequest {
    SearchRequest::infinite(SearchPosition::new(Position::from_fen(fen).unwrap()))
}

fn search_start() -> SearchRequest {
    SearchRequest::infinite(SearchPosition::new(Position::new()))
}

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

async fn ready(fake: &ScriptedEngine) -> (Engine, EngineEvents) {
    let (engine, mut events) = fake.start(config()).await.unwrap();
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));
    (engine, events)
}

#[tokio::test(start_paused = true)]
async fn handshake_with_done_answers_features_and_reports_identity() {
    let fake = dialect(|_| {});
    let (engine, mut events) = fake.start(config()).await.unwrap();
    let EngineEvent::Ready(identity) = next(&mut events).await else {
        panic!("first event must be Ready");
    };
    assert_eq!(identity.name, "FakeFish");
    assert_eq!(identity.options.len(), 0);
    let caps = &identity.capabilities;
    assert_eq!(caps.multipv, None);
    assert!(!caps.search_moves && !caps.chess960 && !caps.ponder && !caps.wdl);
    assert_eq!(
        fake.received(),
        [
            "xboard",
            "protover 2",
            "accepted myname",
            "accepted setboard",
            "accepted analyze",
            "accepted ping",
            "accepted done",
            "easy",
            "post",
            "force"
        ]
    );
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn legacy_engine_is_ready_after_the_feature_deadline() {
    let fake = dialect(|d| d.legacy = true);
    let started = tokio::time::Instant::now();
    let (_engine, mut events) = fake.start(config().with_name("Oldie")).await.unwrap();
    assert!(started.elapsed() >= Duration::from_secs(2));
    let EngineEvent::Ready(identity) = next(&mut events).await else {
        panic!()
    };
    assert_eq!(identity.name, "Oldie");
    assert_eq!(
        fake.received(),
        ["xboard", "protover 2", "easy", "post", "force"]
    );
}

#[tokio::test(start_paused = true)]
async fn done_zero_waits_for_done_one() {
    let fake = dialect(|d| d.slow_feature = Duration::from_secs(8));
    let started = tokio::time::Instant::now();
    let (_engine, _events) = fake.start(config()).await.unwrap();
    assert!(started.elapsed() >= Duration::from_secs(8));
}

#[tokio::test(start_paused = true)]
async fn done_zero_for_too_long_times_out() {
    let fake = dialect(|d| d.slow_feature = Duration::from_secs(60));
    let error = fake.start(config()).await.unwrap_err();
    assert!(
        matches!(error, EngineError::Timeout(Phase::Features)),
        "{error:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn unsupported_features_are_rejected() {
    let fake = dialect(|d| {
        d.features = "san=1 option=\"Hash -spin 1 1 10\" smp=1 variants=\"normal\" done=1".into();
    });
    let _ready = ready(&fake).await;
    let lines = fake.received();
    assert_eq!(
        lines[3..8],
        [
            "rejected san",
            "rejected option",
            "rejected smp",
            "accepted variants",
            "accepted done"
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn crafty_banner_applies_the_quirk_and_its_commands() {
    let fake = dialect(|d| {
        d.legacy = true;
        d.banner = Some("Crafty v23.4 JA (1 cpus)".into());
    });
    let (engine, mut events) = ready(&fake).await;
    let lines = fake.received();
    assert_eq!(
        lines[2..],
        [
            "log off",
            "noise 1000",
            "egtb off",
            "resign 0",
            "easy",
            "post",
            "force"
        ]
    );
    // setboard and analyze were assumed.
    engine.send(EngineCommand::Search(search_start())).unwrap();
    until(&mut events, |e| {
        matches!(e, EngineEvent::SearchStarted(_)).then_some(())
    })
    .await;
    let tail = fake.received();
    assert!(tail.iter().any(|l| l == "analyze"), "{tail:?}");
    assert!(tail.iter().any(|l| l.starts_with("setboard ")), "{tail:?}");
}

#[tokio::test(start_paused = true)]
async fn announced_features_win_over_the_table() {
    let fake = dialect(|d| {
        d.banner = Some("Crafty v23.4".into());
        d.features = "analyze=0 setboard=0 done=1".into();
    });
    let (engine, _events) = ready(&fake).await;
    // Without setboard, a non-standard start is refused right in `send`.
    let odd = search_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
    assert!(matches!(
        engine.send(EngineCommand::Search(odd)),
        Err(EngineError::Unsupported("non-standard start position"))
    ));
}

#[tokio::test(start_paused = true)]
async fn analyze_sequence_with_setboard_and_ping() {
    let fake = dialect(|_| {});
    let (engine, mut events) = ready(&fake).await;
    engine.send(EngineCommand::Search(search_start())).unwrap();
    let id = until(&mut events, |e| match e {
        EngineEvent::SearchStarted(id) => Some(id),
        _ => None,
    })
    .await;
    let info = until(&mut events, |e| match e {
        EngineEvent::Info(i) => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(info.search, id);
    assert_eq!(info.multipv, 1);
    assert!(!info.pv.is_empty() && !info.pv_truncated);
    assert_eq!(info.score.map(|s| s.0), Some(Score::Cp(0)));
    let after_first = fake.received();
    assert_eq!(
        after_first[INIT_LINES..],
        [
            "force".to_owned(),
            format!("setboard {START_FEN}"),
            "analyze".to_owned(),
            "ping 1".to_owned()
        ]
    );
    // A new position replaces the analysis: exit, force, setboard, analyze.
    let fen2 = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
    engine
        .send(EngineCommand::Search(search_fen(fen2)))
        .unwrap();
    let old = best_move(&mut events).await;
    assert_eq!(old.search, id);
    assert_eq!(old.mv, info.pv.first().copied());
    let id2 = until(&mut events, |e| match e {
        EngineEvent::SearchStarted(id) => Some(id),
        _ => None,
    })
    .await;
    assert!(id2 > id);
    let info2 = until(&mut events, |e| match e {
        EngineEvent::Info(i) if i.search == id2 => Some(i),
        _ => None,
    })
    .await;
    assert_ne!(info2.pv.len(), 0);
    let lines = fake.received();
    assert_eq!(
        lines[INIT_LINES + 4..],
        [
            "exit".to_owned(),
            "force".to_owned(),
            format!("setboard {fen2}"),
            "analyze".to_owned(),
            "ping 2".to_owned()
        ]
    );
    // Stop sends `exit` and synthesizes the best move.
    engine.send(EngineCommand::Stop).unwrap();
    let last = best_move(&mut events).await;
    assert_eq!(last.search, id2);
    assert!(last.mv.is_some());
    assert_eq!(fake.received().last().map(String::as_str), Some("exit"));
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn analyze_without_setboard_sends_only_the_difference() {
    let fake = dialect(|d| d.features = "analyze=1 done=1".into());
    let (engine, mut events) = ready(&fake).await;
    let start = Position::new();
    let e4 = start.parse_uci("e2e4").unwrap();
    let after = start.after(e4).unwrap();
    let e5 = after.parse_uci("e7e5").unwrap();
    let position = |moves: &[mychess_core::Move]| {
        SearchRequest::infinite(SearchPosition {
            start: start.clone(),
            moves: moves.to_vec(),
        })
    };
    let d4 = start.parse_uci("d2d4").unwrap();
    for moves in [&[][..], &[e4], &[e4, e5], &[e4], &[d4]] {
        engine.send(EngineCommand::Search(position(moves))).unwrap();
        started(&mut events).await;
    }
    let lines = fake.received();
    let tail: Vec<&str> = lines[8..].iter().map(String::as_str).collect();
    assert_eq!(
        tail,
        [
            // first: from scratch
            "new", "force", "white", "analyze", // + e2e4
            "exit", "force", "e2e4", "analyze", // + e7e5
            "exit", "force", "e7e5", "analyze", // back one move
            "exit", "force", "undo", "analyze", // a different line: from scratch
            "exit", "new", "force", "d2d4", "black", "analyze",
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn engine_without_analyze_uses_go() {
    let fake = dialect(|d| d.features = "setboard=1 ping=1 done=1".into());
    let (engine, mut events) = ready(&fake).await;
    engine.send(EngineCommand::Search(search_start())).unwrap();
    until(&mut events, |e| {
        matches!(e, EngineEvent::Info(_)).then_some(())
    })
    .await;
    let n = fake.received().len();
    assert_eq!(
        fake.received()[n - 8..],
        [
            "new".to_owned(),
            "force".to_owned(),
            format!("setboard {START_FEN}"),
            "st 120000".to_owned(),
            "sd 50".to_owned(),
            "post".to_owned(),
            "ping 1".to_owned(),
            "go".to_owned()
        ]
    );
    engine.send(EngineCommand::Stop).unwrap();
    let done = best_move(&mut events).await;
    assert!(done.mv.is_some());
    let lines = fake.received();
    assert_eq!(lines[lines.len() - 1], "?");
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn depth_limit_goes_through_sd_and_ends_with_the_engine_move() {
    // The engine could analyze, but a limit needs `st`/`sd`.
    let fake = dialect(|_| {});
    let (engine, mut events) = ready(&fake).await;
    let mut request = search_start();
    request.limits = SearchLimits {
        depth: Some(3),
        movetime: Some(Duration::from_millis(1200)),
        ..SearchLimits::default()
    };
    engine.send(EngineCommand::Search(request)).unwrap();
    let mut deepest = 0;
    let done = until(&mut events, |e| match e {
        EngineEvent::Info(i) => {
            deepest = deepest.max(i.depth.unwrap_or(0));
            None
        }
        EngineEvent::BestMove(b) => Some(b),
        _ => None,
    })
    .await;
    assert_eq!(deepest, 3);
    assert!(done.mv.is_some());
    let lines = fake.received();
    assert!(lines.iter().any(|l| l == "st 2") && lines.iter().any(|l| l == "sd 3"));
    assert!(!lines.iter().any(|l| l == "analyze"));
}

#[tokio::test(start_paused = true)]
async fn refusals_for_multipv_searchmoves_and_ponder() {
    let fake = dialect(|_| {});
    let (engine, _events) = ready(&fake).await;
    assert!(matches!(
        engine.send(EngineCommand::SetMultiPv(2)),
        Err(EngineError::Unsupported("MultiPV"))
    ));
    assert!(engine.send(EngineCommand::SetMultiPv(1)).is_ok());
    let start = Position::new();
    let mut request = search_start();
    request.search_moves = vec![start.parse_uci("e2e4").unwrap()];
    assert!(matches!(
        engine.send(EngineCommand::Search(request)),
        Err(EngineError::Unsupported("searchmoves"))
    ));
    let mut request = search_start();
    request.ponder = true;
    assert!(matches!(
        engine.send(EngineCommand::Search(request)),
        Err(EngineError::Unsupported("ponder"))
    ));
}

#[tokio::test(start_paused = true)]
async fn request_stats_returns_an_info_without_a_line() {
    let fake = dialect(|_| {});
    // No rate limit, so the report is not merged into the previous line.
    let mut cfg = config();
    cfg.info_interval = Duration::ZERO;
    let (engine, mut events) = fake.start(cfg).await.unwrap();
    engine.send(EngineCommand::Search(search_start())).unwrap();
    until(&mut events, |e| {
        matches!(e, EngineEvent::Info(_)).then_some(())
    })
    .await;
    engine.send(EngineCommand::RequestStats).unwrap();
    let stat = until(&mut events, |e| match e {
        EngineEvent::Info(i) if i.pv.is_empty() => Some(i),
        _ => None,
    })
    .await;
    assert!(stat.score.is_none() && stat.nodes.is_some() && stat.depth.is_some());
    assert!(fake.received().iter().any(|l| l == "."));
}

#[tokio::test(start_paused = true)]
async fn terminal_position_is_answered_without_the_engine() {
    let fake = dialect(|_| {});
    let (engine, mut events) = ready(&fake).await;
    // Fool's mate: White is checkmated.
    let mated = "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3";
    let before = fake.received().len();
    engine
        .send(EngineCommand::Search(search_fen(mated)))
        .unwrap();
    let done = best_move(&mut events).await;
    assert_eq!(done.mv, None);
    assert_eq!(fake.received().len(), before);
}

#[tokio::test(start_paused = true)]
async fn scores_are_white_point_of_view_and_time_is_centiseconds() {
    // Black captures a pawn: +100 for Black, so -100 for White.
    let fake = dialect(|d| d.features = "analyze=1 setboard=1 done=1".into());
    let (engine, mut events) = ready(&fake).await;
    engine
        .send(EngineCommand::Search(search_fen(BLACK_CAPTURES)))
        .unwrap();
    let info = until(&mut events, |e| match e {
        EngineEvent::Info(i) => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(info.score.map(|s| s.0), Some(Score::Cp(-100)));
    assert_eq!(info.time_ms, Some(u64::from(info.depth.unwrap()) * 100));
    engine.send(EngineCommand::Stop).unwrap();
    best_move(&mut events).await;
}

#[tokio::test(start_paused = true)]
async fn quirk_table_switches_seconds_and_white_scores() {
    let fake = dialect(|d| {
        d.legacy = true;
        d.banner = Some("Crafty v23.4 JA".into());
        d.white_pov = true;
    });
    let (engine, mut events) = ready(&fake).await;
    engine
        .send(EngineCommand::Search(search_fen(BLACK_CAPTURES)))
        .unwrap();
    let info = until(&mut events, |e| match e {
        EngineEvent::Info(i) => Some(i),
        _ => None,
    })
    .await;
    // Crafty >= 18: scores are White's already, so there is no flip.
    assert_eq!(info.score.map(|s| s.0), Some(Score::Cp(-100)));

    let fake = dialect(|d| {
        d.legacy = true;
        d.banner = Some("Baron 0.28a".into());
        d.seconds = true;
    });
    let (engine2, mut events2) = ready(&fake).await;
    engine2.send(EngineCommand::Search(search_start())).unwrap();
    let info = until(&mut events2, |e| match e {
        EngineEvent::Info(i) => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(info.time_ms, Some(u64::from(info.depth.unwrap()) * 1000));
    engine.shutdown().await;
    engine2.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn engine_that_ignores_stop_but_answers_ping_is_dropped_after_best_move() {
    let fake = ScriptedEngine::builder()
        .xboard(XBoardFake {
            features: "setboard=1 ping=1 done=1".into(),
            ..XBoardFake::default()
        })
        .hang_on_stop()
        .build();
    let (engine, mut events) = ready(&fake).await;
    engine.send(EngineCommand::Search(search_start())).unwrap();
    until(&mut events, |e| {
        matches!(e, EngineEvent::Info(_)).then_some(())
    })
    .await;
    engine.send(EngineCommand::Stop).unwrap();
    let done = best_move(&mut events).await;
    assert!(done.mv.is_some());
    let exit = until(&mut events, |e| match e {
        EngineEvent::Exited(info) => Some(info),
        _ => None,
    })
    .await;
    assert_eq!(exit.reason, ExitReason::StopTimeout);
}

#[tokio::test(start_paused = true)]
async fn missing_pong_ends_the_session() {
    let fake = dialect(|d| d.no_pong = true);
    let (engine, mut events) = ready(&fake).await;
    engine.send(EngineCommand::Search(search_start())).unwrap();
    let exit = until(&mut events, |e| match e {
        EngineEvent::Exited(info) => Some(info),
        _ => None,
    })
    .await;
    assert_eq!(exit.reason, ExitReason::Timeout(Phase::Pong));
}

#[tokio::test(start_paused = true)]
async fn quit_leaves_analyze_mode_first() {
    let fake = dialect(|_| {});
    let (engine, mut events) = ready(&fake).await;
    engine.send(EngineCommand::Search(search_start())).unwrap();
    until(&mut events, |e| {
        matches!(e, EngineEvent::Info(_)).then_some(())
    })
    .await;
    engine.send(EngineCommand::Quit).unwrap();
    let exit = until(&mut events, |e| match e {
        EngineEvent::Exited(info) => Some(info),
        _ => None,
    })
    .await;
    assert_eq!(exit.reason, ExitReason::Quit);
    // Let the fake engine read what was written before the pipe closed.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let lines = fake.received();
    assert_eq!(lines[lines.len() - 2..], ["exit", "quit"]);
}

#[tokio::test(start_paused = true)]
async fn probe_reads_identity_and_tells_whether_features_were_seen() {
    use crate::log::LogHandle;
    use crate::testing::{FakeBehavior, FakeProtocol, run_fake};
    for (legacy, expect_features) in [(false, true), (true, false)] {
        let (wire, read, write) = crate::testing::pipe_wire();
        let mut behavior = FakeBehavior {
            protocol: FakeProtocol::XBoard,
            ..FakeBehavior::default()
        };
        behavior.xboard.legacy = legacy;
        tokio::spawn(async move {
            drop(run_fake(read, write, behavior, None).await);
        });
        let log = LogHandle::new(100);
        let (identity, saw) = crate::xboard::handshake::probe(wire, &config(), &log)
            .await
            .unwrap();
        assert_eq!(saw, expect_features);
        assert_eq!(identity.options.len(), 0);
        assert_eq!(identity.name, if legacy { "fake" } else { "FakeFish" });
    }
}

fn clock_request() -> SearchRequest {
    use crate::request::Clock;
    let mut request = search_start();
    request.limits = SearchLimits {
        clock: Some(Clock {
            white_time: Duration::from_millis(61_500),
            black_time: Duration::from_secs(30),
            white_increment: Duration::from_millis(1000),
            black_increment: Duration::from_millis(1000),
            moves_to_go: None,
        }),
        ..SearchLimits::default()
    };
    request
}

#[tokio::test(start_paused = true)]
async fn a_clock_search_sends_level_time_and_otim_before_go() {
    // Spec M8-2c, D2522: no `analyze`, no `st`; the clock is the limit.
    let fake = dialect(|_| {});
    let (engine, mut events) = ready(&fake).await;
    engine.send(EngineCommand::Search(clock_request())).unwrap();
    let done = best_move(&mut events).await;
    assert!(done.mv.is_some());
    let lines = fake.received();
    let level = lines
        .iter()
        .position(|l| l == "level 0 1:01 1")
        .expect("level");
    let time = lines.iter().position(|l| l == "time 6150").expect("time");
    let otim = lines.iter().position(|l| l == "otim 3000").expect("otim");
    let go = lines.iter().position(|l| l == "go").expect("go");
    assert!(level < time && time < otim && otim < go, "{lines:?}");
    assert!(!lines.iter().any(|l| l == "analyze" || l.starts_with("st ")));
}

#[tokio::test(start_paused = true)]
async fn the_opponent_name_follows_every_new_and_the_result_ends_the_game() {
    let fake = dialect(|_| {});
    let (engine, mut events) = ready(&fake).await;
    engine
        .send(EngineCommand::Opponent("Rival\nquit \u{7}Two".to_owned()))
        .unwrap();
    engine.send(EngineCommand::NewGame).unwrap();
    engine.send(EngineCommand::Search(clock_request())).unwrap();
    best_move(&mut events).await;
    engine
        .send(EngineCommand::GameOver {
            result: "1-0".to_owned(),
            comment: "White {mates}\nquit".to_owned(),
        })
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let lines = fake.received();
    let new = lines.iter().position(|l| l == "new").expect("new");
    assert_eq!(lines[new + 1], "name Rival quit Two", "{lines:?}");
    assert_eq!(
        lines.last().map(String::as_str),
        Some("result 1-0 {White mates quit}")
    );
    // The injected `quit` never became a line of its own.
    assert_eq!(lines.iter().filter(|l| l.as_str() == "quit").count(), 0);
}

#[tokio::test(start_paused = true)]
async fn an_unknown_result_is_sent_as_a_star_and_a_busy_engine_is_left_alone() {
    let fake = dialect(|_| {});
    let (engine, mut events) = ready(&fake).await;
    engine
        .send(EngineCommand::GameOver {
            result: "banana".to_owned(),
            comment: String::new(),
        })
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(fake.received().last().map(String::as_str), Some("result *"));
    engine.send(EngineCommand::Search(search_start())).unwrap();
    started(&mut events).await;
    let before = fake.received().len();
    engine
        .send(EngineCommand::GameOver {
            result: "1-0".to_owned(),
            comment: String::new(),
        })
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        fake.received().len(),
        before,
        "nothing written while searching"
    );
}

#[tokio::test(start_paused = true)]
async fn a_resignation_ends_the_search_without_a_move() {
    // Spec M8-2c, D2526: no wait for the watchdog.
    let fake = dialect(|d| d.resign = true);
    let (engine, mut events) = ready(&fake).await;
    engine.send(EngineCommand::Search(clock_request())).unwrap();
    let done = best_move(&mut events).await;
    assert_eq!(done.mv, None);
}
