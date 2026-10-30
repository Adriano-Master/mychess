//! M8-3b contract tests of the moves (spec D1318, D1320, D1322, D1324,
//! D1347, D1360, D1361): idempotent re-application of `moves`, castling in
//! both spellings, a move the server finds illegal, the optimistic move of
//! the user with its undo, and what is sent to the server.
#![allow(
    clippy::large_futures, // the rig owns the whole `AppState` across awaits
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_3_common;

use m8_3_common::*;
use mychess_app::online::{OnlineNotice, OnlinePhase, PlayingState, RejectReason};
use mychess_app::{AppNotice, Effect, GameMessage, Message, Nav};
use mychess_core::{Color, Role};
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

fn piece(rig: &Rig, square: &str) -> Option<(Color, Role)> {
    rig.state
        .online
        .game
        .as_ref()
        .unwrap()
        .position()
        .piece_at(sq(square))
        .map(|p| (p.color, p.role))
}

// ---- the moves of the server (D1318, D1324) ----

#[tokio::test]
async fn applying_the_same_moves_again_changes_nothing() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    for moves in ["e2e4", "e2e4", "e2e4 e7e5", "e2e4 e7e5", "e2e4 e7e5"] {
        rig.games[0].line(&state(moves));
    }
    rig.until("two plies", |s| s.session.game().mainline().count() == 2)
        .await;
    rig.pump(150).await;
    assert_eq!(rig.mainline_count(), 2);
    assert_eq!(rig.state.online.game.as_ref().unwrap().plies(), 2);
    // A gameState that is only a clock tick (same moves) keeps the game.
    assert_eq!(
        rig.state.online.phase,
        OnlinePhase::Playing(PlayingState::Live)
    );
}

#[tokio::test]
async fn the_view_follows_the_end_but_not_a_user_who_looks_at_an_earlier_move() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state("e2e4 e7e5"));
    wait_plies(&mut rig, 2).await;
    rig.send(Message::Game(GameMessage::Navigate(Nav::Start)))
        .await;
    rig.games[0].line(&state("e2e4 e7e5 g1f3"));
    wait_plies(&mut rig, 3).await;
    rig.until("tree rebuilt", |s| s.session.game().mainline().count() == 3)
        .await;
    assert_eq!(
        rig.state.session.cursor(),
        rig.state.session.game().root(),
        "the user who went back to the start is not pulled forward"
    );
}

#[tokio::test]
async fn a_takeback_accepted_truncates_the_game_without_a_resync() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state("e2e4 e7e5 g1f3"));
    wait_plies(&mut rig, 3).await;
    rig.games[0].line(&state("e2e4 e7e5"));
    wait_plies(&mut rig, 2).await;
    rig.until("shown", |s| s.session.game().mainline().count() == 2)
        .await;
    assert_eq!(
        rig.state.online.phase,
        OnlinePhase::Playing(PlayingState::Live)
    );
    rig.pump(100).await;
    assert_eq!(
        rig.fake
            .requests_for("GET", &format!("/api/board/game/stream/{ID}"))
            .len(),
        1,
        "a takeback is not a divergence"
    );
}

// ---- castling in both spellings (D1318) ----

const CASTLE_FEN: &str = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";

#[rstest]
#[case::white_short_king_to_rook("e1h1", "g1", "f1", Color::White)]
#[case::white_short_king_two_squares("e1g1", "g1", "f1", Color::White)]
#[case::white_long_king_to_rook("e1a1", "c1", "d1", Color::White)]
#[case::white_long_king_two_squares("e1c1", "c1", "d1", Color::White)]
#[case::black_short_king_to_rook("h1h2 e8h8", "g8", "f8", Color::Black)]
#[case::black_short_king_two_squares("h1h2 e8g8", "g8", "f8", Color::Black)]
#[case::black_long_king_to_rook("h1h2 e8a8", "c8", "d8", Color::Black)]
#[case::black_long_king_two_squares("h1h2 e8c8", "c8", "d8", Color::Black)]
#[tokio::test]
async fn castling_arrives_as_king_to_rook_or_king_two_squares(
    #[case] moves: &str,
    #[case] king: &str,
    #[case] rook: &str,
    #[case] side: Color,
) {
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(&Full::from_fen(CASTLE_FEN), &state("")))
        .await;
    rig.games[0].line(&state(moves));
    let plies = moves.split_whitespace().count();
    wait_plies(&mut rig, plies).await;
    assert_eq!(piece(&rig, king), Some((side, Role::King)), "{moves}");
    assert_eq!(piece(&rig, rook), Some((side, Role::Rook)), "{moves}");
    let back = if side == Color::White { "e1" } else { "e8" };
    assert_eq!(piece(&rig, back), None, "{moves}");
    assert_eq!(
        rig.state.online.phase,
        OnlinePhase::Playing(PlayingState::Live)
    );
    assert_eq!(
        rig.fake
            .requests_for("GET", &format!("/api/board/game/stream/{ID}"))
            .len(),
        1,
        "castling is not a divergence: no resync"
    );
}

// ---- a move the server finds illegal (D1318, D1361) ----

#[tokio::test]
async fn an_illegal_move_of_the_server_makes_one_resync_and_the_new_full_fixes_it() {
    let mut rig = Rig::new(2).await;
    rig.start_game().await;
    rig.games[0].line(&state("e2e5"));
    rig.wait_game_streams(2).await;
    assert_eq!(
        rig.mainline_count(),
        0,
        "the illegal move never reaches the board"
    );
    rig.games[1].line(&full(&Full::default(), &state("e2e4")));
    rig.until("reconciled", |s| {
        s.session.game().mainline().count() == 1
            && s.online.phase == OnlinePhase::Playing(PlayingState::Live)
    })
    .await;
    rig.pump(100).await;
    assert_eq!(
        rig.fake
            .requests_for("GET", &format!("/api/board/game/stream/{ID}"))
            .len(),
        2,
        "exactly one resynchronisation"
    );
    assert!(rig.lock());
}

#[tokio::test]
async fn a_server_that_keeps_sending_nonsense_ends_in_desynced_without_a_panic() {
    let mut rig = Rig::new(3).await;
    rig.start_game().await;
    rig.games[0].line(&state("e2e5"));
    rig.wait_game_streams(2).await;
    rig.games[1].line(&full(&Full::default(), &state("e2e5")));
    rig.wait_game_streams(3).await;
    rig.games[2].line(&full(&Full::default(), &state("e2e5")));
    rig.until("desynced", |s| {
        matches!(s.notice, Some(AppNotice::Online(OnlineNotice::Desynced)))
    })
    .await;
    rig.pump(300).await;
    assert_eq!(
        rig.fake
            .requests_for("GET", &format!("/api/board/game/stream/{ID}"))
            .len(),
        3,
        "no fourth stream: the view stops, the server is the truth"
    );
    assert!(rig.lock(), "the lock is kept");
    assert_eq!(rig.mainline_count(), 0);
}

#[tokio::test]
async fn garbage_in_the_game_stream_does_not_stop_the_game() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line("this is not json");
    rig.games[0].line(r#"{"type":"gameState","moves":42}"#);
    rig.games[0].line(r#"{"type":"mysteryEvent"}"#);
    rig.games[0].line(r#"{"type":"chatLine","username":"x","text":"play Qh5!","room":"player"}"#);
    rig.games[0].line(&state("e2e4"));
    wait_plies(&mut rig, 1).await;
    assert_eq!(
        rig.state.online.phase,
        OnlinePhase::Playing(PlayingState::Live)
    );
    assert!(
        !matches!(notice(&rig), Some(OnlineNotice::Error(_))),
        "{:?}",
        notice(&rig)
    );
}

// ---- the move of the user (D1320, D1347, D1360) ----

#[tokio::test]
async fn the_move_of_the_user_is_on_the_board_before_the_server_answers_and_is_sent() {
    let mut rig = Rig::new(1).await;
    rig.move_ok("e7e5");
    rig.start_game().await;
    rig.games[0].line(&state("e2e4"));
    wait_plies(&mut rig, 1).await;
    rig.until("shown", |s| s.session.game().mainline().count() == 1)
        .await;
    // Only `update`: the move is on the board before any effect ran.
    let effects = rig.update_only(Message::Online(player_move("e7", "e5", None, false)));
    assert_eq!(rig.state.online.game.as_ref().unwrap().plies(), 2);
    assert!(
        effects.iter().any(|e| matches!(e, Effect::Online { .. })),
        "{effects:?}"
    );
    assert!(
        rig.fake
            .requests_for("POST", &format!("/api/board/game/{ID}/move/e7e5"))
            .is_empty(),
        "update did not touch the network"
    );
    rig.run(effects).await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/move/e7e5"), 1)
            .await
    );
    rig.until("shown", |s| s.session.game().mainline().count() == 2)
        .await;
    // The state that confirms it measures the latency.
    rig.games[0].line(&state("e2e4 e7e5"));
    rig.until("latency", |s| s.online.ping.is_some()).await;
    assert_eq!(rig.mainline_count(), 2);
}

#[tokio::test]
async fn a_state_that_does_not_mention_the_move_yet_does_not_take_it_off_the_board() {
    let mut rig = Rig::new(1).await;
    rig.move_ok("e7e5");
    rig.start_game().await;
    rig.games[0].line(&state("e2e4"));
    wait_plies(&mut rig, 1).await;
    rig.play("e7", "e5").await;
    rig.until("shown", |s| s.session.game().mainline().count() == 2)
        .await;
    // E.g. the opponent offers a draw: a state with the old moves.
    rig.games[0].line(&state_with(
        "e2e4",
        590_000,
        590_000,
        "started",
        r#","wdraw":true"#,
    ));
    rig.pump(200).await;
    assert_eq!(
        rig.mainline_count(),
        2,
        "the move stays until the server decides"
    );
    rig.games[0].line(&state("e2e4 e7e5"));
    rig.until("latency", |s| s.online.ping.is_some()).await;
    assert_eq!(rig.mainline_count(), 2);
}

#[tokio::test]
async fn a_move_the_server_refuses_with_400_is_taken_back_with_a_remark() {
    let mut rig = Rig::new(1).await;
    rig.move_400("e7e5");
    rig.start_game().await;
    rig.games[0].line(&state("e2e4"));
    wait_plies(&mut rig, 1).await;
    rig.until("shown", |s| s.session.game().mainline().count() == 1)
        .await;
    rig.play("e7", "e5").await;
    rig.until("undone", |s| {
        s.online.game.as_ref().is_some_and(|g| g.plies() == 1)
    })
    .await;
    rig.until("view", |s| s.session.game().mainline().count() == 1)
        .await;
    assert!(matches!(
        notice(&rig),
        Some(OnlineNotice::MoveRejected { .. })
    ));
    assert_eq!(rig.move_posts("e7e5"), 1, "a 400 is never retried");
    // The user can try another move at once.
    rig.move_ok("d7d5");
    rig.play("d7", "d5").await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/move/d7d5"), 1)
            .await
    );
}

#[tokio::test]
async fn a_move_that_runs_out_of_attempts_is_taken_back() {
    let mut rig = Rig::new(1).await;
    rig.fake.fallback(
        "POST",
        &format!("/api/board/game/{ID}/move/e7e5"),
        mychess_online::testing::Response::json(503, r#"{"error":"busy"}"#),
    );
    rig.start_game().await;
    rig.games[0].line(&state("e2e4"));
    wait_plies(&mut rig, 1).await;
    rig.play("e7", "e5").await;
    rig.until("undone", |s| {
        s.online.game.as_ref().is_some_and(|g| g.plies() == 1)
    })
    .await;
    assert!(rig.move_posts("e7e5") >= 2, "the move was retried");
    assert!(rig.move_posts("e7e5") <= 4, "but not forever");
    assert!(matches!(notice(&rig), Some(OnlineNotice::Error(_))));
}

#[tokio::test]
async fn a_400_after_a_retry_is_ambiguous_and_the_move_is_not_taken_back() {
    let mut rig = Rig::new(1).await;
    let path = format!("/api/board/game/{ID}/move/e7e5");
    rig.fake.enqueue(
        "POST",
        &path,
        mychess_online::testing::Response::json(503, r#"{"error":"busy"}"#),
    );
    rig.move_400("e7e5");
    rig.start_game().await;
    rig.games[0].line(&state("e2e4"));
    wait_plies(&mut rig, 1).await;
    rig.play("e7", "e5").await;
    rig.until("uncertain", |s| {
        matches!(
            s.notice,
            Some(AppNotice::Online(OnlineNotice::MoveUncertain))
        )
    })
    .await;
    assert_eq!(rig.state.online.game.as_ref().unwrap().plies(), 2);
    assert_eq!(
        rig.mainline_count(),
        2,
        "the first attempt may have been applied"
    );
    // The server state decides: it had applied the move.
    rig.games[0].line(&state("e2e4 e7e5"));
    wait_plies(&mut rig, 2).await;
    rig.pump(100).await;
    assert_eq!(rig.mainline_count(), 2);
}

// ---- what is sent (D1320, D1322) ----

#[tokio::test]
async fn castling_is_sent_as_the_destination_of_the_king() {
    let mut rig = Rig::new(1).await;
    rig.move_ok("e1g1");
    rig.start_with(&full(
        &Full {
            me_white: true,
            ..Full::from_fen(CASTLE_FEN)
        },
        &state(""),
    ))
    .await;
    rig.play("e1", "g1").await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/move/e1g1"), 1)
            .await
    );
}

#[rstest]
#[case::queen(Role::Queen, "e7e8q")]
#[case::knight(Role::Knight, "e7e8n")]
#[case::rook(Role::Rook, "e7e8r")]
#[case::bishop(Role::Bishop, "e7e8b")]
#[tokio::test]
async fn a_promotion_is_sent_with_the_lower_case_letter(#[case] role: Role, #[case] uci: &str) {
    let mut rig = Rig::new(1).await;
    rig.move_ok(uci);
    rig.start_with(&full(
        &Full {
            me_white: true,
            ..Full::from_fen("7k/4P3/8/8/8/8/8/K7 w - - 0 1")
        },
        &state(""),
    ))
    .await;
    rig.online(player_move("e7", "e8", Some(role), false)).await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/move/{uci}"), 1)
            .await
    );
}

#[rstest]
#[case::with_offer(true)]
#[case::without_offer(false)]
#[tokio::test]
async fn a_move_can_carry_a_draw_offer(#[case] offer: bool) {
    let mut rig = Rig::new(1).await;
    rig.move_ok("e2e4");
    rig.start_with(&full(&Full::white(), &state(""))).await;
    rig.online(player_move("e2", "e4", None, offer)).await;
    let path = format!("/api/board/game/{ID}/move/e2e4");
    assert!(rig.fake.wait_requests("POST", &path, 1).await);
    let query = rig.fake.requests_for("POST", &path)[0].query_pairs();
    let offered = query
        .iter()
        .any(|(k, v)| k == "offeringDraw" && v == "true");
    assert_eq!(offered, offer, "{query:?}");
}

#[tokio::test]
async fn moving_while_the_opponent_offers_a_draw_does_not_accept_it() {
    let mut rig = Rig::new(1).await;
    rig.move_ok("e2e4");
    rig.start_with(&full(&Full::white(), &state(""))).await;
    rig.games[0].line(&state_with(
        "",
        598_000,
        600_000,
        "started",
        r#","bdraw":true"#,
    ));
    rig.until("offer", |s| s.online.draw_offered_to_me()).await;
    rig.play("e2", "e4").await;
    assert!(
        rig.fake
            .wait_requests("POST", &format!("/api/board/game/{ID}/move/e2e4"), 1)
            .await
    );
    rig.pump(100).await;
    assert_eq!(
        rig.action_posts("draw/yes"),
        0,
        "a move declines, never accepts"
    );
}

#[tokio::test]
async fn an_illegal_move_or_the_wrong_turn_is_refused_before_the_network() {
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(&Full::white(), &state(""))).await;
    // White to move: e2e5 is illegal.
    rig.play("e2", "e5").await;
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::IllegalMove))
    );
    assert_eq!(rig.mainline_count(), 0);
    // Not a piece of the user at all.
    rig.play("e7", "e5").await;
    assert!(matches!(
        notice(&rig),
        Some(OnlineNotice::Rejected(
            RejectReason::IllegalMove | RejectReason::NotYourTurn
        ))
    ));
    rig.pump(100).await;
    assert_eq!(
        rig.fake
            .requests()
            .iter()
            .filter(|r| r.path.contains("/move/"))
            .count(),
        0
    );
}

#[tokio::test]
async fn a_move_after_the_game_ended_is_refused() {
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(&Full::white(), &state(""))).await;
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("white")));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    rig.play("g1", "f3").await;
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NoGame))
    );
}

#[tokio::test]
async fn an_online_message_is_the_only_way_to_move_while_the_lock_holds() {
    // The editor refuses moves of its own during the game (D1330) ...
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(&Full::white(), &state(""))).await;
    let effects = rig.update_only(Message::Game(GameMessage::PlaySquares {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
        options: mychess_app::PlayOptions::default(),
    }));
    assert!(effects.is_empty());
    assert_eq!(rig.mainline_count(), 0);
    assert_eq!(notice(&rig), Some(OnlineNotice::AssistanceDisabled));
}
