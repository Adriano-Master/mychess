//! M4-10 review fixes: "Continue analysis" keeps the effort of the first
//! run (D2190), only an engine report can be continued (D2191), the side of
//! the user does not leak between games (D2196) and a refused run keeps the
//! review (D2198). A synthetic UCI engine answers the searches.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::assert_is_empty
)]

mod m4_6_common;

use m4_6_common::{HANGING_QUEEN, Rig, game_of, identity, material_engine};
use mychess_app::engines::{EngineDraft, EngineMessage, RunId, StartMode};
use mychess_app::report::{ReportConfig, ReportMessage, ReportOrigin, SideFilter};
use mychess_app::{AnnotateNotice, AppNotice, Effect, GameMessage, Message, update};
use mychess_core::Color;
use std::time::Duration;

fn stopped_after(plies: usize, searches: usize) -> Rig {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..plies]));
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, searches, &mut material_engine);
    assert!(rig.state.report_running());
    rig.send(ReportMessage::Cancel);
    assert!(!rig.state.report_running());
    rig
}

fn notice(rig: &Rig) -> Option<&AppNotice> {
    rig.state.notice.as_ref()
}

/// A second engine, started and ready; returns its run.
fn second_engine(rig: &mut Rig) -> RunId {
    let engine = rig
        .state
        .engines
        .add(EngineDraft::new("Other", "/other"))
        .unwrap();
    let fx = update(
        &mut rig.state,
        Message::Engine(EngineMessage::Start(engine, StartMode::Analyze)),
    );
    let run = fx
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("StartEngine");
    update(
        &mut rig.state,
        Message::Engine(EngineMessage::Started {
            run,
            identity: Box::new(identity(Vec::new(), Some(5))),
            at: 1_700_000_001,
        }),
    );
    run
}

// ---- the effort of the first run (D2190) ----

#[test]
fn the_effort_of_the_first_run_is_recorded_with_the_report() {
    let rig = stopped_after(12, 6);
    let data = rig.state.report_data().unwrap();
    let effort = data.effort().expect("an engine made it");
    let wanted = ReportConfig::default();
    assert!(!effort.by_time);
    assert_eq!(effort.depth, wanted.depth);
    assert_eq!(effort.movetime_ms, wanted.movetime_ms);
    assert_eq!(effort.engine, Some(rig.run));
    assert!(matches!(data.origin, ReportOrigin::Engine(_)));
}

#[test]
fn changing_the_depth_between_the_two_runs_does_not_mix_them() {
    let mut rig = stopped_after(12, 6);
    let first = rig.state.report_data().unwrap().effort().unwrap().depth;
    rig.state.prefs.report_config.depth = first + 9;
    let fx = rig.send(ReportMessage::Continue);
    let log = rig.drive(fx, 5000, &mut material_engine);
    assert!(!log.requests.is_empty());
    for request in &log.requests {
        assert_eq!(
            request.limits.depth,
            Some(first),
            "the depth of the first run"
        );
    }
    let data = rig.state.report_data().unwrap();
    assert!(data.complete);
    assert_eq!(data.effort().unwrap().depth, first);
}

#[test]
fn changing_to_a_time_limit_between_the_two_runs_does_not_mix_them() {
    let mut rig = stopped_after(12, 6);
    rig.state.prefs.report_config.by_time = true;
    rig.state.prefs.report_config.movetime_ms = 1500;
    let fx = rig.send(ReportMessage::Continue);
    let log = rig.drive(fx, 5000, &mut material_engine);
    assert!(!log.requests.is_empty());
    for request in &log.requests {
        assert!(request.limits.depth.is_some(), "still by depth");
        assert_eq!(request.limits.movetime, None);
    }
    assert!(!rig.state.report_data().unwrap().effort().unwrap().by_time);
}

#[test]
fn a_report_by_time_goes_on_by_time_whatever_the_preferences_say() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..12]));
    rig.state.prefs.report_config.by_time = true;
    rig.state.prefs.report_config.movetime_ms = 700;
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 6, &mut material_engine);
    rig.send(ReportMessage::Cancel);
    rig.state.prefs.report_config.by_time = false;
    let fx = rig.send(ReportMessage::Continue);
    let log = rig.drive(fx, 5000, &mut material_engine);
    assert!(!log.requests.is_empty());
    for request in &log.requests {
        assert_eq!(request.limits.movetime, Some(Duration::from_millis(700)));
    }
}

#[test]
fn the_same_configuration_gives_the_same_report_as_a_single_run() {
    let mut rig = stopped_after(12, 6);
    let fx = rig.send(ReportMessage::Continue);
    rig.drive(fx, 5000, &mut material_engine);
    let continued = rig.state.report_data().unwrap().report.clone();

    let mut single = Rig::new(game_of(&HANGING_QUEEN[..12]));
    let fx = single.send(ReportMessage::Generate);
    single.drive(fx, 5000, &mut material_engine);
    let whole = &single.state.report_data().unwrap().report;
    for (a, b) in continued.moves.iter().zip(&whole.moves) {
        assert_eq!(a.class, b.class, "ply {}", a.ply);
    }
    assert_eq!(continued.moves.len(), whole.moves.len());
}

#[test]
fn the_engine_of_the_first_run_is_kept_when_it_still_runs() {
    let mut rig = stopped_after(12, 6);
    let other = second_engine(&mut rig);
    rig.state.report.engine = Some(other);
    let fx = rig.send(ReportMessage::Continue);
    assert!(rig.state.report_running());
    assert!(
        !matches!(
            notice(&rig),
            Some(AppNotice::Annotate(
                AnnotateNotice::ReportContinuedOtherEngine
            ))
        ),
        "no warning"
    );
    rig.drive(fx, 5000, &mut material_engine);
    assert_eq!(
        rig.state.report_data().unwrap().effort().unwrap().engine,
        Some(rig.run)
    );
}

#[test]
fn a_gone_engine_is_replaced_by_the_current_one_with_a_warning() {
    let mut rig = stopped_after(12, 6);
    let other = second_engine(&mut rig);
    update(
        &mut rig.state,
        Message::Engine(EngineMessage::Stopped(rig.run)),
    );
    rig.state.report.engine = Some(other);
    rig.send(ReportMessage::Continue);
    assert!(rig.state.report_running());
    assert_eq!(
        notice(&rig),
        Some(&AppNotice::Annotate(
            AnnotateNotice::ReportContinuedOtherEngine
        ))
    );
}

// ---- only an engine report can be continued (D2191) ----

#[test]
fn a_report_rebuilt_from_the_game_cannot_be_continued() {
    let mut game = game_of(&HANGING_QUEEN[..12]);
    let nodes: Vec<_> = game.mainline().collect();
    for (node, text) in nodes.iter().zip([
        "[%eval 0.20]",
        "[%eval 0.20]",
        "[%eval 0.30]",
        "[%eval 0.20]",
    ]) {
        game.set_comment(*node, text).unwrap();
    }
    let mut rig = Rig::new(game);
    rig.send(ReportMessage::FromGame);
    let data = rig.state.report_data().expect("rebuilt from the comments");
    assert!(!data.complete, "only four moves have an evaluation");
    assert_eq!(data.origin, ReportOrigin::Game);
    assert!(!data.resumable());
    rig.send(ReportMessage::Continue);
    assert!(!rig.state.report_running());
}

// ---- the side of the user (D2196) ----

fn named(white: &str, black: &str) -> mychess_core::Game {
    let mut game = game_of(&HANGING_QUEEN[..6]);
    white.clone_into(&mut game.tags_mut().white);
    black.clone_into(&mut game.tags_mut().black);
    game
}

#[test]
fn the_side_chosen_for_a_game_does_not_stick_to_the_next_one() {
    let mut rig = Rig::new(named("Ana", "Bruno"));
    "Ana".clone_into(&mut rig.state.prefs.play.player_name);
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut material_engine);
    assert_eq!(rig.state.report.user_side, Some(Color::White));

    rig.state.session.replace_game(named("Carla", "Davi"));
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut material_engine);
    assert!(rig.state.report_data().is_some());
    assert_eq!(
        rig.state.report.user_side, None,
        "nobody known in the new game"
    );
}

#[test]
fn a_side_picked_by_hand_survives_a_new_report_of_the_same_game() {
    let mut rig = Rig::new(named("Carla", "Davi"));
    rig.send(ReportMessage::SetUserSide(Some(Color::Black)));
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut material_engine);
    assert_eq!(rig.state.report.user_side, Some(Color::Black));
}

// ---- the review and a refused run (D2198) ----

#[test]
fn a_refused_run_keeps_the_review_and_a_started_one_closes_it() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..12]));
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut material_engine);
    rig.send(ReportMessage::StartReview {
        side: SideFilter::Both,
        highlights: false,
    });
    assert!(rig.state.report.review.is_some());
    // Trial mode refuses the run.
    rig.send(GameMessage::EnterTrial { null_move: false });
    rig.send(ReportMessage::Generate);
    assert!(!rig.state.report_running());
    assert!(rig.state.report.review.is_some(), "the review stays");
    rig.send(GameMessage::ExitTrial);
    rig.send(ReportMessage::Generate);
    assert!(rig.state.report_running());
    assert!(rig.state.report.review.is_none(), "the run closed it");
}
