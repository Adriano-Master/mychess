//! M4-2 end-to-end tests of the session against the scripted XBoard fake
//! engine (`ScriptedEngine`), in virtual time. The commands the engine
//! received are read only after an event proves it has had time to read them.

#[path = "m4_1_common/helpers.rs"]
mod common;

use std::time::Duration;

use common::*;
use mychess_core::Position;
use mychess_engine::testing::{ScriptedEngine, XBoardFake};
use mychess_engine::{
    Bound, EngineCommand, EngineConfig, EngineEvent, ExitReason, Phase, Protocol, Score,
    SearchInfo, SearchRequest,
};
use tokio::time::Instant;

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const MATE_WHITE: &str = "6k1/5ppp/8/8/8/8/8/R3K3 w Q - 0 1";
const MATE_BLACK: &str = "r3k3/8/8/8/8/8/5PPP/6K1 b q - 0 1";

fn cfg() -> EngineConfig {
    let mut c = EngineConfig::new("fake");
    c.protocol = Protocol::XBoard;
    c.info_interval = Duration::ZERO;
    c
}

fn dialect(f: impl FnOnce(&mut XBoardFake)) -> ScriptedEngine {
    let mut d = XBoardFake::default();
    f(&mut d);
    ScriptedEngine::builder().xboard(d).build()
}

async fn infos(events: &mut mychess_engine::EngineEvents, n: usize) -> Vec<SearchInfo> {
    let mut v = Vec::new();
    while v.len() < n {
        v.push(next_info(events).await);
    }
    v
}

async fn first_info_of(fake: &ScriptedEngine, request: SearchRequest) -> SearchInfo {
    let (engine, mut events) = fake.start(cfg()).await.expect("start");
    engine.send(EngineCommand::Search(request)).expect("send");
    let info = next_info(&mut events).await;
    engine.shutdown().await;
    info
}

async fn info_at_depth(fake: &ScriptedEngine, depth: u32) -> SearchInfo {
    let (engine, mut events) = fake.start(cfg()).await.expect("start");
    engine
        .send(EngineCommand::Search(infinite(START_FEN)))
        .expect("send");
    let info = until(&mut events, |e| match e {
        EngineEvent::Info(i) if i.depth == Some(depth) => Some(i),
        _ => None,
    })
    .await;
    engine.shutdown().await;
    info
}

#[tokio::test(start_paused = true)]
async fn analyze_session_sends_the_exact_script_of_commands() {
    let fake = dialect(|_| {});
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    next_info(&mut events).await;
    engine.send(EngineCommand::Stop).unwrap();
    next_best(&mut events).await;
    settle().await;
    let want = [
        "xboard",
        "protover 2",
        "accepted myname",
        "accepted setboard",
        "accepted analyze",
        "accepted ping",
        "accepted done",
        "easy",
        "post",
        "force",
        "force",
        &format!("setboard {}", pos(KPK).fen()),
        "analyze",
        "ping 1",
        "exit",
    ];
    assert_eq!(fake.received(), want);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn infos_carry_growing_depth_and_a_legal_pv_and_stop_yields_its_first_move() {
    let fake = dialect(|_| {});
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    engine
        .send(EngineCommand::Search(infinite(START_FEN)))
        .unwrap();
    let list = infos(&mut events, 4).await;
    let depths: Vec<_> = list.iter().map(|i| i.depth.unwrap()).collect();
    assert!(depths.windows(2).all(|w| w[0] < w[1]), "{depths:?}");
    for i in &list {
        assert!(!i.pv.is_empty() && !i.pv_truncated);
        assert_eq!(i.multipv, 1);
        assert_eq!(
            i.time_ms,
            Some(u64::from(i.depth.unwrap()) * 100),
            "depth x 10 cs"
        );
        assert_eq!(i.nodes, Some(u64::from(i.depth.unwrap()) * 1000));
    }
    engine.send(EngineCommand::Stop).unwrap();
    let best = next_best(&mut events).await;
    assert!(best.mv.is_some());
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn san_and_coordinate_variations_give_the_same_pv() {
    // The first infos can be dropped legitimately (they precede the `pong`),
    // so compare at a fixed depth.
    let san = info_at_depth(&dialect(|d| d.san_pv = true), 3).await;
    let coord = info_at_depth(&dialect(|d| d.san_pv = false), 3).await;
    assert_eq!(san.pv.len(), 3);
    assert_eq!(san.pv, coord.pv);
    assert_eq!(san.score, coord.score);
}

#[tokio::test(start_paused = true)]
async fn whole_second_engines_report_depth_thousand_milliseconds() {
    let i = first_info_of(&dialect(|d| d.seconds = true), infinite(START_FEN)).await;
    // Without a banner the table does not apply: the fake's "seconds" are
    // read as centiseconds, as the spec says (quirks come from the table).
    assert_eq!(i.time_ms, Some(u64::from(i.depth.unwrap()) * 10));
    let i = first_info_of(
        &dialect(|d| {
            d.seconds = true;
            d.banner = Some("Baron 0.28a".to_owned());
        }),
        infinite(START_FEN),
    )
    .await;
    assert_eq!(i.time_ms, Some(u64::from(i.depth.unwrap()) * 1000));
}

#[tokio::test(start_paused = true)]
async fn black_to_move_scores_are_inverted_unless_the_engine_is_white_pov() {
    let normal = first_info_of(&dialect(|_| {}), infinite(BLACK_WINS_QUEEN)).await;
    let (Score::Cp(n), Bound::Exact) = normal.score.unwrap() else {
        panic!()
    };
    assert!(n < 0, "Black is winning: negative for White, got {n}");
    let white = first_info_of(
        &dialect(|d| {
            d.white_pov = true;
            d.banner = Some("Crafty v23.4 JA (1 cpus)".to_owned());
        }),
        infinite(BLACK_WINS_QUEEN),
    )
    .await;
    assert_eq!(
        white.score.map(|s| s.0),
        Some(Score::Cp(n)),
        "fake negates for white_pov; adapter must not"
    );
}

#[tokio::test(start_paused = true)]
async fn mate_in_one_is_a_mate_score_for_either_side() {
    for (fen, want) in [(MATE_WHITE, Score::Mate(1)), (MATE_BLACK, Score::Mate(-1))] {
        let i = first_info_of(&dialect(|_| {}), infinite(fen)).await;
        assert_eq!(i.score.map(|s| s.0), Some(want), "{fen}");
        assert_eq!(i.pv.len(), 1, "{fen}: mating move first");
    }
}

#[tokio::test(start_paused = true)]
async fn go_mode_runs_to_the_depth_and_ends_with_the_engines_move() {
    let fake = dialect(|_| {});
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    engine
        .send(EngineCommand::Search(depth(START_FEN, 3)))
        .unwrap();
    let events_to_best = until_best(&mut events).await;
    let depths: Vec<u32> = events_to_best
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Info(i) => i.depth,
            _ => None,
        })
        .collect();
    assert_eq!(depths, [1, 2, 3]);
    let Some(EngineEvent::BestMove(b)) = events_to_best.last() else {
        panic!()
    };
    assert!(b.mv.is_some());
    settle().await;
    let got = fake.received();
    assert!(
        got.ends_with(&[
            "new".to_owned(),
            "force".to_owned(),
            format!("setboard {START_FEN}"),
            "st 120000".to_owned(),
            "sd 3".to_owned(),
            "post".to_owned(),
            "ping 1".to_owned(),
            "go".to_owned()
        ]),
        "{got:?}"
    );
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn go_mode_with_movetime_ends_after_the_time_with_a_move() {
    let fake = dialect(|_| {});
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    let t0 = Instant::now();
    let request = limits(
        START_FEN,
        mychess_engine::SearchLimits {
            movetime: Some(Duration::from_millis(1200)),
            ..mychess_engine::SearchLimits::default()
        },
    );
    engine.send(EngineCommand::Search(request)).unwrap();
    let best = next_best(&mut events).await;
    assert!(best.mv.is_some());
    // 1200 ms rounds up to `st 2`.
    assert!(t0.elapsed() >= Duration::from_secs(2), "{:?}", t0.elapsed());
    assert!(fake.received().contains(&"st 2".to_owned()));
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn go_mode_stop_gets_the_move_from_the_engine() {
    let fake = dialect(|d| d.features = "setboard=1 ping=1 done=1".to_owned());
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    engine
        .send(EngineCommand::Search(infinite(START_FEN)))
        .unwrap();
    next_info(&mut events).await;
    engine.send(EngineCommand::Stop).unwrap();
    let best = next_best(&mut events).await;
    assert!(best.mv.is_some());
    settle().await;
    assert_eq!(fake.received().last().map(String::as_str), Some("?"));
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_ignores_the_question_mark_is_dropped_with_a_best_move_first() {
    let fake = ScriptedEngine::builder()
        .xboard(XBoardFake {
            features: "setboard=1 ping=1 done=1".to_owned(),
            ..XBoardFake::default()
        })
        .hang_on_stop()
        .build();
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    engine
        .send(EngineCommand::Search(infinite(START_FEN)))
        .unwrap();
    next_info(&mut events).await;
    engine.send(EngineCommand::Stop).unwrap();
    let mut saw_best = false;
    let exit = loop {
        match next(&mut events).await {
            EngineEvent::BestMove(_) => saw_best = true,
            EngineEvent::Exited(x) => break x,
            _ => {}
        }
    };
    assert!(saw_best);
    assert_eq!(exit.reason, ExitReason::StopTimeout);
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_never_pongs_times_out_on_the_first_search() {
    let fake = dialect(|d| d.no_pong = true);
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    engine
        .send(EngineCommand::Search(infinite(START_FEN)))
        .unwrap();
    assert_eq!(
        next_exit(&mut events).await.reason,
        ExitReason::Timeout(Phase::Pong)
    );
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_crashes_on_analyze_ends_as_crashed() {
    let fake = ScriptedEngine::builder()
        .xboard(XBoardFake::default())
        .crash_after_go()
        .build();
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    engine
        .send(EngineCommand::Search(infinite(START_FEN)))
        .unwrap();
    assert_eq!(next_exit(&mut events).await.reason, ExitReason::Crashed);
}

#[tokio::test(start_paused = true)]
async fn garbage_lines_among_thinking_output_are_ignored() {
    let fake = ScriptedEngine::builder()
        .xboard(XBoardFake::default())
        .garbage()
        .build();
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    engine
        .send(EngineCommand::Search(infinite(START_FEN)))
        .unwrap();
    let list = infos(&mut events, 3).await;
    assert!(list.iter().all(|i| !i.pv.is_empty()));
    engine.send(EngineCommand::Stop).unwrap();
    assert!(next_best(&mut events).await.mv.is_some());
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn crlf_output_from_the_engine_is_understood() {
    let fake = ScriptedEngine::builder()
        .xboard(XBoardFake::default())
        .crlf()
        .build();
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    engine
        .send(EngineCommand::Search(infinite(START_FEN)))
        .unwrap();
    assert_ne!(next_info(&mut events).await.pv.len(), 0);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn request_stats_end_to_end() {
    let fake = dialect(|_| {});
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    engine
        .send(EngineCommand::Search(infinite(START_FEN)))
        .unwrap();
    next_info(&mut events).await;
    engine.send(EngineCommand::RequestStats).unwrap();
    let stat = until(&mut events, |e| match e {
        EngineEvent::Info(i) if i.pv.is_empty() => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(stat.time_ms, Some(u64::from(stat.depth.unwrap()) * 100));
    assert_eq!(stat.score, None);
    assert!(fake.received().contains(&".".to_owned()));
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn usermove_engine_without_setboard_receives_usermove_commands() {
    let fake = dialect(|d| d.features = "analyze=1 usermove=1 done=1".to_owned());
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    let start = Position::new();
    let e4 = start.parse_uci("e2e4").unwrap();
    engine
        .send(EngineCommand::Search(SearchRequest::infinite(
            mychess_engine::SearchPosition {
                start,
                moves: vec![e4],
            },
        )))
        .unwrap();
    next_info(&mut events).await;
    settle().await;
    let got = fake.received();
    assert!(
        got.ends_with(&[
            "new".to_owned(),
            "force".to_owned(),
            "usermove e2e4".to_owned(),
            "black".to_owned(),
            "analyze".to_owned()
        ]),
        "{got:?}"
    );
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn legacy_fake_engine_with_a_crafty_banner_is_analysed_via_the_table() {
    let fake = dialect(|d| {
        d.legacy = true;
        d.banner = Some("Crafty v23.4 JA (1 cpus)".to_owned());
    });
    let (engine, mut events) = fake.start(cfg()).await.unwrap();
    engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    next_info(&mut events).await;
    settle().await;
    let got = fake.received();
    assert_eq!(
        got[..8],
        [
            "xboard",
            "protover 2",
            "log off",
            "noise 1000",
            "egtb off",
            "resign 0",
            "easy",
            "post"
        ]
    );
    assert!(got.contains(&format!("setboard {}", pos(KPK).fen())));
    assert!(got.contains(&"analyze".to_owned()));
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn slow_done_one_handshake_end_to_end() {
    let fake = dialect(|d| d.slow_feature = Duration::from_secs(9));
    let t0 = Instant::now();
    let (engine, _events) = fake.start(cfg()).await.unwrap();
    assert!(t0.elapsed() >= Duration::from_secs(9));
    engine.shutdown().await;
}
