//! M8-3b contract tests of the reconnection of the stream of the game (spec
//! D1318, D1333, D1349, R7): the game survives a drop, the new `gameFull`
//! reconciles everything, and no move is ever sent twice without looking at
//! the state first.
#![allow(
    clippy::large_futures, // the rig owns the whole `AppState` across awaits
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_3_common;

use std::time::Instant;

use m8_3_common::*;
use mychess_app::AppNotice;
use mychess_app::online::{OnlineNotice, OnlinePhase, PlayingState};
use mychess_core::Color;
use mychess_online::testing::Response;

fn notice(rig: &Rig) -> Option<OnlineNotice> {
    match rig.notice() {
        Some(AppNotice::Online(n)) => Some(n.clone()),
        _ => None,
    }
}

fn live(state: &mychess_app::AppState) -> bool {
    state.online.phase == OnlinePhase::Playing(PlayingState::Live)
}

/// Black, `e2e4` played, the user sent `e7e5` and the server accepted the
/// request; then the stream drops before the state mentions the move.
async fn pending_move_then_drop() -> Rig {
    let mut rig = Rig::new(2).await;
    rig.move_ok("e7e5");
    rig.start_game().await;
    rig.games[0].line(&state("e2e4"));
    rig.until("e4", |s| s.session.game().mainline().count() == 1)
        .await;
    rig.play("e7", "e5").await;
    rig.wait_post(&format!("/api/board/game/{ID}/move/e7e5"), 1)
        .await;
    rig.games[0].drop_connection();
    rig.wait_game_streams(2).await;
    rig
}

#[tokio::test]
async fn a_drop_keeps_the_game_and_the_lock_and_the_full_brings_it_back() {
    let mut rig = Rig::new(2).await;
    rig.start_game().await;
    rig.games[0].line(&state("e2e4"));
    rig.until("e4", |s| s.session.game().mainline().count() == 1)
        .await;
    rig.games[0].drop_connection();
    rig.until("reconnecting", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Reconnecting)
    })
    .await;
    assert!(rig.lock(), "the lock holds while reconnecting");
    assert_eq!(rig.mainline_count(), 1, "the game is not lost");
    assert!(rig.state.online.game.is_some());
    rig.wait_game_streams(2).await;
    rig.games[1].line(&full(&Full::default(), &state("e2e4 e7e5 g1f3")));
    rig.until("reconciled", |s| {
        live(s) && s.session.game().mainline().count() == 3
    })
    .await;
    assert!(rig.lock());
    // The server sent three plies; the board has exactly three.
    assert_eq!(rig.state.online.game.as_ref().unwrap().plies(), 3);
}

#[tokio::test]
async fn the_clock_keeps_running_while_reconnecting() {
    let mut rig = Rig::new(2).await;
    rig.start_game().await;
    rig.games[0].line(&state_with("e2e4 e7e5", 300_000, 290_000, "started", ""));
    rig.until("two plies", |s| {
        s.online.game.as_ref().is_some_and(|g| g.plies() == 2)
    })
    .await;
    rig.games[0].drop_connection();
    rig.until("reconnecting", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Reconnecting)
    })
    .await;
    let later = Instant::now() + std::time::Duration::from_secs(5);
    let shown = rig.state.online.shown(Color::White, later);
    assert!(
        shown <= std::time::Duration::from_secs(296),
        "White's clock goes on: {shown:?}"
    );
}

#[tokio::test]
async fn a_move_the_new_full_already_contains_is_not_sent_again() {
    let mut rig = pending_move_then_drop().await;
    rig.games[1].line(&full(&Full::default(), &state("e2e4 e7e5")));
    rig.until("live", |s| {
        live(s) && s.session.game().mainline().count() == 2
    })
    .await;
    rig.pump(400).await;
    assert_eq!(rig.move_posts("e7e5"), 1, "no duplicate move");
    assert_eq!(rig.mainline_count(), 2);
}

#[tokio::test]
async fn a_move_the_new_full_does_not_show_is_sent_again_once_if_it_is_still_his_turn() {
    let mut rig = pending_move_then_drop().await;
    rig.games[1].line(&full(&Full::default(), &state("e2e4")));
    rig.wait_post(&format!("/api/board/game/{ID}/move/e7e5"), 2)
        .await;
    rig.pump(400).await;
    assert_eq!(rig.move_posts("e7e5"), 2, "sent again exactly once");
    // The same full twice (the server repeats itself) does not resend it
    // forever: the state decides.
    rig.games[1].line(&state("e2e4 e7e5"));
    rig.until("confirmed", |s| {
        s.online.game.as_ref().is_some_and(|g| g.plies() == 2)
    })
    .await;
    rig.pump(200).await;
    assert_eq!(rig.move_posts("e7e5"), 2);
}

#[tokio::test]
async fn a_move_is_not_sent_again_when_the_turn_is_no_longer_his() {
    let mut rig = pending_move_then_drop().await;
    // The opponent took back her move meanwhile: it is White's turn.
    rig.games[1].line(&full(&Full::default(), &state("")));
    rig.until("truncated", |s| {
        live(s) && s.online.game.as_ref().is_some_and(|g| g.plies() == 0)
    })
    .await;
    rig.pump(400).await;
    assert_eq!(rig.move_posts("e7e5"), 1);
    assert_eq!(rig.mainline_count(), 0);
}

#[tokio::test]
async fn a_game_stream_that_cannot_come_back_keeps_the_lock_and_the_game() {
    let mut rig = Rig::new(1).await;
    // The second connection of the game is refused for good.
    rig.fake.enqueue(
        "GET",
        &format!("/api/board/game/stream/{ID}"),
        Response::status(404),
    );
    rig.start_game().await;
    rig.games[0].line(&state("e2e4"));
    rig.until("e4", |s| s.session.game().mainline().count() == 1)
        .await;
    rig.games[0].drop_connection();
    rig.until("failure noticed", |s| {
        matches!(s.notice, Some(AppNotice::Online(OnlineNotice::Error(_))))
    })
    .await;
    assert!(rig.lock(), "the server is the truth: the lock is kept");
    assert_eq!(
        rig.state.online.phase,
        OnlinePhase::Playing(PlayingState::Reconnecting)
    );
    assert_eq!(rig.mainline_count(), 1);
    assert!(notice(&rig).is_some());
}

#[tokio::test]
async fn a_game_found_by_the_event_stream_at_start_is_resumed_with_its_moves() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    // No seek: a game of the account is already on.
    rig.game_start();
    rig.until("resumed", live).await;
    assert_eq!(notice(&rig), Some(OnlineNotice::Resumed));
    assert!(rig.lock());
    rig.games[0].line(&full(&Full::default(), &state("e2e4 e7e5 g1f3")));
    rig.until("described", |s| s.session.game().mainline().count() == 3)
        .await;
    assert!(rig.state.board_flipped);
    // It is Black's turn: the user can answer at once.
    rig.move_ok("b8c6");
    rig.play("b8", "c6").await;
    rig.wait_post(&format!("/api/board/game/{ID}/move/b8c6"), 1)
        .await;
}
