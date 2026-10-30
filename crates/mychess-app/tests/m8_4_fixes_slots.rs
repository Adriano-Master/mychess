//! Regression tests of the review of the tactics training (spec M8-4,
//! D1952, D1953): the problem never becomes the game of a slot, and a
//! session that reads the current base ends when the base goes away.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m3_4a_common;

use m3_4a_common::*;
use mychess_app::tactics::{
    SourceKind, TacticsConfig, TacticsMessage, TacticsNotice, TacticsPhase,
};
use mychess_core::Position;

const MATE1: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
const MATE2: &str = "kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1";

fn fen_game(event: &str, fen: &str) -> String {
    format!("[Event \"{event}\"]\n[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n*\n\n")
}

fn problems() -> String {
    format!("{}{}", fen_game("one", MATE1), fen_game("two", MATE2))
}

fn start_file(rig: &mut Rig, file: &std::path::Path) {
    let config = TacticsConfig {
        file: Some(file.to_path_buf()),
        ..TacticsConfig::default()
    };
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(config),
        seed: 1,
    }));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    assert_eq!(rig.state.session.game().tags().event, "Tactics");
}

fn event(rig: &Rig) -> String {
    rig.state.session.game().tags().event.clone()
}

/// A rig with two slots open and the user's game (E1, clean) in slot 1.
fn two_slots(dir: &std::path::Path) -> Rig {
    let mut rig = Rig::new();
    rig.open(&write(dir, "a.pgn", TWO));
    rig.open(&write(dir, "b.pgn", TWO));
    rig.select(1);
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    assert_eq!(event(&rig), "E1");
    assert!(!rig.state.is_dirty());
    rig
}

#[test]
fn switching_slots_gives_the_game_of_the_user_back_to_the_slot_it_left() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = two_slots(dir.path());
    let file = write(dir.path(), "p.pgn", &problems());
    start_file(&mut rig, &file);

    rig.select(2);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert_eq!(rig.current(), sel(2));
    assert_ne!(event(&rig), "Tactics");

    // Slot 1 comes back with the game of the user, not with the problem.
    rig.select(1);
    assert_eq!(event(&rig), "E1");
    assert_eq!(rig.plies(), 2);
}

#[test]
fn closing_the_current_base_does_not_leave_the_problem_behind() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = two_slots(dir.path());
    let file = write(dir.path(), "p.pgn", &problems());
    start_file(&mut rig, &file);

    rig.send(BasesMessage::Close(sel(1)));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    assert_ne!(event(&rig), "Tactics");
    assert_eq!(*rig.state.session.position(), Position::new());
    assert!(rig.state.bases.slot(sel(1)).is_none());
}

#[test]
fn loading_a_game_ends_the_session_before_the_load_arrives() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = two_slots(dir.path());
    let file = write(dir.path(), "p.pgn", &problems());
    start_file(&mut rig, &file);

    // The request leaves; its answer has not come back yet.
    let effects = rig.raw(BasesMessage::LoadGame {
        slot: sel(1),
        number: 2,
    });
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert_eq!(event(&rig), "E1", "the game of the user is back meanwhile");
    // The answer then replaces it, as a load does.
    for effect in effects {
        if !effect.runs_on_host() {
            let answer = rig.perform(effect);
            rig.raw(answer);
        }
    }
    assert_eq!(event(&rig), "E2");
}

#[test]
fn a_session_of_the_current_base_ends_when_the_base_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    // The Clipbase holds the problems: copy two games into it.
    rig.open(&write(dir.path(), "src.pgn", &problems()));
    for number in [1, 2] {
        rig.send(BasesMessage::LoadGame {
            slot: sel(1),
            number,
        });
        rig.send(BasesMessage::ClipbaseCopy);
    }
    rig.select(9);
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    let config = TacticsConfig {
        source: SourceKind::CurrentBase,
        ..TacticsConfig::default()
    };
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(config),
        seed: 1,
    }));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);

    // The base changes under the session: the numbers of the games are
    // stale and the session ends, explaining why.
    rig.send(BasesMessage::ClipbaseClear);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert_ne!(event(&rig), "Tactics");
    assert!(rig.state.tactics_dialog_open());
    assert_eq!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::SourceChanged)
    );
}

fn si4_problems(dir: &std::path::Path) -> std::path::PathBuf {
    make_si4(dir, "problems", &problems())
}

fn start_current_base(rig: &mut Rig) {
    let config = TacticsConfig {
        source: SourceKind::CurrentBase,
        ..TacticsConfig::default()
    };
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(config),
        seed: 1,
    }));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
}

#[test]
fn closing_the_base_the_session_reads_gives_the_game_back_and_drops_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = {
        rig.open(&si4_problems(dir.path()));
        rig.current()
    };
    start_current_base(&mut rig);
    rig.send(BasesMessage::Close(slot));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert!(rig.state.bases.slot(slot).is_none());
    assert_eq!(rig.current(), SlotId::CLIPBASE);
    assert_ne!(event(&rig), "Tactics");
    assert_eq!(*rig.state.session.position(), Position::new());
}

#[test]
fn changing_only_the_flags_of_a_game_of_the_current_base_does_not_end_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&si4_problems(dir.path()));
    let slot = rig.current();
    start_current_base(&mut rig);
    rig.send(BasesMessage::SelectRow {
        slot,
        number: Some(2),
    });
    rig.send(BasesMessage::List(
        mychess_app::ListMessage::ToggleDeleted { slot },
    ));
    assert_eq!(
        rig.state.tactics_phase(),
        TacticsPhase::Solving,
        "the numbers of the games did not change"
    );
    assert_ne!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::SourceChanged)
    );
    assert!(!rig.state.tactics_dialog_open());
    // The file was read again by the change of flags: the session follows
    // the new handle and goes on to the next problem.
    rig.send(Message::Tactics(TacticsMessage::Next));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
}
