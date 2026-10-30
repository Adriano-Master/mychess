//! M4-2 contract tests: the XBoard handshake (spec
//! `docs/specs/m4-2-protocolo-xboard.md`, D416, D417, D421, D422, rule 8).
//! The engine is played by hand over an in-memory pipe, with a paused clock.

#[path = "m4_1_common/helpers.rs"]
mod common;
#[path = "m4_2_common/xb.rs"]
mod xb;

use std::time::Duration;

use common::*;
use mychess_core::Position;
use mychess_engine::testing::{ManualEngine, ScriptedEngine, XBoardFake};
use mychess_engine::{
    EngineCommand, EngineEvent, EngineWarning, ExitReason, Phase, SearchPosition, SearchRequest,
};
use tokio::time::Instant;
use xb::*;

async fn ready_identity(
    events: &mut mychess_engine::EngineEvents,
) -> mychess_engine::EngineIdentity {
    match next(events).await {
        EngineEvent::Ready(i) => i,
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn done_one_is_ready_at_once_with_the_exact_command_sequence() {
    let t0 = Instant::now();
    let r = full().await;
    assert!(
        t0.elapsed() < Duration::from_millis(100),
        "no deadline wait"
    );
    assert_eq!(
        strs(&r.init),
        [
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
}

#[tokio::test(start_paused = true)]
async fn xboard_then_protover_2_are_the_first_two_lines() {
    let (mut peer, _e, _ev) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
}

#[tokio::test(start_paused = true)]
async fn ready_carries_myname_no_options_and_the_d422_capabilities() {
    let (mut peer, _e, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    peer.say(r#"feature myname="Crafty Like 1.2" setboard=1 analyze=1 done=1"#)
        .await;
    let id = ready_identity(&mut events).await;
    assert_eq!(id.name, "Crafty Like 1.2");
    assert_eq!(id.options.len(), 0);
    let c = &id.capabilities;
    assert_eq!(c.multipv, None);
    assert!(!c.search_moves && !c.chess960 && !c.analyse_mode && !c.wdl && !c.ponder);
}

#[tokio::test(start_paused = true)]
async fn missing_or_empty_myname_falls_back_to_the_configured_name() {
    for feature in ["feature done=1", r#"feature myname="" done=1"#] {
        let (mut peer, _e, mut events) = ManualEngine::launch(xconfig().with_name("Configured"));
        expect_lines(&mut peer, &["xboard", "protover 2"]).await;
        peer.say(feature).await;
        assert_eq!(
            ready_identity(&mut events).await.name,
            "Configured",
            "{feature}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn feature_answers_are_exactly_the_d417_table() {
    let accepted = [
        "myname=\"X\"",
        "setboard=1",
        "analyze=1",
        "usermove=1",
        "ping=1",
        "time=1",
        "draw=1",
        "reuse=1",
        "name=1",
        "playother=1",
        "sigint=1",
        "variants=\"normal,fischerandom\"",
        "colors=0",
    ];
    let rejected = [
        "san=1",
        "colors=1",
        "option=\"Hash -spin 1 1 100\"",
        "memory=1",
        "smp=1",
        "egt=\"syzygy\"",
        "debug=1",
        "exclude=1",
        "setscore=1",
        "banana=1",
        "variants=\"suicide\"",
    ];
    for (items, verb) in [(&accepted[..], "accepted"), (&rejected[..], "rejected")] {
        for item in items {
            let (mut peer, _e, mut events) = ManualEngine::launch(xconfig());
            expect_lines(&mut peer, &["xboard", "protover 2"]).await;
            peer.say(&format!("feature {item}")).await;
            let key = item.split('=').next().unwrap();
            assert_eq!(line(&mut peer).await, format!("{verb} {key}"), "{item}");
            peer.say("feature done=1").await;
            drop(ready_identity(&mut events).await);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn several_pairs_on_one_line_are_answered_in_order_and_repeats_too() {
    let r = rig(&[r#"feature myname="A B" setboard=1 san=1 setboard=0 zzz=3 done=1"#]).await;
    assert_eq!(
        strs(&r.init)[..5],
        [
            "accepted myname",
            "accepted setboard",
            "rejected san",
            "accepted setboard",
            "rejected zzz"
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn feature_lines_split_across_several_lines_are_all_answered() {
    let r = rig(&[
        "feature myname=\"M\"",
        "feature setboard=1 analyze=1",
        "feature done=1",
    ])
    .await;
    assert_eq!(
        strs(&r.init)[..4],
        [
            "accepted myname",
            "accepted setboard",
            "accepted analyze",
            "accepted done"
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn chatter_before_and_between_features_is_ignored() {
    let r = rig_with(
        xconfig(),
        &["Hello, I am an engine", "", "   ", "garbage \u{7f} line"],
        &["feature setboard=1", "# comment", "feature done=1"],
    )
    .await;
    assert_eq!(strs(&r.init)[..2], ["accepted setboard", "accepted done"]);
}

#[tokio::test(start_paused = true)]
async fn done_zero_then_done_one_keeps_waiting_past_the_two_second_deadline() {
    let (mut peer, _e, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    peer.say(r#"feature myname="Slow" setboard=1 analyze=1 done=0"#)
        .await;
    expect_lines(
        &mut peer,
        &[
            "accepted myname",
            "accepted setboard",
            "accepted analyze",
            "accepted done",
        ],
    )
    .await;
    let t0 = Instant::now();
    tokio::time::sleep(Duration::from_secs(10)).await;
    settle().await;
    quiet(&mut events);
    peer.say("feature done=1").await;
    let id = ready_identity(&mut events).await;
    assert_eq!(id.name, "Slow");
    assert!(t0.elapsed() >= Duration::from_secs(10));
}

#[tokio::test(start_paused = true)]
async fn done_zero_never_followed_by_done_one_times_out_at_15_seconds() {
    let (mut peer, _e, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    let t0 = Instant::now();
    peer.say("feature done=0").await;
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::Timeout(Phase::Features));
    let waited = t0.elapsed();
    assert!(
        waited >= Duration::from_secs(14) && waited <= Duration::from_secs(16),
        "{waited:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn done_zero_ceiling_is_15_seconds_from_the_start_even_with_more_feature_lines() {
    let (mut peer, _e, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    let t0 = Instant::now();
    peer.say("feature done=0").await;
    // Keeps the engine chatty with feature lines every second: still 15 s max.
    for _ in 0..30 {
        peer.say("feature reuse=1").await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        if let Some(EngineEvent::Exited(x)) = events.try_recv() {
            assert_eq!(x.reason, ExitReason::Timeout(Phase::Features));
            assert!(
                t0.elapsed() <= Duration::from_secs(17),
                "{:?}",
                t0.elapsed()
            );
            return;
        }
    }
    panic!("the ceiling of 15 s did not end the handshake");
}

#[tokio::test(start_paused = true)]
async fn legacy_engine_without_any_feature_is_ready_after_two_seconds() {
    let t0 = Instant::now();
    let (mut peer, _e, mut events) = ManualEngine::launch(xconfig().with_name("Oldie"));
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    peer.say("Error (unknown command): protover").await;
    expect_lines(&mut peer, &["easy", "post", "force"]).await;
    let id = ready_identity(&mut events).await;
    let waited = t0.elapsed();
    assert!(
        waited >= Duration::from_secs(2) && waited < Duration::from_secs(3),
        "{waited:?}"
    );
    assert_eq!(id.name, "Oldie");
    assert_eq!(id.options.len(), 0);
}

#[tokio::test(start_paused = true)]
async fn features_without_done_renew_the_two_second_window() {
    let (mut peer, _e, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    let t0 = Instant::now();
    peer.say("feature setboard=1").await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    peer.say("feature analyze=1").await;
    settle().await;
    quiet(&mut events);
    tokio::time::sleep(Duration::from_millis(1000)).await;
    settle().await;
    quiet(&mut events);
    drop(ready_identity(&mut events).await);
    assert!(
        t0.elapsed() >= Duration::from_millis(3500),
        "{:?}",
        t0.elapsed()
    );
}

#[tokio::test(start_paused = true)]
async fn legacy_engine_is_analyse_less_and_setboard_less() {
    // protover 1: analyze=0 and setboard=0 (D416): an infinite search uses the
    // Go mode with the full move list, and no `setboard` is ever sent.
    let mut r = rig(&[]).await;
    assert_eq!(strs(&r.init), ["easy", "post", "force"]);
    let (_, lines) = r.start_raw(infinite(START_FEN)).await;
    assert_eq!(
        strs(&lines),
        ["new", "force", "white", "st 120000", "sd 50", "post", "go"]
    );
}

#[tokio::test(start_paused = true)]
async fn late_feature_lines_after_ready_are_still_answered() {
    let mut r = full().await;
    r.peer.say("feature san=1 myname=\"Late\"").await;
    expect_lines(&mut r.peer, &["rejected san", "accepted myname"]).await;
}

#[tokio::test(start_paused = true)]
async fn eof_during_the_handshake_ends_the_session_as_crashed() {
    let (mut peer, _e, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    peer.say("feature setboard=1").await;
    peer.close().await;
    let exit = next_exit(&mut events).await;
    assert_eq!(exit.reason, ExitReason::Crashed);
}

#[tokio::test(start_paused = true)]
async fn scripted_engine_with_only_done_one_starts_through_engine_start() {
    // `Engine::start` over a scripted engine that closes at once.
    let fake = ScriptedEngine::builder()
        .xboard(XBoardFake {
            features: "done=1".to_owned(),
            ..XBoardFake::default()
        })
        .build();
    let (engine, mut events) = fake.start(xconfig()).await.expect("handshake");
    drop(ready_identity(&mut events).await);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn crlf_line_endings_are_accepted_in_the_handshake() {
    let (mut peer, _e, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    peer.say_raw(b"feature myname=\"Crlf\" setboard=1 done=1\r\n")
        .await;
    assert_eq!(ready_identity(&mut events).await.name, "Crlf");
    // Whatever the engine sends, the session writes `\n` only.
    let a = line(&mut peer).await;
    assert!(!a.ends_with('\r'), "{a:?}");
}

#[tokio::test(start_paused = true)]
async fn invalid_utf8_and_a_huge_line_before_features_do_not_break_the_handshake() {
    let (mut peer, _e, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    peer.say_raw(b"\xff\xfe\xfd not utf8 \x80\n").await;
    let mut huge = vec![b'x'; 3 * 1024 * 1024];
    huge.push(b'\n');
    peer.say_raw(&huge).await;
    peer.say("feature myname=\"Fine\" done=1").await;
    assert_eq!(ready_identity(&mut events).await.name, "Fine");
}

#[tokio::test(start_paused = true)]
async fn a_search_sent_during_the_handshake_runs_after_the_init_lines() {
    let (mut peer, engine, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    engine
        .send(EngineCommand::Search(SearchRequest::infinite(
            SearchPosition::new(Position::new()),
        )))
        .unwrap();
    peer.say(NO_PING).await;
    expect_lines(
        &mut peer,
        &[
            "accepted myname",
            "accepted setboard",
            "accepted analyze",
            "accepted done",
            "easy",
            "post",
            "force",
            "force",
            &format!("setboard {START_FEN}"),
            "analyze",
        ],
    )
    .await;
    assert!(matches!(next(&mut events).await, EngineEvent::Ready(_)));
    assert!(matches!(
        next(&mut events).await,
        EngineEvent::SearchStarted(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn only_the_last_search_sent_during_the_handshake_runs() {
    let (mut peer, engine, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    engine
        .send(EngineCommand::Search(infinite(START_FEN)))
        .unwrap();
    engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    peer.say(NO_PING).await;
    let mut seen = Vec::new();
    while seen.last().map(String::as_str) != Some("analyze") {
        seen.push(line(&mut peer).await);
    }
    assert!(
        seen.iter().any(|l| l == &format!("setboard {KPK}")),
        "{seen:?}"
    );
    assert!(!seen.iter().any(|l| l.contains("rnbqkbnr")), "{seen:?}");
    let _ = &mut events;
}

#[tokio::test(start_paused = true)]
async fn the_log_records_the_traffic_in_both_directions() {
    let r = full().await;
    let entries = r.engine.log().snapshot_since(0).0;
    let text: Vec<String> = entries.iter().map(|e| e.text.clone()).collect();
    for want in ["xboard", "protover 2", "accepted done", "force"] {
        assert!(
            text.iter().any(|t| t == want),
            "{want} missing from {text:?}"
        );
    }
    assert!(text.iter().any(|t| t.starts_with("feature myname")));
}

#[tokio::test(start_paused = true)]
async fn nonstandard_start_sent_before_ready_without_setboard_is_refused_with_a_warning() {
    // A non-standard start without `setboard`, sent before the handshake ends,
    // is refused with a warning (the synchronous error needs the identity).
    let (mut peer, engine, mut events) = ManualEngine::launch(xconfig());
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    peer.say("feature analyze=1 done=1").await;
    let w = until(&mut events, |e| match e {
        EngineEvent::Warning(w) => Some(w),
        _ => None,
    })
    .await;
    assert!(matches!(w, EngineWarning::CommandRefused { .. }), "{w:?}");
}
