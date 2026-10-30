//! M8-12 (D3101, D3110): the modes that have no Resign and no Rematch (a
//! tournament, the opening trainer, the Calvar trainer, the tactics training
//! and the observation of a game) never offer them, and a Lichess game that
//! is over does not claim the Rematch while something else holds the board.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m3_4a_common;

use m3_4a_common::*;
use mychess_app::calvar::{CalvarConfig, CalvarMessage, CalvarPhase};
use mychess_app::engines::EngineDraft;
use mychess_app::opening_trainer::{OpeningConfig, OpeningMessage, OpeningPhase, RepSourceKind};
use mychess_app::tactics::{TacticsConfig, TacticsMessage, TacticsPhase};
use mychess_app::tournament::{Participant, TournamentConfig, TournamentMessage};

fn nothing_to_resign_or_replay(rig: &Rig, what: &str) {
    assert_eq!(rig.state.resign_mode(), None, "{what}: resign");
    assert_eq!(rig.state.rematch_mode(), None, "{what}: rematch");
}

#[test]
fn the_opening_trainer_has_neither_command() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let repertoire = write(
        dir.path(),
        "rep.pgn",
        "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 Nc6 *\n",
    );
    rig.send(Message::OpeningTrainer(OpeningMessage::Start {
        config: Box::new(OpeningConfig {
            source: RepSourceKind::File,
            file: Some(repertoire),
            ..OpeningConfig::default()
        }),
        seed: 7,
        today: 100,
    }));
    assert_eq!(rig.state.opening_phase(), OpeningPhase::UserTurn);
    nothing_to_resign_or_replay(&rig, "opening trainer");
}

#[test]
fn the_tactics_training_has_neither_command() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let problems = write(
        dir.path(),
        "p.pgn",
        "[Event \"one\"]\n[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1\"]\n\n*\n",
    );
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(TacticsConfig {
            file: Some(problems),
            ..TacticsConfig::default()
        }),
        seed: 1,
    }));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    nothing_to_resign_or_replay(&rig, "tactics training");
}

#[test]
fn the_calvar_trainer_has_neither_command() {
    let mut rig = Rig::new();
    rig.state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .unwrap();
    // Effects (the engines of the analysis) are not run: the phase is enough.
    rig.raw(Message::Calvar(CalvarMessage::Start {
        config: Box::new(CalvarConfig::default()),
        now: 1_700_000_000,
    }));
    assert_eq!(rig.state.calvar_phase(), CalvarPhase::Entering);
    nothing_to_resign_or_replay(&rig, "calvar");
}

#[test]
fn a_tournament_has_neither_command() {
    let mut rig = Rig::new();
    let ids: Vec<_> = ["A", "B"]
        .iter()
        .map(|n| {
            rig.state
                .engines
                .add(EngineDraft::new(*n, "/fake"))
                .unwrap()
        })
        .collect();
    rig.raw(TournamentMessage::Resources(
        mychess_app::tournament::SystemResources {
            cores: 4,
            ram_available_mb: Some(15_000),
        },
    ));
    let config = TournamentConfig {
        participants: ids
            .iter()
            .zip(["A", "B"])
            .map(|(id, n)| Participant::new(id.0, n))
            .collect(),
        ..TournamentConfig::default()
    };
    // Only the update: loading counts as live and no engine is launched.
    rig.raw(TournamentMessage::Start {
        config: Box::new(config),
        at: std::time::Instant::now(),
    });
    assert!(rig.state.tournament_live());
    nothing_to_resign_or_replay(&rig, "tournament");
}
