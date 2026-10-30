//! M8-3b contract tests of the state machine of the online session (spec
//! D1311-D1317, D1301, D1305, D1307, D1312, D1315, D1316, D1338) against the
//! fake Lichess: connect, seek, challenge sent and received, decline,
//! cancel, and the refusals that never touch the network.
#![allow(
    clippy::large_futures, // the rig owns the whole `AppState` across awaits
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_3_common;

use m8_3_common::*;
use mychess_app::online::{
    ChallengeRequest, OnlineColor, OnlineError, OnlineMessage, OnlineNotice, OnlinePhase,
    PlayingState, RejectReason, SeekRequest,
};
use mychess_app::{AppNotice, Message};
use mychess_online::TokenStore;
use mychess_online::testing::Response;
use rstest::rstest;

fn online_notice(rig: &Rig) -> Option<OnlineNotice> {
    match rig.notice() {
        Some(AppNotice::Online(n)) => Some(n.clone()),
        _ => None,
    }
}

fn challenge_request() -> ChallengeRequest {
    ChallengeRequest {
        variant: mychess_app::online::OnlineVariant::Standard,
        username: "Alice".to_owned(),
        limit_secs: 600,
        increment: 5,
        rated: false,
        color: OnlineColor::Random,
    }
}

// ---- connecting (D1301, D1305, D1307) ----

#[tokio::test]
async fn connecting_reaches_idle_with_the_account_and_remembers_only_the_login() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    let account = rig.state.online.account.clone().expect("account");
    assert_eq!(account.login, "Bob");
    assert_eq!(account.id, "bob");
    assert!(account.can_challenge);
    assert!(
        account
            .ratings
            .iter()
            .any(|(name, rating, prov)| name == "rapid" && *rating == 1500 && !*prov)
    );
    assert!(
        account
            .ratings
            .iter()
            .any(|(name, rating, prov)| name == "blitz" && *rating == 1400 && *prov)
    );
    assert_eq!(online_notice(&rig), Some(OnlineNotice::Connected));
    assert_eq!(rig.state.prefs.online.login, "Bob");
    // The token travels only in the header, never in the URL.
    let requests = rig.fake.requests_for("GET", "/api/account");
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].header("authorization"),
        Some(format!("Bearer {SECRET}").as_str())
    );
    for r in rig.fake.requests() {
        assert!(!r.path.contains(SECRET) && !r.query.contains(SECRET));
    }
}

#[tokio::test]
async fn the_event_stream_is_open_before_any_seek_is_possible() {
    let mut rig = Rig::new(1).await;
    // Not connected: the seek is refused and nothing is sent.
    rig.online(OnlineMessage::Seek(Rig::seek_request())).await;
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotConnected))
    );
    assert_eq!(rig.state.online.phase, OnlinePhase::Disconnected);
    assert_eq!(rig.posts("/api/board/seek"), 0);
}

#[tokio::test]
async fn a_refused_token_leaves_the_session_disconnected_with_the_variant_of_the_error() {
    let mut rig = Rig::build(Opts {
        account_status: 401,
        ..Opts::default()
    })
    .await;
    rig.online(OnlineMessage::Connect(
        mychess_app::online::ConnectRequest {
            source: mychess_app::online::TokenSource::Staged,
            store: mychess_app::online::StoreChoice::Session,
        },
    ))
    .await;
    rig.until("failure", |s| s.notice.is_some()).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Disconnected);
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Error(OnlineError::Unauthorized))
    );
    assert!(rig.state.online.account.is_none());
}

#[tokio::test]
async fn a_bot_account_is_refused() {
    let mut rig = Rig::build(Opts {
        account: r#"{"id":"botty","username":"Botty","title":"BOT"}"#,
        ..Opts::default()
    })
    .await;
    rig.online(OnlineMessage::Connect(
        mychess_app::online::ConnectRequest {
            source: mychess_app::online::TokenSource::Staged,
            store: mychess_app::online::StoreChoice::Session,
        },
    ))
    .await;
    rig.until("failure", |s| s.notice.is_some()).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Disconnected);
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Error(OnlineError::BotAccount))
    );
}

#[tokio::test]
async fn a_token_without_board_play_is_refused() {
    let mut rig = Rig::build(Opts {
        scopes: "challenge:write",
        ..Opts::default()
    })
    .await;
    rig.online(OnlineMessage::Connect(
        mychess_app::online::ConnectRequest {
            source: mychess_app::online::TokenSource::Staged,
            store: mychess_app::online::StoreChoice::Session,
        },
    ))
    .await;
    rig.until("failure", |s| s.notice.is_some()).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Disconnected);
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Error(OnlineError::ScopeMissing))
    );
}

#[tokio::test]
async fn a_token_without_the_challenge_scopes_connects_for_seeks_only() {
    let mut rig = Rig::build(Opts {
        scopes: "board:play",
        ..Opts::default()
    })
    .await;
    rig.connect().await;
    assert!(!rig.state.online.account.as_ref().unwrap().can_challenge);
    rig.online(OnlineMessage::Challenge(challenge_request()))
        .await;
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotAvailable))
    );
    assert_eq!(rig.state.online.phase, OnlinePhase::Idle);
    assert_eq!(rig.posts("/api/challenge/Alice"), 0);
    // A seek still works.
    rig.online(OnlineMessage::Seek(Rig::seek_request())).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Seeking);
}

#[tokio::test]
async fn an_insecure_server_address_never_gets_the_token() {
    let mut rig = Rig::new(1).await;
    rig.state.prefs.online.base_url = "http://lichess.example".to_owned();
    rig.online(OnlineMessage::Connect(
        mychess_app::online::ConnectRequest {
            source: mychess_app::online::TokenSource::Staged,
            store: mychess_app::online::StoreChoice::Session,
        },
    ))
    .await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Disconnected);
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Error(OnlineError::InsecureUrl))
    );
    assert!(rig.fake.requests().is_empty());
}

#[tokio::test]
async fn connecting_twice_is_refused_without_a_second_request() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.online(OnlineMessage::Connect(
        mychess_app::online::ConnectRequest {
            source: mychess_app::online::TokenSource::Staged,
            store: mychess_app::online::StoreChoice::Session,
        },
    ))
    .await;
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::WrongState))
    );
    assert_eq!(rig.fake.requests_for("GET", "/api/account").len(), 1);
}

#[tokio::test]
async fn signing_out_deletes_the_token_of_the_store_and_closes_the_stream() {
    let mut rig = Rig::new(1).await;
    rig.online(OnlineMessage::Connect(
        mychess_app::online::ConnectRequest {
            source: mychess_app::online::TokenSource::Staged,
            store: mychess_app::online::StoreChoice::Keyring,
        },
    ))
    .await;
    rig.until("connected", |s| s.online.account.is_some()).await;
    assert!(
        rig.store.load("Bob").expect("load").is_some(),
        "the token was kept for the account"
    );
    rig.online(OnlineMessage::Disconnect).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Disconnected);
    assert_eq!(online_notice(&rig), Some(OnlineNotice::SignedOut));
    assert!(rig.state.online.account.is_none());
    rig.pump(300).await;
    assert!(rig.store.load("Bob").expect("load").is_none());
    assert!(
        rig.fake.wait_closed("GET", "/api/stream/event", 1).await,
        "the event stream was closed"
    );
}

#[tokio::test]
async fn after_signing_out_connecting_with_the_stored_login_finds_no_token() {
    let mut rig = Rig::new(1).await;
    rig.online(OnlineMessage::Connect(
        mychess_app::online::ConnectRequest {
            source: mychess_app::online::TokenSource::Staged,
            store: mychess_app::online::StoreChoice::Keyring,
        },
    ))
    .await;
    rig.until("connected", |s| s.online.account.is_some()).await;
    rig.online(OnlineMessage::Disconnect).await;
    rig.pump(200).await;
    // The token is gone with the sign-out: nothing to connect with.
    let (response, _stream) = Response::live();
    rig.fake.enqueue("GET", "/api/stream/event", response);
    rig.online(OnlineMessage::Connect(
        mychess_app::online::ConnectRequest {
            source: mychess_app::online::TokenSource::Stored {
                login: "Bob".to_owned(),
            },
            store: mychess_app::online::StoreChoice::Session,
        },
    ))
    .await;
    rig.until("failure", |s| {
        s.online.phase == OnlinePhase::Disconnected
            && matches!(
                s.notice,
                Some(AppNotice::Online(OnlineNotice::Error(OnlineError::NoToken)))
            )
    })
    .await;
}

// ---- seek (D1315, D1314, D1331) ----

#[tokio::test]
async fn a_seek_is_posted_with_the_form_of_the_request_and_ends_in_a_game() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.online(OnlineMessage::Seek(SeekRequest {
        variant: mychess_app::online::OnlineVariant::Standard,
        minutes: 15,
        increment: 10,
        rated: true,
        color: OnlineColor::White,
        rating_range: Some((1400, 1600)),
    }))
    .await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Seeking);
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::AssistanceWillBeDisabled)
    );
    assert!(rig.fake.wait_requests("POST", "/api/board/seek", 1).await);
    let request = &rig.fake.requests_for("POST", "/api/board/seek")[0];
    let form = request.form();
    let has = |k: &str, v: &str| form.iter().any(|(a, b)| a == k && b == v);
    assert!(has("time", "15"), "{form:?}");
    assert!(has("increment", "10"), "{form:?}");
    assert!(has("rated", "true"), "{form:?}");
    assert!(has("variant", "standard"), "{form:?}");
    assert!(has("color", "white"), "{form:?}");
    assert!(has("ratingRange", "1400-1600"), "{form:?}");
    assert!(!request.path.contains(SECRET));
    // Still not locked: nobody has accepted yet (D1331).
    assert!(!rig.lock());
    rig.game_start();
    rig.until("playing", |s| s.online.phase.is_live_for_tests())
        .await;
}

trait LiveForTests {
    fn is_live_for_tests(&self) -> bool;
}

impl LiveForTests for OnlinePhase {
    fn is_live_for_tests(&self) -> bool {
        matches!(self, OnlinePhase::Playing(PlayingState::Live))
    }
}

#[tokio::test]
async fn the_game_start_closes_the_seek_connection_and_opens_the_game_stream() {
    let mut rig = Rig::new(1).await;
    rig.seek_and_start().await;
    assert!(rig.lock(), "the lock begins with the game");
    assert!(
        rig.fake.wait_closed("POST", "/api/board/seek", 1).await,
        "the connection of the seek is closed"
    );
    let stream = rig
        .fake
        .requests_for("GET", &format!("/api/board/game/stream/{ID}"));
    assert_eq!(stream.len(), 1);
    assert_eq!(
        stream[0].header("authorization"),
        Some(format!("Bearer {SECRET}").as_str())
    );
}

#[tokio::test]
async fn the_game_full_creates_the_game_and_turns_the_board_to_the_user() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    let game = rig.state.online.game.as_ref().expect("game");
    assert_eq!(game.id, ID);
    assert_eq!(game.my_color, mychess_core::Color::Black);
    assert_eq!(game.white.name, "alice");
    assert_eq!(game.black.name, "Bob");
    assert_eq!(game.white.rating, Some(1800));
    assert!(
        rig.state.board_flipped,
        "Black sees the board from his side"
    );
    assert_eq!(
        rig.state.online.my_color(),
        Some(mychess_core::Color::Black)
    );
    let mut white = Rig::new(1).await;
    white.start_with(&full(&Full::white(), &state(""))).await;
    assert!(!white.state.board_flipped);
    assert_eq!(
        white.state.online.my_color(),
        Some(mychess_core::Color::White)
    );
}

#[tokio::test]
async fn cancelling_the_seek_closes_the_connection_and_leaves_everything_enabled() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.online(OnlineMessage::Seek(Rig::seek_request())).await;
    assert!(rig.fake.wait_requests("POST", "/api/board/seek", 1).await);
    rig.online(OnlineMessage::CancelSeek).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Idle);
    assert!(
        rig.fake.wait_closed("POST", "/api/board/seek", 1).await,
        "closing the connection cancels the seek"
    );
    assert!(!rig.lock());
    // A gameStart that arrives late for a seek that was cancelled is a
    // game that exists at the server: it is resumed, not lost.
    rig.game_start();
    rig.until("playing", |s| s.online.phase.is_live_for_tests())
        .await;
}

#[rstest]
#[case::bullet_1_0(1, 0, false)]
#[case::bullet_2_1(2, 1, false)]
#[case::blitz_3_0(3, 0, false)]
#[case::blitz_5_3(5, 3, false)]
#[case::rapid_10_0(10, 0, true)]
#[case::rapid_10_5(10, 5, true)]
#[case::rapid_15_10(15, 10, true)]
#[case::classical_30_20(30, 20, true)]
#[tokio::test]
async fn a_seek_needs_rapid_or_slower(
    #[case] minutes: u32,
    #[case] increment: u32,
    #[case] ok: bool,
) {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.online(OnlineMessage::Seek(SeekRequest {
        variant: mychess_app::online::OnlineVariant::Standard,
        minutes,
        increment,
        rated: false,
        color: OnlineColor::Random,
        rating_range: None,
    }))
    .await;
    if ok {
        assert_eq!(rig.state.online.phase, OnlinePhase::Seeking);
        assert!(rig.fake.wait_requests("POST", "/api/board/seek", 1).await);
    } else {
        assert_eq!(rig.state.online.phase, OnlinePhase::Idle);
        assert_eq!(
            online_notice(&rig),
            Some(OnlineNotice::Rejected(RejectReason::TimeControl))
        );
        rig.pump(100).await;
        assert_eq!(
            rig.posts("/api/board/seek"),
            0,
            "validated before the network"
        );
    }
}

#[tokio::test]
async fn a_second_seek_or_a_seek_during_a_game_is_refused_without_a_request() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.online(OnlineMessage::Seek(Rig::seek_request())).await;
    assert!(rig.fake.wait_requests("POST", "/api/board/seek", 1).await);
    rig.online(OnlineMessage::Seek(Rig::seek_request())).await;
    assert!(matches!(
        online_notice(&rig),
        Some(OnlineNotice::Rejected(
            RejectReason::WrongState | RejectReason::GameInProgress
        ))
    ));
    rig.game_start();
    rig.until("playing", |s| s.online.phase.is_live_for_tests())
        .await;
    rig.online(OnlineMessage::Seek(Rig::seek_request())).await;
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::GameInProgress))
    );
    rig.online(OnlineMessage::Challenge(challenge_request()))
        .await;
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::GameInProgress))
    );
    assert_eq!(rig.posts("/api/board/seek"), 1);
    assert_eq!(rig.posts("/api/challenge/Alice"), 0);
    // The session never panics and signing out in a game is refused.
    rig.online(OnlineMessage::Disconnect).await;
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::GameInProgress))
    );
    assert!(rig.state.online.phase.is_live_for_tests());
}

#[tokio::test]
async fn a_second_game_start_is_ignored_with_a_remark() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.event_stream.line(
        r#"{"type":"gameStart","game":{"gameId":"zzzz9999","fullId":"zzzz9999abcd","color":"white"}}"#,
    );
    rig.until("remark", |s| {
        matches!(s.notice, Some(AppNotice::Online(OnlineNotice::AnotherGame)))
    })
    .await;
    assert_eq!(rig.state.online.game.as_ref().unwrap().id, ID);
    rig.pump(100).await;
    assert_eq!(
        rig.fake
            .requests_for("GET", "/api/board/game/stream/zzzz9999")
            .len(),
        0
    );
}

#[tokio::test]
async fn a_game_start_with_a_hostile_id_is_ignored() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    for id in ["../etc/passwd", "short", "has space!", ""] {
        rig.event_stream.line(&format!(
            r#"{{"type":"gameStart","game":{{"gameId":"{id}","color":"white"}}}}"#
        ));
    }
    rig.pump(200).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Idle);
    assert!(!rig.lock());
}

// ---- challenges sent (D1316) ----

#[tokio::test]
async fn a_challenge_is_sent_with_keep_alive_and_cancelled_with_the_id() {
    let mut rig = Rig::new(1).await;
    let (response, live) = Response::live();
    rig.fake.enqueue("POST", "/api/challenge/Alice", response);
    rig.fake.fallback(
        "POST",
        "/api/challenge/chal0001/cancel",
        Response::json(200, r#"{"ok":true}"#),
    );
    rig.connect().await;
    rig.online(OnlineMessage::Challenge(challenge_request()))
        .await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Challenging);
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::AssistanceWillBeDisabled)
    );
    assert!(
        rig.fake
            .wait_requests("POST", "/api/challenge/Alice", 1)
            .await
    );
    let form = rig.fake.requests_for("POST", "/api/challenge/Alice")[0].form();
    let has = |k: &str, v: &str| form.iter().any(|(a, b)| a == k && b == v);
    assert!(has("keepAliveStream", "true"), "{form:?}");
    assert!(has("clock.limit", "600"), "{form:?}");
    assert!(has("clock.increment", "5"), "{form:?}");
    assert!(has("rated", "false"), "{form:?}");
    assert!(has("variant", "standard"), "{form:?}");
    assert!(!rig.lock());
    live.line(r#"{"id":"chal0001","status":"created"}"#);
    rig.until("id known", |s| {
        s.online.outgoing.as_deref() == Some("chal0001")
    })
    .await;
    rig.online(OnlineMessage::CancelChallenge).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Idle);
    assert!(
        rig.fake
            .wait_requests("POST", "/api/challenge/chal0001/cancel", 1)
            .await
    );
    assert!(
        rig.fake
            .wait_closed("POST", "/api/challenge/Alice", 1)
            .await,
        "the connection of the challenge is closed"
    );
}

#[tokio::test]
async fn a_declined_challenge_returns_to_idle_with_the_reason() {
    let mut rig = Rig::new(1).await;
    let (response, live) = Response::live();
    rig.fake.enqueue("POST", "/api/challenge/Alice", response);
    rig.connect().await;
    rig.online(OnlineMessage::Challenge(challenge_request()))
        .await;
    assert!(
        rig.fake
            .wait_requests("POST", "/api/challenge/Alice", 1)
            .await
    );
    live.line(r#"{"id":"chal0001","status":"created"}"#);
    rig.event_stream.line(
        r#"{"type":"challengeDeclined","challenge":{"id":"chal0001","status":"declined","direction":"out","declineReason":"Not now","declineReasonKey":"later"}}"#,
    );
    rig.until("declined", |s| s.online.phase == OnlinePhase::Idle)
        .await;
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::ChallengeDeclined {
            reason: Some("later".to_owned())
        })
    );
    assert!(!rig.lock());
    assert!(rig.state.online.outgoing.is_none());
}

#[tokio::test]
async fn a_challenge_stream_that_ends_declined_returns_to_idle() {
    let mut rig = Rig::new(1).await;
    let (response, live) = Response::live();
    rig.fake.enqueue("POST", "/api/challenge/Alice", response);
    rig.connect().await;
    rig.online(OnlineMessage::Challenge(challenge_request()))
        .await;
    assert!(
        rig.fake
            .wait_requests("POST", "/api/challenge/Alice", 1)
            .await
    );
    live.line(r#"{"done":"declined"}"#);
    rig.until("idle", |s| s.online.phase == OnlinePhase::Idle)
        .await;
    assert!(matches!(
        online_notice(&rig),
        Some(OnlineNotice::ChallengeDeclined { .. })
    ));
}

#[tokio::test]
async fn an_accepted_challenge_waits_for_the_game_start_of_the_event_stream() {
    let mut rig = Rig::new(1).await;
    let (response, live) = Response::live();
    rig.fake.enqueue("POST", "/api/challenge/Alice", response);
    rig.connect().await;
    rig.online(OnlineMessage::Challenge(challenge_request()))
        .await;
    assert!(
        rig.fake
            .wait_requests("POST", "/api/challenge/Alice", 1)
            .await
    );
    live.line(r#"{"done":"accepted"}"#);
    rig.pump(150).await;
    assert_eq!(
        rig.state.online.phase,
        OnlinePhase::Challenging,
        "the confirmation is the gameStart"
    );
    rig.game_start();
    rig.until("playing", |s| s.online.phase.is_live_for_tests())
        .await;
    assert!(rig.lock());
}

#[rstest]
#[case::bad_name("a b")]
#[case::path("../x")]
#[case::empty("")]
#[case::short("a")]
#[tokio::test]
async fn a_challenge_to_an_invalid_user_name_is_refused_before_the_network(#[case] name: &str) {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.online(OnlineMessage::Challenge(ChallengeRequest {
        variant: mychess_app::online::OnlineVariant::Standard,
        username: name.to_owned(),
        ..challenge_request()
    }))
    .await;
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::BadUser))
    );
    assert_eq!(rig.state.online.phase, OnlinePhase::Idle);
}

#[tokio::test]
async fn a_challenge_with_bullet_time_is_refused_before_the_network() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.online(OnlineMessage::Challenge(ChallengeRequest {
        variant: mychess_app::online::OnlineVariant::Standard,
        limit_secs: 60,
        increment: 0,
        ..challenge_request()
    }))
    .await;
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::TimeControl))
    );
    assert_eq!(rig.state.online.phase, OnlinePhase::Idle);
}

// ---- challenges received (D1316) ----

#[tokio::test]
async fn a_challenge_received_is_listed_once_and_accepting_it_starts_the_game() {
    let mut rig = Rig::new(1).await;
    rig.fake.fallback(
        "POST",
        "/api/challenge/chal0001/accept",
        Response::json(200, r#"{"ok":true}"#),
    );
    rig.connect().await;
    let line = challenge_event("chal0001", "Alice", Some("GM"), "standard", 600, 5);
    rig.event_stream.line(&line);
    rig.event_stream.line(&line);
    rig.until("listed", |s| !s.online.incoming.is_empty()).await;
    rig.pump(100).await;
    let incoming = &rig.state.online.incoming;
    assert_eq!(incoming.len(), 1, "the same challenge is not listed twice");
    assert_eq!(incoming[0].id, "chal0001");
    assert_eq!(incoming[0].from, "Alice");
    assert_eq!(incoming[0].title.as_deref(), Some("GM"));
    assert_eq!(incoming[0].rating, Some(1800));
    assert_eq!(incoming[0].time, "10+5");
    assert!(incoming[0].playable);
    rig.online(OnlineMessage::AcceptChallenge("chal0001".to_owned()))
        .await;
    assert!(
        rig.fake
            .wait_requests("POST", "/api/challenge/chal0001/accept", 1)
            .await
    );
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::AssistanceWillBeDisabled)
    );
    rig.game_start();
    rig.until("playing", |s| s.online.phase.is_live_for_tests())
        .await;
    assert!(rig.state.online.incoming.is_empty());
}

#[tokio::test]
async fn declining_a_challenge_posts_the_reason_and_removes_it() {
    let mut rig = Rig::new(1).await;
    rig.fake.fallback(
        "POST",
        "/api/challenge/chal0001/decline",
        Response::json(200, r#"{"ok":true}"#),
    );
    rig.connect().await;
    rig.event_stream.line(&challenge_event(
        "chal0001", "Alice", None, "standard", 600, 0,
    ));
    rig.until("listed", |s| !s.online.incoming.is_empty()).await;
    rig.online(OnlineMessage::DeclineChallenge("chal0001".to_owned()))
        .await;
    assert!(rig.state.online.incoming.is_empty());
    assert!(
        rig.fake
            .wait_requests("POST", "/api/challenge/chal0001/decline", 1)
            .await
    );
    let form = rig
        .fake
        .requests_for("POST", "/api/challenge/chal0001/decline")[0]
        .form();
    assert!(form.iter().any(|(k, _)| k == "reason"), "{form:?}");
    // Unknown ids are refused locally.
    rig.online(OnlineMessage::DeclineChallenge("nope0000".to_owned()))
        .await;
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::ChallengeNotPlayable))
    );
}

#[rstest]
#[case::atomic("atomic", 600, 0, "variant")]
#[case::bullet("standard", 60, 0, "timeControl")]
#[tokio::test]
async fn a_challenge_the_app_cannot_play_is_marked_and_only_declined(
    #[case] variant: &str,
    #[case] limit: u32,
    #[case] inc: u32,
    #[case] reason: &str,
) {
    let mut rig = Rig::new(1).await;
    rig.fake.fallback(
        "POST",
        "/api/challenge/chal0002/decline",
        Response::json(200, r#"{"ok":true}"#),
    );
    rig.connect().await;
    rig.event_stream.line(&challenge_event(
        "chal0002", "Alice", None, variant, limit, inc,
    ));
    rig.until("listed", |s| !s.online.incoming.is_empty()).await;
    assert!(!rig.state.online.incoming[0].playable);
    rig.online(OnlineMessage::AcceptChallenge("chal0002".to_owned()))
        .await;
    assert_eq!(
        online_notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::ChallengeNotPlayable))
    );
    assert_eq!(
        rig.fake
            .requests_for("POST", "/api/challenge/chal0002/accept")
            .len(),
        0
    );
    rig.online(OnlineMessage::DeclineChallenge("chal0002".to_owned()))
        .await;
    assert!(
        rig.fake
            .wait_requests("POST", "/api/challenge/chal0002/decline", 1)
            .await
    );
    let form = rig
        .fake
        .requests_for("POST", "/api/challenge/chal0002/decline")[0]
        .form();
    assert!(
        form.iter().any(|(k, v)| k == "reason" && v == reason),
        "{form:?}"
    );
}

#[tokio::test]
async fn a_challenge_from_a_bot_is_declined_at_once_and_never_listed() {
    let mut rig = Rig::new(1).await;
    rig.fake.fallback(
        "POST",
        "/api/challenge/chal0003/decline",
        Response::json(200, r#"{"ok":true}"#),
    );
    rig.connect().await;
    rig.event_stream.line(&challenge_event(
        "chal0003",
        "SomeBot",
        Some("BOT"),
        "standard",
        600,
        0,
    ));
    assert!(
        rig.fake
            .wait_requests("POST", "/api/challenge/chal0003/decline", 1)
            .await
            || {
                rig.pump(300).await;
                !rig.fake
                    .requests_for("POST", "/api/challenge/chal0003/decline")
                    .is_empty()
            }
    );
    assert!(rig.state.online.incoming.is_empty());
    let form = rig
        .fake
        .requests_for("POST", "/api/challenge/chal0003/decline")[0]
        .form();
    assert!(
        form.iter().any(|(k, v)| k == "reason" && v == "noBot"),
        "{form:?}"
    );
}

#[tokio::test]
async fn a_challenge_cancelled_by_the_challenger_leaves_the_list() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.event_stream.line(&challenge_event(
        "chal0001", "Alice", None, "standard", 600, 0,
    ));
    rig.until("listed", |s| !s.online.incoming.is_empty()).await;
    rig.event_stream.line(
        r#"{"type":"challengeCanceled","challenge":{"id":"chal0001","status":"canceled","direction":"in"}}"#,
    );
    rig.until("gone", |s| s.online.incoming.is_empty()).await;
}

#[tokio::test]
async fn a_challenge_we_sent_that_the_stream_echoes_is_not_listed_as_received() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    let line = challenge_event("chal0004", "Bob", None, "standard", 600, 0)
        .replace(r#""direction":"in""#, r#""direction":"out""#);
    rig.event_stream.line(&line);
    rig.pump(150).await;
    assert!(rig.state.online.incoming.is_empty());
}

#[tokio::test]
async fn unknown_events_and_garbage_lines_do_not_disturb_the_session() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.event_stream.line(r#"{"type":"someFutureEvent","x":1}"#);
    rig.event_stream.line("not json at all");
    rig.event_stream.line("{");
    rig.pump(200).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Idle);
    // And a real event after the garbage still works.
    rig.game_start();
    rig.until("playing", |s| s.online.phase.is_live_for_tests())
        .await;
}

#[tokio::test]
async fn the_session_messages_are_refused_without_a_game_and_never_panic() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    for message in [
        OnlineMessage::Resign,
        OnlineMessage::Abort,
        OnlineMessage::OfferDraw,
        OnlineMessage::AnswerDraw(true),
        OnlineMessage::AnswerTakeback(false),
        OnlineMessage::ClaimVictory {
            at: std::time::Instant::now(),
        },
        OnlineMessage::DismissResult,
        player_move("e2", "e4", None, false),
    ] {
        let effects = rig.update_only(Message::Online(message.clone()));
        assert!(
            effects
                .iter()
                .all(|e| !matches!(e, mychess_app::Effect::Online { .. })),
            "{message:?} must not reach the network"
        );
        assert!(
            matches!(online_notice(&rig), Some(OnlineNotice::Rejected(_))),
            "{message:?}"
        );
    }
}

#[tokio::test]
async fn m8_3_d4520_entering_a_game_is_signalled_once() {
    let mut rig = Rig::new(1).await;
    assert_eq!(rig.state.online_entries, 0);
    rig.start_game().await;
    assert_eq!(rig.state.online_entries, 1, "the entry is signalled");
}

#[tokio::test]
async fn m9_6_an_incoming_chess960_challenge_is_listed_as_playable_and_accepted() {
    let mut rig = Rig::new(1).await;
    rig.fake.fallback(
        "POST",
        "/api/challenge/chal0960/accept",
        Response::json(200, r#"{"ok":true}"#),
    );
    rig.connect().await;
    rig.event_stream.line(&challenge_event(
        "chal0960", "Alice", None, "chess960", 600, 0,
    ));
    rig.until("listed", |s| !s.online.incoming.is_empty()).await;
    assert!(rig.state.online.incoming[0].playable);
    assert!(rig.state.online.incoming[0].chess960);
    rig.online(OnlineMessage::AcceptChallenge("chal0960".to_owned()))
        .await;
    assert!(
        rig.fake
            .wait_requests("POST", "/api/challenge/chal0960/accept", 1)
            .await
    );
}
