//! M8-3d contract tests (spec D3000-D3015): takeback proposal, claiming a
//! draw, rematch, watching a game of others and the login through the
//! browser (OAuth + PKCE), all against the fake Lichess. The browser is played
//! by the test through `OnlineHub::set_opener`.
#![allow(
    clippy::large_futures,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_3_common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use m8_3_common::*;
use mychess_app::AppNotice;
use mychess_app::online::{
    Link, OnlineError, OnlineMessage, OnlineNotice, OnlinePhase, PlayingState, RejectReason,
    StoreChoice, WatchPhase,
};
use mychess_online::testing::{Response, Step};

fn notice(rig: &Rig) -> Option<OnlineNotice> {
    match rig.notice() {
        Some(AppNotice::Online(n)) => Some(n.clone()),
        _ => None,
    }
}

async fn wait_plies(rig: &mut Rig, plies: usize) {
    rig.until("plies", |s| {
        s.online.game.as_ref().is_some_and(|g| g.plies() == plies)
    })
    .await;
}

// ---- takeback proposal (D3011) ----

#[tokio::test]
async fn proposing_a_takeback_needs_a_move_of_the_user_and_posts_takeback_yes() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.move_ok("e7e5");
    // The user (Black) has not moved yet.
    rig.online(OnlineMessage::ProposeTakeback).await;
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotAvailable))
    );
    rig.games[0].line(&state("e2e4"));
    wait_plies(&mut rig, 1).await;
    rig.online(OnlineMessage::ProposeTakeback).await;
    assert_eq!(
        rig.action_posts("takeback/yes"),
        0,
        "still no move of his own"
    );
    rig.play("e7", "e5").await;
    rig.games[0].line(&state("e2e4 e7e5"));
    wait_plies(&mut rig, 2).await;
    assert!(rig.state.online.can_propose_takeback());
    rig.online(OnlineMessage::ProposeTakeback).await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/takeback/yes"), 1)
            .await
    );
    // Pending proposal of his own: the button waits.
    rig.games[0].line(&state_with(
        "e2e4 e7e5",
        598_000,
        600_000,
        "started",
        r#","btakeback":true"#,
    ));
    rig.pump(150).await;
    assert!(!rig.state.online.can_propose_takeback());
    rig.online(OnlineMessage::ProposeTakeback).await;
    assert_eq!(rig.action_posts("takeback/yes"), 1);
}

#[tokio::test]
async fn a_takeback_proposed_by_the_opponent_is_answered_not_proposed() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.move_ok("e7e5");
    rig.games[0].line(&state("e2e4"));
    wait_plies(&mut rig, 1).await;
    rig.play("e7", "e5").await;
    rig.games[0].line(&state_with(
        "e2e4 e7e5",
        598_000,
        600_000,
        "started",
        r#","wtakeback":true"#,
    ));
    rig.until("proposal", |s| s.online.takeback_proposed_to_me())
        .await;
    assert!(!rig.state.online.can_propose_takeback());
}

// ---- claim a draw (D3011) ----

#[tokio::test]
async fn claiming_a_draw_needs_an_absent_opponent_and_posts_claim_draw() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.online(OnlineMessage::ClaimDraw {
        at: std::time::Instant::now(),
    })
    .await;
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotAvailable))
    );
    assert_eq!(rig.action_posts("claim-draw"), 0);
    rig.games[0].line(r#"{"type":"opponentGone","gone":true,"claimWinInSeconds":0}"#);
    rig.until("opponent gone", |s| s.online.opponent_gone.is_some())
        .await;
    assert!(rig.state.online.can_claim_draw(std::time::Instant::now()));
    rig.online(OnlineMessage::ClaimDraw {
        at: std::time::Instant::now(),
    })
    .await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/claim-draw"), 1)
            .await
    );
}

// ---- rematch (D3012) ----

#[tokio::test]
async fn rematch_challenges_the_opponent_again_with_the_colours_swapped() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.online(OnlineMessage::Rematch).await;
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotAvailable)),
        "not before the game is over"
    );
    rig.fake.enqueue(
        "POST",
        "/api/challenge/alice",
        Response::stream(vec![Step::Hold]),
    );
    rig.games[0].line(&finished("e2e4", "resign", Some("white")));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    assert!(rig.state.online.can_rematch());
    rig.online(OnlineMessage::Rematch).await;
    assert!(
        rig.fake
            .wait_requests("POST", "/api/challenge/alice", 1)
            .await
    );
    assert_eq!(rig.state.online.phase, OnlinePhase::Challenging);
    assert_eq!(notice(&rig), Some(OnlineNotice::RematchSent));
    let form = rig.fake.requests_for("POST", "/api/challenge/alice")[0].form();
    for (k, v) in [
        ("clock.limit", "600"),
        ("clock.increment", "0"),
        ("rated", "false"),
        ("color", "white"),
        ("variant", "standard"),
    ] {
        assert!(
            form.contains(&(k.to_owned(), v.to_owned())),
            "{k}: {form:?}"
        );
    }
}

#[tokio::test]
async fn rematch_is_refused_against_the_ai_or_without_a_clock() {
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(
        &Full {
            clock: None,
            ..Full::default()
        },
        &state(""),
    ))
    .await;
    rig.games[0].line(&finished("e2e4", "resign", Some("white")));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    assert!(!rig.state.online.can_rematch());
    rig.online(OnlineMessage::Rematch).await;
    assert_eq!(
        rig.state.online.phase,
        OnlinePhase::Playing(PlayingState::Finished)
    );
    assert!(
        rig.fake
            .requests_for("POST", "/api/challenge/alice")
            .is_empty()
    );
}

// ---- watching (D3000-D3004) ----

const WATCH_ID: &str = "wxyz5678";
const DESCRIPTION: &str = r#"{"id":"wxyz5678","rated":false,"variant":"standard","speed":"blitz","status":"started","players":{"white":{"user":{"name":"Carol"},"rating":1800},"black":{"user":{"name":"Dave"},"rating":1700}},"fen":"rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2"}"#;
const EXPORT: &str = "[Event \"Casual blitz game\"]\n[Site \"https://lichess.org/wxyz5678\"]\n[White \"Carol\"]\n[Black \"Dave\"]\n[Result \"*\"]\n\n1. e4 { [%clk 0:05:00] } e5 { [%clk 0:05:00] } *\n";
const AFTER_NF3: &str = r#"{"fen":"rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2","lm":"g1f3","wc":300,"bc":300}"#;

fn watch_routes(rig: &Rig) -> mychess_online::testing::LiveStream {
    let (response, live) = Response::live();
    rig.fake
        .enqueue("GET", &format!("/api/stream/game/{WATCH_ID}"), response);
    rig.fake.fallback(
        "GET",
        &format!("/game/export/{WATCH_ID}"),
        Response::json(200, EXPORT),
    );
    live
}

fn editor_plies(rig: &Rig) -> usize {
    rig.mainline_count()
}

#[tokio::test]
async fn watching_a_link_shows_the_game_follows_its_moves_and_ends() {
    let mut rig = Rig::new(1).await;
    let live = watch_routes(&rig);
    rig.state.session.game(); // the editor starts with its own game
    rig.online(OnlineMessage::Watch(format!(
        "https://lichess.org/{WATCH_ID}/black"
    )))
    .await;
    assert!(!rig.lock(), "watching others does not lock the assistance");
    live.line(DESCRIPTION);
    rig.until("history from the export", |s| {
        s.online
            .watch
            .as_ref()
            .and_then(|w| w.game.as_ref())
            .is_some_and(|g| g.plies() == 2)
    })
    .await;
    assert_eq!(editor_plies(&rig), 2);
    live.line(AFTER_NF3);
    rig.until("third ply", |s| s.session.game().mainline().count() == 3)
        .await;
    assert_eq!(rig.state.session.game().tags().white, "Carol");
    // The export carried the clocks as comments; the stream is public.
    assert!(
        rig.fake
            .requests_for("GET", &format!("/api/stream/game/{WATCH_ID}"))[0]
            .header("authorization")
            .is_none()
    );
    live.line(&format!(
        r#"{{"id":"{WATCH_ID}","status":"resign","winner":"white"}}"#
    ));
    rig.until("finished", |s| {
        s.online
            .watch
            .as_ref()
            .is_some_and(|w| w.phase == WatchPhase::Finished)
    })
    .await;
    assert_eq!(
        rig.state.session.game().tags().result,
        mychess_core::GameResult::White
    );
}

#[tokio::test]
async fn watching_a_game_offers_neither_resign_nor_rematch() {
    let mut rig = Rig::new(1).await;
    let live = watch_routes(&rig);
    rig.online(OnlineMessage::Watch(WATCH_ID.to_owned())).await;
    live.line(DESCRIPTION);
    rig.until("opened", |s| {
        s.online.watch.as_ref().is_some_and(|w| w.game.is_some())
    })
    .await;
    assert_eq!(rig.state.resign_mode(), None);
    assert_eq!(rig.state.rematch_mode(), None);
}

#[tokio::test]
async fn stopping_the_watch_brings_the_previous_game_back_and_closes_the_stream() {
    let mut rig = Rig::new(1).await;
    let live = watch_routes(&rig);
    let before = rig.state.session.game().clone();
    rig.online(OnlineMessage::Watch(WATCH_ID.to_owned())).await;
    live.line(DESCRIPTION);
    rig.until("opened", |s| {
        s.online.watch.as_ref().is_some_and(|w| w.game.is_some())
    })
    .await;
    assert!(!rig.state.session.game().same_content(&before));
    rig.online(OnlineMessage::StopWatching).await;
    assert!(rig.state.online.watch.is_none());
    assert!(rig.state.session.game().same_content(&before));
    assert!(
        rig.fake
            .wait_closed("GET", &format!("/api/stream/game/{WATCH_ID}"), 1)
            .await
    );
}

#[tokio::test]
async fn a_move_that_does_not_follow_restarts_the_watched_game_as_incomplete() {
    let mut rig = Rig::new(1).await;
    let live = watch_routes(&rig);
    rig.online(OnlineMessage::Watch(WATCH_ID.to_owned())).await;
    live.line(DESCRIPTION);
    rig.until("history", |s| {
        s.online
            .watch
            .as_ref()
            .and_then(|w| w.game.as_ref())
            .is_some_and(|g| g.plies() == 2)
    })
    .await;
    // A position unrelated to the game, then garbage: no panic, no stall.
    live.line(r#"{"fen":"8/8/8/8/8/8/8/K6k w - - 0 1","lm":"a1a2","wc":1,"bc":1}"#);
    live.line("this is not json");
    live.line(r#"{"fen":"x","lm":"zz","wc":1,"bc":1}"#);
    rig.until("restarted", |s| {
        s.online
            .watch
            .as_ref()
            .and_then(|w| w.game.as_ref())
            .is_some_and(|g| !g.complete)
    })
    .await;
    assert_eq!(notice(&rig), Some(OnlineNotice::WatchIncomplete));
}

#[tokio::test]
async fn watching_works_without_the_export_starting_from_the_stream_position() {
    let mut rig = Rig::new(1).await;
    let (response, live) = Response::live();
    rig.fake
        .enqueue("GET", &format!("/api/stream/game/{WATCH_ID}"), response);
    rig.fake.fallback(
        "GET",
        &format!("/game/export/{WATCH_ID}"),
        Response::status(404),
    );
    rig.online(OnlineMessage::Watch(WATCH_ID.to_owned())).await;
    live.line(DESCRIPTION);
    live.line(AFTER_NF3);
    rig.until("one ply after the start position", |s| {
        s.online
            .watch
            .as_ref()
            .and_then(|w| w.game.as_ref())
            .is_some_and(|g| g.plies() == 1 && !g.complete)
    })
    .await;
}

#[tokio::test]
async fn bad_references_and_the_own_game_are_refused_without_network() {
    let mut rig = Rig::new(1).await;
    for bad in [
        "",
        "nope",
        "https://example.com/wxyz5678",
        "../../x",
        "https://lichess.org/tournament/wxyz5678",
        "https://lichess.org/training",
    ] {
        rig.online(OnlineMessage::Watch(bad.to_owned())).await;
        assert_eq!(
            notice(&rig),
            Some(OnlineNotice::Rejected(RejectReason::BadGameRef)),
            "{bad:?}"
        );
    }
    rig.start_game().await;
    // The game of the user, by id and by link.
    for own in [ID.to_owned(), format!("https://lichess.org/{ID}/white")] {
        rig.online(OnlineMessage::Watch(own)).await;
        assert_eq!(
            notice(&rig),
            Some(OnlineNotice::Rejected(RejectReason::OwnGame))
        );
    }
    // Any other game too: the lock is for the whole app (D3001).
    rig.online(OnlineMessage::Watch(WATCH_ID.to_owned())).await;
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::GameInProgress))
    );
    assert!(rig.state.online.watch.is_none());
    assert!(
        rig.fake
            .requests_for("GET", &format!("/api/stream/game/{WATCH_ID}"))
            .is_empty()
    );
    assert!(
        rig.fake
            .requests_for("GET", &format!("/api/stream/game/{ID}"))
            .is_empty()
    );
}

#[tokio::test]
async fn a_game_of_the_user_starting_ends_the_watch() {
    let mut rig = Rig::new(1).await;
    let live = watch_routes(&rig);
    rig.online(OnlineMessage::Watch(WATCH_ID.to_owned())).await;
    live.line(DESCRIPTION);
    rig.until("opened", |s| {
        s.online.watch.as_ref().is_some_and(|w| w.game.is_some())
    })
    .await;
    rig.seek_and_start().await;
    assert!(rig.state.online.watch.is_none());
    assert!(rig.lock());
    assert!(
        rig.fake
            .wait_closed("GET", &format!("/api/stream/game/{WATCH_ID}"), 1)
            .await
    );
}

#[tokio::test]
async fn an_unsupported_variant_is_not_watched() {
    let mut rig = Rig::new(1).await;
    let (response, live) = Response::live();
    rig.fake
        .enqueue("GET", &format!("/api/stream/game/{WATCH_ID}"), response);
    rig.online(OnlineMessage::Watch(WATCH_ID.to_owned())).await;
    live.line(r#"{"id":"wxyz5678","variant":"crazyhouse","status":"started"}"#);
    rig.until("refused", |s| s.online.watch.is_none()).await;
    assert_eq!(notice(&rig), Some(OnlineNotice::UnsupportedVariant));
}

// ---- login through the browser (D3005-D3009) ----

const OAUTH_TOKEN: &str = "lio_OAuthTokenForTests42";
const AUTH_CODE: &str = "authcode-for-tests";

/// Plays the browser: `callback` builds the query from the `state` of the page.
fn browser(
    rig: &Rig,
    seen: Arc<Mutex<Option<String>>>,
    callback: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
) {
    let runtime = tokio::runtime::Handle::current();
    rig.state.online.hub.set_opener(move |url| {
        *seen.lock().unwrap() = Some(url.to_owned());
        let parsed = reqwest::Url::parse(url).expect("page url");
        let q: std::collections::HashMap<String, String> =
            parsed.query_pairs().into_owned().collect();
        let (redirect, state) = (q["redirect_uri"].clone(), q["state"].clone());
        if let Some(query) = callback(&state) {
            runtime.spawn(async move {
                reqwest::get(format!("{redirect}?{query}")).await.ok();
            });
        }
        true
    });
}

fn token_route(rig: &Rig) {
    rig.fake.fallback(
        "POST",
        "/api/token",
        Response::json(
            200,
            &format!(r#"{{"token_type":"Bearer","access_token":"{OAUTH_TOKEN}"}}"#),
        ),
    );
}

#[tokio::test]
async fn oauth_login_connects_with_the_token_of_the_exchange() {
    let mut rig = Rig::new(0).await;
    token_route(&rig);
    let seen = Arc::new(Mutex::new(None));
    browser(&rig, seen.clone(), |state| {
        Some(format!("code={AUTH_CODE}&state={state}"))
    });
    // The token typed by hand is not involved: forget the staged one.
    rig.state.online.hub.clear_staged();
    rig.online(OnlineMessage::StartOAuth {
        store: StoreChoice::Session,
    })
    .await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Connecting);
    assert!(rig.state.online.oauth.is_some());
    rig.until("connected", |s| {
        s.online.phase == OnlinePhase::Idle && s.online.link == Link::Up
    })
    .await;
    assert!(rig.state.online.oauth.is_none());
    let account = &rig.fake.requests_for("GET", "/api/account")[0];
    assert_eq!(
        account.header("authorization"),
        Some(format!("Bearer {OAUTH_TOKEN}").as_str())
    );
    let page = seen.lock().unwrap().clone().expect("page opened");
    assert!(page.contains("code_challenge_method=S256"));
    assert!(!page.contains("code_verifier"));
    let form = rig.fake.requests_for("POST", "/api/token")[0].form();
    assert!(form.contains(&("code".to_owned(), AUTH_CODE.to_owned())));
    // No secret in any message, effect or state seen by `update`.
    let everything = format!("{:?}{:?}", rig.log, rig.state.online);
    for secret in [OAUTH_TOKEN, AUTH_CODE] {
        assert!(!everything.contains(secret), "{secret} leaked");
    }
    let verifier = &form.iter().find(|(k, _)| k == "code_verifier").unwrap().1;
    assert!(!everything.contains(verifier.as_str()));
}

#[tokio::test]
async fn oauth_login_denied_in_the_browser_returns_to_disconnected() {
    let mut rig = Rig::new(0).await;
    token_route(&rig);
    browser(&rig, Arc::new(Mutex::new(None)), |state| {
        Some(format!("error=access_denied&state={state}"))
    });
    rig.state.online.hub.clear_staged();
    rig.online(OnlineMessage::StartOAuth {
        store: StoreChoice::Session,
    })
    .await;
    rig.until("denied", |s| s.online.oauth.is_none()).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Disconnected);
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Error(OnlineError::LoginFailed))
    );
    assert!(rig.fake.requests_for("POST", "/api/token").is_empty());
}

#[tokio::test]
async fn oauth_login_cancelled_closes_the_callback_port() {
    let mut rig = Rig::new(0).await;
    let seen = Arc::new(Mutex::new(None));
    browser(&rig, seen.clone(), |_| None);
    rig.state.online.hub.clear_staged();
    rig.online(OnlineMessage::StartOAuth {
        store: StoreChoice::Session,
    })
    .await;
    rig.until("page ready", |s| {
        s.online
            .oauth
            .as_ref()
            .is_some_and(|w| w.url.is_some() && w.opened)
    })
    .await;
    let page = seen.lock().unwrap().clone().expect("page");
    let redirect = reqwest::Url::parse(&page)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "redirect_uri")
        .map(|(_, v)| v.into_owned())
        .expect("redirect");
    let port = reqwest::Url::parse(&redirect)
        .unwrap()
        .port()
        .expect("port");
    rig.online(OnlineMessage::CancelOAuth).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Disconnected);
    assert!(rig.state.online.oauth.is_none());
    // The listener is gone shortly after the task is aborted.
    let mut closed = false;
    for _ in 0..40 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err()
        {
            closed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(closed, "the callback port stayed open");
}

#[tokio::test]
async fn oauth_login_tells_the_user_when_the_browser_cannot_be_opened() {
    let mut rig = Rig::new(0).await;
    rig.state.online.hub.set_opener(|_| false);
    rig.state.online.hub.clear_staged();
    rig.online(OnlineMessage::StartOAuth {
        store: StoreChoice::Session,
    })
    .await;
    rig.until("url shown", |s| {
        s.online.oauth.as_ref().is_some_and(|w| w.url.is_some())
    })
    .await;
    assert_eq!(notice(&rig), Some(OnlineNotice::BrowserNotOpened));
    assert_eq!(
        rig.state.online.oauth.as_ref().map(|w| w.opened),
        Some(false)
    );
    rig.online(OnlineMessage::CancelOAuth).await;
}

#[tokio::test]
async fn oauth_login_is_refused_when_not_disconnected_or_with_an_insecure_server() {
    let mut rig = Rig::new(0).await;
    rig.connect().await;
    rig.online(OnlineMessage::StartOAuth {
        store: StoreChoice::Session,
    })
    .await;
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::WrongState))
    );
    let mut rig = Rig::new(0).await;
    rig.state.prefs.online.base_url = "http://lichess.org".to_owned();
    rig.online(OnlineMessage::StartOAuth {
        store: StoreChoice::Session,
    })
    .await;
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Error(OnlineError::InsecureUrl))
    );
    assert!(rig.state.online.oauth.is_none());
}
