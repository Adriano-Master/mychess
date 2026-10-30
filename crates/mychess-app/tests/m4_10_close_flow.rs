//! M4-10 close-out contract tests, part 2: "Continue analysis", the side of
//! the user, the arrow keys of the review, the time left and the class mark
//! of the board (spec `m4-10-relatorio-da-partida.md`, D1437, D1443, D1444,
//! D1434, D2110-D2121). A synthetic UCI engine answers the searches; no real
//! time passes (the timers are delivered by hand).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod m4_6_common;

use std::time::Duration;

use m4_6_common::{
    HANGING_QUEEN, Rig, add, game_of, last_search, material_engine, nth_main, ply_of,
};
use mychess_app::report::{
    CLOCK_TICK_SECS, ClassMark, MoveClass, ReportMessage, ReviewStep, SideFilter, eta_seconds,
    infer_user_side,
};
use mychess_app::{Action, Effect, GameMessage, Message, Nav};
use mychess_core::{Color, Game, Square};

fn rig_with(plies: usize) -> Rig {
    Rig::new(game_of(&HANGING_QUEEN[..plies]))
}

/// A run that stopped after `searches` searches (the next one not started).
fn stopped_after(plies: usize, searches: usize) -> Rig {
    let mut rig = rig_with(plies);
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, searches, &mut material_engine);
    assert!(rig.state.report_running());
    rig.send(ReportMessage::Cancel);
    assert!(!rig.state.report_running());
    rig
}

fn full_report(plies: usize) -> Rig {
    let mut rig = rig_with(plies);
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut material_engine);
    assert!(!rig.state.report_running());
    rig
}

fn square(name: &str) -> Square {
    name.parse().expect("square")
}

// ---- continue the analysis (D1443, D2110) ----

#[test]
fn a_stopped_report_can_be_continued_a_complete_one_cannot() {
    let rig = stopped_after(12, 6);
    let data = rig.state.report_data().expect("partial");
    assert!(!data.complete && data.resumable());
    assert_eq!(data.report.moves.len(), 5);

    let done = full_report(12);
    assert!(!done.state.report_data().unwrap().resumable());
}

#[test]
fn continuing_searches_only_the_positions_that_are_missing() {
    let mut rig = stopped_after(12, 6);
    let fx = rig.send(ReportMessage::Continue);
    assert!(rig.state.report_running());
    let progress = rig.state.annotation_progress().expect("progress");
    assert_eq!(progress.done_plies, 5, "the five moves already judged");
    assert_eq!(progress.total_plies, 12);
    let first = last_search(&fx).expect("the first search of the new run");
    assert_eq!(ply_of(&first), 6, "it starts at the first missing position");

    let log = rig.drive(fx, 5000, &mut material_engine);
    // Positions 6..=12 are searched, the first six are not.
    assert_eq!(log.requests.len(), 7, "{}", log.requests.len());
    let data = rig.state.report_data().expect("report");
    assert!(data.complete);
    assert_eq!(data.report.moves.len(), 12);
}

#[test]
fn a_continued_report_with_the_same_configuration_equals_a_single_run() {
    let mut rig = stopped_after(12, 6);
    let fx = rig.send(ReportMessage::Continue);
    rig.drive(fx, 5000, &mut material_engine);
    let continued = rig.state.report_data().expect("continued");

    let single = full_report(12);
    let whole = single.state.report_data().expect("single");
    assert_eq!(continued.report.moves.len(), whole.report.moves.len());
    for (a, b) in continued.report.moves.iter().zip(&whole.report.moves) {
        assert_eq!(a.class, b.class, "ply {}", a.ply);
        assert_eq!(a.best, b.best, "ply {}", a.ply);
        assert!((a.loss - b.loss).abs() < 1e-4, "ply {}", a.ply);
    }
    assert_eq!(continued.report.white.accuracy, whole.report.white.accuracy);
    assert_eq!(continued.report.black.accuracy, whole.report.black.accuracy);
}

#[test]
fn a_continued_run_that_is_stopped_again_keeps_what_both_runs_did() {
    let mut rig = stopped_after(14, 4);
    assert_eq!(rig.state.report_data().unwrap().report.moves.len(), 3);
    let fx = rig.send(ReportMessage::Continue);
    rig.drive(fx, 3, &mut material_engine);
    rig.send(ReportMessage::Cancel);
    let data = rig.state.report_data().expect("report");
    assert_eq!(
        data.report.moves.len(),
        6,
        "3 + 3 positions beyond the start"
    );
    assert!(data.resumable());
    // And again.
    let fx = rig.send(ReportMessage::Continue);
    rig.drive(fx, 5000, &mut material_engine);
    assert!(rig.state.report_data().unwrap().complete);
}

#[test]
fn continuing_without_a_partial_report_does_nothing() {
    let mut rig = rig_with(6);
    rig.send(ReportMessage::Continue);
    assert!(!rig.state.report_running());

    let mut done = full_report(6);
    done.send(ReportMessage::Continue);
    assert!(!done.state.report_running());
    assert!(done.state.report_data().unwrap().complete);
}

#[test]
fn a_game_that_changed_cannot_be_continued() {
    let mut rig = stopped_after(12, 6);
    rig.send(GameMessage::Navigate(Nav::Start));
    rig.send(GameMessage::TruncateEnd);
    assert!(rig.state.report_data().is_none(), "the report is stale");
    rig.send(ReportMessage::Continue);
    assert!(!rig.state.report_running());
}

#[test]
fn continuing_during_a_run_does_not_start_a_second_one() {
    let mut rig = stopped_after(12, 6);
    let fx = rig.send(ReportMessage::Continue);
    assert!(last_search(&fx).is_some());
    let again = rig.send(ReportMessage::Continue);
    assert!(last_search(&again).is_none(), "no new search: {again:?}");
    assert!(rig.state.report_running());
}

// ---- the side of the user (D1437, D2115) ----

fn named(white: &str, black: &str) -> Game {
    let mut game = game_of(&HANGING_QUEEN[..6]);
    white.clone_into(&mut game.tags_mut().white);
    black.clone_into(&mut game.tags_mut().black);
    game
}

#[test]
fn the_side_is_inferred_from_the_names_of_the_game_and_the_preferences() {
    let game = named("  ANA Souza ", "Bruno");
    assert_eq!(infer_user_side(&game, &["ana souza"]), Some(Color::White));
    assert_eq!(infer_user_side(&game, &["Bruno"]), Some(Color::Black));
    assert_eq!(infer_user_side(&game, &["", "bruno"]), Some(Color::Black));
    // Neither, or both, or no name at all: no guess.
    assert_eq!(infer_user_side(&game, &["Carla"]), None);
    assert_eq!(infer_user_side(&game, &[]), None);
    assert_eq!(infer_user_side(&game, &[" ", ""]), None);
    let same = named("Ana", "ana");
    assert_eq!(infer_user_side(&same, &["Ana"]), None);
    // Both sides match the same name: no guess.
    let unknown = named("?", "?");
    assert_eq!(infer_user_side(&unknown, &["?"]), None);
}

#[test]
fn a_report_of_a_game_of_the_user_knows_the_side() {
    let mut rig = Rig::new(named("Ana", "Bruno"));
    rig.state.prefs.play.player_name = "Bruno".to_owned();
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut material_engine);
    assert_eq!(rig.state.report.user_side, Some(Color::Black));
}

#[test]
fn the_lichess_login_also_names_the_user() {
    let mut rig = Rig::new(named("Ana", "Bruno"));
    rig.state.prefs.online.login = "ana".to_owned();
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut material_engine);
    assert_eq!(rig.state.report.user_side, Some(Color::White));
}

#[test]
fn without_a_name_the_side_stays_unset_until_the_user_chooses_it() {
    let mut rig = Rig::new(named("Ana", "Bruno"));
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut material_engine);
    assert_eq!(rig.state.report.user_side, None);
    rig.send(ReportMessage::SetUserSide(Some(Color::White)));
    assert_eq!(rig.state.report.user_side, Some(Color::White));
    rig.send(ReportMessage::SetUserSide(None));
    assert_eq!(rig.state.report.user_side, None);
    rig.send(ReportMessage::SetUserSide(Some(Color::Black)));
    rig.send(ReportMessage::Clear);
    assert_eq!(rig.state.report.user_side, None, "Clear forgets it");
}

#[test]
fn a_choice_of_the_user_survives_a_new_report_of_a_game_without_names() {
    let mut rig = Rig::new(named("Ana", "Bruno"));
    rig.send(ReportMessage::SetUserSide(Some(Color::Black)));
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut material_engine);
    assert_eq!(rig.state.report.user_side, Some(Color::Black));
}

// ---- the arrow keys of the review (D1435, D2118) ----

/// A review of the blunder, with a second moment (ply 9) added by hand, on a
/// board without animation: the cursor is on the position before each.
fn reviewing() -> Rig {
    let mut rig = full_report(20);
    rig.state.prefs.board.animation_ms = 0;
    rig.send(GameMessage::Navigate(Nav::End));
    rig.send(ReportMessage::StartReview {
        side: SideFilter::Both,
        highlights: false,
    });
    let review = rig.state.report.review.as_mut().expect("a review");
    let first = review.moments[0];
    assert_eq!(first.ply, 5, "the hanging queen");
    review.moments.push(mychess_app::report::Moment {
        ply: 9,
        class: MoveClass::Mistake,
    });
    rig
}

fn cursor_ply(rig: &Rig) -> usize {
    let cursor = rig.state.session.cursor();
    if cursor == rig.game().root() {
        return 0;
    }
    rig.game()
        .mainline()
        .position(|n| n == cursor)
        .map(|i| i + 1)
        .expect("on the main line")
}

#[test]
fn the_right_and_left_arrows_visit_the_next_and_the_previous_moment() {
    let mut rig = reviewing();
    assert_eq!(cursor_ply(&rig), 4);
    rig.send(Message::Action(Action::Forward));
    assert_eq!(rig.state.report.review.as_ref().unwrap().index, 1);
    assert_eq!(cursor_ply(&rig), 8, "before the move of the second moment");
    rig.send(Message::Action(Action::Back));
    assert_eq!(rig.state.report.review.as_ref().unwrap().index, 0);
    assert_eq!(cursor_ply(&rig), 4);
}

#[test]
fn the_arrows_stop_at_the_ends_of_the_list_and_do_not_walk_the_game() {
    let mut rig = reviewing();
    rig.send(Message::Action(Action::Back));
    assert_eq!(rig.state.report.review.as_ref().unwrap().index, 0);
    assert_eq!(cursor_ply(&rig), 4, "no moment before the first");
    rig.send(Message::Action(Action::Forward));
    rig.send(Message::Action(Action::Forward));
    assert_eq!(rig.state.report.review.as_ref().unwrap().index, 1);
    assert_eq!(cursor_ply(&rig), 8, "no moment after the last");
}

#[test]
fn while_the_user_tries_the_arrows_are_the_ones_of_the_game() {
    let mut rig = reviewing();
    rig.send(ReportMessage::ReviewTry);
    assert_eq!(
        rig.state.report.review.as_ref().unwrap().step,
        ReviewStep::Trying
    );
    rig.send(Message::Action(Action::Forward));
    assert_eq!(rig.state.report.review.as_ref().unwrap().index, 0);
    assert_eq!(cursor_ply(&rig), 5, "a plain step forward in the game");
}

#[test]
fn off_the_moment_the_arrows_are_the_ones_of_the_game() {
    let mut rig = reviewing();
    rig.send(GameMessage::Navigate(Nav::GoToMainlinePly(10)));
    rig.send(Message::Action(Action::Back));
    assert_eq!(cursor_ply(&rig), 9);
    assert_eq!(rig.state.report.review.as_ref().unwrap().index, 0);
    rig.send(Message::Action(Action::Forward));
    assert_eq!(cursor_ply(&rig), 10);
}

#[test]
fn without_a_review_the_arrows_are_the_ones_of_the_game() {
    let mut rig = full_report(20);
    rig.send(GameMessage::Navigate(Nav::Start));
    rig.send(Message::Action(Action::Forward));
    assert_eq!(cursor_ply(&rig), 1);
    rig.send(Message::Action(Action::Back));
    assert_eq!(cursor_ply(&rig), 0);
}

#[test]
fn an_open_dialog_keeps_the_arrows_for_itself() {
    let mut rig = reviewing();
    rig.send(ReportMessage::OpenConfig);
    assert!(rig.state.report.config_dialog.is_some());
    assert!(!rig.state.review_owns_arrows());
}

#[test]
fn the_arrows_have_no_effect_on_the_walk_of_a_review_with_animation() {
    let mut rig = full_report(20);
    rig.state.prefs.board.animation_ms = 200;
    rig.send(GameMessage::Navigate(Nav::End));
    rig.send(ReportMessage::StartReview {
        side: SideFilter::Both,
        highlights: false,
    });
    assert!(rig.state.review_walking());
    // A press in the middle of the walk ends it on the target and is then
    // taken as "next": there is no second moment, so it stays there.
    rig.send(Message::Action(Action::Forward));
    assert!(!rig.state.review_walking());
    assert_eq!(cursor_ply(&rig), 4);
}

// ---- the time left (D1444, D2121) ----

#[test]
fn the_estimate_needs_a_pace_and_scales_with_what_is_left() {
    assert_eq!(eta_seconds(0, 3, 10), None, "no time has gone by");
    assert_eq!(eta_seconds(10, 0, 10), None, "nothing is done yet");
    assert_eq!(eta_seconds(10, 5, 15), Some(30));
    assert_eq!(eta_seconds(10, 3, 1), Some(4), "rounded up");
    assert_eq!(eta_seconds(10, 5, 0), Some(0), "nothing left");
    assert!(eta_seconds(u32::MAX, 1, u32::MAX).is_some(), "no overflow");
    // More left never means less time.
    let mut last = 0;
    for left in 0..200 {
        let eta = eta_seconds(12, 4, left).unwrap();
        assert!(eta >= last);
        last = eta;
    }
}

fn clock_stamp(effects: &[Effect]) -> Option<u64> {
    effects.iter().find_map(|e| match e {
        Effect::ScheduleReviewTick { after, stamp }
            if *after == Duration::from_secs(u64::from(CLOCK_TICK_SECS)) =>
        {
            Some(*stamp)
        }
        _ => None,
    })
}

#[test]
fn a_run_starts_a_one_second_clock_and_each_tick_asks_for_the_next() {
    let mut rig = rig_with(12);
    let fx = rig.send(ReportMessage::Generate);
    let stamp = clock_stamp(&fx).expect("the clock timer");
    assert_eq!(rig.state.report_eta_seconds(), None, "no pace yet");
    let next = rig.send(ReportMessage::WalkTick { stamp });
    assert_eq!(clock_stamp(&next), Some(stamp), "the same clock goes on");
    let clock = rig.state.report.clock.expect("clock");
    assert_eq!(clock.seconds, CLOCK_TICK_SECS);
}

#[test]
fn the_estimate_follows_the_moves_done_and_the_seconds_counted() {
    let mut rig = rig_with(12);
    let fx = rig.send(ReportMessage::Generate);
    let stamp = clock_stamp(&fx).expect("clock");
    // Four seconds go by, then three positions are answered (two moves).
    for _ in 0..4 {
        rig.send(ReportMessage::WalkTick { stamp });
    }
    assert_eq!(rig.state.report_eta_seconds(), None, "no move done yet");
    rig.drive(fx, 3, &mut material_engine);
    let progress = rig.state.annotation_progress().expect("progress");
    assert_eq!(progress.done_plies, 2);
    let expected = eta_seconds(4, 2, progress.total_plies - 2);
    assert!(expected.is_some());
    assert_eq!(rig.state.report_eta_seconds(), expected);
}

#[test]
fn a_continued_run_measures_only_the_moves_it_made() {
    let mut rig = stopped_after(12, 6);
    let fx = rig.send(ReportMessage::Continue);
    let stamp = clock_stamp(&fx).expect("clock");
    for _ in 0..6 {
        rig.send(ReportMessage::WalkTick { stamp });
    }
    // Five moves were already there: the pace of this run is still unknown.
    assert_eq!(rig.state.report_eta_seconds(), None);
    rig.drive(fx, 2, &mut material_engine);
    let progress = rig.state.annotation_progress().unwrap();
    assert_eq!(progress.done_plies, 7);
    assert_eq!(
        rig.state.report_eta_seconds(),
        eta_seconds(6, 2, progress.total_plies - 7)
    );
}

#[test]
fn a_tick_of_another_stamp_does_not_count() {
    let mut rig = rig_with(8);
    let fx = rig.send(ReportMessage::Generate);
    let stamp = clock_stamp(&fx).expect("clock");
    for wrong in [stamp.wrapping_add(1), stamp.wrapping_sub(1), 0, u64::MAX] {
        let fx = rig.send(ReportMessage::WalkTick { stamp: wrong });
        assert!(fx.is_empty(), "{fx:?}");
    }
    assert_eq!(rig.state.report.clock.unwrap().seconds, 0);
}

#[test]
fn the_clock_dies_with_the_run_and_its_late_tick_does_nothing() {
    let mut rig = rig_with(8);
    let fx = rig.send(ReportMessage::Generate);
    let stamp = clock_stamp(&fx).expect("clock");
    rig.send(ReportMessage::Cancel);
    assert!(!rig.state.report_running());
    assert!(rig.state.report.clock.is_none());
    let late = rig.send(ReportMessage::WalkTick { stamp });
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(rig.state.report_eta_seconds(), None);
}

#[test]
fn a_new_run_gets_a_new_clock() {
    let mut rig = rig_with(8);
    let first = clock_stamp(&rig.send(ReportMessage::Generate)).expect("clock");
    rig.send(ReportMessage::Cancel);
    let second = clock_stamp(&rig.send(ReportMessage::Generate)).expect("clock");
    assert_ne!(first, second);
    // The old timer is dead: it neither counts nor continues.
    let fx = rig.send(ReportMessage::WalkTick { stamp: first });
    assert!(fx.is_empty());
    assert_eq!(rig.state.report.clock.unwrap().seconds, 0);
}

#[test]
fn a_run_that_never_started_has_no_clock() {
    let mut rig = Rig::new(Game::new());
    let fx = rig.send(ReportMessage::Generate);
    assert!(clock_stamp(&fx).is_none(), "{fx:?}");
    assert!(rig.state.report.clock.is_none());
}

// ---- the class mark of the board (D1434, D2110) ----

fn mark_at(rig: &mut Rig, ply: u32) -> Option<ClassMark> {
    rig.send(GameMessage::Navigate(Nav::GoToMainlinePly(ply)));
    rig.state.board_class_mark()
}

#[test]
fn the_mark_is_the_class_of_the_move_on_its_destination_square() {
    let mut rig = full_report(20);
    let data_class =
        |rig: &Rig, ply: usize| rig.state.report_data().unwrap().report.moves[ply - 1].class;
    let mark = mark_at(&mut rig, 5).expect("a mark at ply 5");
    assert_eq!(mark.square, square("e5"), "Qxe5");
    assert_eq!(mark.class, data_class(&rig, 5));
    assert_eq!(mark.class, MoveClass::Blunder);
    // Another move, another square.
    let mark = mark_at(&mut rig, 1).expect("a mark at ply 1");
    assert_eq!(mark.square, square("e4"));
    assert_eq!(mark.class, data_class(&rig, 1));
}

#[test]
fn castling_marks_the_square_of_the_king() {
    let mut rig = full_report(20);
    // 10.O-O-O (e1c1): the king lands on c1.
    let mark = mark_at(&mut rig, 19).expect("a mark");
    assert_eq!(mark.square, square("c1"));
}

#[test]
fn there_is_no_mark_at_the_start_without_a_report_or_with_the_marks_off() {
    let mut rig = full_report(20);
    assert_eq!(mark_at(&mut rig, 0), None, "no move at the start");
    assert!(mark_at(&mut rig, 5).is_some());
    rig.state.prefs.report_config.board_marks = false;
    assert_eq!(rig.state.board_class_mark(), None, "marks off");
    rig.state.prefs.report_config.board_marks = true;
    assert!(rig.state.board_class_mark().is_some());
    rig.send(ReportMessage::Clear);
    assert_eq!(rig.state.board_class_mark(), None, "no report");
}

#[test]
fn there_is_no_mark_inside_a_variation_or_for_a_stale_report() {
    let mut game = game_of(&HANGING_QUEEN[..8]);
    let parent = nth_main(&game, 2);
    let variation = add(&mut game, parent, "g1f3");
    let mut rig = Rig::new(game);
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut material_engine);
    assert!(
        rig.state.report_data().is_some(),
        "variations do not matter"
    );
    assert!(mark_at(&mut rig, 3).is_some());
    rig.send(GameMessage::Navigate(Nav::GoToNode(variation)));
    assert_eq!(rig.state.board_class_mark(), None, "off the main line");
    rig.send(GameMessage::Navigate(Nav::GoToMainlinePly(3)));
    rig.send(GameMessage::TruncateEnd);
    assert!(rig.state.report_data().is_none());
    assert_eq!(rig.state.board_class_mark(), None, "a stale report");
}

#[test]
fn the_mark_is_hidden_during_a_review_and_a_run() {
    let mut rig = reviewing();
    assert_eq!(
        rig.state.board_class_mark(),
        None,
        "the review draws its own"
    );
    rig.send(ReportMessage::ReviewExit);
    rig.send(GameMessage::Navigate(Nav::GoToMainlinePly(5)));
    assert!(rig.state.board_class_mark().is_some());
    rig.send(ReportMessage::Generate);
    assert!(rig.state.report_running());
    assert_eq!(rig.state.board_class_mark(), None, "a run in progress");
}
