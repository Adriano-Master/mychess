//! Contract tests of the HTTP `Client` against the local fake server (spec m8-3
//! D1301, D1305, D1308, D1341, D1344, D1347, D1352; acceptance criterion
//! "Client"). No real network.

#![allow(clippy::assert_is_empty, clippy::type_complexity)]

mod m8_3_support;

use std::time::Duration;

use m8_3_support::{GAME, SECRET, client, client_with, fake};
use mychess_online::{
    Client, ClientConfig, Error, GameAction, Token,
    models::{ChallengeParams, SeekParams},
    testing::{FakeLichess, Response, Step},
};

const ACCOUNT: &str =
    r#"{"id":"bob","username":"Bob","perfs":{"rapid":{"games":3,"rating":1500}}}"#;

fn move_path(uci: &str) -> String {
    format!("/api/board/game/{GAME}/move/{uci}")
}

fn assert_no_secret_in_requests(fake: &FakeLichess) {
    for r in fake.requests() {
        assert!(!r.path.contains(SECRET), "path leaks the token: {}", r.path);
        assert!(!r.query.contains(SECRET), "query leaks the token");
        assert!(!r.body.contains(SECRET), "body leaks the token");
        let leaked_header = r
            .headers
            .iter()
            .any(|(k, v)| k != "authorization" && v.contains(SECRET));
        assert!(!leaked_header, "token in a header other than Authorization");
    }
}

// ------------------------------------------------------------------ bearer

#[tokio::test]
async fn bearer_is_sent_on_every_authenticated_call_and_never_elsewhere() {
    let fake = fake().await;
    fake.fallback("GET", "/api/account", Response::json(200, ACCOUNT));
    fake.fallback("GET", "/api/user/alice", Response::json(200, "{}"));
    fake.fallback("POST", &move_path("e2e4"), Response::ok());
    fake.fallback("POST", "/api/challenge/chal0001/accept", Response::ok());
    fake.fallback("POST", "/api/challenge/chal0001/decline", Response::ok());
    fake.fallback("POST", "/api/challenge/chal0001/cancel", Response::ok());
    let c = client(&fake);
    c.account().await.expect("account");
    c.user_exists("alice").await.expect("user");
    c.play_move(GAME, "e2e4", true).await.expect("move");
    c.accept_challenge("chal0001").await.expect("accept");
    c.decline_challenge("chal0001", "later")
        .await
        .expect("decline");
    c.cancel_challenge("chal0001").await.expect("cancel");

    let reqs = fake.requests();
    assert_eq!(reqs.len(), 6);
    let want = format!("Bearer {SECRET}");
    for r in &reqs {
        assert_eq!(r.header("authorization"), Some(want.as_str()), "{}", r.path);
    }
    assert_no_secret_in_requests(&fake);
}

#[tokio::test]
async fn a_client_without_token_sends_no_authorization_header() {
    let fake = fake().await;
    fake.fallback("GET", "/api/user/alice", Response::json(200, "{}"));
    let c = client_with(&fake, |cfg| cfg.token = None);
    assert!(!c.is_authenticated());
    c.user_exists("alice").await.expect("public call");
    assert_eq!(fake.requests()[0].header("authorization"), None);
    assert!(client(&fake).is_authenticated());
}

#[tokio::test]
async fn the_token_is_sent_exactly_once_per_request_and_unmodified() {
    // 600 characters is a legal token (the spec asks for at least 512).
    let long = format!("lip_{}", "aB3_".repeat(150));
    let fake = fake().await;
    fake.fallback("GET", "/api/account", Response::json(200, ACCOUNT));
    let c = client_with(&fake, |cfg| cfg.token = Token::new(&long).ok());
    c.account().await.expect("account");
    let auth: Vec<_> = fake.requests()[0]
        .headers
        .iter()
        .filter(|(k, _)| k == "authorization")
        .cloned()
        .collect();
    assert_eq!(auth.len(), 1);
    assert_eq!(auth[0].1, format!("Bearer {long}"));
}

#[tokio::test]
async fn user_agent_is_honest_and_configurable() {
    let fake = fake().await;
    fake.fallback("GET", "/api/account", Response::json(200, ACCOUNT));
    client(&fake).account().await.expect("account");
    let ua = fake.requests()[0].header("user-agent").map(str::to_owned);
    assert_eq!(
        ua.as_deref(),
        Some(concat!("MyChess/", env!("CARGO_PKG_VERSION")))
    );
    let c = client_with(&fake, |cfg| cfg.user_agent = "MyChess/9.9 test".to_owned());
    c.account().await.expect("account");
    assert_eq!(
        fake.requests()[1].header("user-agent"),
        Some("MyChess/9.9 test")
    );
}

// ------------------------------------------------------------ account/scopes

#[tokio::test]
async fn a_bot_account_is_refused_by_account_and_by_validate_login() {
    let fake = fake().await;
    fake.fallback(
        "GET",
        "/api/account",
        Response::json(
            200,
            r#"{"id":"somebot","username":"SomeBot","title":"BOT"}"#,
        )
        .header("X-OAuth-Scopes", "board:play"),
    );
    let c = client(&fake);
    assert_eq!(c.account().await.err(), Some(Error::BotAccount));
    assert_eq!(c.validate_login().await.err(), Some(Error::BotAccount));
}

#[tokio::test]
async fn a_titled_human_account_is_not_a_bot() {
    let fake = fake().await;
    fake.fallback(
        "GET",
        "/api/account",
        Response::json(200, r#"{"id":"g","username":"G","title":"GM"}"#),
    );
    let info = client(&fake).account().await.expect("human");
    assert_eq!(info.account.title.as_deref(), Some("GM"));
}

#[tokio::test]
async fn scope_header_parsing_and_login_decision() {
    let cases: Vec<(Option<&str>, bool, bool)> = vec![
        // (header, can_play, can_challenge)
        (Some("board:play"), true, false),
        (
            Some("board:play,challenge:read,challenge:write"),
            true,
            true,
        ),
        (Some(" challenge:write ,  board:play "), true, true),
        (Some("challenge:read,challenge:write"), false, true),
        (Some("challenge:read"), false, false),
        (Some(""), false, false),
        (Some(",,"), false, false),
        // Look-alikes must not satisfy the check.
        (Some("xboard:play,board:play2,board:"), false, false),
        (Some("BOARD:PLAY"), false, false),
        // Absent header: unknown scopes are assumed enough.
        (None, true, true),
    ];
    for (header, play, challenge) in cases {
        let fake = fake().await;
        let mut resp = Response::json(200, ACCOUNT);
        if let Some(h) = header {
            resp = resp.header("X-OAuth-Scopes", h);
        }
        fake.fallback("GET", "/api/account", resp);
        let c = client(&fake);
        let info = c.account().await.expect("account");
        assert_eq!(info.can_play(), play, "{header:?}");
        assert_eq!(info.can_challenge(), challenge, "{header:?}");
        let login = c.validate_login().await;
        if play {
            assert!(login.is_ok(), "{header:?}");
        } else {
            assert_eq!(
                login.err(),
                Some(Error::ScopeMissing {
                    scope: "board:play".to_owned()
                }),
                "{header:?}"
            );
        }
    }
}

#[tokio::test]
async fn the_scope_header_name_is_case_insensitive() {
    let fake = fake().await;
    fake.fallback(
        "GET",
        "/api/account",
        Response::json(200, ACCOUNT).header("x-oauth-scopes", "challenge:read"),
    );
    assert!(client(&fake).validate_login().await.is_err());
}

#[tokio::test]
async fn account_status_codes_map_to_error_variants() {
    let cases: Vec<(u16, fn(&Error) -> bool)> = vec![
        (401, |e| *e == Error::Unauthorized),
        (403, |e| matches!(e, Error::ScopeMissing { .. })),
        (404, |e| *e == Error::NotFound),
        (500, |e| *e == Error::Server(500)),
        (502, |e| *e == Error::Server(502)),
        (400, |e| matches!(e, Error::Rejected { status: 400, .. })),
        (418, |e| matches!(e, Error::Rejected { status: 418, .. })),
        (302, |e| *e == Error::Redirect(302)),
        (301, |e| *e == Error::Redirect(301)),
    ];
    for (status, check) in cases {
        let fake = fake().await;
        fake.fallback(
            "GET",
            "/api/account",
            Response::json(status, r#"{"error":"nope"}"#).header("Location", "/elsewhere"),
        );
        let err = client(&fake).account().await.expect_err("error status");
        assert!(check(&err), "status {status}: {err:?}");
        assert_eq!(
            fake.requests().len(),
            1,
            "no retry or redirect for {status}"
        );
    }
}

#[tokio::test]
async fn error_reasons_are_clipped_and_raw_bodies_are_not_exposed() {
    let fake = fake().await;
    let long = "x".repeat(2000);
    fake.fallback(
        "POST",
        &move_path("e2e4"),
        Response::json(400, &format!(r#"{{"error":"{long}"}}"#)),
    );
    let Err(Error::Rejected { reason, .. }) = client(&fake).play_move(GAME, "e2e4", false).await
    else {
        panic!("expected Rejected");
    };
    assert!(
        reason.chars().count() <= 200,
        "reason of {} chars",
        reason.len()
    );

    let fake2 = m8_3_support::fake().await;
    fake2.fallback(
        "POST",
        &move_path("e2e4"),
        Response::json(400, "<html>internal secret page</html>"),
    );
    let err = client(&fake2)
        .play_move(GAME, "e2e4", false)
        .await
        .expect_err("rejected");
    assert!(!format!("{err} {err:?}").contains("internal secret page"));
}

#[tokio::test]
async fn invalid_account_json_is_a_protocol_error_without_the_body() {
    let fake = fake().await;
    fake.fallback("GET", "/api/account", Response::json(200, "oops-not-json"));
    let err = client(&fake).account().await.expect_err("protocol");
    assert!(matches!(err, Error::Protocol(_)));
    assert!(!format!("{err}").contains("oops-not-json"));
}

#[tokio::test]
async fn bodies_over_the_limit_are_refused() {
    let fake = fake().await;
    let big = format!(r#"{{"id":"bob","pad":"{}"}}"#, "p".repeat(5000));
    fake.fallback("GET", "/api/account", Response::json(200, &big));
    let c = client_with(&fake, |cfg| cfg.max_body = 1000);
    assert_eq!(c.account().await.err(), Some(Error::LineTooLong));
}

#[tokio::test]
async fn chunked_bodies_without_content_length_are_bounded_too() {
    let fake = fake().await;
    // A chunked reply has no Content-Length: the limit must hold while reading.
    let piece = vec![b'p'; 400];
    let mut steps = vec![Step::Raw(b"{\"id\":\"bob\",\"pad\":\"".to_vec())];
    steps.extend((0..10).map(|_| Step::Raw(piece.clone())));
    steps.push(Step::Raw(b"\"}".to_vec()));
    fake.fallback("GET", "/api/account", Response::stream(steps));
    let c = client_with(&fake, |cfg| cfg.max_body = 1000);
    let r = c.account().await;
    assert_eq!(r.as_ref().err(), Some(&Error::LineTooLong), "{r:?}");

    // Under the limit the same shape works.
    let fake = m8_3_support::fake().await;
    fake.fallback(
        "GET",
        "/api/account",
        Response::stream(vec![
            Step::Raw(br#"{"id":"b"#.to_vec()),
            Step::Raw(br#"ob","username":"Bob"}"#.to_vec()),
        ]),
    );
    let info = client(&fake).account().await.expect("chunked body");
    assert_eq!(info.account.username, "Bob");
}

#[tokio::test]
async fn redirects_are_not_followed_and_the_token_is_not_forwarded() {
    let fake = fake().await;
    fake.fallback(
        "GET",
        "/api/account",
        Response::status(302).header("Location", "/evil"),
    );
    fake.fallback("GET", "/evil", Response::json(200, ACCOUNT));
    assert_eq!(
        client(&fake).account().await.err(),
        Some(Error::Redirect(302))
    );
    assert!(fake.requests_for("GET", "/evil").is_empty());

    fake.fallback(
        "POST",
        &format!("/api/board/game/{GAME}/resign"),
        Response::status(307).header("Location", "/evil"),
    );
    let err = client(&fake)
        .game_action(GAME, GameAction::Resign)
        .await
        .err();
    assert_eq!(err, Some(Error::Redirect(307)));
    assert!(fake.requests_for("POST", "/evil").is_empty());
    assert_eq!(
        fake.requests_for("POST", &format!("/api/board/game/{GAME}/resign"))
            .len(),
        1
    );
}

#[tokio::test]
async fn network_errors_and_timeouts_never_carry_the_token() {
    let dead = {
        let f = fake().await;
        f.base_url()
    };
    let probe = fake().await;
    let c = client_with(&probe, |cfg| cfg.base_url = dead);
    let err = c.account().await.expect_err("nothing listens");
    assert!(matches!(err, Error::Network(_) | Error::Timeout));
    assert!(!format!("{err} {err:?}").contains(SECRET));
    assert!(
        !format!("{err} {err:?}").contains("127.0.0.1"),
        "no URL in error text"
    );

    let fake = fake().await;
    fake.fallback(
        "GET",
        "/api/account",
        Response::json(200, ACCOUNT).head_delay(Duration::from_secs(3)),
    );
    let c = client_with(&fake, |cfg| {
        cfg.request_timeout = Duration::from_millis(150);
    });
    let err = c.account().await.expect_err("timeout");
    assert_eq!(err, Error::Timeout);
}

// ---------------------------------------------------------- game actions

#[tokio::test]
async fn every_game_action_uses_its_documented_method_and_path() {
    let base = format!("/api/board/game/{GAME}");
    let cases: Vec<(GameAction<'static>, String, &str)> = vec![
        (
            GameAction::Move {
                uci: "e2e4",
                offering_draw: false,
            },
            format!("{base}/move/e2e4"),
            "",
        ),
        (
            GameAction::Move {
                uci: "e7e8q",
                offering_draw: true,
            },
            format!("{base}/move/e7e8q"),
            "offeringDraw=true",
        ),
        (GameAction::Resign, format!("{base}/resign"), ""),
        (GameAction::Abort, format!("{base}/abort"), ""),
        (GameAction::Draw(true), format!("{base}/draw/yes"), ""),
        (GameAction::Draw(false), format!("{base}/draw/no"), ""),
        (
            GameAction::Takeback(true),
            format!("{base}/takeback/yes"),
            "",
        ),
        (
            GameAction::Takeback(false),
            format!("{base}/takeback/no"),
            "",
        ),
        (
            GameAction::ClaimVictory,
            format!("{base}/claim-victory"),
            "",
        ),
        (GameAction::ClaimDraw, format!("{base}/claim-draw"), ""),
    ];
    for (action, path, query) in cases {
        let fake = fake().await;
        fake.fallback("POST", &path, Response::ok());
        client(&fake)
            .game_action(GAME, action)
            .await
            .unwrap_or_else(|e| panic!("{action:?}: {e:?}"));
        let reqs = fake.requests();
        assert_eq!(reqs.len(), 1, "{action:?}");
        assert_eq!(reqs[0].method, "POST", "{action:?}");
        assert_eq!(reqs[0].path, path, "{action:?}");
        assert_eq!(reqs[0].query, query, "{action:?}");
        assert_eq!(
            reqs[0].header("authorization"),
            Some(format!("Bearer {SECRET}").as_str())
        );
    }
}

#[tokio::test]
async fn promotion_letters_and_every_square_are_accepted() {
    let fake = fake().await;
    for uci in ["e7e8q", "e7e8r", "e7e8b", "e7e8n", "a1h8", "h8a1", "a2a1q"] {
        fake.fallback("POST", &move_path(uci), Response::ok());
        client(&fake)
            .play_move(GAME, uci, false)
            .await
            .unwrap_or_else(|e| panic!("{uci}: {e:?}"));
    }
}

#[tokio::test]
async fn invalid_arguments_are_rejected_before_any_request() {
    let fake = fake().await;
    let c = client(&fake);

    for id in [
        "",
        "abc",
        "abcdefg",
        "abcd123",
        "abcdefghijklm",
        "abcd/234",
        "../../..",
        "abcd 234",
        "abcd123?",
        "abcd1234%2f",
        "abcd1234\n",
        "ábcd1234",
        "abcd-234",
        "abcd1234#x",
    ] {
        assert!(
            matches!(
                c.game_action(id, GameAction::Resign).await,
                Err(Error::InvalidArgument { .. })
            ),
            "game id {id:?}"
        );
        assert!(
            matches!(c.open_game_stream(id), Err(Error::InvalidArgument { .. })),
            "stream id {id:?}"
        );
        assert!(
            matches!(
                c.accept_challenge(id).await,
                Err(Error::InvalidArgument { .. })
            ),
            "challenge id {id:?}"
        );
    }

    for uci in [
        "", "e2", "e2e", "e2e44", "E2E4", "e2E4", "i2i4", "e0e4", "e9e4", "e2e0", "e2e9", "e7e8k",
        "e7e8Q", "e7e8qq", "e2-e4", "e2e4 ", " e2e4", "e2e4/..", "0000", "e2e4\n", "é2e4",
        "e7e8=Q",
    ] {
        assert!(
            matches!(
                c.game_action(
                    GAME,
                    GameAction::Move {
                        uci,
                        offering_draw: false
                    }
                )
                .await,
                Err(Error::InvalidArgument { .. })
            ),
            "uci {uci:?}"
        );
    }

    for name in [
        "",
        "a",
        &"a".repeat(31),
        "a/b",
        "a b",
        "../x",
        "ünï",
        "a?b=1",
        "a#b",
        "a%2fb",
        "a.b",
        "a\nb",
        "ab\0",
    ] {
        assert!(
            matches!(
                c.user_exists(name).await,
                Err(Error::InvalidArgument { .. })
            ),
            "user {name:?}"
        );
        assert!(
            matches!(
                c.challenge(name, &ChallengeParams::new(600, 0)),
                Err(Error::InvalidArgument { .. })
            ),
            "challenge to {name:?}"
        );
    }

    for reason in ["", "a/b", "x y", "gen3ric", "later\n", "é"] {
        assert!(
            matches!(
                c.decline_challenge("chal0001", reason).await,
                Err(Error::InvalidArgument { .. })
            ),
            "reason {reason:?}"
        );
    }

    assert_eq!(fake.requests().len(), 0, "nothing may reach the network");
}

#[tokio::test]
async fn valid_boundary_ids_and_user_names_are_accepted() {
    let fake = fake().await;
    fake.fallback("GET", "/api/user/ab", Response::json(200, "{}"));
    fake.fallback("GET", "/api/user/A_b-9", Response::json(200, "{}"));
    let long = "u".repeat(30);
    fake.fallback(
        "GET",
        &format!("/api/user/{long}"),
        Response::json(200, "{}"),
    );
    let c = client(&fake);
    for name in ["ab", "A_b-9", long.as_str()] {
        assert_eq!(c.user_exists(name).await, Ok(true), "{name}");
    }
    for id in ["abcd1234", "abcd1234wxyz", "ABCD12345"] {
        let fake = m8_3_support::fake().await;
        fake.fallback(
            "POST",
            &format!("/api/board/game/{id}/resign"),
            Response::ok(),
        );
        client(&fake)
            .game_action(id, GameAction::Resign)
            .await
            .unwrap_or_else(|e| panic!("{id}: {e:?}"));
    }
}

#[tokio::test]
async fn challenge_answers_use_form_body_and_documented_paths() {
    let fake = fake().await;
    for a in ["accept", "decline", "cancel"] {
        fake.fallback(
            "POST",
            &format!("/api/challenge/chal0001/{a}"),
            Response::ok(),
        );
    }
    let c = client(&fake);
    c.accept_challenge("chal0001").await.expect("accept");
    c.decline_challenge("chal0001", "variant")
        .await
        .expect("decline");
    c.cancel_challenge("chal0001").await.expect("cancel");
    let reqs = fake.requests();
    assert_eq!(
        reqs.iter()
            .map(|r| (r.method.as_str(), r.path.as_str()))
            .collect::<Vec<_>>(),
        [
            ("POST", "/api/challenge/chal0001/accept"),
            ("POST", "/api/challenge/chal0001/decline"),
            ("POST", "/api/challenge/chal0001/cancel"),
        ]
    );
    assert_eq!(
        reqs[1].form(),
        [("reason".to_owned(), "variant".to_owned())]
    );
    assert!(reqs[0].body.is_empty() && reqs[2].body.is_empty());
}

#[tokio::test]
async fn user_exists_distinguishes_missing_from_failing() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/user/ghost",
        Response::json(404, r#"{"error":"Not found"}"#),
    );
    fake.enqueue("GET", "/api/user/boss", Response::json(401, "{}"));
    fake.enqueue("GET", "/api/user/dead", Response::status(500));
    let c = client(&fake);
    assert_eq!(c.user_exists("ghost").await, Ok(false));
    assert_eq!(c.user_exists("boss").await, Err(Error::Unauthorized));
    assert_eq!(c.user_exists("dead").await, Err(Error::Server(500)));
    for r in fake.requests() {
        assert_eq!(r.query, "profile=false");
    }
}

// --------------------------------------------------------------- retries

#[tokio::test]
async fn transient_failures_are_retried_with_identical_requests() {
    let fake = fake().await;
    let path = move_path("e2e4");
    fake.enqueue("POST", &path, Response::status(502));
    fake.enqueue("POST", &path, Response::stream(vec![Step::Drop]));
    fake.enqueue("POST", &path, Response::ok());
    client(&fake)
        .game_action(
            GAME,
            GameAction::Move {
                uci: "e2e4",
                offering_draw: true,
            },
        )
        .await
        .expect("third attempt");
    let reqs = fake.requests();
    assert_eq!(reqs.len(), 3);
    for r in &reqs {
        assert_eq!(
            (r.method.as_str(), r.path.as_str(), r.query.as_str()),
            ("POST", path.as_str(), "offeringDraw=true")
        );
        assert_eq!(
            r.header("authorization"),
            Some(format!("Bearer {SECRET}").as_str())
        );
    }
}

#[tokio::test]
async fn the_attempt_count_is_configurable_and_never_below_one() {
    for (attempts, expected) in [(1u32, 1usize), (2, 2), (5, 5), (0, 1)] {
        let fake = fake().await;
        fake.fallback("POST", &move_path("e2e4"), Response::status(503));
        let c = client_with(&fake, |cfg| cfg.action_attempts = attempts);
        let err = c.play_move(GAME, "e2e4", false).await.err();
        assert_eq!(err, Some(Error::Server(503)), "attempts={attempts}");
        assert_eq!(fake.requests().len(), expected, "attempts={attempts}");
    }
}

#[tokio::test]
async fn permanent_errors_are_never_retried() {
    let cases: Vec<(u16, fn(&Error) -> bool)> = vec![
        (400, |e| matches!(e, Error::Rejected { status: 400, .. })),
        (401, |e| *e == Error::Unauthorized),
        (403, |e| matches!(e, Error::ScopeMissing { .. })),
        (404, |e| *e == Error::NotFound),
        (409, |e| matches!(e, Error::Rejected { status: 409, .. })),
        (302, |e| matches!(e, Error::Redirect(302))),
    ];
    for (status, check) in cases {
        let fake = fake().await;
        fake.fallback(
            "POST",
            &move_path("e2e4"),
            Response::json(status, r#"{"error":"no"}"#),
        );
        let err = client(&fake)
            .play_move(GAME, "e2e4", false)
            .await
            .expect_err("permanent");
        assert!(check(&err), "{status}: {err:?}");
        assert_eq!(
            fake.requests().len(),
            1,
            "status {status} must not be retried"
        );
    }
}

#[tokio::test]
async fn a_400_after_any_kind_of_retry_is_ambiguous_and_keeps_the_reason() {
    let first_attempts: Vec<(&str, Response)> = vec![
        ("5xx", Response::status(503)),
        ("429", Response::status(429)),
        ("cut connection", Response::stream(vec![Step::Drop])),
        ("timeout", Response::ok().head_delay(Duration::from_secs(2))),
    ];
    for (name, first) in first_attempts {
        let fake = fake().await;
        let path = move_path("e2e4");
        fake.enqueue("POST", &path, first);
        fake.enqueue(
            "POST",
            &path,
            Response::json(400, r#"{"error":"Not your turn"}"#),
        );
        let c = client_with(&fake, |cfg| {
            cfg.request_timeout = Duration::from_millis(300);
        });
        let err = c.play_move(GAME, "e2e4", false).await.err();
        assert_eq!(
            err,
            Some(Error::AmbiguousAfterRetry {
                status: 400,
                reason: "Not your turn".to_owned()
            }),
            "after {name}"
        );
        assert_eq!(fake.requests().len(), 2, "after {name}");
    }
}

#[tokio::test]
async fn only_a_400_becomes_ambiguous_other_final_errors_stay_themselves() {
    for (status, want) in [(404u16, Error::NotFound), (401, Error::Unauthorized)] {
        let fake = fake().await;
        let path = move_path("e2e4");
        fake.enqueue("POST", &path, Response::status(500));
        fake.enqueue("POST", &path, Response::status(status));
        let err = client(&fake).play_move(GAME, "e2e4", false).await.err();
        assert_eq!(err, Some(want), "status {status}");
        assert_eq!(fake.requests().len(), 2);
    }
}

#[tokio::test]
async fn a_request_timeout_is_retried_and_then_reported() {
    let fake = fake().await;
    let path = move_path("e2e4");
    fake.enqueue(
        "POST",
        &path,
        Response::ok().head_delay(Duration::from_secs(3)),
    );
    fake.enqueue("POST", &path, Response::ok());
    let c = client_with(&fake, |cfg| {
        cfg.request_timeout = Duration::from_millis(250);
    });
    c.play_move(GAME, "e2e4", false)
        .await
        .expect("second attempt");
    assert_eq!(fake.requests().len(), 2);

    let fake = m8_3_support::fake().await;
    fake.fallback(
        "POST",
        &path,
        Response::ok().head_delay(Duration::from_secs(3)),
    );
    let c = client_with(&fake, |cfg| {
        cfg.request_timeout = Duration::from_millis(150);
    });
    assert_eq!(
        c.play_move(GAME, "e2e4", false).await.err(),
        Some(Error::Timeout)
    );
    assert_eq!(
        fake.requests().len(),
        3,
        "three attempts, then the last error"
    );
}

#[tokio::test]
async fn non_game_requests_are_never_retried() {
    let fake = fake().await;
    fake.fallback("GET", "/api/account", Response::status(503));
    fake.fallback("GET", "/api/user/alice", Response::status(500));
    fake.fallback(
        "POST",
        "/api/challenge/chal0001/accept",
        Response::status(502),
    );
    let c = client(&fake);
    assert_eq!(c.account().await.err(), Some(Error::Server(503)));
    assert_eq!(c.user_exists("alice").await.err(), Some(Error::Server(500)));
    assert_eq!(
        c.accept_challenge("chal0001").await.err(),
        Some(Error::Server(502))
    );
    assert_eq!(fake.requests().len(), 3, "one request each");
}

// ------------------------------------------------------------- 429 / gate

#[tokio::test]
async fn a_429_blocks_normal_calls_without_touching_the_network_and_honors_retry_after() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::status(429).header("Retry-After", "90"),
    );
    fake.fallback("GET", "/api/account", Response::json(200, ACCOUNT));
    let c = client(&fake);
    let Err(Error::RateLimited { retry_after }) = c.account().await else {
        panic!("expected RateLimited");
    };
    assert!(retry_after >= Duration::from_secs(90), "{retry_after:?}");
    for _ in 0..3 {
        assert!(matches!(c.account().await, Err(Error::RateLimited { .. })));
        assert!(matches!(
            c.user_exists("alice").await,
            Err(Error::RateLimited { .. })
        ));
        assert!(matches!(
            c.accept_challenge("chal0001").await,
            Err(Error::RateLimited { .. })
        ));
        assert!(matches!(
            c.seek(&SeekParams::new(10, 0)),
            Err(Error::RateLimited { .. })
        ));
        assert!(matches!(
            c.challenge("alice", &ChallengeParams::new(600, 0)),
            Err(Error::RateLimited { .. })
        ));
    }
    assert_eq!(
        fake.requests().len(),
        1,
        "only the request that got the 429"
    );
}

#[tokio::test]
async fn game_actions_ignore_the_block_and_a_429_on_a_move_is_resent() {
    let fake = fake().await;
    let path = move_path("e2e4");
    fake.enqueue("POST", &path, Response::status(429));
    fake.enqueue("POST", &path, Response::ok());
    fake.fallback("GET", "/api/account", Response::json(200, ACCOUNT));
    let c = client(&fake);
    c.play_move(GAME, "e2e4", false)
        .await
        .expect("resent after the 429");
    assert_eq!(fake.requests_for("POST", &path).len(), 2);
    // The 429 seen by the urgent move still blocks normal calls.
    assert!(matches!(c.account().await, Err(Error::RateLimited { .. })));
    assert_eq!(fake.requests_for("GET", "/api/account").len(), 0);
    // And another move goes through during the block.
    fake.enqueue("POST", &path, Response::ok());
    c.play_move(GAME, "e2e4", false)
        .await
        .expect("urgent ignores block");
}

#[tokio::test]
async fn normal_calls_resume_after_the_block_expires() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::status(429).header("Retry-After", "0"),
    );
    fake.fallback("GET", "/api/account", Response::json(200, ACCOUNT));
    let c = client_with(&fake, |cfg| {
        cfg.gate.base_block = Duration::from_millis(150);
        cfg.gate.max_block = Duration::from_millis(300);
    });
    assert!(matches!(c.account().await, Err(Error::RateLimited { .. })));
    assert!(c.gate().blocked_for().is_some());
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(c.gate().blocked_for().is_none());
    c.account().await.expect("works again");
}

#[tokio::test]
async fn normal_requests_are_serialized_and_urgent_ones_are_not_queued_behind_them() {
    let fake = fake().await;
    fake.enqueue(
        "GET",
        "/api/account",
        Response::json(200, ACCOUNT).head_delay(Duration::from_millis(700)),
    );
    fake.enqueue("GET", "/api/account", Response::json(200, ACCOUNT));
    fake.fallback("POST", &move_path("e2e4"), Response::ok());
    let c = client(&fake);

    let (c1, c2) = (c.clone(), c.clone());
    let first = tokio::spawn(async move { c1.account().await.map(|_| ()) });
    assert!(fake.wait_requests("GET", "/api/account", 1).await);
    let second = tokio::spawn(async move { c2.account().await.map(|_| ()) });
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        fake.requests_for("GET", "/api/account").len(),
        1,
        "the second normal request must wait for the first"
    );
    // The move does not wait for either.
    c.play_move(GAME, "e2e4", false).await.expect("urgent");
    assert_eq!(fake.requests_for("GET", "/api/account").len(), 1);

    first.await.expect("join").expect("first");
    second.await.expect("join").expect("second");
    assert_eq!(fake.requests_for("GET", "/api/account").len(), 2);
}

// ---------------------------------------------------------- base URL rules

fn config_for(url: &str) -> ClientConfig {
    ClientConfig {
        base_url: url.to_owned(),
        token: Token::new(SECRET).ok(),
        ..ClientConfig::default()
    }
}

#[test]
fn insecure_or_malformed_base_urls_are_refused() {
    for bad in [
        "http://lichess.org",
        "http://example.com",
        "http://192.168.1.10:8080",
        "http://10.0.0.1",
        "http://0.0.0.0:80",
        "http://localhost.evil.com",
        "http://127.0.0.1.evil.com",
        "http://evil-localhost",
        "http://[2001:db8::1]:80",
        "https://user@lichess.org",
        "https://user:pw@lichess.org",
        "http://user:pw@127.0.0.1:80",
        "https://lichess.org/api",
        "https://lichess.org/a/b",
        "https://lichess.org/?token=abc",
        "https://lichess.org/#frag",
        "https://lichess.org?x=1",
        "https://",
        "ws://127.0.0.1",
        "wss://lichess.org",
        "ftp://lichess.org",
        "file:///etc/passwd",
        "data:text/plain,hi",
        "mailto:a@b.c",
        "lichess.org",
        "//lichess.org",
        "",
        " ",
        "not a url",
    ] {
        let err = Client::new(config_for(bad)).err();
        assert_eq!(err, Some(Error::InsecureUrl), "{bad:?}");
        assert!(!format!("{err:?}").contains(SECRET));
    }
}

#[test]
fn https_hosts_and_loopback_http_are_accepted() {
    for good in [
        "https://lichess.org",
        "https://lichess.org/",
        "HTTPS://LICHESS.ORG",
        "https://example.org:8443",
        "http://127.0.0.1:1234",
        "http://127.0.0.1",
        "http://127.0.0.2:80",
        "http://localhost:80",
        "http://LOCALHOST:80",
        "http://[::1]:8080",
    ] {
        assert!(Client::new(config_for(good)).is_ok(), "{good:?}");
    }
}

#[test]
fn building_a_client_never_needs_a_runtime_but_streams_do() {
    let c = Client::new(config_for("https://lichess.org")).expect("client");
    assert!(matches!(c.open_event_stream(), Err(Error::Closed)));
    assert!(matches!(c.open_game_stream(GAME), Err(Error::Closed)));
    assert!(matches!(
        c.seek(&SeekParams::new(10, 0)),
        Err(Error::Closed)
    ));
    assert!(matches!(
        c.challenge("alice", &ChallengeParams::new(600, 0)),
        Err(Error::Closed)
    ));
}

#[test]
fn argument_errors_win_over_the_missing_runtime() {
    let c = Client::new(config_for("https://lichess.org")).expect("client");
    assert!(matches!(
        c.open_game_stream("bad"),
        Err(Error::InvalidArgument { .. })
    ));
    assert!(matches!(
        c.seek(&SeekParams::new(3, 0)),
        Err(Error::TimeControl(_))
    ));
    assert!(matches!(
        c.challenge("a/b", &ChallengeParams::new(600, 0)),
        Err(Error::InvalidArgument { .. })
    ));
}

#[tokio::test]
async fn seek_and_challenge_validate_before_any_request() {
    let fake = fake().await;
    let c = client(&fake);
    for (m, i) in [(3, 0), (5, 3), (1, 0), (181, 0), (10, 181)] {
        assert!(
            matches!(c.seek(&SeekParams::new(m, i)), Err(Error::TimeControl(_))),
            "{m}+{i}"
        );
    }
    let mut p = SeekParams::new(10, 0);
    p.rating_range = Some((2000, 1000));
    assert!(matches!(c.seek(&p), Err(Error::InvalidArgument { .. })));
    for (l, i) in [(60, 0), (75, 0), (600, 61), (10_860, 0)] {
        assert!(
            matches!(
                c.challenge("alice", &ChallengeParams::new(l, i)),
                Err(Error::TimeControl(_))
            ),
            "{l}+{i}"
        );
    }
    assert_eq!(fake.requests().len(), 0);
}

// ---------------------------------------------------------- redaction

#[test]
fn debug_output_of_client_and_config_never_shows_the_token() {
    let cfg = config_for("https://lichess.org");
    let c = Client::new(config_for("https://lichess.org")).expect("client");
    for text in [
        format!("{cfg:?}"),
        format!("{cfg:#?}"),
        format!("{c:?}"),
        format!("{c:#?}"),
    ] {
        assert!(!text.contains(SECRET), "{text}");
        assert!(!text.contains("SuperSecret"), "{text}");
    }
}

#[tokio::test]
async fn the_token_never_appears_in_any_error_of_any_call() {
    let fake = fake().await;
    // Everything fails in a different way; collect every error text.
    fake.fallback(
        "GET",
        "/api/account",
        Response::json(401, &format!(r#"{{"error":"bad {SECRET}"}}"#)),
    );
    fake.fallback("POST", &move_path("e2e4"), Response::json(400, "{}"));
    let c = client(&fake);
    let mut errors: Vec<Error> = Vec::new();
    errors.extend(c.account().await.err());
    errors.extend(c.play_move(GAME, "e2e4", false).await.err());
    errors.extend(c.play_move("bad", "e2e4", false).await.err());
    errors.extend(c.user_exists("a/b").await.err());
    for e in errors {
        let text = format!("{e} | {e:?}");
        // The server echoing the token in its own error body is out of our
        // control for `Rejected`, but Unauthorized must carry nothing.
        if matches!(e, Error::Unauthorized) {
            assert!(!text.contains(SECRET), "{text}");
        }
    }
    assert_no_secret_in_requests(&fake);
}
