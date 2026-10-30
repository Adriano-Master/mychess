//! Contract tests of the streams against the local fake server (spec m8-3
//! D1308, D1315, D1316, D1333, D1342, D1349, D1351; acceptance criterion
//! "Streams"). Real clock with short timings and generous margins; no
//! `tokio::time::pause` because the fake uses real sockets (D1345).

#![allow(clippy::assert_is_empty, clippy::type_complexity)]

mod m8_3_support;

use std::time::{Duration, Instant};

use m8_3_support::{
    GAME, GAME_FULL, SECRET, STATE_RESIGN, STATE_STARTED, client, client_with, fake, item, recv,
};
use mychess_online::{
    Error, SeekOutcome, StreamMessage,
    models::{ChallengeParams, ChallengeUpdate, Event, GameEvent, SeekParams, Status},
    testing::{Response, Step},
};

const EVENTS: &str = "/api/stream/event";

fn game_path() -> String {
    format!("/api/board/game/stream/{GAME}")
}

fn challenge_line(id: &str) -> String {
    format!(
        r#"{{"type":"challenge","challenge":{{"id":"{id}","status":"created","direction":"in","timeControl":{{"type":"clock","limit":600,"increment":0}}}}}}"#
    )
}

/// Asserts that nothing arrives within `d`.
async fn quiet<T: std::fmt::Debug>(h: &mut mychess_online::StreamHandle<T>, d: Duration) {
    if let Ok(m) = tokio::time::timeout(d, h.next()).await {
        panic!("expected silence, got {m:?}");
    }
}

// ------------------------------------------------------------ event stream

#[tokio::test]
async fn event_stream_delivers_connected_items_and_skips_keepalives() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        EVENTS,
        Response::stream(vec![
            Step::KeepAlive,
            Step::line(&challenge_line("chal0001")),
            Step::KeepAlive,
            Step::KeepAlive,
            Step::line(r#"{"type":"gameStart","game":{"gameId":"abcd1234","color":"white"}}"#),
            Step::Hold,
        ]),
    );
    let mut s = client(&fake).open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(matches!(item(recv(&mut s).await), Event::Challenge(c) if c.id == "chal0001"));
    assert!(matches!(item(recv(&mut s).await), Event::GameStart(g) if g.id == GAME));
    quiet(&mut s, Duration::from_millis(100)).await;
    let r = &fake.requests()[0];
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", EVENTS));
    assert_eq!(
        r.header("authorization"),
        Some(format!("Bearer {SECRET}").as_str())
    );
    assert!(!r.query.contains(SECRET));
    assert!(s.try_next().is_none());
}

#[tokio::test]
async fn bad_lines_become_items_and_the_stream_goes_on() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        EVENTS,
        Response::stream(vec![
            Step::line("garbage"),
            Step::Raw(b"\xff\xfe\xfd\n".to_vec()),
            Step::line(r#"{"type":"futureThing"}"#),
            Step::line(&challenge_line("chal0002")),
            Step::Hold,
        ]),
    );
    let mut s = client(&fake).open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(matches!(item(recv(&mut s).await), Event::Unparsed { .. }));
    assert!(matches!(item(recv(&mut s).await), Event::Unparsed { .. }));
    assert_eq!(item(recv(&mut s).await), Event::Unknown);
    assert!(matches!(item(recv(&mut s).await), Event::Challenge(c) if c.id == "chal0002"));
}

#[tokio::test]
async fn a_line_split_in_the_middle_of_a_multibyte_character_survives() {
    let fake = fake().await;
    let line = challenge_line("chal0003").replace("created", "cr\u{e9}\u{20ac}ated");
    let bytes = format!("{line}\r\n").into_bytes();
    let pos = bytes.iter().position(|b| *b == 0xE2).expect("euro sign") + 1;
    fake.enqueue(
        "GET",
        EVENTS,
        Response::stream(vec![
            Step::Raw(bytes[..pos].to_vec()),
            Step::Delay(Duration::from_millis(30)),
            Step::Raw(bytes[pos..bytes.len() - 1].to_vec()),
            Step::Delay(Duration::from_millis(30)),
            Step::Raw(b"\n".to_vec()),
            Step::Hold,
        ]),
    );
    let mut s = client(&fake).open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    let Event::Challenge(c) = item(recv(&mut s).await) else {
        panic!("expected a challenge");
    };
    assert_eq!(c.status, "cr\u{e9}\u{20ac}ated");
}

#[tokio::test]
async fn a_slow_consumer_loses_nothing_and_keeps_the_order() {
    let fake = fake().await;
    let mut steps: Vec<Step> = (0..300)
        .map(|i| Step::line(&challenge_line(&format!("chal{i:04}"))))
        .collect();
    steps.push(Step::Hold);
    fake.enqueue("GET", EVENTS, Response::stream(steps));
    let mut s = client(&fake).open_event_stream().expect("stream");
    tokio::time::sleep(Duration::from_millis(300)).await; // channel fills up
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    for i in 0..300 {
        let Event::Challenge(c) = item(recv(&mut s).await) else {
            panic!("expected a challenge");
        };
        assert_eq!(c.id, format!("chal{i:04}"));
    }
}

#[tokio::test]
async fn permanent_http_errors_end_the_stream_without_reconnecting() {
    for (status, check) in [
        (
            401u16,
            (|e: &Error| *e == Error::Unauthorized) as fn(&Error) -> bool,
        ),
        (403, |e| matches!(e, Error::ScopeMissing { .. })),
        (404, |e| *e == Error::NotFound),
        (400, |e| matches!(e, Error::Rejected { status: 400, .. })),
    ] {
        let fake = fake().await;
        fake.fallback("GET", EVENTS, Response::json(status, r#"{"error":"no"}"#));
        let mut s = client(&fake).open_event_stream().expect("stream");
        match recv(&mut s).await {
            StreamMessage::Failed(e) => assert!(check(&e), "{status}: {e:?}"),
            other => panic!("{status}: expected Failed, got {other:?}"),
        }
        assert!(s.next().await.is_none(), "nothing after Failed ({status})");
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(fake.requests().len(), 1, "status {status}: no reconnection");
    }
}

#[tokio::test]
async fn a_drop_reconnects_with_growing_backoff_and_the_item_survives() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        EVENTS,
        Response::stream(vec![Step::line(&challenge_line("chal0001")), Step::Drop]),
    );
    fake.enqueue(
        "GET",
        EVENTS,
        Response::stream(vec![Step::line(&challenge_line("chal0002")), Step::Hold]),
    );
    let mut s = client(&fake).open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(matches!(item(recv(&mut s).await), Event::Challenge(c) if c.id == "chal0001"));
    match recv(&mut s).await {
        StreamMessage::Reconnecting {
            attempt,
            delay,
            cause,
        } => {
            assert_eq!(attempt, 1);
            assert!(
                delay >= Duration::from_millis(4) && delay <= Duration::from_millis(6),
                "{delay:?}"
            );
            assert!(matches!(cause, Error::Network(_)), "{cause:?}");
            assert!(!format!("{cause:?}").contains(SECRET));
        }
        other => panic!("expected Reconnecting, got {other:?}"),
    }
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(matches!(item(recv(&mut s).await), Event::Challenge(c) if c.id == "chal0002"));
    assert_eq!(fake.requests_for("GET", EVENTS).len(), 2);
}

#[tokio::test]
async fn consecutive_failures_double_the_delay_and_data_resets_the_counter() {
    let fake = fake().await;
    // Three failures without data, then a connection with data and a drop,
    // then another failure.
    for _ in 0..3 {
        fake.enqueue("GET", EVENTS, Response::status(503));
    }
    fake.enqueue(
        "GET",
        EVENTS,
        Response::stream(vec![Step::line(&challenge_line("chal0001")), Step::Drop]),
    );
    fake.enqueue("GET", EVENTS, Response::status(503));
    fake.enqueue("GET", EVENTS, Response::stream(vec![Step::Hold]));
    let c = client_with(&fake, |cfg| {
        cfg.streams.backoff_unit = Duration::from_millis(20);
    });
    let mut s = c.open_event_stream().expect("stream");

    let mut seen = Vec::new();
    loop {
        match recv(&mut s).await {
            StreamMessage::Reconnecting {
                attempt,
                delay,
                cause,
            } => seen.push((attempt, delay, cause)),
            StreamMessage::Connected => {
                if fake.requests_for("GET", EVENTS).len() >= 6 {
                    break;
                }
            }
            StreamMessage::Item(_) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    let attempts: Vec<u32> = seen.iter().map(|(a, ..)| *a).collect();
    assert_eq!(
        attempts,
        [1, 2, 3, 1, 2],
        "counter restarts after data: {seen:?}"
    );
    // Nominal 20, 40, 80 ms (+-20%); after the connection that carried data the
    // drop is failure 1 again (20 ms) and the next failure doubles it (40 ms).
    for ((_, delay, _), nominal) in seen.iter().zip([20.0f64, 40.0, 80.0, 20.0, 40.0]) {
        let n = nominal;
        let ms = delay.as_secs_f64() * 1000.0;
        assert!(
            ms >= n * 0.8 - 0.5 && ms <= n * 1.2 + 0.5,
            "delay {ms} ms vs nominal {n}"
        );
    }
    assert!(matches!(seen[0].2, Error::Server(503)));
}

#[tokio::test]
async fn the_event_stream_gives_up_after_the_configured_failures() {
    let fake = fake().await;
    fake.fallback("GET", EVENTS, Response::status(503));
    let c = client_with(&fake, |cfg| cfg.streams.event_max_reconnects = Some(2));
    let mut s = c.open_event_stream().expect("stream");
    let mut reconnects = 0;
    let end = loop {
        match recv(&mut s).await {
            StreamMessage::Reconnecting { .. } => reconnects += 1,
            StreamMessage::Failed(e) => break e,
            other => panic!("unexpected {other:?}"),
        }
    };
    assert_eq!(end, Error::Server(503));
    assert_eq!(reconnects, 2);
    assert_eq!(fake.requests().len(), 3, "first try plus two reconnections");
    assert!(s.next().await.is_none());
}

#[tokio::test]
async fn the_default_event_limit_is_five_reconnections() {
    let fake = fake().await;
    fake.fallback("GET", EVENTS, Response::status(500));
    let mut s = client(&fake).open_event_stream().expect("stream");
    let mut reconnects = 0;
    loop {
        match recv(&mut s).await {
            StreamMessage::Reconnecting { .. } => reconnects += 1,
            StreamMessage::Failed(_) => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(reconnects, 5, "D1333: five attempts, then give up");
}

#[tokio::test]
async fn a_server_that_closes_the_event_stream_cleanly_is_reconnected() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        EVENTS,
        Response::stream(vec![Step::KeepAlive, Step::Close]),
    );
    fake.enqueue("GET", EVENTS, Response::stream(vec![Step::Hold]));
    let mut s = client(&fake).open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(matches!(
        recv(&mut s).await,
        StreamMessage::Reconnecting { .. }
    ));
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
}

#[tokio::test]
async fn a_line_above_the_limit_ends_the_stream_for_good() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        EVENTS,
        Response::stream(vec![Step::line(&"x".repeat(5000)), Step::Hold]),
    );
    let c = client_with(&fake, |cfg| cfg.streams.max_line_len = 1024);
    let mut s = c.open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert_eq!(
        recv(&mut s).await,
        StreamMessage::Failed(Error::LineTooLong)
    );
    assert!(s.next().await.is_none());
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(fake.requests().len(), 1, "LineTooLong must not reconnect");
}

// ---------------------------------------------------------------- silence

#[tokio::test]
async fn silence_reconnects_and_any_byte_counts_as_life() {
    let fake = fake().await;
    // 1st connection: dead silence. 2nd: keep-alives at 60 ms for ~700 ms with
    // a 250 ms silence limit.
    fake.enqueue("GET", EVENTS, Response::stream(vec![Step::Hold]));
    let mut alive = Vec::new();
    for _ in 0..12 {
        alive.push(Step::KeepAlive);
        alive.push(Step::Delay(Duration::from_millis(60)));
    }
    alive.push(Step::Hold);
    fake.enqueue("GET", EVENTS, Response::stream(alive));
    let c = client_with(&fake, |cfg| {
        cfg.streams.silence = Some(Duration::from_millis(250));
    });
    let mut s = c.open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    let t0 = Instant::now();
    match recv(&mut s).await {
        StreamMessage::Reconnecting { cause, .. } => assert_eq!(cause, Error::Timeout),
        other => panic!("expected Reconnecting(Timeout), got {other:?}"),
    }
    assert!(
        t0.elapsed() >= Duration::from_millis(200),
        "{:?}",
        t0.elapsed()
    );
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    // The keep-alive connection survives well past the silence limit.
    quiet(&mut s, Duration::from_millis(600)).await;
    assert_eq!(fake.requests_for("GET", EVENTS).len(), 2);
}

#[tokio::test]
async fn disabling_the_silence_watch_keeps_a_quiet_connection() {
    let fake = fake().await;
    fake.enqueue("GET", EVENTS, Response::stream(vec![Step::Hold]));
    let c = client_with(&fake, |cfg| cfg.streams.silence = None);
    let mut s = c.open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    quiet(&mut s, Duration::from_millis(500)).await;
    assert_eq!(fake.requests().len(), 1);
}

// ----------------------------------------------------------- 429 and gate

#[tokio::test]
async fn the_event_stream_waits_for_the_429_block_before_reconnecting() {
    let fake = fake().await;
    fake.enqueue("GET", EVENTS, Response::status(429));
    fake.enqueue("GET", EVENTS, Response::stream(vec![Step::Hold]));
    let c = client(&fake); // base block 200 ms
    let t0 = Instant::now();
    let mut s = c.open_event_stream().expect("stream");
    match recv(&mut s).await {
        StreamMessage::Reconnecting { cause, delay, .. } => {
            assert!(matches!(cause, Error::RateLimited { .. }), "{cause:?}");
            assert!(
                delay >= Duration::from_millis(150),
                "waits the block: {delay:?}"
            );
        }
        other => panic!("expected Reconnecting, got {other:?}"),
    }
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(
        t0.elapsed() >= Duration::from_millis(150),
        "{:?}",
        t0.elapsed()
    );
}

#[tokio::test]
async fn a_block_set_by_other_requests_delays_the_event_stream_but_not_the_game_stream() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::status(429).header("Retry-After", "30"),
    );
    fake.enqueue("GET", EVENTS, Response::stream(vec![Step::Hold]));
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(GAME_FULL), Step::Hold]),
    );
    let c = client(&fake);
    assert!(matches!(c.account().await, Err(Error::RateLimited { .. })));

    // D1349: the game stream is urgent and connects through the block.
    let mut g = c.open_game_stream(GAME).expect("game stream");
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
    assert!(matches!(item(recv(&mut g).await), GameEvent::Full(_)));

    // The event stream respects it: it announces the wait and does not connect.
    let mut e = c.open_event_stream().expect("event stream");
    assert!(matches!(
        recv(&mut e).await,
        StreamMessage::Reconnecting {
            cause: Error::RateLimited { .. },
            ..
        }
    ));
    quiet(&mut e, Duration::from_millis(400)).await;
    assert_eq!(
        fake.requests_for("GET", EVENTS).len(),
        0,
        "no request during the block"
    );
}

#[tokio::test]
async fn the_game_stream_reconnects_through_a_foreign_block() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::status(429).header("Retry-After", "30"),
    );
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(GAME_FULL), Step::Drop]),
    );
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(GAME_FULL), Step::Hold]),
    );
    let c = client(&fake);
    assert!(c.account().await.is_err());
    let mut g = c.open_game_stream(GAME).expect("game stream");
    let t0 = Instant::now();
    let mut connects = 0;
    while connects < 2 {
        if let StreamMessage::Connected = recv(&mut g).await {
            connects += 1;
        }
    }
    assert!(
        t0.elapsed() < Duration::from_secs(5),
        "must not wait for the 30 s block"
    );
}

#[tokio::test]
async fn the_game_stream_waits_out_its_own_429() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        &game_path(),
        Response::status(429).header("Retry-After", "1"),
    );
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(GAME_FULL), Step::Hold]),
    );
    let c = client(&fake);
    let t0 = Instant::now();
    let mut g = c.open_game_stream(GAME).expect("game stream");
    match recv(&mut g).await {
        StreamMessage::Reconnecting { cause, delay, .. } => {
            assert!(matches!(cause, Error::RateLimited { .. }));
            assert!(
                delay >= Duration::from_millis(900),
                "honors Retry-After: {delay:?}"
            );
        }
        other => panic!("expected Reconnecting, got {other:?}"),
    }
    // Its 429 also blocks the normal requests.
    assert!(c.gate().blocked_for().is_some());
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
    assert!(
        t0.elapsed() >= Duration::from_millis(900),
        "{:?}",
        t0.elapsed()
    );
}

#[tokio::test]
async fn streams_do_not_take_the_one_request_at_a_time_slot() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::json(200, r#"{"id":"bob","username":"Bob"}"#)
            .head_delay(Duration::from_millis(900)),
    );
    fake.enqueue("GET", EVENTS, Response::stream(vec![Step::Hold]));
    let c = client(&fake);
    let c2 = c.clone();
    let slow = tokio::spawn(async move { c2.account().await.map(|_| ()) });
    assert!(fake.wait_requests("GET", "/api/account", 1).await);
    let mut e = c.open_event_stream().expect("event stream");
    assert_eq!(recv(&mut e).await, StreamMessage::Connected);
    assert!(
        !slow.is_finished(),
        "the slow account call is still in flight"
    );
    slow.await.expect("join").expect("account");
}

// ------------------------------------------------------------ game stream

#[tokio::test]
async fn game_stream_delivers_full_state_and_ends_on_a_final_status() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![
            Step::line(GAME_FULL),
            Step::line(STATE_STARTED),
            Step::line(r#"{"type":"chatLine","username":"alice","text":"hello","room":"player"}"#),
            Step::line(r#"{"type":"opponentGone","gone":true,"claimWinInSeconds":10}"#),
            Step::line(STATE_RESIGN),
            Step::line(STATE_STARTED), // must never be delivered
            Step::Hold,
        ]),
    );
    let mut g = client(&fake).open_game_stream(GAME).expect("stream");
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
    assert!(matches!(item(recv(&mut g).await), GameEvent::Full(f) if f.id == GAME));
    assert!(matches!(item(recv(&mut g).await), GameEvent::State(s) if s.move_list().len() == 3));
    assert_eq!(item(recv(&mut g).await), GameEvent::ChatLine);
    assert!(matches!(
        item(recv(&mut g).await),
        GameEvent::OpponentGone {
            gone: true,
            claim_win_in_seconds: Some(10)
        }
    ));
    assert!(
        matches!(item(recv(&mut g).await), GameEvent::State(s) if s.status == Some(Status::Resign))
    );
    assert_eq!(recv(&mut g).await, StreamMessage::Ended);
    assert!(g.next().await.is_none());
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        fake.requests().len(),
        1,
        "a finished game is not reconnected"
    );
    let r = &fake.requests()[0];
    assert_eq!(
        r.header("authorization"),
        Some(format!("Bearer {SECRET}").as_str())
    );
}

#[tokio::test]
async fn a_game_that_is_already_over_in_the_game_full_ends_at_once() {
    let fake = fake().await;
    let over = GAME_FULL.replace(
        r#""status":"started""#,
        r#""status":"mate","winner":"white""#,
    );
    assert_ne!(over, GAME_FULL);
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(&over), Step::Hold]),
    );
    let mut g = client(&fake).open_game_stream(GAME).expect("stream");
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
    assert!(matches!(item(recv(&mut g).await), GameEvent::Full(_)));
    assert_eq!(recv(&mut g).await, StreamMessage::Ended);
}

#[tokio::test]
async fn an_unknown_status_ends_the_game_stream_like_a_final_one() {
    let fake = fake().await;
    let line = r#"{"type":"gameState","moves":"e2e4","status":"someFutureStatus"}"#;
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(GAME_FULL), Step::line(line), Step::Hold]),
    );
    let mut g = client(&fake).open_game_stream(GAME).expect("stream");
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
    drop(recv(&mut g).await);
    drop(recv(&mut g).await);
    assert_eq!(recv(&mut g).await, StreamMessage::Ended);
}

#[tokio::test]
async fn a_non_final_game_that_loses_the_connection_is_resynchronized_by_a_new_game_full() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(GAME_FULL), Step::Drop]),
    );
    let later = GAME_FULL.replace(r#""moves":"e2e4 e7e5""#, r#""moves":"e2e4 e7e5 g1f3""#);
    fake.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(&later), Step::Hold]),
    );
    let mut g = client(&fake).open_game_stream(GAME).expect("stream");
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
    assert!(
        matches!(item(recv(&mut g).await), GameEvent::Full(f) if f.state.move_list().len() == 2)
    );
    assert!(matches!(
        recv(&mut g).await,
        StreamMessage::Reconnecting { .. }
    ));
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
    assert!(
        matches!(item(recv(&mut g).await), GameEvent::Full(f) if f.state.move_list().len() == 3)
    );
}

#[tokio::test]
async fn the_game_stream_never_gives_up_by_default() {
    let fake = fake().await;
    let path = game_path();
    for _ in 0..8 {
        fake.enqueue("GET", &path, Response::status(503));
    }
    fake.enqueue(
        "GET",
        &path,
        Response::stream(vec![Step::line(GAME_FULL), Step::Hold]),
    );
    let c = client(&fake);
    let mut g = c.open_game_stream(GAME).expect("stream");
    let mut reconnects = 0;
    loop {
        match recv(&mut g).await {
            StreamMessage::Reconnecting { .. } => reconnects += 1,
            StreamMessage::Connected => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(reconnects, 8, "more than the five of the event stream");
}

#[tokio::test]
async fn game_stream_errors_that_are_not_transient_fail_for_good() {
    for status in [401u16, 403, 404] {
        let fake = fake().await;
        fake.fallback("GET", &game_path(), Response::json(status, "{}"));
        let mut g = client(&fake).open_game_stream(GAME).expect("stream");
        assert!(
            matches!(recv(&mut g).await, StreamMessage::Failed(_)),
            "{status}"
        );
        assert!(g.next().await.is_none());
        assert_eq!(fake.requests().len(), 1);
    }
}

// ----------------------------------------------------------------- drop

#[tokio::test]
async fn dropping_a_stream_handle_closes_the_connection_and_stops_reconnecting() {
    let fake = fake().await;
    fake.enqueue("GET", EVENTS, Response::stream(vec![Step::Hold]));
    let mut s = client(&fake).open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    drop(s);
    assert!(
        fake.wait_closed("GET", EVENTS, 1).await,
        "the server must see the close"
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(fake.requests().len(), 1, "no reconnection after the drop");

    let fake2 = m8_3_support::fake().await;
    fake2.enqueue(
        "GET",
        &game_path(),
        Response::stream(vec![Step::line(GAME_FULL), Step::Hold]),
    );
    let mut g = client(&fake2).open_game_stream(GAME).expect("game stream");
    assert_eq!(recv(&mut g).await, StreamMessage::Connected);
    g.close();
    assert!(fake2.wait_closed("GET", &game_path(), 1).await);
}

#[tokio::test]
async fn dropping_during_the_backoff_wait_sends_no_further_request() {
    let fake = fake().await;
    fake.fallback("GET", EVENTS, Response::status(503));
    let c = client_with(&fake, |cfg| {
        cfg.streams.backoff_unit = Duration::from_millis(300);
    });
    let mut s = c.open_event_stream().expect("stream");
    assert!(matches!(
        recv(&mut s).await,
        StreamMessage::Reconnecting { .. }
    ));
    drop(s);
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(fake.requests().len(), 1);
}

// -------------------------------------------------------------- challenge

#[tokio::test]
async fn a_challenge_posts_the_documented_form_and_ends_with_done() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/challenge/alice",
        Response::stream(vec![
            Step::line(r#"{"id":"chal0001","status":"created","url":"https://x/chal0001"}"#),
            Step::Delay(Duration::from_millis(20)),
            Step::line(r#"{"done":"declined"}"#),
            Step::Hold,
        ]),
    );
    let mut p = ChallengeParams::new(600, 5);
    p.rated = true;
    let mut h = client(&fake).challenge("alice", &p).expect("challenge");
    assert_eq!(recv(&mut h).await, StreamMessage::Connected);
    assert!(
        matches!(item(recv(&mut h).await), ChallengeUpdate::Challenge(c) if c.id == "chal0001")
    );
    assert_eq!(
        item(recv(&mut h).await),
        ChallengeUpdate::Done("declined".to_owned())
    );
    assert_eq!(recv(&mut h).await, StreamMessage::Ended);
    let r = &fake.requests()[0];
    assert_eq!(r.method, "POST");
    assert_eq!(
        r.header("authorization"),
        Some(format!("Bearer {SECRET}").as_str())
    );
    let form = r.form();
    let get = |k: &str| form.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    assert_eq!(get("clock.limit"), Some("600"));
    assert_eq!(get("clock.increment"), Some("5"));
    assert_eq!(get("rated"), Some("true"));
    assert_eq!(get("variant"), Some("standard"));
    assert_eq!(get("keepAliveStream"), Some("true"));
    assert!(!r.body.contains(SECRET) && !r.query.contains(SECRET));
}

#[tokio::test]
async fn a_dropped_challenge_connection_fails_and_is_never_resent() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/challenge/alice",
        Response::stream(vec![
            Step::line(r#"{"id":"chal0001","status":"created"}"#),
            Step::Drop,
        ]),
    );
    let mut h = client(&fake)
        .challenge("alice", &ChallengeParams::new(600, 0))
        .expect("challenge");
    assert_eq!(recv(&mut h).await, StreamMessage::Connected);
    drop(recv(&mut h).await);
    assert!(matches!(
        recv(&mut h).await,
        StreamMessage::Failed(Error::Network(_))
    ));
    assert!(h.next().await.is_none());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        fake.requests().len(),
        1,
        "D1342: reposting would duplicate the challenge"
    );
}

#[tokio::test]
async fn a_server_close_without_done_ends_the_challenge_stream_without_reposting() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/challenge/alice",
        Response::stream(vec![Step::Close]),
    );
    let mut h = client(&fake)
        .challenge("alice", &ChallengeParams::new(600, 0))
        .expect("challenge");
    assert_eq!(recv(&mut h).await, StreamMessage::Connected);
    assert_eq!(recv(&mut h).await, StreamMessage::Ended);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn challenge_errors_map_and_do_not_retry() {
    for (status, check) in [
        (
            404u16,
            (|e: &Error| *e == Error::NotFound) as fn(&Error) -> bool,
        ),
        (401, |e| *e == Error::Unauthorized),
        (400, |e| matches!(e, Error::Rejected { .. })),
        (503, |e| *e == Error::Server(503)),
    ] {
        let fake = fake().await;
        fake.fallback(
            "POST",
            "/api/challenge/alice",
            Response::json(status, r#"{"error":"x"}"#),
        );
        let mut h = client(&fake)
            .challenge("alice", &ChallengeParams::new(600, 0))
            .expect("challenge");
        match recv(&mut h).await {
            StreamMessage::Failed(e) => assert!(check(&e), "{status}: {e:?}"),
            other => panic!("{status}: {other:?}"),
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(fake.requests().len(), 1, "status {status}");
    }
}

#[tokio::test]
async fn the_challenge_stream_has_no_silence_watch_and_a_header_timeout() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/challenge/alice",
        Response::stream(vec![Step::Hold]),
    );
    let c = client_with(&fake, |cfg| {
        cfg.streams.silence = Some(Duration::from_millis(150));
    });
    let mut h = c
        .challenge("alice", &ChallengeParams::new(600, 0))
        .expect("challenge");
    assert_eq!(recv(&mut h).await, StreamMessage::Connected);
    quiet(&mut h, Duration::from_millis(500)).await;
    assert_eq!(
        fake.requests().len(),
        1,
        "waiting for the opponent is not silence"
    );

    let fake = m8_3_support::fake().await;
    fake.enqueue(
        "POST",
        "/api/challenge/alice",
        Response::stream(vec![Step::Hold]).head_delay(Duration::from_secs(3)),
    );
    let c = client_with(&fake, |cfg| {
        cfg.streams.header_timeout = Duration::from_millis(200);
    });
    let mut h = c
        .challenge("alice", &ChallengeParams::new(600, 0))
        .expect("challenge");
    assert_eq!(recv(&mut h).await, StreamMessage::Failed(Error::Timeout));
}

#[tokio::test]
async fn dropping_the_challenge_handle_closes_the_connection() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/challenge/alice",
        Response::stream(vec![Step::Hold]),
    );
    let mut h = client(&fake)
        .challenge("alice", &ChallengeParams::new(600, 0))
        .expect("challenge");
    assert_eq!(recv(&mut h).await, StreamMessage::Connected);
    h.close();
    assert!(fake.wait_closed("POST", "/api/challenge/alice", 1).await);
}

// ------------------------------------------------------------------- seek

#[tokio::test]
async fn a_seek_stays_open_and_cancel_closes_the_connection() {
    let fake = fake().await;
    let (resp, _live) = Response::live();
    fake.enqueue("POST", "/api/board/seek", resp);
    let mut p = SeekParams::new(10, 5);
    p.rated = true;
    let mut h = client(&fake).seek(&p).expect("seek");
    assert!(fake.wait_requests("POST", "/api/board/seek", 1).await);
    let r = &fake.requests()[0];
    assert_eq!(
        r.header("authorization"),
        Some(format!("Bearer {SECRET}").as_str())
    );
    let form = r.form();
    let get = |k: &str| form.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    assert_eq!(get("time"), Some("10"));
    assert_eq!(get("increment"), Some("5"));
    assert_eq!(get("rated"), Some("true"));
    assert_eq!(get("variant"), Some("standard"));
    assert!(!r.body.contains(SECRET) && !r.query.contains(SECRET));

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!h.is_finished(), "the seek connection must stay open");
    assert_eq!(fake.closed_count("POST", "/api/board/seek"), 0);
    // `outcome` does not resolve while open.
    assert!(
        tokio::time::timeout(Duration::from_millis(100), h.outcome())
            .await
            .is_err()
    );
    h.cancel();
    assert!(
        fake.wait_closed("POST", "/api/board/seek", 1).await,
        "cancel closes the connection"
    );
}

#[tokio::test]
async fn dropping_a_seek_handle_also_cancels_it() {
    let fake = fake().await;
    let (resp, _live) = Response::live();
    fake.enqueue("POST", "/api/board/seek", resp);
    let h = client(&fake).seek(&SeekParams::new(15, 10)).expect("seek");
    assert!(fake.wait_requests("POST", "/api/board/seek", 1).await);
    drop(h);
    assert!(fake.wait_closed("POST", "/api/board/seek", 1).await);
}

#[tokio::test]
async fn seek_outcomes() {
    // Closed by the server: counts as paired.
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/board/seek",
        Response::stream(vec![Step::Close]),
    );
    let mut h = client(&fake).seek(&SeekParams::new(10, 0)).expect("seek");
    assert_eq!(h.outcome().await, SeekOutcome::Closed);
    assert_eq!(
        h.outcome().await,
        SeekOutcome::Failed(Error::Closed),
        "a second call has nothing left"
    );
    assert!(
        h.is_finished()
            || tokio::time::timeout(Duration::from_millis(200), async {
                while !h.is_finished() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .is_ok()
    );

    // Connection cut.
    let fake = m8_3_support::fake().await;
    fake.enqueue(
        "POST",
        "/api/board/seek",
        Response::stream(vec![Step::Drop]),
    );
    let mut h = client(&fake).seek(&SeekParams::new(10, 0)).expect("seek");
    assert!(matches!(
        h.outcome().await,
        SeekOutcome::Failed(Error::Network(_))
    ));

    // HTTP errors.
    for (status, check) in [
        (
            400u16,
            (|e: &Error| matches!(e, Error::Rejected { status: 400, .. })) as fn(&Error) -> bool,
        ),
        (401, |e| *e == Error::Unauthorized),
        (429, |e| matches!(e, Error::RateLimited { .. })),
        (503, |e| *e == Error::Server(503)),
    ] {
        let fake = m8_3_support::fake().await;
        fake.fallback(
            "POST",
            "/api/board/seek",
            Response::json(status, r#"{"error":"bad"}"#),
        );
        let mut h = client(&fake).seek(&SeekParams::new(10, 0)).expect("seek");
        match h.outcome().await {
            SeekOutcome::Failed(e) => assert!(check(&e), "{status}: {e:?}"),
            other => panic!("{status}: {other:?}"),
        }
        assert_eq!(
            fake.requests().len(),
            1,
            "a seek is never resent ({status})"
        );
    }
}

#[tokio::test]
async fn a_seek_header_timeout_fails_with_timeout() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/board/seek",
        Response::stream(vec![Step::Hold]).head_delay(Duration::from_secs(3)),
    );
    let c = client_with(&fake, |cfg| {
        cfg.streams.header_timeout = Duration::from_millis(200);
    });
    let mut h = c.seek(&SeekParams::new(10, 0)).expect("seek");
    assert_eq!(h.outcome().await, SeekOutcome::Failed(Error::Timeout));
}

#[tokio::test]
async fn a_seek_429_blocks_the_next_seek_without_the_network() {
    let fake = fake().await;
    fake.enqueue("POST", "/api/board/seek", Response::status(429));
    let c = client(&fake);
    let mut h = c.seek(&SeekParams::new(10, 0)).expect("seek");
    assert!(matches!(
        h.outcome().await,
        SeekOutcome::Failed(Error::RateLimited { .. })
    ));
    assert!(matches!(
        c.seek(&SeekParams::new(10, 0)),
        Err(Error::RateLimited { .. })
    ));
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn m9_6_a_chess960_challenge_posts_variant_and_fen() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/challenge/alice",
        Response::stream(vec![
            Step::line(r#"{"id":"chal0001","status":"created"}"#),
            Step::line(r#"{"done":"accepted"}"#),
            Step::Hold,
        ]),
    );
    let mut p = ChallengeParams::new(600, 5);
    p.chess960 = true;
    p.initial_fen = Some("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1".to_owned());
    let mut h = client(&fake).challenge("alice", &p).expect("challenge");
    assert_eq!(recv(&mut h).await, StreamMessage::Connected);
    assert!(fake.wait_requests("POST", "/api/challenge/alice", 1).await);
    let form = fake.requests()[0].form();
    let get = |k: &str| form.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    assert_eq!(get("variant"), Some("chess960"));
    assert_eq!(
        get("fen"),
        Some("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1")
    );
    assert_eq!(get("keepAliveStream"), Some("true"));
}

#[tokio::test]
async fn m9_6_a_chess960_seek_posts_the_variant() {
    let fake = fake().await;
    let (resp, _live) = Response::live();
    fake.enqueue("POST", "/api/board/seek", resp);
    let mut p = SeekParams::new(10, 5);
    p.chess960 = true;
    let h = client(&fake).seek(&p).expect("seek");
    assert!(fake.wait_requests("POST", "/api/board/seek", 1).await);
    let form = fake.requests()[0].form();
    let get = |k: &str| form.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    assert_eq!(get("variant"), Some("chess960"));
    assert_eq!(get("fen"), None);
    h.cancel();
}

#[tokio::test]
async fn m9_6_an_incoming_chess960_challenge_is_playable_and_can_be_accepted() {
    let fake = fake().await;
    let line = r#"{"type":"challenge","challenge":{"id":"chal0960","status":"created","direction":"in","variant":{"key":"chess960","name":"Chess960"},"initialFen":"bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1","timeControl":{"type":"clock","limit":600,"increment":0}}}"#;
    fake.enqueue(
        "GET",
        EVENTS,
        Response::stream(vec![Step::line(line), Step::Hold]),
    );
    fake.fallback("POST", "/api/challenge/chal0960/accept", Response::ok());
    let c = client(&fake);
    let mut s = c.open_event_stream().expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    let Event::Challenge(ch) = item(recv(&mut s).await) else {
        panic!("expected a challenge");
    };
    assert!(ch.is_playable(), "Chess960 is a variant the app plays");
    assert!(ch.initial_fen.is_some());
    c.accept_challenge(&ch.id).await.expect("accept");
    assert!(
        fake.wait_requests("POST", "/api/challenge/chal0960/accept", 1)
            .await
    );
}
