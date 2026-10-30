//! M8-3b contract tests of the actions of the game and of its end (spec
//! D1322-D1327, D1356): draw, resign and abort, takeback, absent opponent,
//! claiming the victory, chat ignored, and how each `status` ends the game.
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
use mychess_app::online::{OnlineMessage, OnlineNotice, OnlinePhase, PlayingState, RejectReason};
use mychess_app::{AppNotice, Effect, Message};
use mychess_core::GameResult;
use mychess_online::testing::Response;
use rstest::rstest;

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

async fn wait_finished(rig: &mut Rig) {
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
}

fn goes_online(effects: &[Effect]) -> bool {
    effects.iter().any(|e| matches!(e, Effect::Online { .. }))
}

// ---- draw (D1322) ----

#[tokio::test]
async fn offering_a_draw_posts_draw_yes_and_the_button_waits_for_the_answer() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state("e2e4"));
    wait_plies(&mut rig, 1).await;
    rig.online(OnlineMessage::OfferDraw).await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/draw/yes"), 1)
            .await
    );
    // Black (the user) has an offer pending: the server shows `bdraw`.
    rig.games[0].line(&state_with(
        "e2e4",
        598_000,
        600_000,
        "started",
        r#","bdraw":true"#,
    ));
    rig.pump(150).await;
    let effects = rig.update_only(Message::Online(OnlineMessage::OfferDraw));
    assert!(
        !goes_online(&effects),
        "no second offer while one is pending"
    );
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotAvailable))
    );
    assert!(
        !rig.state.online.draw_offered_to_me(),
        "the own offer is not an offer to me"
    );
}

#[rstest]
#[case::accept(true, "draw/yes", "draw/no")]
#[case::decline(false, "draw/no", "draw/yes")]
#[tokio::test]
async fn the_offer_of_the_opponent_is_accepted_or_declined(
    #[case] accept: bool,
    #[case] posted: &str,
    #[case] not_posted: &str,
) {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state_with(
        "e2e4",
        598_000,
        600_000,
        "started",
        r#","wdraw":true"#,
    ));
    rig.until("offer", |s| s.online.draw_offered_to_me()).await;
    rig.online(OnlineMessage::AnswerDraw(accept)).await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/{posted}"), 1)
            .await
    );
    assert_eq!(rig.action_posts(not_posted), 0);
}

#[tokio::test]
async fn answering_a_draw_nobody_offered_is_refused() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    for yes in [true, false] {
        let effects = rig.update_only(Message::Online(OnlineMessage::AnswerDraw(yes)));
        assert!(!goes_online(&effects));
        assert_eq!(
            notice(&rig),
            Some(OnlineNotice::Rejected(RejectReason::NotAvailable))
        );
    }
    assert_eq!(
        rig.action_posts("draw/yes") + rig.action_posts("draw/no"),
        0
    );
}

#[tokio::test]
async fn an_offer_that_goes_away_hides_the_buttons() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state_with(
        "e2e4",
        598_000,
        600_000,
        "started",
        r#","wdraw":true"#,
    ));
    rig.until("offer", |s| s.online.draw_offered_to_me()).await;
    rig.games[0].line(&state("e2e4"));
    rig.until("withdrawn", |s| !s.online.draw_offered_to_me())
        .await;
}

// ---- resign and abort (D1323) ----

#[rstest]
#[case::no_moves("")]
#[case::one_move("e2e4")]
#[tokio::test]
async fn before_two_plies_the_game_can_only_be_aborted(#[case] moves: &str) {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state(moves));
    wait_plies(&mut rig, moves.split_whitespace().count()).await;
    assert!(rig.state.online.can_abort());
    let effects = rig.update_only(Message::Online(OnlineMessage::Resign));
    assert!(
        !goes_online(&effects),
        "resigning before two plies is not offered"
    );
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotAvailable))
    );
    rig.online(OnlineMessage::Abort).await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/abort"), 1)
            .await
    );
    assert_eq!(rig.action_posts("resign"), 0);
}

#[tokio::test]
async fn from_the_second_ply_on_the_game_can_only_be_resigned() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state("e2e4 e7e5"));
    wait_plies(&mut rig, 2).await;
    assert!(
        !rig.state.online.can_abort(),
        "the button changes by itself"
    );
    let effects = rig.update_only(Message::Online(OnlineMessage::Abort));
    assert!(!goes_online(&effects));
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotAvailable))
    );
    rig.online(OnlineMessage::Resign).await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/resign"), 1)
            .await
    );
    assert_eq!(rig.action_posts("abort"), 0);
    // The game ends when the server says so, not when the request is sent.
    assert_eq!(
        rig.state.online.phase,
        OnlinePhase::Playing(PlayingState::Live)
    );
    assert!(rig.lock());
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("white")));
    wait_finished(&mut rig).await;
    assert_eq!(rig.state.online.finished.unwrap().result, GameResult::White);
}

#[tokio::test]
async fn a_refused_resignation_keeps_the_game_and_reports_the_error() {
    let mut rig = Rig::new(1).await;
    rig.fake.fallback(
        "POST",
        &format!("/api/board/game/{ID}/resign"),
        Response::json(400, r#"{"error":"Not playing"}"#),
    );
    rig.start_game().await;
    rig.games[0].line(&state("e2e4 e7e5"));
    wait_plies(&mut rig, 2).await;
    rig.online(OnlineMessage::Resign).await;
    rig.until("error", |s| {
        matches!(s.notice, Some(AppNotice::Online(OnlineNotice::Error(_))))
    })
    .await;
    assert_eq!(
        rig.state.online.phase,
        OnlinePhase::Playing(PlayingState::Live)
    );
    assert!(rig.lock());
    assert_eq!(rig.action_posts("resign"), 1, "a 400 is not retried");
}

// ---- takeback (D1324) ----

#[rstest]
#[case::accept(true, "takeback/yes", "takeback/no")]
#[case::decline(false, "takeback/no", "takeback/yes")]
#[tokio::test]
async fn a_takeback_proposed_by_the_opponent_is_answered(
    #[case] accept: bool,
    #[case] posted: &str,
    #[case] not_posted: &str,
) {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state_with(
        "e2e4 e7e5",
        598_000,
        600_000,
        "started",
        r#","wtakeback":true"#,
    ));
    rig.until("proposal", |s| s.online.takeback_proposed_to_me())
        .await;
    rig.online(OnlineMessage::AnswerTakeback(accept)).await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/{posted}"), 1)
            .await
    );
    assert_eq!(rig.action_posts(not_posted), 0);
}

#[tokio::test]
async fn answering_a_takeback_nobody_proposed_is_refused() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    let effects = rig.update_only(Message::Online(OnlineMessage::AnswerTakeback(true)));
    assert!(!goes_online(&effects));
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotAvailable))
    );
}

// ---- absent opponent (D1325) ----

#[tokio::test]
async fn the_victory_can_be_claimed_only_after_the_countdown() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state("e2e4 e7e5"));
    wait_plies(&mut rig, 2).await;
    assert!(!rig.state.online.can_claim_victory(Instant::now()));
    rig.games[0].line(r#"{"type":"opponentGone","gone":true,"claimWinInSeconds":30}"#);
    rig.until("gone", |s| s.online.opponent_gone.is_some())
        .await;
    let now = Instant::now();
    assert!(
        !rig.state.online.can_claim_victory(now),
        "countdown running"
    );
    assert!(
        rig.state
            .online
            .can_claim_victory(now + Duration::from_secs(31))
    );
    // The opponent comes back: the countdown is hidden.
    rig.games[0].line(r#"{"type":"opponentGone","gone":false}"#);
    rig.until("back", |s| s.online.opponent_gone.is_none())
        .await;
    assert!(
        !rig.state
            .online
            .can_claim_victory(now + Duration::from_secs(3600))
    );
}

#[tokio::test]
async fn an_opponent_gone_with_no_wait_can_be_claimed_at_once() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state("e2e4 e7e5"));
    wait_plies(&mut rig, 2).await;
    rig.games[0].line(r#"{"type":"opponentGone","gone":true,"claimWinInSeconds":0}"#);
    rig.until("gone", |s| s.online.opponent_gone.is_some())
        .await;
    assert!(rig.state.online.can_claim_victory(Instant::now()));
    rig.online(OnlineMessage::ClaimVictory { at: Instant::now() })
        .await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/claim-victory"), 1)
            .await
    );
}

#[tokio::test]
async fn claiming_when_the_opponent_is_present_is_refused() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state("e2e4 e7e5"));
    wait_plies(&mut rig, 2).await;
    let effects = rig.update_only(Message::Online(OnlineMessage::ClaimVictory {
        at: Instant::now(),
    }));
    assert!(!goes_online(&effects));
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotAvailable))
    );
    assert_eq!(rig.action_posts("claim-victory"), 0);
}

#[tokio::test]
async fn the_countdown_is_cleared_when_the_game_ends() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state("e2e4 e7e5"));
    wait_plies(&mut rig, 2).await;
    rig.games[0].line(r#"{"type":"opponentGone","gone":true,"claimWinInSeconds":10}"#);
    rig.until("gone", |s| s.online.opponent_gone.is_some())
        .await;
    rig.games[0].line(&finished("e2e4 e7e5", "timeout", Some("black")));
    wait_finished(&mut rig).await;
    assert!(rig.state.online.opponent_gone.is_none());
}

// ---- chat (D1326) ----

#[tokio::test]
async fn chat_lines_change_nothing() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    let before = rig.state.notice.clone();
    rig.games[0]
        .line(r#"{"type":"chatLine","username":"alice","text":"play Qh5 now","room":"player"}"#);
    rig.pump(150).await;
    assert_eq!(rig.state.notice, before);
    assert_eq!(rig.mainline_count(), 0);
    let all = format!("{:?}", rig.log);
    assert!(!all.contains("Qh5"), "the text of the chat is never kept");
}

// ---- the end of the game (D1327, D1356, D1335) ----

#[rstest]
#[case::mate_white("mate", Some("white"), GameResult::White, "Normal", true)]
#[case::mate_black("mate", Some("black"), GameResult::Black, "Normal", true)]
#[case::resign_black("resign", Some("black"), GameResult::Black, "Normal", true)]
#[case::timeout_white("timeout", Some("white"), GameResult::White, "Time forfeit", true)]
#[case::outoftime_black("outoftime", Some("black"), GameResult::Black, "Time forfeit", true)]
#[case::variant_end("variantEnd", Some("white"), GameResult::White, "Normal", true)]
#[case::cheat("cheat", Some("white"), GameResult::White, "Rules infraction", true)]
#[case::stalemate("stalemate", None, GameResult::Draw, "Normal", true)]
#[case::draw("draw", None, GameResult::Draw, "Normal", true)]
#[case::insufficient_material("insufficientMaterialClaim", None, GameResult::Draw, "Normal", true)]
#[case::unknown_finish("unknownFinish", None, GameResult::Unknown, "Unterminated", true)]
#[tokio::test]
async fn each_status_ends_the_game_with_its_result_and_the_game_is_stored(
    #[case] status: &str,
    #[case] winner: Option<&str>,
    #[case] result: GameResult,
    #[case] termination: &str,
    #[case] stored: bool,
) {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&finished("e2e4 e7e5", status, winner));
    wait_finished(&mut rig).await;
    assert!(!rig.lock(), "the lock is released at the end ({status})");
    assert_eq!(
        rig.state.online.finished.unwrap().result,
        result,
        "{status}"
    );
    let game = rig.state.session.game();
    assert_eq!(game.tags().result, result, "{status}");
    assert_eq!(
        game.tags().get_extra("Termination"),
        Some(termination),
        "{status}"
    );
    assert_eq!(rig.clipbase_games(), u32::from(stored), "{status}");
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Imported),
        "{status}: the game is stored"
    );
    assert!(!rig.state.online.clock.is_running(), "{status}");
}

#[rstest]
#[case::aborted("aborted")]
#[case::no_start("noStart")]
#[tokio::test]
async fn a_game_that_was_aborted_is_not_stored(#[case] status: &str) {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&finished("", status, None));
    wait_finished(&mut rig).await;
    assert!(!rig.lock());
    assert_eq!(rig.clipbase_games(), 0, "{status}");
    assert_eq!(notice(&rig), Some(OnlineNotice::NotImported), "{status}");
}

#[tokio::test]
async fn the_rules_of_the_core_do_not_end_an_online_game() {
    // Fool's mate on the board is a status `mate` at the server; without it
    // the game is still on (the server decides, D1327).
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(&Full::white(), &state(""))).await;
    rig.games[0].line(&state("f2f3 e7e5 g2g4 d8h4"));
    wait_plies(&mut rig, 4).await;
    rig.pump(150).await;
    assert_eq!(
        rig.state.online.phase,
        OnlinePhase::Playing(PlayingState::Live)
    );
    assert!(rig.lock());
    assert_eq!(rig.clipbase_games(), 0);
}

#[tokio::test]
async fn dismissing_the_result_returns_to_idle_and_a_new_seek_is_possible() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("black")));
    wait_finished(&mut rig).await;
    // While the result is shown, a new search can already start.
    rig.online(OnlineMessage::DismissResult).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Idle);
    assert!(rig.state.online.game.is_none());
    assert!(!rig.lock());
    rig.online(OnlineMessage::Seek(Rig::seek_request())).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Seeking);
}

#[tokio::test]
async fn the_event_stream_ending_the_game_with_game_finish_does_not_lock_again() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("black")));
    wait_finished(&mut rig).await;
    rig.event_stream.line(&format!(
        r#"{{"type":"gameFinish","game":{{"gameId":"{ID}","color":"black"}}}}"#
    ));
    rig.pump(150).await;
    assert!(!rig.lock());
    assert_eq!(rig.clipbase_games(), 1);
}
