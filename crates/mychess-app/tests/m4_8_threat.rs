//! Contract tests of the threat analysis (spec M4-8, D490, D491 and rules
//! 1-4; Scid `setTrialMode`, `main.tcl` 1882-1913).
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unnecessary_wraps,
    clippy::default_trait_access,
    clippy::assert_is_empty
)]

mod m4_8_common;

use m4_8_common::*;
use mychess_app::engines::RunId;
use mychess_app::{
    AnalysisMessage, AnalysisNotice, AppNotice, EngineMode, GameMessage, Message, ModeMessage,
    ModeNotice, Nav,
};
use mychess_core::{Move, Position};
use mychess_engine::EngineEvent;

const IN_CHECK: &str = "4k3/8/8/8/8/8/4r3/4K3 w - - 0 1";
const EN_PASSANT: &str = "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1";

fn threat(rig: &mut Rig) -> Vec<mychess_app::Effect> {
    let run = rig.run;
    rig.mode(ModeMessage::ToggleThreat(run))
}

#[test]
fn threat_enters_trial_and_the_engine_gets_the_null_move_position() {
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    let before = rig.position().clone();

    let fx = threat(&mut rig);

    assert!(rig.state.session.in_trial());
    assert!(rig.state.threat_active());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Threat);
    let request = searches(&fx)
        .pop()
        .expect("the engine analyses the new position");
    assert_eq!(
        request.position.moves.last().copied(),
        Some(Move::Null),
        "the target ends with the null move"
    );
    assert_eq!(position_of(&request).fen(), rig.position().fen());
    assert_ne!(rig.position().turn(), before.turn(), "the side is swapped");
    assert!(rig.mode_notice().is_none());
}

#[test]
fn threat_clears_the_en_passant_square() {
    let mut rig = Rig::fen(EN_PASSANT);
    let fx = threat(&mut rig);
    assert!(rig.state.session.in_trial());
    let fen = rig.position().fen();
    assert_eq!(fen.split(' ').nth(3), Some("-"), "{fen}");
    let request = searches(&fx).pop().expect("search");
    assert_eq!(position_of(&request).fen(), fen);
}

#[test]
fn threat_in_check_is_refused_without_entering_trial() {
    let mut rig = Rig::fen(IN_CHECK);
    let cursor = rig.state.session.cursor();
    let fx = threat(&mut rig);
    assert_eq!(rig.mode_notice(), Some(ModeNotice::NullMoveInCheck));
    assert!(!rig.state.session.in_trial(), "must not be left in trial");
    assert!(!rig.state.threat_active());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(rig.state.session.cursor(), cursor);
    assert_eq!(rig.position().fen(), IN_CHECK);
    assert!(
        searches(&fx)
            .iter()
            .all(|s| !s.position.moves.contains(&Move::Null)),
        "no null-move position may reach the engine"
    );
}

#[test]
fn threat_button_is_disabled_in_check_and_enabled_otherwise() {
    let rig = Rig::fen(IN_CHECK);
    let a = rig.state.mode_availability(rig.run);
    assert!(!a.threat);
    let rig = Rig::new(game_of(&["e2e4"]));
    assert!(rig.state.mode_availability(rig.run).threat);
}

#[test]
fn leaving_the_threat_restores_game_cursor_revision_and_resends_the_position() {
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5", "g1f3"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    rig.game_msg(GameMessage::Navigate(Nav::Back(1)));
    let fen = rig.position().fen();
    let cursor = rig.state.session.cursor();
    let revision = rig.state.session.revision();
    let nodes = rig.game().node_count();
    let main = rig.mainline();
    threat(&mut rig);
    assert!(rig.state.threat_active());

    let fx = threat(&mut rig);

    assert!(!rig.state.session.in_trial());
    assert!(!rig.state.threat_active());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(rig.position().fen(), fen);
    assert_eq!(rig.state.session.cursor(), cursor);
    assert_eq!(rig.state.session.revision(), revision);
    assert_eq!(rig.game().node_count(), nodes);
    assert_eq!(rig.mainline(), main);
    let request = searches(&fx)
        .pop()
        .expect("the original position goes back to the engine");
    assert!(!request.position.moves.contains(&Move::Null));
    assert_eq!(position_of(&request).fen(), fen);
}

#[test]
fn threat_ends_when_the_user_leaves_trial_mode() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    threat(&mut rig);
    rig.game_msg(GameMessage::ExitTrial);
    assert!(!rig.state.session.in_trial());
    assert!(!rig.state.threat_active());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
}

#[test]
fn threat_ends_when_the_user_leaves_the_null_move_node() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    threat(&mut rig);
    rig.game_msg(GameMessage::Navigate(Nav::Back(1)));
    assert!(!rig.state.threat_active(), "navigating off the null move");
}

#[test]
fn threat_is_refused_when_the_user_is_already_in_trial_mode() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    rig.game_msg(GameMessage::EnterTrial { null_move: false });
    threat(&mut rig);
    assert!(!rig.state.threat_active());
    assert!(rig.state.session.in_trial(), "the user's trial stays on");
    assert!(!rig.state.mode_availability(rig.run).threat);
}

#[test]
fn threat_is_refused_with_a_dead_engine() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    rig.event(exited());
    threat(&mut rig);
    assert!(!rig.state.session.in_trial());
    assert!(!rig.state.threat_active());
    assert!(!rig.state.mode_availability(rig.run).threat);
}

#[test]
fn threat_is_refused_for_an_unknown_run() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    rig.mode(ModeMessage::ToggleThreat(RunId(987_654)));
    assert!(!rig.state.session.in_trial());
    assert!(!rig.state.threat_active());
}

#[test]
fn threat_is_refused_with_a_locked_analysis() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    let run = rig.run;
    rig.send(Message::Analysis(AnalysisMessage::ToggleLock(run)));
    threat(&mut rig);
    assert!(!rig.state.session.in_trial());
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Analysis(AnalysisNotice::Locked))
    );
    assert!(!rig.state.mode_availability(run).threat);
}

#[test]
fn the_report_of_the_threat_shows_in_san_for_the_side_that_would_move() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    let fx = threat(&mut rig);
    let request = searches(&fx).pop().expect("search");
    let pos = position_of(&request);
    // After 1.e4 and a null move White moves again.
    let reply = mv(&pos, "d2d4");
    rig.event(EngineEvent::SearchStarted(mychess_engine::SearchId(500)));
    rig.event(EngineEvent::Info(mychess_engine::SearchInfo {
        search: mychess_engine::SearchId(500),
        multipv: 1,
        depth: Some(10),
        score: Some((mychess_engine::Score::Cp(30), mychess_engine::Bound::Exact)),
        pv: vec![reply],
        ..mychess_engine::SearchInfo::default()
    }));
    let view = rig.state.analysis_view(rig.run).expect("view");
    assert!(
        view.lines.iter().any(|l| l.text.contains("d4")),
        "{:?}",
        view.lines
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_null_move_position_is_legal_to_play_from_for_the_engine() {
    // Whatever the engine plays from the threat position must be legal there.
    let mut rig = Rig::new(Default::default());
    let fx = threat(&mut rig);
    let request = searches(&fx).pop().expect("search");
    let pos: Position = position_of(&request);
    assert!(!pos.legal_moves().is_empty());
}
