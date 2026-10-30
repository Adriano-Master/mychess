//! `Client` against the local fake server (no real network).

mod common;

use std::time::Duration;

use common::{GAME_ID, TOKEN, client, client_with, fake};
use mychess_online::{
    Error, GameAction,
    models::{ChallengeParams, SeekParams},
    testing::{Response, Step},
};

const ACCOUNT: &str = include_str!("fixtures/account.json");

#[tokio::test]
async fn account_sends_bearer_and_reads_scopes() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::json(200, ACCOUNT).header(
            "X-OAuth-Scopes",
            "board:play, challenge:read,challenge:write",
        ),
    );
    let info = client(&fake).validate_login().await.expect("account");
    assert_eq!(info.account.username, "Bob");
    assert!(info.can_play() && info.can_challenge());
    let reqs = fake.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(
        reqs[0].header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    assert!(
        reqs[0]
            .header("user-agent")
            .is_some_and(|u| u.starts_with("MyChess/"))
    );
    assert!(!reqs[0].path.contains(TOKEN) && !reqs[0].query.contains(TOKEN));
}

#[tokio::test]
async fn scopes_decide_the_login() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::json(200, ACCOUNT).header("X-OAuth-Scopes", "challenge:read"),
    );
    let err = client(&fake).validate_login().await.err();
    assert_eq!(
        err,
        Some(Error::ScopeMissing {
            scope: "board:play".to_owned()
        })
    );

    fake.enqueue(
        "GET",
        "/api/account",
        Response::json(200, ACCOUNT).header("X-OAuth-Scopes", "board:play"),
    );
    let info = client(&fake).validate_login().await.expect("play only");
    assert!(info.can_play() && !info.can_challenge());

    // Header absent: scopes unknown, assumed enough.
    fake.enqueue("GET", "/api/account", Response::json(200, ACCOUNT));
    let info = client(&fake).validate_login().await.expect("no header");
    assert_eq!(info.scopes, None);
    assert!(info.can_play());
}

#[tokio::test]
async fn account_errors_never_leak_the_token() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::json(401, r#"{"error":"No such token"}"#),
    );
    let err = client(&fake).account().await.expect_err("error");
    assert_eq!(err, Error::Unauthorized);
    assert!(!format!("{err:?} {err}").contains(TOKEN));

    fake.enqueue(
        "GET",
        "/api/account",
        Response::json(200, r#"{"id":"bot","username":"Bot","title":"BOT"}"#),
    );
    assert_eq!(client(&fake).account().await.err(), Some(Error::BotAccount));

    fake.enqueue("GET", "/api/account", Response::json(200, "not json"));
    assert!(matches!(
        client(&fake).account().await,
        Err(Error::Protocol(_))
    ));
    for r in fake.requests() {
        assert!(!r.path.contains(TOKEN) && !r.query.contains(TOKEN));
    }
}

#[tokio::test]
async fn network_failure_is_an_error_not_a_panic() {
    // Nothing listens on this port (the fake was dropped).
    let url = {
        let f = fake().await;
        f.base_url()
    };
    let c = client_with(&fake().await, |cfg| cfg.base_url = url);
    let err = c.account().await.expect_err("error");
    assert!(matches!(err, Error::Network(_) | Error::Timeout), "{err:?}");
    assert!(!format!("{err:?}").contains(TOKEN));
}

#[tokio::test]
async fn user_exists_maps_404() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/user/alice",
        Response::json(200, r#"{"id":"alice"}"#),
    );
    fake.enqueue(
        "GET",
        "/api/user/ghost",
        Response::json(404, r#"{"error":"Not found"}"#),
    );
    let c = client(&fake);
    assert_eq!(c.user_exists("alice").await, Ok(true));
    assert_eq!(c.user_exists("ghost").await, Ok(false));
    assert_eq!(fake.requests()[0].query, "profile=false");
    assert!(matches!(
        c.user_exists("a/b").await,
        Err(Error::InvalidArgument { .. })
    ));
    assert_eq!(fake.requests().len(), 2);
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::status(302).header("Location", "/elsewhere"),
    );
    assert_eq!(
        client(&fake).account().await.err(),
        Some(Error::Redirect(302))
    );
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn bodies_are_bounded() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::json(200, &format!("{{\"id\":\"{}\"}}", "x".repeat(5000))),
    );
    let c = client_with(&fake, |cfg| cfg.max_body = 1000);
    assert_eq!(c.account().await.err(), Some(Error::LineTooLong));
}

#[tokio::test]
async fn move_request_shape_and_draw_offer() {
    let fake = fake().await;
    fake.fallback(
        "POST",
        &format!("/api/board/game/{GAME_ID}/move/e2e4"),
        Response::ok(),
    );
    fake.fallback(
        "POST",
        &format!("/api/board/game/{GAME_ID}/move/e7e8q"),
        Response::ok(),
    );
    let c = client(&fake);
    c.play_move(GAME_ID, "e2e4", false).await.expect("move");
    c.play_move(GAME_ID, "e7e8q", true)
        .await
        .expect("promotion with draw offer");
    let reqs = fake.requests();
    assert_eq!(reqs[0].query, "");
    assert_eq!(reqs[1].query, "offeringDraw=true");
    assert_eq!(
        reqs[0].header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
}

#[tokio::test]
async fn invalid_arguments_send_nothing() {
    let fake = fake().await;
    let c = client(&fake);
    assert!(matches!(
        c.play_move("../x", "e2e4", false).await,
        Err(Error::InvalidArgument { .. })
    ));
    assert!(matches!(
        c.play_move(GAME_ID, "e2e9", false).await,
        Err(Error::InvalidArgument { .. })
    ));
    assert!(matches!(
        c.play_move(GAME_ID, "E2E4", false).await,
        Err(Error::InvalidArgument { .. })
    ));
    assert!(matches!(
        c.seek(&SeekParams::new(3, 0)),
        Err(Error::TimeControl(_))
    ));
    assert!(matches!(
        c.challenge("bob", &ChallengeParams::new(60, 0)),
        Err(Error::TimeControl(_))
    ));
    assert!(matches!(
        c.challenge("b/b", &ChallengeParams::new(600, 0)),
        Err(Error::InvalidArgument { .. })
    ));
    assert!(matches!(
        c.open_game_stream("x"),
        Err(Error::InvalidArgument { .. })
    ));
    assert!(matches!(
        c.decline_challenge(GAME_ID, "no way").await,
        Err(Error::InvalidArgument { .. })
    ));
    assert_eq!(fake.requests().len(), 0);
}

#[tokio::test]
async fn a_400_is_reported_and_not_retried() {
    let fake = fake().await;
    fake.fallback(
        "POST",
        &format!("/api/board/game/{GAME_ID}/move/e2e4"),
        Response::json(400, r#"{"error":"Not your turn"}"#),
    );
    let err = client(&fake).play_move(GAME_ID, "e2e4", false).await.err();
    assert_eq!(
        err,
        Some(Error::Rejected {
            status: 400,
            reason: "Not your turn".to_owned()
        })
    );
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn moves_are_retried_up_to_three_times() {
    let fake = fake().await;
    let path = format!("/api/board/game/{GAME_ID}/move/e2e4");
    fake.enqueue("POST", &path, Response::status(503));
    fake.enqueue("POST", &path, Response::status(500));
    fake.enqueue("POST", &path, Response::ok());
    client(&fake)
        .play_move(GAME_ID, "e2e4", false)
        .await
        .expect("third attempt works");
    assert_eq!(fake.requests().len(), 3);

    let fake = fake_all_failing().await;
    let err = client(&fake).play_move(GAME_ID, "e2e4", false).await.err();
    assert_eq!(err, Some(Error::Server(503)));
    assert_eq!(fake.requests().len(), 3, "gives up after three attempts");
}

#[tokio::test]
async fn a_400_after_a_cut_connection_is_ambiguous() {
    let fake = fake().await;
    let path = format!("/api/board/game/{GAME_ID}/move/e2e4");
    // The first attempt dies mid-response (it may have been applied)...
    fake.enqueue("POST", &path, Response::stream(vec![Step::Drop]));
    // ...and the retry is told it is not our turn.
    fake.enqueue(
        "POST",
        &path,
        Response::json(400, r#"{"error":"Not your turn"}"#),
    );
    let err = client(&fake).play_move(GAME_ID, "e2e4", false).await.err();
    assert_eq!(
        err,
        Some(Error::AmbiguousAfterRetry {
            status: 400,
            reason: "Not your turn".to_owned()
        })
    );
    assert_eq!(fake.requests().len(), 2);
}

async fn fake_all_failing() -> mychess_online::testing::FakeLichess {
    let fake = fake().await;
    fake.fallback(
        "POST",
        &format!("/api/board/game/{GAME_ID}/move/e2e4"),
        Response::status(503),
    );
    fake
}

#[tokio::test]
async fn a_429_blocks_normal_requests_but_not_moves() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::status(429).header("Retry-After", "0"),
    );
    fake.fallback("GET", "/api/account", Response::json(200, ACCOUNT));
    fake.fallback(
        "POST",
        &format!("/api/board/game/{GAME_ID}/resign"),
        Response::ok(),
    );
    let c = client(&fake);

    let first = c.account().await.expect_err("429");
    assert!(matches!(first, Error::RateLimited { .. }));
    // Blocked: fails fast, without a second request to the server.
    let second = c.account().await.expect_err("blocked");
    assert!(matches!(second, Error::RateLimited { .. }));
    assert_eq!(fake.requests_for("GET", "/api/account").len(), 1);
    assert!(c.gate().blocked_for().is_some());
    // A game action ignores the block.
    c.game_action(GAME_ID, GameAction::Resign)
        .await
        .expect("resign");

    tokio::time::sleep(Duration::from_millis(200)).await;
    c.account().await.expect("block is over");
    assert_eq!(fake.requests_for("GET", "/api/account").len(), 2);
}

#[tokio::test]
async fn game_actions_hit_the_documented_paths() {
    let fake = fake().await;
    let base = format!("/api/board/game/{GAME_ID}");
    for tail in [
        "resign",
        "abort",
        "draw/yes",
        "draw/no",
        "takeback/yes",
        "takeback/no",
        "claim-victory",
        "claim-draw",
    ] {
        fake.fallback("POST", &format!("{base}/{tail}"), Response::ok());
    }
    let c = client(&fake);
    for action in [
        GameAction::Resign,
        GameAction::Abort,
        GameAction::Draw(true),
        GameAction::Draw(false),
        GameAction::Takeback(true),
        GameAction::Takeback(false),
        GameAction::ClaimVictory,
        GameAction::ClaimDraw,
    ] {
        c.game_action(GAME_ID, action).await.expect("action");
    }
    let paths: Vec<String> = fake.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(paths.len(), 8);
    assert!(paths.iter().all(|p| p.starts_with(&base)));
}

#[tokio::test]
async fn challenge_answers() {
    let fake = fake().await;
    fake.fallback("POST", "/api/challenge/chal0001/accept", Response::ok());
    fake.fallback("POST", "/api/challenge/chal0001/decline", Response::ok());
    fake.fallback("POST", "/api/challenge/chal0001/cancel", Response::ok());
    let c = client(&fake);
    c.accept_challenge("chal0001").await.expect("accept");
    c.decline_challenge("chal0001", "variant")
        .await
        .expect("decline");
    c.cancel_challenge("chal0001").await.expect("cancel");
    let reqs = fake.requests();
    assert_eq!(
        reqs[1].form(),
        [("reason".to_owned(), "variant".to_owned())]
    );
    assert_eq!(reqs.len(), 3);
}
