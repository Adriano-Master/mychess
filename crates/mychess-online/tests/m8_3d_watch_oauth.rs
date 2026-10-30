//! Phase d of spec m8-3 against the fake server: the public watch stream, the
//! PGN export, the OAuth + PKCE token exchange and the whole loopback login
//! (D3000-D3015). No `lichess.org`.

mod m8_3_support;

use std::time::Duration;

use m8_3_support::{GAME, SECRET, client, fake, item, recv};
use mychess_online::{
    Client, ClientConfig, Error, StreamMessage,
    models::{Status, WatchEvent},
    oauth::{CLIENT_ID, OAuthFlow},
    testing::{Response, Step},
};

const DESCRIPTION: &str = r#"{"id":"abcd1234","rated":false,"variant":"standard","speed":"blitz","status":"started","players":{"white":{"user":{"name":"Alice","id":"alice"},"rating":1900},"black":{"user":{"name":"Bob","id":"bob"},"rating":1800}},"fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"}"#;
const MOVE: &str = r#"{"fen":"rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1","lm":"e2e4","wc":300,"bc":300}"#;
const FINAL: &str = r#"{"id":"abcd1234","status":"resign","winner":"white"}"#;

fn tokenless(fake: &mychess_online::testing::FakeLichess) -> Client {
    Client::new(ClientConfig {
        base_url: fake.base_url(),
        ..ClientConfig::default()
    })
    .expect("client")
}

#[tokio::test]
async fn watch_stream_is_public_and_ends_with_the_final_description() {
    let fake = fake().await;
    let path = format!("/api/stream/game/{GAME}");
    fake.enqueue(
        "GET",
        &path,
        Response::stream(vec![
            Step::line(DESCRIPTION),
            Step::KeepAlive,
            Step::line(MOVE),
            Step::line("not json"),
            Step::line(FINAL),
            Step::Hold,
        ]),
    );
    // The client has a token; the watch stream must not send it.
    let mut s = client(&fake).open_watch_stream(GAME).expect("stream");
    assert_eq!(recv(&mut s).await, StreamMessage::Connected);
    assert!(
        matches!(item(recv(&mut s).await), WatchEvent::Description(g) if g.white.name == "Alice")
    );
    assert!(
        matches!(item(recv(&mut s).await), WatchEvent::Move(m) if m.last_move.as_deref() == Some("e2e4") && m.white_clock == 300)
    );
    assert!(matches!(
        item(recv(&mut s).await),
        WatchEvent::Unparsed { .. }
    ));
    assert!(
        matches!(item(recv(&mut s).await), WatchEvent::Description(g) if g.status == Some(Status::Resign))
    );
    assert_eq!(recv(&mut s).await, StreamMessage::Ended);
    let req = fake.requests_for("GET", &path);
    assert_eq!(req.len(), 1);
    assert!(
        req[0].header("authorization").is_none(),
        "token leaked to the public stream"
    );
    assert!(!format!("{req:?}").contains(SECRET));
}

#[tokio::test]
async fn watch_stream_refuses_bad_ids_without_touching_the_network() {
    let fake = fake().await;
    let c = client(&fake);
    for bad in ["", "abc", "../../etc/passwd", "abcd 1234"] {
        assert_eq!(
            c.open_watch_stream(bad).err(),
            Some(Error::InvalidArgument { what: "game id" }),
            "{bad:?}"
        );
    }
    assert_eq!(fake.requests().len(), 0);
}

#[tokio::test]
async fn export_pgn_is_public_bounded_and_asks_for_clocks() {
    let fake = fake().await;
    let path = format!("/game/export/{GAME}");
    fake.enqueue(
        "GET",
        &path,
        Response::json(200, "[Event \"x\"]\n\n1. e4 *\n"),
    );
    let c = client(&fake);
    let pgn = c.export_pgn(GAME).await.expect("pgn");
    assert!(String::from_utf8_lossy(&pgn).contains("1. e4"));
    let req = &fake.requests_for("GET", &path)[0];
    assert!(req.header("authorization").is_none());
    assert!(
        req.query_pairs()
            .contains(&("clocks".to_owned(), "true".to_owned()))
    );
    fake.enqueue("GET", &path, Response::status(404));
    assert_eq!(c.export_pgn(GAME).await.err(), Some(Error::NotFound));
    assert_eq!(
        c.export_pgn("x/y").await.err(),
        Some(Error::InvalidArgument { what: "game id" })
    );
}

#[tokio::test]
async fn exchange_code_sends_pkce_form_and_never_echoes_secrets() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/token",
        Response::json(
            200,
            r#"{"token_type":"Bearer","access_token":"lio_FreshOAuthToken123","expires_in":31536000}"#,
        ),
    );
    let c = tokenless(&fake);
    let token = c
        .exchange_code(
            "the-code",
            "the-verifier",
            "http://127.0.0.1:1/callback",
            CLIENT_ID,
        )
        .await
        .expect("token");
    assert_eq!(token.expose(), "lio_FreshOAuthToken123");
    assert!(!format!("{token:?}").contains("FreshOAuthToken"));
    let form = fake.requests_for("POST", "/api/token")[0].form();
    for (k, v) in [
        ("grant_type", "authorization_code"),
        ("code", "the-code"),
        ("code_verifier", "the-verifier"),
        ("redirect_uri", "http://127.0.0.1:1/callback"),
        ("client_id", CLIENT_ID),
    ] {
        assert!(form.contains(&(k.to_owned(), v.to_owned())), "missing {k}");
    }
}

#[tokio::test]
async fn exchange_code_errors_carry_no_secret() {
    let fake = fake().await;
    let c = tokenless(&fake);
    fake.enqueue(
        "POST",
        "/api/token",
        Response::json(400, r#"{"error":"invalid_grant"}"#),
    );
    let err = c
        .exchange_code("c0de", "v3rifier", "http://127.0.0.1:1/callback", CLIENT_ID)
        .await;
    let err = err.expect_err("rejected");
    assert!(matches!(err, Error::Rejected { status: 400, .. }));
    assert!(!format!("{err} {err:?}").contains("c0de") && !format!("{err:?}").contains("v3rifier"));
    fake.enqueue(
        "POST",
        "/api/token",
        Response::json(200, r#"{"token_type":"Bearer"}"#),
    );
    assert!(matches!(
        c.exchange_code("a", "b", "http://127.0.0.1:1/callback", CLIENT_ID)
            .await,
        Err(Error::Protocol(_))
    ));
    fake.enqueue(
        "POST",
        "/api/token",
        Response::json(200, r#"{"access_token":"has space"}"#),
    );
    assert!(matches!(
        c.exchange_code("a", "b", "http://127.0.0.1:1/callback", CLIENT_ID)
            .await,
        Err(Error::Protocol(_))
    ));
}

/// Plays the browser: opens the callback URL the flow listens on.
async fn browser_visit(redirect: &str, query: &str) -> u16 {
    let http = reqwest::Client::new();
    http.get(format!("{redirect}?{query}"))
        .send()
        .await
        .expect("callback reachable")
        .status()
        .as_u16()
}

#[tokio::test]
async fn full_loopback_login_checks_state_and_exchanges_the_code() {
    let fake = fake().await;
    fake.enqueue(
        "POST",
        "/api/token",
        Response::json(
            200,
            r#"{"token_type":"Bearer","access_token":"lio_LoopbackToken42"}"#,
        ),
    );
    let flow = OAuthFlow::start(&fake.base_url()).await.expect("flow");
    let url = flow.authorize_url().expect("url");
    assert!(
        url.contains("code_challenge_method=S256")
            && url.contains(&format!("state={}", flow.state()))
    );
    let (redirect, state) = (flow.redirect_uri(), flow.state().to_owned());
    let finish = tokio::spawn(flow.finish(Duration::from_secs(10)));
    // A forged callback (wrong state) is ignored; the real one completes.
    assert_eq!(
        browser_visit(&redirect, "code=forged&state=nope").await,
        400
    );
    assert_eq!(
        browser_visit(&redirect, &format!("code=real-code&state={state}")).await,
        200
    );
    let token = finish.await.expect("join").expect("token");
    assert_eq!(token.expose(), "lio_LoopbackToken42");
    let form = fake.requests_for("POST", "/api/token")[0].form();
    assert!(form.contains(&("code".to_owned(), "real-code".to_owned())));
    let verifier = form
        .iter()
        .find(|(k, _)| k == "code_verifier")
        .expect("verifier");
    assert_eq!(verifier.1.len(), 43);
    assert!(form.contains(&("redirect_uri".to_owned(), redirect)));
    // The challenge in the URL is the S256 of that verifier.
    let challenge = mychess_online::oauth::Pkce::from_verifier(verifier.1.clone());
    assert!(url.contains(&format!("code_challenge={}", challenge.challenge())));
}

#[tokio::test]
async fn login_denied_in_the_browser_fails_without_asking_for_a_token() {
    let fake = fake().await;
    let flow = OAuthFlow::start(&fake.base_url()).await.expect("flow");
    let (redirect, state) = (flow.redirect_uri(), flow.state().to_owned());
    let finish = tokio::spawn(flow.finish(Duration::from_secs(10)));
    browser_visit(&redirect, &format!("error=access_denied&state={state}")).await;
    assert_eq!(
        finish.await.expect("join").err(),
        Some(Error::OAuth("authorization denied"))
    );
    assert_eq!(fake.requests_for("POST", "/api/token").len(), 0);
}

#[tokio::test]
async fn login_times_out_when_the_browser_never_returns() {
    let fake = fake().await;
    let flow = OAuthFlow::start(&fake.base_url()).await.expect("flow");
    assert_eq!(
        flow.finish(Duration::from_millis(100)).await.err(),
        Some(Error::Timeout)
    );
}

#[tokio::test]
async fn login_refuses_an_insecure_base_url() {
    assert_eq!(
        OAuthFlow::start("http://lichess.org").await.err(),
        Some(Error::InsecureUrl)
    );
}
