//! M8-1a contract tests of the controller (spec D1225-D1229, D1234-D1240,
//! D1244, D1246-D1250): what is asked of the opponent per mode, illegal and
//! null answers, quit/pause/resume, a lost opponent, colours and the
//! records of the moves.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1_common;

use std::time::Duration;

use m8_1_common::*;
use mychess_app::{
    ColorChoice, PlayEnd, PlayError, PlayOutput, PlayPhase, PlayRemark, ThinkLimits, TimeMode,
};
use mychess_core::{Color, GameResult, Move, Position};
use proptest::prelude::*;
use rstest::rstest;

// ---- what the opponent is asked (D1238, D1250) ----

#[rstest]
#[case(TimeMode::Depth(3), ThinkLimits::Depth(3))]
#[case(TimeMode::Depth(20), ThinkLimits::Depth(20))]
#[case(TimeMode::Nodes(1000), ThinkLimits::Nodes(1000))]
#[case(TimeMode::Nodes(2000), ThinkLimits::Nodes(2000))]
#[case(
    TimeMode::MoveTime(6),
    ThinkLimits::MoveTime(Duration::from_millis(6000))
)]
#[case(
    TimeMode::MoveTime(120),
    ThinkLimits::MoveTime(Duration::from_millis(120_000))
)]
fn the_first_request_carries_the_limit_of_the_mode(
    #[case] mode: TimeMode,
    #[case] expected: ThinkLimits,
) {
    let (c, out, _) = begin(setup(Color::Black, mode));
    let t = thinks(&out);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].limits, expected);
    assert_eq!(t[0].side, Color::White);
    assert!(t[0].moves.is_empty());
    assert_eq!(t[0].start, Position::new());
    assert_eq!(c.phase(), PlayPhase::Opponent);
}

#[rstest]
#[case(TimeMode::Depth(0), ThinkLimits::Depth(1))]
#[case(TimeMode::Depth(99), ThinkLimits::Depth(20))]
#[case(TimeMode::Nodes(5), ThinkLimits::Nodes(100))]
#[case(TimeMode::Nodes(u32::MAX), ThinkLimits::Nodes(2000))]
#[case(TimeMode::MoveTime(0), ThinkLimits::MoveTime(Duration::from_secs(1)))]
#[case(
    TimeMode::MoveTime(u32::MAX),
    ThinkLimits::MoveTime(Duration::from_secs(120))
)]
fn out_of_range_modes_are_clamped_before_they_reach_the_engine(
    #[case] mode: TimeMode,
    #[case] expected: ThinkLimits,
) {
    let (_, out, _) = begin(setup(Color::Black, mode));
    assert_eq!(thinks(&out)[0].limits, expected);
}

#[test]
fn the_timebonus_request_carries_the_clock_of_both_sides_in_the_go_fields() {
    let (_, out, _) = begin(setup(Color::Black, timebonus(10, 7, 3, 2)));
    let ThinkLimits::Clock(clock) = thinks(&out)[0].limits else {
        panic!("expected a clock");
    };
    assert_eq!(clock.white_time, 600 * SEC);
    assert_eq!(clock.black_time, 180 * SEC);
    assert_eq!(clock.white_increment, 7 * SEC);
    assert_eq!(clock.black_increment, 2 * SEC);
}

#[test]
fn the_clock_in_the_request_is_read_at_the_moment_the_engine_starts() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    let e4 = uci(&c, "e2e4");
    let out = c.player_move(e4, t0 + 4 * SEC);
    let t = thinks(&out);
    assert_eq!(t.len(), 1);
    let ThinkLimits::Clock(clock) = t[0].limits else {
        panic!("expected a clock");
    };
    // 300 - 4 + 3 for White: the increment of the move just made counts.
    assert_eq!(clock.white_time, 299 * SEC);
    assert_eq!(clock.black_time, 300 * SEC);
    assert_eq!(t[0].moves, [e4]);
}

#[test]
fn the_request_always_carries_every_move_since_the_start() {
    let (mut c, _, t0) = begin(setup(Color::White, TimeMode::Depth(2)));
    let mut seen = Vec::new();
    for (a, b) in [("e2e4", "e7e5"), ("g1f3", "b8c6")] {
        let m = uci(&c, a);
        seen.push(m);
        let out = c.player_move(m, t0);
        let t = thinks(&out);
        assert_eq!(t[0].moves, seen);
        let r = uci(&c, b);
        seen.push(r);
        c.opponent_moved(Some(r), None, t0, None);
    }
}

// ---- illegal, null and missing answers (D1238) ----

#[rstest]
#[case::none(None)]
#[case::null(Some(Move::Null))]
fn a_missing_or_null_move_ends_the_game_without_result(#[case] mv: Option<Move>) {
    let (mut c, _, t0) = begin(setup(Color::Black, TimeMode::Depth(1)));
    let out = c.opponent_moved(mv, None, t0, None);
    assert_eq!(ended(&out), Some(PlayEnd::OpponentFailure));
    assert_eq!(c.result(), GameResult::Unknown);
    assert_eq!(c.game().mainline().count(), 0);
    assert!(!c.is_live());
}

#[test]
fn an_illegal_move_of_the_engine_ends_the_game_without_result_and_changes_nothing() {
    let (mut c, _, t0) = begin(setup(Color::White, TimeMode::Depth(1)));
    play_ucis(&mut c, &["e2e4"], t0);
    // A legal move of the start position, illegal now: e2e4 again.
    let stale = Position::new().parse_uci("e2e4").expect("uci");
    let before = c.position().fen();
    let out = c.opponent_moved(Some(stale), None, t0, None);
    assert_eq!(ended(&out), Some(PlayEnd::OpponentFailure));
    assert_eq!(c.result(), GameResult::Unknown);
    assert_eq!(c.position().fen(), before);
    assert_eq!(c.game().mainline().count(), 1);
}

#[test]
fn an_illegal_move_ending_the_game_stops_the_search() {
    let (mut c, _, t0) = begin(setup(Color::Black, TimeMode::Depth(1)));
    // A move of Black, offered for White's turn.
    let black_to_move =
        Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1").unwrap();
    let wrong = black_to_move.parse_uci("e7e5").expect("uci");
    let out = c.opponent_moved(Some(wrong), None, t0, None);
    assert_eq!(ended(&out), Some(PlayEnd::OpponentFailure));
    assert!(has_stop(&out));
}

#[test]
fn an_answer_that_is_not_expected_is_dropped() {
    let (mut c, _, t0) = begin(setup(Color::White, TimeMode::Depth(1)));
    // It is the player's turn.
    let e5 = Position::new().parse_uci("e2e4").expect("uci");
    assert!(c.opponent_moved(Some(e5), None, t0, None).is_empty());
    assert!(c.opponent_moved(None, None, t0, None).is_empty());
    assert!(c.is_live());
    assert_eq!(c.game().mainline().count(), 0);
}

#[test]
fn an_answer_after_a_pause_is_dropped() {
    let (mut c, _, t0) = begin(setup(Color::White, TimeMode::Depth(1)));
    play_ucis(&mut c, &["e2e4"], t0);
    c.pause(t0);
    let reply = uci(&c, "e7e5");
    assert!(c.opponent_moved(Some(reply), None, t0, None).is_empty());
    assert_eq!(c.game().mainline().count(), 1);
    assert_eq!(c.phase(), PlayPhase::Paused { lost: false });
}

#[test]
fn an_answer_after_the_end_is_dropped() {
    let (mut c, _, t0) = begin(setup(Color::White, TimeMode::Depth(1)));
    play_ucis(&mut c, &["e2e4"], t0);
    c.resign(t0);
    let reply = uci(&c, "e7e5");
    assert!(c.opponent_moved(Some(reply), None, t0, None).is_empty());
    assert_eq!(c.game().mainline().count(), 1);
}

#[test]
fn the_player_cannot_move_out_of_turn_illegally_or_after_the_end() {
    let (mut c, _, t0) = begin(setup(Color::White, TimeMode::Depth(1)));
    let e2e5 = Position::new().parse_uci("e2e4").expect("uci");
    // Null move: never a real move.
    assert_eq!(
        c.player_move(Move::Null, t0),
        [PlayOutput::Notice(PlayRemark::IllegalMove)]
    );
    // A Black move in White's position.
    let black_move = Position::new()
        .after(e2e5)
        .expect("after")
        .parse_uci("e7e5")
        .expect("uci");
    assert_eq!(
        c.player_move(black_move, t0),
        [PlayOutput::Notice(PlayRemark::IllegalMove)]
    );
    assert_eq!(c.game().mainline().count(), 0);
    // Out of turn.
    c.player_move(e2e5, t0);
    let d4 = Position::new().parse_uci("d2d4").expect("uci");
    assert_eq!(
        c.player_move(d4, t0),
        [PlayOutput::Notice(PlayRemark::NotYourTurn)]
    );
    // After the end.
    c.resign(t0);
    assert_eq!(
        c.player_move(d4, t0),
        [PlayOutput::Notice(PlayRemark::NotYourTurn)]
    );
}

// ---- the records of the moves ----

#[test]
fn every_move_is_reported_with_mover_ply_node_and_time() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    let e4 = uci(&c, "e2e4");
    let out = c.player_move(e4, t0 + 4 * SEC);
    let PlayOutput::Moved(m) = out[0].clone() else {
        panic!("a Moved output comes first: {out:?}");
    };
    assert_eq!(m.mover, Color::White);
    assert_eq!(m.mv, e4);
    assert_eq!(m.ply, 1);
    assert_eq!(m.spent, 4 * SEC);
    assert!(!m.free);
    assert_eq!(c.tip(), m.node);
    let e5 = uci(&c, "e7e5");
    let out = c.opponent_moved(Some(e5), None, t0 + 9 * SEC, None);
    let PlayOutput::Moved(m) = out[0].clone() else {
        panic!("a Moved output comes first: {out:?}");
    };
    assert_eq!((m.mover, m.ply, m.spent), (Color::Black, 2, 5 * SEC));
}

#[test]
fn the_time_measured_by_the_task_replaces_the_instants() {
    let (mut c, _, t0) = begin(setup(Color::Black, blitz()));
    let e4 = uci(&c, "e2e4");
    let out = c.opponent_moved(Some(e4), Some(2 * SEC), t0 + 50 * SEC, None);
    let PlayOutput::Moved(m) = out[0].clone() else {
        panic!("{out:?}");
    };
    assert_eq!(m.spent, 2 * SEC);
    assert_eq!(c.shown(Color::White, t0 + 50 * SEC), 301 * SEC);
}

#[test]
fn the_game_keeps_the_moves_in_the_mainline_in_order() {
    let (mut c, _, t0) = begin(setup(Color::White, TimeMode::Depth(1)));
    play_ucis(&mut c, &["e2e4", "e7e5", "g1f3", "b8c6"], t0);
    assert_eq!(
        mainline_ucis(c.game()),
        ["e2e4", "e7e5", "g1f3", "b8c6"].map(str::to_owned)
    );
}

// ---- quit (D1227) ----

#[test]
fn quit_closes_without_result_and_without_an_end() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    play_ucis(&mut c, &["e2e4"], t0);
    let out = c.quit(t0 + SEC);
    assert_eq!(ended(&out), None);
    assert!(has_stop(&out), "the engine was thinking");
    assert_eq!(c.phase(), PlayPhase::Closed);
    assert_eq!(c.result(), GameResult::Unknown);
    assert!(!c.is_live());
    assert_eq!(c.end(), None);
}

#[test]
fn quit_on_the_turn_of_the_player_does_not_stop_anything() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    let out = c.quit(t0);
    assert!(!has_stop(&out));
    assert_eq!(c.phase(), PlayPhase::Closed);
}

#[test]
fn nothing_works_after_quit() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    c.quit(t0);
    assert!(c.quit(t0).is_empty());
    assert!(c.resign(t0).is_empty());
    assert!(c.pause(t0).is_empty());
    assert!(c.resume(t0).is_empty());
    assert!(c.tick(t0 + 999 * SEC).is_empty());
    assert!(c.opponent_lost(t0).is_empty());
    assert_eq!(
        c.player_move(uci(&c, "e2e4"), t0),
        [PlayOutput::Notice(PlayRemark::NotYourTurn)]
    );
    assert_eq!(c.result(), GameResult::Unknown);
}

#[test]
fn quit_after_the_end_keeps_the_result() {
    let (mut c, _, t0) = begin(setup(Color::White, TimeMode::Depth(1)));
    c.resign(t0);
    assert!(c.quit(t0).is_empty());
    assert_eq!(c.result(), GameResult::Black);
    assert!(matches!(c.phase(), PlayPhase::Ended(_)));
}

// ---- pause and resume (D1240) ----

#[test]
fn pausing_on_the_turn_of_the_player_stops_his_clock() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    let out = c.pause(t0 + 10 * SEC);
    assert!(!has_stop(&out));
    assert_eq!(c.phase(), PlayPhase::Paused { lost: false });
    assert_eq!(c.shown(Color::White, t0 + 10 * SEC), 290 * SEC);
    assert_eq!(c.shown(Color::White, t0 + 500 * SEC), 290 * SEC);
    assert!(
        c.tick(t0 + 999 * SEC).is_empty(),
        "a paused clock does not flag"
    );
    assert!(c.is_live());
}

#[test]
fn a_move_while_paused_is_refused() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    c.pause(t0);
    assert_eq!(
        c.player_move(uci(&c, "e2e4"), t0),
        [PlayOutput::Notice(PlayRemark::NotYourTurn)]
    );
}

#[test]
fn resume_on_the_turn_of_the_player_restarts_his_clock_without_charging_the_pause() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    c.pause(t0 + 10 * SEC);
    let out = c.resume(t0 + 100 * SEC);
    assert!(thinks(&out).is_empty());
    assert_eq!(c.phase(), PlayPhase::Player);
    assert_eq!(c.shown(Color::White, t0 + 105 * SEC), 285 * SEC);
}

#[test]
fn pausing_while_the_engine_thinks_stops_the_search_and_resume_asks_again() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    play_ucis(&mut c, &["e2e4"], t0);
    let out = c.pause(t0 + 20 * SEC);
    assert!(has_stop(&out));
    assert_eq!(c.shown(Color::Black, t0 + 99 * SEC), 280 * SEC);
    let out = c.resume(t0 + 100 * SEC);
    assert_eq!(out[0], PlayOutput::ClearOpponentHash);
    let t = thinks(&out);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].moves.len(), 1);
    assert_eq!(c.phase(), PlayPhase::Opponent);
    // The clock of Black resumed at 100 s: its time at 110 s is 270 s.
    let ThinkLimits::Clock(clock) = t[0].limits else {
        panic!("clock");
    };
    assert_eq!(clock.black_time, 280 * SEC);
    assert_eq!(c.shown(Color::Black, t0 + 110 * SEC), 270 * SEC);
}

#[test]
fn pausing_twice_or_resuming_a_running_game_does_nothing() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    assert!(c.resume(t0).is_empty());
    c.pause(t0 + 5 * SEC);
    assert!(c.pause(t0 + 50 * SEC).is_empty());
    assert_eq!(
        c.shown(Color::White, t0 + 50 * SEC),
        295 * SEC,
        "the second pause does not charge more"
    );
}

#[test]
fn a_paused_game_can_still_be_resigned() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    c.pause(t0);
    let out = c.resign(t0);
    assert!(matches!(ended(&out), Some(PlayEnd::Resignation { .. })));
}

// ---- a lost opponent (D1239) ----

#[test]
fn a_lost_opponent_pauses_with_a_notice_and_resume_does_not_work() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    play_ucis(&mut c, &["e2e4"], t0);
    let out = c.opponent_lost(t0 + 10 * SEC);
    assert_eq!(out, [PlayOutput::Notice(PlayRemark::OpponentLost)]);
    assert_eq!(c.phase(), PlayPhase::Paused { lost: true });
    assert!(c.is_live());
    assert_eq!(c.result(), GameResult::Unknown);
    // The clock of the engine does not run while it is gone.
    assert_eq!(c.shown(Color::Black, t0 + 500 * SEC), 290 * SEC);
    assert!(c.tick(t0 + 500 * SEC).is_empty());
    let out = c.resume(t0 + 20 * SEC);
    assert_eq!(out, [PlayOutput::Notice(PlayRemark::OpponentLost)]);
    assert_eq!(c.phase(), PlayPhase::Paused { lost: true });
}

#[test]
fn another_engine_takes_over_updates_the_tag_and_asks_for_the_move_again() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    play_ucis(&mut c, &["e2e4"], t0);
    c.opponent_lost(t0 + 10 * SEC);
    let other = mychess_app::OpponentInfo {
        name: "Other 2".to_owned(),
        rating: None,
    };
    let out = c.replace_opponent(other.clone(), t0 + 30 * SEC);
    assert_eq!(c.opponent(), &other);
    assert_eq!(c.game().tags().black, "Other 2");
    assert_eq!(c.game().tags().white, "Ana");
    assert_eq!(thinks(&out).len(), 1);
    assert_eq!(c.phase(), PlayPhase::Opponent);
    // The time of the dead engine's pause was not charged to anyone.
    assert_eq!(c.shown(Color::Black, t0 + 30 * SEC), 290 * SEC);
}

#[test]
fn the_tag_of_the_engine_playing_white_is_the_white_one() {
    let (mut c, _, t0) = begin(setup(Color::Black, blitz()));
    c.opponent_lost(t0);
    c.replace_opponent(
        mychess_app::OpponentInfo {
            name: "Other 2".to_owned(),
            rating: None,
        },
        t0,
    );
    assert_eq!(c.game().tags().white, "Other 2");
    assert_eq!(c.game().tags().black, "Ana");
}

#[test]
fn losing_the_engine_on_the_turn_of_the_player_waits_for_a_replacement() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    c.opponent_lost(t0 + 5 * SEC);
    assert_eq!(c.phase(), PlayPhase::Paused { lost: true });
    let out = c.replace_opponent(
        mychess_app::OpponentInfo {
            name: "B".to_owned(),
            rating: None,
        },
        t0 + 50 * SEC,
    );
    assert!(thinks(&out).is_empty());
    assert_eq!(c.phase(), PlayPhase::Player);
    assert_eq!(c.shown(Color::White, t0 + 60 * SEC), 285 * SEC);
}

#[test]
fn replacing_an_opponent_that_is_not_lost_does_nothing() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    let out = c.replace_opponent(
        mychess_app::OpponentInfo {
            name: "B".to_owned(),
            rating: None,
        },
        t0,
    );
    assert!(out.is_empty());
    assert_eq!(c.opponent().name, "Fake 1");
}

#[test]
fn losing_the_opponent_of_a_finished_game_is_ignored() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    c.resign(t0);
    assert!(c.opponent_lost(t0).is_empty());
    assert!(matches!(c.phase(), PlayPhase::Ended(_)));
}

#[test]
fn an_opponent_lost_during_a_pause_marks_the_pause_as_lost() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    c.pause(t0);
    c.opponent_lost(t0);
    assert_eq!(c.phase(), PlayPhase::Paused { lost: true });
}

// ---- colours (D1222) ----

#[rstest]
#[case(ColorChoice::White, 0, Color::White)]
#[case(ColorChoice::White, 1, Color::White)]
#[case(ColorChoice::Black, 0, Color::Black)]
#[case(ColorChoice::Black, 1, Color::Black)]
#[case(ColorChoice::Random, 0, Color::White)]
#[case(ColorChoice::Random, 1, Color::Black)]
#[case(ColorChoice::Random, u64::MAX, Color::Black)]
#[case(ColorChoice::Random, u64::MAX - 1, Color::White)]
fn the_colour_choice_resolves_with_the_roll(
    #[case] choice: ColorChoice,
    #[case] roll: u64,
    #[case] expected: Color,
) {
    assert_eq!(choice.resolve(roll), expected);
}

proptest! {
    #[test]
    fn a_random_colour_always_gives_both_colours_over_consecutive_rolls(roll in any::<u64>()) {
        let a = ColorChoice::Random.resolve(roll);
        let b = ColorChoice::Random.resolve(roll.wrapping_add(1));
        prop_assert_ne!(a, b);
    }
}

#[test]
fn the_player_with_black_sees_the_engine_move_first_with_a_running_clock() {
    let (c, out, t0) = begin(setup(Color::Black, blitz()));
    assert_eq!(c.player_color(), Color::Black);
    assert_eq!(c.phase(), PlayPhase::Opponent);
    assert_eq!(thinks(&out).len(), 1);
    assert_eq!(c.shown(Color::White, t0 + 7 * SEC), 293 * SEC);
    assert_eq!(c.shown(Color::Black, t0 + 7 * SEC), 300 * SEC);
}

// ---- the start position (D1236) ----

#[test]
fn the_current_position_is_the_start_of_the_game_and_of_the_request() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3";
    let (mut c, _, t0) = begin(fen_setup(Color::White, TimeMode::Depth(2), fen));
    assert_eq!(c.game().start_position().fen(), fen);
    assert_eq!(c.position().fen(), fen);
    let out = c.player_move(uci(&c, "f1b5"), t0);
    let t = thinks(&out);
    assert_eq!(t[0].start, Position::from_fen(fen).unwrap());
    assert_eq!(t[0].moves.len(), 1);
}

#[test]
fn a_position_with_black_to_move_starts_the_clock_of_black() {
    let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
    let (c, out, t0) = begin(fen_setup(Color::White, blitz(), fen));
    // The engine plays Black: it thinks at once.
    assert_eq!(thinks(&out).len(), 1);
    assert_eq!(thinks(&out)[0].side, Color::Black);
    assert_eq!(c.shown(Color::Black, t0 + 5 * SEC), 295 * SEC);
    assert_eq!(c.shown(Color::White, t0 + 5 * SEC), 300 * SEC);
}

#[test]
fn a_black_player_with_black_to_move_in_the_start_position_moves_first() {
    let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
    let (c, _, _) = begin(fen_setup(Color::Black, TimeMode::Depth(1), fen));
    assert_eq!(c.phase(), PlayPhase::Player);
}

#[test]
fn an_illegal_opening_line_is_refused() {
    let mut s = setup(Color::White, TimeMode::Depth(1));
    s.config.start = mychess_app::StartFrom::Opening(
        mychess_app::OpeningLine::parse("Bad: e4 e4").expect("parses"),
    );
    assert_eq!(
        mychess_app::PlayController::new(s, std::time::Instant::now()).err(),
        Some(PlayError::InvalidOpening)
    );
}
