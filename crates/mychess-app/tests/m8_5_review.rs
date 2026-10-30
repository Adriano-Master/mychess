//! Regression tests of the second review of the opening trainer (spec M8-5,
//! D2261-D2263): the collection of the parked game and a dialog that shows
//! up while the repertoire is being read.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m3_4a_common;

use std::sync::Arc;

use m3_4a_common::*;
use mychess_app::opening_trainer::{
    OpeningConfig, OpeningMessage, OpeningNotice, OpeningPhase, RepSourceKind,
};
use mychess_app::{DiscardPrompt, Pending, PgnLoad, PgnSource, ScanLimits, scan_pgn};
use mychess_pgn::read::ReadOptions;

const REP: &str = "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 *\n";

fn start(path: &std::path::Path) -> Message {
    Message::OpeningTrainer(OpeningMessage::Start {
        config: Box::new(OpeningConfig {
            source: RepSourceKind::File,
            file: Some(path.to_path_buf()),
            ..OpeningConfig::default()
        }),
        seed: 7,
        today: 100,
    })
}

#[test]
fn the_game_comes_back_with_its_collection() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "rep.pgn", REP);
    let mut rig = Rig::new();
    let text = "[Event \"a\"]\n\n1. e4 *\n\n[Event \"b\"]\n\n1. d4 *\n".to_owned();
    let (load, _) = scan_pgn(
        PgnSource::Text(text),
        &ScanLimits::default(),
        &ReadOptions::default(),
    )
    .unwrap();
    let PgnLoad::Many(collection) = load else {
        panic!("two games make a collection");
    };
    rig.state.collection = Some(collection.clone());
    rig.send(start(&path));
    assert_eq!(rig.state.opening_phase(), OpeningPhase::UserTurn);
    assert!(rig.state.collection.is_none(), "the line has no collection");
    rig.send(Message::OpeningTrainer(OpeningMessage::Close));
    assert!(
        rig.state
            .collection
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, &collection)),
        "next/previous game still work"
    );
}

#[test]
fn a_dialog_opened_while_the_repertoire_is_read_ends_the_start_in_silence_no_more() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "rep.pgn", REP);
    let mut rig = Rig::new();
    let effects = rig.raw(start(&path));
    rig.state.discard_prompt = Some(DiscardPrompt {
        pending: Pending::Quit,
    });
    for effect in effects {
        if matches!(effect, Effect::OpeningTrainer(_)) {
            let answer = rig.perform(effect);
            rig.raw(answer);
        }
    }
    assert_eq!(rig.state.opening_phase(), OpeningPhase::Closed);
    assert_eq!(
        rig.state.opening_view().notice,
        Some(OpeningNotice::Busy),
        "the user is told"
    );
    assert!(rig.state.discard_prompt.is_some(), "the dialog stays");
}
