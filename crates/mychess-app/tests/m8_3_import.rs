//! M8-3b contract tests of the import of the finished game (spec D1327,
//! D1335, D1354, D1356): the tags of the Lichess PGN, `[%clk]` per move,
//! one copy per `Site`, no copy of an aborted game, and the Clipbase as the
//! only destination (a read-only database in the list is never written).
#![allow(
    clippy::large_futures, // the rig owns the whole `AppState` across awaits
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_3_common;

use m8_3_common::*;
use mychess_app::online::{OnlineMessage, OnlineNotice, OnlinePhase, PlayingState, SaveTo};
use mychess_app::{AppNotice, BasesMessage, GameMessage, Message, Nav, SlotId};
use mychess_core::{Game, GameResult, PartialDate};

fn notice(rig: &Rig) -> Option<OnlineNotice> {
    match rig.notice() {
        Some(AppNotice::Online(n)) => Some(n.clone()),
        _ => None,
    }
}

async fn wait_finished(rig: &mut Rig) {
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
}

fn comments(game: &Game) -> Vec<Option<String>> {
    game.mainline()
        .filter(|id| *id != game.root())
        .map(|id| game.comment(id).expect("node").map(str::to_owned))
        .collect()
}

// ---- tags (D1335) ----

#[tokio::test]
async fn the_tags_follow_the_pgn_of_the_lichess() {
    let mut rig = Rig::new(1).await;
    let opts = Full {
        rated: true,
        speed: "rapid".to_owned(),
        clock: Some((600_000, 5_000)),
        white_title: Some("GM".to_owned()),
        ..Full::default()
    };
    rig.start_with(&full(&opts, &state(""))).await;
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("white")));
    wait_finished(&mut rig).await;
    let game = rig.state.session.game().clone();
    let tags = game.tags();
    assert_eq!(tags.event, "Rated rapid game");
    assert_eq!(tags.site, format!("https://lichess.org/{ID}"));
    assert_eq!(tags.round, "-");
    assert_eq!(tags.white, "alice");
    assert_eq!(tags.black, "Bob");
    assert_eq!(tags.result, GameResult::White);
    // createdAt 1700000000000 ms = 2023-11-14 22:13:20 UTC (from the server,
    // never from the clock of the PC).
    assert_eq!(
        tags.date,
        PartialDate::new(Some(2023), Some(11), Some(14)).unwrap()
    );
    assert_eq!(tags.get_extra("UTCDate"), Some("2023.11.14"));
    assert_eq!(tags.get_extra("UTCTime"), Some("22:13:20"));
    assert_eq!(tags.white_rating.map(|r| r.value.get()), Some(1800));
    assert_eq!(tags.black_rating.map(|r| r.value.get()), Some(1500));
    assert_eq!(tags.get_extra("WhiteTitle"), Some("GM"));
    assert_eq!(tags.get_extra("BlackTitle"), None);
    assert_eq!(tags.get_extra("TimeControl"), Some("600+5"));
    assert_eq!(tags.get_extra("Termination"), Some("Normal"));
    assert_eq!(
        tags.get_extra("Variant"),
        None,
        "standard has no Variant tag"
    );
    assert_eq!(game.mainline().count(), 2);
}

#[tokio::test]
async fn a_casual_classical_game_is_named_like_the_lichess_does() {
    let mut rig = Rig::new(1).await;
    let opts = Full {
        speed: "classical".to_owned(),
        clock: Some((1_800_000, 20_000)),
        ..Full::default()
    };
    rig.start_with(&full(&opts, &state(""))).await;
    rig.games[0].line(&finished("e2e4", "draw", None));
    wait_finished(&mut rig).await;
    let tags = rig.state.session.game().tags().clone();
    assert_eq!(tags.event, "Casual classical game");
    assert_eq!(tags.get_extra("TimeControl"), Some("1800+20"));
    assert_eq!(tags.result, GameResult::Draw);
}

#[tokio::test]
async fn a_game_from_a_position_keeps_it_as_the_start() {
    let fen = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(&Full::from_fen(fen), &state("")))
        .await;
    rig.games[0].line(&finished("e2e4", "resign", Some("white")));
    wait_finished(&mut rig).await;
    let game = rig.state.session.game();
    assert_eq!(
        game.start_position().fen(),
        mychess_core::Position::from_fen(fen).unwrap().fen()
    );
    assert_eq!(game.mainline().count(), 1);
}

#[tokio::test]
async fn what_is_stored_in_the_clipbase_is_the_game_with_its_tags() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("black")));
    wait_finished(&mut rig).await;
    let rows = rig.state.bases.list_page(SlotId::CLIPBASE, 1, 10);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].white, "alice");
    assert_eq!(rows[0].black, "Bob");
    assert_eq!(rows[0].result, GameResult::Black);
    assert_eq!(rows[0].white_elo, 1800);
    assert_eq!(rows[0].event, "Casual rapid game");
    // Load it back: moves, clocks and the Site survive the database.
    rig.send(Message::Bases(BasesMessage::LoadGame {
        slot: SlotId::CLIPBASE,
        number: 1,
    }))
    .await;
    let loaded = rig.state.session.game();
    assert_eq!(loaded.tags().site, format!("https://lichess.org/{ID}"));
    assert_eq!(loaded.mainline().count(), 2);
    assert_eq!(loaded.tags().get_extra("Termination"), Some("Normal"));
}

// ---- [%clk] (D1335, D1356) ----

#[tokio::test]
async fn each_move_carries_the_clock_of_the_server_in_hours_minutes_seconds() {
    let mut rig = Rig::new(1).await;
    let opts = Full {
        clock: Some((3_725_000, 0)),
        ..Full::default()
    };
    rig.start_with(&full(
        &opts,
        &state_with("", 3_725_000, 3_600_000, "started", ""),
    ))
    .await;
    // White moves: her clock after the move; Black moves: his; and so on.
    rig.games[0].line(&state_with("e2e4", 3_725_000, 3_600_000, "started", ""));
    rig.until("1", |s| s.session.game().mainline().count() == 1)
        .await;
    rig.games[0].line(&state_with(
        "e2e4 e7e5",
        3_725_000,
        3_599_000,
        "started",
        "",
    ));
    rig.until("2", |s| s.session.game().mainline().count() == 2)
        .await;
    rig.games[0].line(&state_with(
        "e2e4 e7e5 g1f3",
        3_700_000,
        3_599_000,
        "started",
        "",
    ));
    rig.until("3", |s| s.session.game().mainline().count() == 3)
        .await;
    rig.games[0].line(&state_with(
        "e2e4 e7e5 g1f3",
        3_700_000,
        3_599_000,
        "resign",
        r#","winner":"white""#,
    ));
    wait_finished(&mut rig).await;
    let c = comments(rig.state.session.game());
    assert_eq!(
        c,
        vec![
            Some("[%clk 1:02:05]".to_owned()),
            Some("[%clk 0:59:59]".to_owned()),
            Some("[%clk 1:01:40]".to_owned()),
        ]
    );
}

#[tokio::test]
async fn the_clock_comment_is_zero_padded_and_survives_the_database() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&state_with("e2e4", 5_000, 600_000, "started", ""));
    rig.until("1", |s| s.session.game().mainline().count() == 1)
        .await;
    rig.games[0].line(&state_with("e2e4 e7e5", 5_000, 61_000, "started", ""));
    rig.until("2", |s| s.session.game().mainline().count() == 2)
        .await;
    rig.games[0].line(&state_with(
        "e2e4 e7e5",
        5_000,
        61_000,
        "outoftime",
        r#","winner":"black""#,
    ));
    wait_finished(&mut rig).await;
    assert_eq!(
        comments(rig.state.session.game()),
        vec![
            Some("[%clk 0:00:05]".to_owned()),
            Some("[%clk 0:01:01]".to_owned()),
        ]
    );
    rig.send(Message::Bases(BasesMessage::LoadGame {
        slot: SlotId::CLIPBASE,
        number: 1,
    }))
    .await;
    assert_eq!(
        comments(rig.state.session.game()),
        vec![
            Some("[%clk 0:00:05]".to_owned()),
            Some("[%clk 0:01:01]".to_owned()),
        ]
    );
}

// ---- idempotence, aborted, destination (D1335, D1354) ----

#[tokio::test]
async fn the_same_game_finishing_twice_is_stored_once() {
    let mut rig = Rig::new(2).await;
    rig.start_game().await;
    let end = finished("e2e4 e7e5", "resign", Some("white"));
    rig.games[0].line(&end);
    wait_finished(&mut rig).await;
    assert_eq!(rig.clipbase_games(), 1);
    assert_eq!(notice(&rig), Some(OnlineNotice::Imported));
    rig.online(OnlineMessage::DismissResult).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Idle);
    // The event stream mentions the game again (a reconnection): the
    // stream opens, the first line says it is over.
    rig.game_start();
    rig.until("reopened", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Live)
    })
    .await;
    rig.wait_game_streams(2).await;
    rig.games[1].line(&full(&Full::default(), &end));
    wait_finished(&mut rig).await;
    assert_eq!(rig.clipbase_games(), 1, "idempotent by Site");
    assert_eq!(notice(&rig), Some(OnlineNotice::AlreadyImported));
}

#[tokio::test]
async fn with_nowhere_as_the_destination_the_game_is_not_stored() {
    let mut rig = Rig::new(1).await;
    rig.state.prefs.online.save_to = SaveTo::Nowhere;
    rig.start_game().await;
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("white")));
    wait_finished(&mut rig).await;
    assert_eq!(rig.clipbase_games(), 0);
    assert_ne!(notice(&rig), Some(OnlineNotice::Imported));
    assert!(!rig.lock(), "the lock is freed all the same");
    // The game stays in the editor for review.
    assert_eq!(rig.mainline_count(), 2);
}

#[tokio::test]
async fn a_read_only_database_in_the_list_is_never_written_the_clipbase_gets_the_game() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("two.pgn");
    std::fs::write(
        &path,
        "[Event \"E1\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n[Event \"E2\"]\n[White \"C\"]\n[Black \"D\"]\n[Result \"*\"]\n\n1. d4 *\n",
    )
    .unwrap();
    let mut rig = Rig::new(1).await;
    rig.state.open_pgn_in_slots = true;
    rig.send(Message::Bases(BasesMessage::Open(path))).await;
    let current = rig.state.bases.current();
    assert_ne!(current, SlotId::CLIPBASE, "the PGN is the current database");
    let slot = rig.state.bases.slot(current).expect("slot");
    assert!(slot.read_only, "a PGN opened in a slot is read-only");
    let before = slot.total();
    assert_eq!(before, 2);
    rig.start_game().await;
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("white")));
    wait_finished(&mut rig).await;
    assert_eq!(rig.clipbase_games(), 1, "the Clipbase gets it");
    assert_eq!(
        rig.state.bases.slot(current).unwrap().total(),
        before,
        "the read-only database is untouched"
    );
    assert_eq!(notice(&rig), Some(OnlineNotice::Imported));
}

#[tokio::test]
async fn the_online_game_can_be_reviewed_after_the_end_with_the_editor_free() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("white")));
    wait_finished(&mut rig).await;
    assert!(!rig.lock());
    rig.state.notice = None;
    rig.update_only(Message::Game(GameMessage::Navigate(Nav::Start)));
    assert_eq!(rig.state.notice, None);
    let effects = rig.update_only(Message::Action(mychess_app::Action::ToggleEvalBar));
    drop(effects);
    assert_ne!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::AssistanceDisabled))
    );
}
