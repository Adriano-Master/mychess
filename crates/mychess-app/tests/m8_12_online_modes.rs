//! M8-12: Resign and Rematch on Lichess, against the fake server (spec
//! `docs/specs/m8-12-desistir-e-revanche.md`, D3101).
#![allow(
    clippy::large_futures,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty
)]

mod m8_3_common;

use m8_3_common::*;
use mychess_app::GameMode;
use mychess_app::online::{OnlinePhase, PlayingState};

#[tokio::test]
async fn an_online_game_is_resigned_once_it_is_past_the_abort_window_and_replayed_after() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    // Hardly begun: the game is aborted, not resigned.
    assert_eq!(rig.state.resign_mode(), None);
    assert_eq!(rig.state.rematch_mode(), None);
    rig.games[0].line(&state("e2e4 e7e5"));
    rig.until("two plies", |s| {
        s.online.game.as_ref().is_some_and(|g| g.plies() == 2)
    })
    .await;
    assert_eq!(rig.state.resign_mode(), Some(GameMode::Online));
    assert_eq!(rig.state.rematch_mode(), None);
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("white")));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    assert_eq!(rig.state.resign_mode(), None);
    assert_eq!(rig.state.rematch_mode(), Some(GameMode::Online));
}

#[tokio::test]
async fn a_finished_online_game_yields_the_rematch_while_something_else_holds_the_board() {
    use mychess_app::engines::EngineDraft;
    use mychess_app::tournament::{
        Participant, SystemResources, TournamentConfig, TournamentMessage,
    };
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("white")));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    assert_eq!(rig.state.rematch_mode(), Some(GameMode::Online));
    // A tournament takes the board (loading already counts as live, D3110).
    let ids: Vec<_> = ["A", "B"]
        .iter()
        .map(|n| {
            rig.state
                .engines
                .add(EngineDraft::new(*n, "/fake"))
                .unwrap()
        })
        .collect();
    mychess_app::update(
        &mut rig.state,
        TournamentMessage::Resources(SystemResources {
            cores: 4,
            ram_available_mb: Some(15_000),
        })
        .into(),
    );
    let config = TournamentConfig {
        participants: ids
            .iter()
            .zip(["A", "B"])
            .map(|(id, n)| Participant::new(id.0, n))
            .collect(),
        ..TournamentConfig::default()
    };
    mychess_app::update(
        &mut rig.state,
        TournamentMessage::Start {
            config: Box::new(config),
            at: std::time::Instant::now(),
        }
        .into(),
    );
    assert!(rig.state.tournament_live());
    assert_eq!(rig.state.rematch_mode(), None);
    assert_eq!(rig.state.resign_mode(), None);
}
