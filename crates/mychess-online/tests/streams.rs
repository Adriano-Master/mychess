//! Event, game, seek and challenge streams against the local fake server.

mod common;

use std::time::Duration;

use common::{GAME_ID, TOKEN, client, client_with, fake, full_json, recv, state_json};
use mychess_online::{
    Error, SeekOutcome, StreamMessage,
    models::{ChallengeParams, ChallengeUpdate, Event, GameEvent, SeekParams, Status},
    testing::{Response, Step},
};

const GAME_START: &str = include_str!("fixtures/game_start.json");
const CHALLENGE: &str = include_str!("fixtures/challenge.json");
const STREAM_EVENT: &str = "/api/stream/event";

fn game_path() -> String {
    format!("/api/board/game/stream/{GAME_ID}")
}

fn item<T>(m: StreamMessage<T>) -> T {
    match m {
        StreamMessage::Item(t) => t,
        _ => panic!("expected an item"),
    }
}

#[tokio::test]
async fn event_stream_frames_split_chunks_and_skips_keepalives() {
    let fake = fake().await;
    let challenge = CHALLENGE.trim();
    let (head, tail) = challenge.split_at(40);
    fake.enqueue(
        "GET",
        STREAM_EVENT,
        Response::stream(vec![
            Step::KeepAlive,
            Step::line(GAME_START.trim()),
            Step::KeepAlive,
            Step::Raw(head.as_bytes().to_vec()),
            Step::Delay(Duration::from_millis(20)),
            Step::Raw(format!("{tail}\r\n").into_bytes()),
            Step::line(r#"{"type":"somethingNew"}"#),
            Step::line("this is not json"),
            Step::Raw(b"\xff\xfe\n".to_vec()),
            Step::Hold,
        ]),
    );
    let mut s = client(&fake).open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(matches!(item(recv(&mut s).await), Event::GameStart(g) if g.id == "abcd1234"));
    assert!(matches!(item(recv(&mut s).await), Event::Challenge(c) if c.id == "chal0001"));
    assert_eq!(item(recv(&mut s).await), Event::Unknown);
    assert!(
        matches!(item(recv(&mut s).await), Event::Unparsed { line } if line == "this is not json")
    );
    assert!(matches!(item(recv(&mut s).await), Event::Unparsed { .. }));
    // Authenticated, token only in the header.
    let req = &fake.requests()[0];
    assert_eq!(
        req.header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    assert!(!req.path.contains(TOKEN) && !req.query.contains(TOKEN));
    // Dropping the handle closes the connection.
    s.close();
    assert!(fake.wait_closed("GET", STREAM_EVENT, 1).await);
}

#[tokio::test]
async fn game_stream_ends_when_the_game_is_over() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![
            Step::line(full_json()),
            Step::line(state_json()),
            Step::line(r#"{"type":"chatLine"}"#),
        ]),
    );
    let mut s = client(&fake).open_game_stream(GAME_ID).expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(matches!(item(recv(&mut s).await), GameEvent::Full(f) if f.state.moves == "e2e4 e7e5"));
    assert!(
        matches!(item(recv(&mut s).await), GameEvent::State(st) if st.status == Some(Status::Resign))
    );
    assert_eq!(recv(&mut s).await, StreamMessage::Ended);
    assert!(s.next().await.is_none());
    assert_eq!(fake.requests().len(), 1, "no reconnection after the end");
}

#[tokio::test]
async fn game_stream_reconnects_after_a_drop_with_a_fresh_full() {
    let fake = fake().await;
    let full_later = full_json().replace("e2e4 e7e5", "e2e4 e7e5 g1f3");
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(full_json()), Step::Drop]),
    );
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(&full_later), Step::line(state_json())]),
    );
    let mut s = client(&fake).open_game_stream(GAME_ID).expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(matches!(item(recv(&mut s).await), GameEvent::Full(_)));
    let StreamMessage::Reconnecting { attempt, cause, .. } = recv(&mut s).await else {
        panic!("expected Reconnecting");
    };
    assert_eq!(attempt, 1);
    assert!(matches!(cause, Error::Network(_)), "{cause:?}");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(
        matches!(item(recv(&mut s).await), GameEvent::Full(f) if f.state.moves == "e2e4 e7e5 g1f3")
    );
    assert!(matches!(item(recv(&mut s).await), GameEvent::State(_)));
    assert_eq!(recv(&mut s).await, StreamMessage::Ended);
    assert_eq!(fake.requests().len(), 2);
}

#[tokio::test]
async fn reconnect_gives_up_after_the_configured_attempts() {
    let fake = fake().await;
    // The server closes cleanly right away, every time, without data.
    fake.fallback("GET", STREAM_EVENT, Response::stream(vec![Step::Close]));
    let c = client_with(&fake, |cfg| cfg.streams.event_max_reconnects = Some(2));
    let mut s = c.open_event_stream().expect("stream");
    let mut reconnects = 0;
    loop {
        match recv(&mut s).await {
            StreamMessage::Reconnecting { .. } => reconnects += 1,
            StreamMessage::Failed(e) => {
                assert!(matches!(e, Error::Network(_)));
                break;
            }
            StreamMessage::Connected => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(reconnects, 2);
    assert_eq!(fake.requests().len(), 3);
    assert!(s.next().await.is_none());
}

#[tokio::test]
async fn silence_makes_the_stream_reconnect() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        STREAM_EVENT,
        Response::stream(vec![Step::line(r#"{"type":"future"}"#), Step::Hold]),
    );
    fake.enqueue(
        "GET",
        STREAM_EVENT,
        Response::stream(vec![Step::line(r#"{"type":"future"}"#), Step::Hold]),
    );
    let c = client_with(&fake, |cfg| {
        cfg.streams.silence = Some(Duration::from_millis(150));
    });
    let mut s = c.open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert_eq!(item(recv(&mut s).await), Event::Unknown);
    let StreamMessage::Reconnecting { cause, .. } = recv(&mut s).await else {
        panic!("expected Reconnecting")
    };
    assert_eq!(cause, Error::Timeout);
    // The dead connection was closed by the client.
    assert!(fake.wait_closed("GET", STREAM_EVENT, 1).await);
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert_eq!(item(recv(&mut s).await), Event::Unknown);
}

#[tokio::test]
async fn keepalives_keep_a_quiet_stream_alive() {
    let fake = fake().await;
    let mut steps = Vec::new();
    for _ in 0..8 {
        steps.push(Step::Delay(Duration::from_millis(60)));
        steps.push(Step::KeepAlive);
    }
    steps.push(Step::line(r#"{"type":"future"}"#));
    steps.push(Step::Hold);
    fake.enqueue("GET", STREAM_EVENT, Response::stream(steps));
    let c = client_with(&fake, |cfg| {
        cfg.streams.silence = Some(Duration::from_millis(200));
    });
    let mut s = c.open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    // 480 ms of keep-alives only: no reconnection, then the event.
    assert_eq!(item(recv(&mut s).await), Event::Unknown);
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn fatal_statuses_do_not_reconnect() {
    let fake = fake().await;
    fake.fallback(
        "GET",
        STREAM_EVENT,
        Response::json(401, r#"{"error":"bad token"}"#),
    );
    let mut s = client(&fake).open_event_stream().expect("stream");
    assert_eq!(
        recv(&mut s).await,
        StreamMessage::Failed(Error::Unauthorized)
    );
    assert_eq!(fake.requests().len(), 1);

    fake.fallback("GET", &game_path(), Response::json(404, "{}"));
    let mut g = client(&fake).open_game_stream(GAME_ID).expect("stream");
    assert_eq!(recv(&mut g).await, StreamMessage::Failed(Error::NotFound));
}

#[tokio::test]
async fn a_429_on_connect_waits_the_block_then_connects() {
    let fake = fake().await;
    fake.enqueue("GET", STREAM_EVENT, Response::status(429));
    fake.enqueue(
        "GET",
        STREAM_EVENT,
        Response::stream(vec![Step::line(r#"{"type":"future"}"#), Step::Hold]),
    );
    let started = std::time::Instant::now();
    let mut s = client(&fake).open_event_stream().expect("stream");
    let StreamMessage::Reconnecting { cause, delay, .. } = recv(&mut s).await else {
        panic!("expected Reconnecting")
    };
    assert!(matches!(cause, Error::RateLimited { .. }));
    assert!(delay >= Duration::from_millis(100), "{delay:?}");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(started.elapsed() >= Duration::from_millis(100));
    assert_eq!(item(recv(&mut s).await), Event::Unknown);
}

#[tokio::test]
async fn an_oversized_line_ends_the_stream() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        STREAM_EVENT,
        Response::stream(vec![Step::Raw(vec![b'x'; 200]), Step::Hold]),
    );
    let c = client_with(&fake, |cfg| cfg.streams.max_line_len = 64);
    let mut s = c.open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert_eq!(
        recv(&mut s).await,
        StreamMessage::Failed(Error::LineTooLong)
    );
    assert!(fake.wait_closed("GET", STREAM_EVENT, 1).await);
}

#[tokio::test]
async fn live_stream_delivers_events_as_they_come() {
    let fake = fake().await;
    let (resp, live) = Response::live();
    fake.enqueue("GET", &game_path(), resp);
    let mut s = client(&fake).open_game_stream(GAME_ID).expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    live.line(full_json());
    assert!(matches!(item(recv(&mut s).await), GameEvent::Full(_)));
    live.line(r#"{"type":"opponentGone","gone":true,"claimWinInSeconds":10}"#);
    assert_eq!(
        item(recv(&mut s).await),
        GameEvent::OpponentGone {
            gone: true,
            claim_win_in_seconds: Some(10)
        }
    );
    live.line(state_json());
    assert!(matches!(item(recv(&mut s).await), GameEvent::State(_)));
    assert_eq!(recv(&mut s).await, StreamMessage::Ended);
}

#[tokio::test]
async fn seek_stays_open_and_cancel_closes_the_connection() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/board/seek",
        Response::stream(vec![Step::Hold]),
    );
    let seek = client(&fake).seek(&SeekParams::new(10, 5)).expect("seek");
    assert!(fake.wait_requests("POST", "/api/board/seek", 1).await);
    let req = &fake.requests()[0];
    assert_eq!(
        req.header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    let form = req.form();
    assert!(form.contains(&("time".to_owned(), "10".to_owned())));
    assert!(form.contains(&("increment".to_owned(), "5".to_owned())));
    assert!(form.contains(&("rated".to_owned(), "false".to_owned())));
    assert!(form.contains(&("variant".to_owned(), "standard".to_owned())));
    assert!(!seek.is_finished());
    assert_eq!(fake.closed_count("POST", "/api/board/seek"), 0);
    seek.cancel();
    assert!(
        fake.wait_closed("POST", "/api/board/seek", 1).await,
        "the fake must see the close"
    );
}

#[tokio::test]
async fn seek_outcomes() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/board/seek",
        Response::stream(vec![Step::Delay(Duration::from_millis(30)), Step::Close]),
    );
    let mut seek = client(&fake).seek(&SeekParams::new(15, 10)).expect("seek");
    assert_eq!(seek.outcome().await, SeekOutcome::Closed);
    assert_eq!(seek.outcome().await, SeekOutcome::Failed(Error::Closed));

    fake.enqueue(
        "POST",
        "/api/board/seek",
        Response::json(400, r#"{"error":"Too fast"}"#),
    );
    let mut seek = client(&fake).seek(&SeekParams::new(10, 0)).expect("seek");
    assert_eq!(
        seek.outcome().await,
        SeekOutcome::Failed(Error::Rejected {
            status: 400,
            reason: "Too fast".to_owned()
        })
    );

    fake.enqueue(
        "POST",
        "/api/board/seek",
        Response::stream(vec![Step::Drop]),
    );
    let mut seek = client(&fake).seek(&SeekParams::new(10, 0)).expect("seek");
    assert!(matches!(
        seek.outcome().await,
        SeekOutcome::Failed(Error::Network(_))
    ));
}

#[tokio::test]
async fn challenge_stream_reports_the_answer_and_never_resends() {
    let fake = fake().await;
    let created = r#"{"id":"chal0002","status":"created","timeControl":{"type":"clock","limit":600,"increment":5}}"#;
    fake.enqueue(
        "POST",
        "/api/challenge/bob",
        Response::stream(vec![
            Step::line(created),
            Step::KeepAlive,
            Step::Delay(Duration::from_millis(20)),
            Step::line(r#"{"done":"declined"}"#),
        ]),
    );
    let mut params = ChallengeParams::new(600, 5);
    params.rated = true;
    let mut h = client(&fake).challenge("bob", &params).expect("challenge");
    assert_eq!(recv(&mut h).await, StreamMessage::Connected);
    assert!(
        matches!(item(recv(&mut h).await), ChallengeUpdate::Challenge(c) if c.id == "chal0002")
    );
    assert_eq!(
        item(recv(&mut h).await),
        ChallengeUpdate::Done("declined".to_owned())
    );
    assert_eq!(recv(&mut h).await, StreamMessage::Ended);
    let form = fake.requests()[0].form();
    assert!(form.contains(&("keepAliveStream".to_owned(), "true".to_owned())));
    assert!(form.contains(&("clock.limit".to_owned(), "600".to_owned())));
    assert!(form.contains(&("rated".to_owned(), "true".to_owned())));

    // A connection that drops is a failure: a new POST would duplicate it.
    fake.enqueue(
        "POST",
        "/api/challenge/carol",
        Response::stream(vec![Step::line(created), Step::Drop]),
    );
    let mut h = client(&fake)
        .challenge("carol", &ChallengeParams::new(300, 0))
        .expect("challenge");
    assert_eq!(recv(&mut h).await, StreamMessage::Connected);
    assert!(matches!(
        item(recv(&mut h).await),
        ChallengeUpdate::Challenge(_)
    ));
    assert!(matches!(
        recv(&mut h).await,
        StreamMessage::Failed(Error::Network(_))
    ));
    assert_eq!(fake.requests_for("POST", "/api/challenge/carol").len(), 1);
}

#[tokio::test]
async fn dropping_a_challenge_handle_cancels_by_closing() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/challenge/bob",
        Response::stream(vec![Step::Hold]),
    );
    let mut h = client(&fake)
        .challenge("bob", &ChallengeParams::new(600, 0))
        .expect("challenge");
    assert_eq!(recv(&mut h).await, StreamMessage::Connected);
    h.close();
    assert!(fake.wait_closed("POST", "/api/challenge/bob", 1).await);
}

#[tokio::test]
async fn game_stream_reconnects_despite_a_block_from_other_requests() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(full_json()), Step::Drop]),
    );
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(full_json()), Step::Hold]),
    );
    fake.enqueue("GET", "/api/account", Response::status(429));
    let c = client(&fake);
    // Block caused by an unrelated request (150 ms in the test config).
    assert!(matches!(c.account().await, Err(Error::RateLimited { .. })));
    assert!(c.gate().blocked_for().is_some());
    let started = std::time::Instant::now();
    let mut g = c.open_game_stream(GAME_ID).expect("stream");
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
    assert!(matches!(item(recv(&mut g).await), GameEvent::Full(_)));
    // Dropped, then reconnected without waiting for the foreign block.
    let StreamMessage::Reconnecting { delay, .. } = recv(&mut g).await else {
        panic!("expected Reconnecting")
    };
    assert!(delay < Duration::from_millis(100), "{delay:?}");
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
    assert!(started.elapsed() < Duration::from_millis(140));
}

#[tokio::test]
async fn game_stream_waits_for_its_own_429() {
    let fake = fake().await;
    fake.enqueue("GET", &game_path(), Response::status(429));
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(full_json()), Step::Hold]),
    );
    let mut g = client(&fake).open_game_stream(GAME_ID).expect("stream");
    let StreamMessage::Reconnecting { cause, delay, .. } = recv(&mut g).await else {
        panic!("expected Reconnecting")
    };
    assert!(matches!(cause, Error::RateLimited { .. }));
    assert!(delay >= Duration::from_millis(100), "{delay:?}");
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
}

#[tokio::test]
async fn dropping_event_and_game_streams_closes_the_connection() {
    let fake = fake().await;
    fake.enqueue("GET", STREAM_EVENT, Response::stream(vec![Step::Hold]));
    fake.enqueue("GET", &game_path(), Response::stream(vec![Step::Hold]));
    let c = client(&fake);
    let mut e = c.open_event_stream().expect("event");
    assert_eq!(recv(&mut e).await, StreamMessage::Connected);
    drop(e);
    assert!(fake.wait_closed("GET", STREAM_EVENT, 1).await);
    let mut g = c.open_game_stream(GAME_ID).expect("game");
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
    g.close();
    assert!(fake.wait_closed("GET", &game_path(), 1).await);
}

#[tokio::test]
async fn seek_and_challenge_headers_time_out() {
    let fake = fake().await;
    // The server accepts the request but is slow to send the headers.
    fake.enqueue(
        "POST",
        "/api/board/seek",
        Response::stream(vec![Step::Hold]).head_delay(Duration::from_secs(3)),
    );
    fake.enqueue(
        "POST",
        "/api/challenge/bob",
        Response::stream(vec![Step::Hold]).head_delay(Duration::from_secs(3)),
    );
    let c = client_with(&fake, |cfg| {
        cfg.streams.header_timeout = Duration::from_millis(80);
    });
    let mut seek = c.seek(&SeekParams::new(10, 5)).expect("seek");
    assert_eq!(seek.outcome().await, SeekOutcome::Failed(Error::Timeout));
    let mut h = c
        .challenge("bob", &ChallengeParams::new(600, 0))
        .expect("challenge");
    assert_eq!(recv(&mut h).await, StreamMessage::Failed(Error::Timeout));
}

#[tokio::test]
async fn streams_can_be_opened_only_inside_a_runtime() {
    let fake = fake().await;
    let c = client(&fake);
    let outside = std::thread::spawn(move || c.open_event_stream().err());
    assert_eq!(outside.join().expect("thread"), Some(Error::Closed));
}
