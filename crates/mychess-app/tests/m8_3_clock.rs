//! M8-3b contract tests of the authoritative clock (spec D1319, D1327,
//! D1356): the server times replace the local ones, only the side to move is
//! interpolated, the increment is never added here and the end of the game
//! comes from the `status`, never from a flag that fell on this side.
#![allow(
    clippy::large_futures, // the rig owns the whole `AppState` across awaits
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_3_common;

use std::time::{Duration, Instant};

use m8_3_common::*;
use mychess_app::online::{OnlinePhase, PlayingState};
use mychess_core::{Color, GameResult};
use rstest::rstest;

const MOVES2: &str = "e2e4 e7e5";

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn near(shown: Duration, want: Duration, slack: Duration) -> bool {
    shown <= want && shown + slack >= want
}

async fn started_with_clock(wtime: u64, btime: u64, moves: &str) -> Rig {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state_with(moves, wtime, btime, "started", ""));
    let plies = moves.split_whitespace().count();
    rig.until("state applied", |s| {
        s.online
            .game
            .as_ref()
            .is_some_and(|g| g.plies() == plies && g.wtime == wtime && g.btime == btime)
    })
    .await;
    rig
}

#[tokio::test]
async fn the_times_of_the_server_replace_the_local_ones_exactly_for_the_side_off_turn() {
    let rig = started_with_clock(300_000, 290_000, MOVES2).await;
    let online = &rig.state.online;
    let now = Instant::now();
    // White is on the move; Black's clock is not running whatever the hour.
    assert_eq!(online.shown(Color::Black, now), secs(290));
    assert_eq!(online.shown(Color::Black, now + secs(3600)), secs(290));
    assert!(
        near(online.shown(Color::White, now), secs(300), secs(3)),
        "{:?}",
        online.shown(Color::White, now)
    );
}

#[tokio::test]
async fn only_the_side_to_move_is_interpolated_between_messages() {
    let rig = started_with_clock(300_000, 290_000, MOVES2).await;
    let now = Instant::now();
    let later = rig.state.online.shown(Color::White, now + secs(10));
    assert!(
        near(later, secs(290), secs(3)),
        "ten seconds later White has about 290 s, not {later:?}"
    );
}

#[tokio::test]
async fn the_displayed_time_never_goes_below_zero() {
    let rig = started_with_clock(5_000, 290_000, MOVES2).await;
    let now = Instant::now();
    assert_eq!(
        rig.state.online.shown(Color::White, now + secs(7200)),
        Duration::ZERO
    );
}

#[tokio::test]
async fn each_state_resets_the_clock_and_the_increment_is_never_added_locally() {
    let mut rig = started_with_clock(300_000, 290_000, MOVES2).await;
    // White moved; the server already added the 5 s increment to its time.
    rig.games[0].line(&state_with(
        "e2e4 e7e5 g1f3",
        303_000,
        290_000,
        "started",
        "",
    ));
    rig.until("third ply", |s| {
        s.online.game.as_ref().is_some_and(|g| g.plies() == 3)
    })
    .await;
    let now = Instant::now();
    // White is now off turn: exactly what the server said.
    assert_eq!(rig.state.online.shown(Color::White, now), secs(303));
    assert_eq!(
        rig.state.online.shown(Color::White, now + secs(60)),
        secs(303)
    );
    // Black runs from the new message.
    assert!(near(
        rig.state.online.shown(Color::Black, now),
        secs(290),
        secs(3)
    ));
}

#[rstest]
#[case::no_move("")]
#[case::one_move("e2e4")]
#[tokio::test]
async fn the_clock_does_not_run_before_the_second_ply(#[case] moves: &str) {
    let rig = started_with_clock(600_000, 600_000, moves).await;
    let turn = if moves.is_empty() {
        Color::White
    } else {
        Color::Black
    };
    let now = Instant::now();
    assert_eq!(
        rig.state.online.shown(turn, now + secs(30)),
        secs(600),
        "Lichess starts the clocks after both sides moved"
    );
}

#[tokio::test]
async fn a_flag_that_falls_locally_does_not_end_the_game() {
    let mut rig = started_with_clock(150, 290_000, MOVES2).await;
    // The 150 ms of White run out here while the server says nothing.
    tokio::time::sleep(Duration::from_millis(300)).await;
    rig.pump(100).await;
    let now = Instant::now();
    assert_eq!(rig.state.online.shown(Color::White, now), Duration::ZERO);
    assert_eq!(
        rig.state.online.phase,
        OnlinePhase::Playing(PlayingState::Live)
    );
    assert!(!rig.state.online.game.as_ref().unwrap().is_over());
    assert!(rig.lock(), "the lock holds until the status says it ended");
    assert_eq!(rig.action_posts("resign"), 0);
    assert_eq!(rig.clipbase_games(), 0);
    // The server decides.
    rig.games[0].line(&finished(MOVES2, "outoftime", Some("black")));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    let outcome = rig.state.online.finished.expect("outcome");
    assert_eq!(outcome.result, GameResult::Black);
    assert!(!rig.lock());
}

#[tokio::test]
async fn the_clock_stops_when_the_game_ends() {
    let mut rig = started_with_clock(300_000, 290_000, MOVES2).await;
    rig.games[0].line(&state_with(
        MOVES2,
        280_000,
        290_000,
        "resign",
        r#","winner":"black""#,
    ));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    let now = Instant::now();
    assert!(!rig.state.online.clock.is_running());
    assert_eq!(rig.state.online.shown(Color::White, now), secs(280));
    assert_eq!(
        rig.state.online.shown(Color::White, now + secs(100)),
        secs(280)
    );
}

#[tokio::test]
async fn the_clock_after_a_new_game_full_is_the_one_of_the_full() {
    // A reconnection brings a `gameFull` with new times: they win.
    let mut rig = Rig::new(2).await;
    rig.start_game().await;
    rig.games[0].line(&state_with(MOVES2, 300_000, 290_000, "started", ""));
    rig.until("two plies", |s| {
        s.online.game.as_ref().is_some_and(|g| g.plies() == 2)
    })
    .await;
    rig.games[0].drop_connection();
    assert!(
        rig.fake
            .wait_requests("GET", &format!("/api/board/game/stream/{ID}"), 2)
            .await
    );
    rig.games[1].line(&full(
        &Full::default(),
        &state_with(MOVES2, 100_000, 90_000, "started", ""),
    ));
    rig.until("clock of the full", |s| {
        s.online.shown(Color::Black, Instant::now()) == secs(90)
    })
    .await;
    assert_eq!(
        rig.state
            .online
            .shown(Color::Black, Instant::now() + secs(500)),
        secs(90)
    );
}
