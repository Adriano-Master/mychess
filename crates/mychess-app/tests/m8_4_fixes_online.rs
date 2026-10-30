//! Regression test of the review of the tactics training (spec M8-4, D1944):
//! an online game ends the training first, so the game of the user is the
//! one that comes back after the online game, never the problem.
#![allow(
    clippy::large_futures, // the rig owns the whole `AppState` across awaits
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_3_common;

use m8_3_common::*;
use mychess_app::Message;
use mychess_app::online::{OnlineMessage, OnlinePhase, PlayingState};
use mychess_app::tactics::{TacticsConfig, TacticsMessage, TacticsPhase};

#[tokio::test]
async fn an_online_game_ends_the_training_and_restores_the_game_of_the_user() {
    let mut rig = Rig::new(1).await;
    let file = rig.dir.path().join("p.pgn");
    std::fs::write(
        &file,
        "[Event \"one\"]\n[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1\"]\n\n*\n",
    )
    .unwrap();
    let config = TacticsConfig {
        file: Some(file),
        ..TacticsConfig::default()
    };
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(config),
        seed: 1,
    }))
    .await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);

    rig.start_game().await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert_ne!(rig.state.session.game().tags().event, "Tactics");

    // The online game ends and the previous game comes back.
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("white")));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    rig.online(OnlineMessage::DismissResult).await;
    rig.online(OnlineMessage::RestorePrevious).await;
    assert_ne!(
        rig.state.session.game().tags().event,
        "Tactics",
        "the problem must not come back as if it were the game of the user"
    );
    assert_eq!(*rig.state.session.position(), mychess_core::Position::new());
}
