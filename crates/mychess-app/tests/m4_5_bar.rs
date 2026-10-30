//! M4-5 contract tests of the evaluation bar source (D452, D824, rules 4,
//! 6, 7 of the spec; Scid `updateMainEvalBar`, `board.tcl` 3335-3346).
#![allow(
    clippy::assert_is_empty,
    clippy::bool_assert_comparison,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp
)]

mod m4_5_common;

use m4_5_common::*;
use mychess_app::engines::EngineMessage;
use mychess_app::{
    Action, AnalysisMessage, AppState, EvalBarMessage, EvalSource, EvalValue, Message, Nav,
    PrefChange, TerminalResult,
};
use mychess_core::{Game, Position};
use mychess_engine::{Bound, EngineEvent, ExitInfo, ExitReason, Score};
use rstest::rstest;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn bar_on(state: &mut AppState) {
    set_eval(state, |p| p.show = true);
}

fn fill_of(state: &AppState) -> Option<f32> {
    state.eval_bar_view().fill
}

/// An analysing engine with the bar on and the line 1 of the start position
/// at `cp` (search 1).
fn engine_at(cp: i32) -> (AppState, mychess_app::engines::RunId) {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, cp, &["e2e4"]);
    (state, run)
}

#[test]
fn a_hidden_bar_is_empty_even_with_an_engine_reporting() {
    let (mut state, run) = analysing();
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, 100, &["e2e4"]);
    let v = state.eval_bar_view();
    assert_eq!(v.fill, None);
    assert_eq!(v.value, None);
    assert!(v.label.is_none());
}

#[test]
fn bar_defaults_off_scale_5_linear_without_ticks_with_label() {
    let state = AppState::default();
    let p = eval_prefs(&state);
    assert!(!p.show && !p.log && !p.ticks && p.label);
    assert_eq!(p.scale, 5);
    let a = arrow_prefs(&state);
    assert!(!a.enabled && a.number_lines && a.click_to_play);
    assert_eq!(a.sequence_len, 3);
    assert!(!state.prefs.engine_view.traffic_lights.show);
}

#[test]
fn with_nothing_to_show_the_bar_is_an_empty_outline() {
    let mut state = AppState::default();
    bar_on(&mut state);
    let v = state.eval_bar_view();
    assert_eq!(v.source, EvalSource::None);
    assert!(v.value.is_none() && v.fill.is_none() && v.label.is_none());
}

#[rstest]
#[case("[%eval 0.35]", 0.35)]
#[case("[%eval -1.20]", -1.2)]
#[case("[%eval +2.5]", 2.5)]
#[case("+0.35", 0.35)]
#[case("good move -0.50 maybe", -0.5)]
fn a_comment_score_feeds_the_bar_without_an_engine(#[case] text: &str, #[case] pawns: f32) {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    comment(&mut state, text);
    bar_on(&mut state);
    let v = state.eval_bar_view();
    assert_eq!(v.source, EvalSource::Comment);
    let Some(EvalValue::Pawns(got)) = v.value else {
        panic!("value {:?}", v.value);
    };
    assert!(close(got, pawns), "{got} != {pawns}");
    assert!(!v.stale);
    let want = (5.0 + pawns) / 10.0;
    assert!(close(v.fill.unwrap(), want));
}

#[rstest]
#[case("[%eval #3]", 327.0, "#", 1.0)]
#[case("[%eval #-3]", -327.0, "-#", 0.0)]
fn a_mate_in_a_comment_is_327_with_a_hash_label(
    #[case] text: &str,
    #[case] pawns: f32,
    #[case] label: &str,
    #[case] fill: f32,
) {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    comment(&mut state, text);
    bar_on(&mut state);
    let v = state.eval_bar_view();
    assert_eq!(v.source, EvalSource::Comment);
    assert_eq!(v.value, Some(EvalValue::Pawns(pawns)));
    assert_eq!(v.label.as_deref(), Some(label));
    assert!(close(v.fill.unwrap(), fill));
}

#[test]
fn the_bar_empties_on_moves_without_a_score_and_follows_navigation() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    comment(&mut state, "[%eval 1.00]");
    play(&mut state, "e7e5");
    bar_on(&mut state);
    assert_eq!(fill_of(&state), None, "1...e5 has no score");
    nav(&mut state, Nav::Back(1));
    assert!(close(fill_of(&state).unwrap(), 0.6));
    nav(&mut state, Nav::Forward(1));
    assert_eq!(fill_of(&state), None);
    nav(&mut state, Nav::Start);
    assert_eq!(fill_of(&state), None, "the start position has no comment");
}

#[test]
fn editing_the_comment_updates_the_bar() {
    let mut state = AppState::default();
    play(&mut state, "e2e4");
    bar_on(&mut state);
    assert_eq!(fill_of(&state), None);
    comment(&mut state, "[%eval -5]");
    assert!(close(fill_of(&state).unwrap(), 0.0));
    comment(&mut state, "");
    assert_eq!(fill_of(&state), None);
}

#[test]
fn the_main_engine_line_1_wins_over_the_comment() {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    comment(&mut state, "[%eval -4.00]");
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, 100, &["e2e4"]);
    let v = state.eval_bar_view();
    assert_eq!(v.source, EvalSource::Engine(run));
    assert_eq!(v.value, Some(EvalValue::Pawns(1.0)));
    assert!(close(v.fill.unwrap(), 0.6));
    assert_eq!(v.label.as_deref(), Some("+1.00"));
    assert!(!v.stale);
}

#[test]
fn the_bar_follows_every_report_in_real_time() {
    let (mut state, run) = engine_at(100);
    for (cp, want) in [(-100, 0.4), (250, 0.75), (0, 0.5), (-700, 0.0)] {
        line(&mut state, run, 1, 1, cp, &["e2e4"]);
        assert!(close(fill_of(&state).unwrap(), want), "{cp}");
    }
}

#[test]
fn the_bar_uses_line_1_not_the_other_lines() {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    started(&mut state, run, 1);
    line(&mut state, run, 1, 2, -300, &["d2d4"]);
    line(&mut state, run, 1, 1, 100, &["e2e4"]);
    line(&mut state, run, 1, 3, -500, &["g1f3"]);
    assert!(close(fill_of(&state).unwrap(), 0.6));
}

#[test]
fn score_is_from_whites_point_of_view_when_black_is_to_move() {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    play(&mut state, "e2e4");
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, 300, &["e7e5"]);
    assert!(fill_of(&state).unwrap() > 0.5);
    assert_eq!(state.eval_bar_view().label.as_deref(), Some("+3.00"));
}

#[test]
fn engine_mate_scores_show_the_distance() {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    started(&mut state, run, 1);
    let pos = here(&state);
    info(
        &mut state,
        run,
        report_score(&pos, 1, 1, 20, Score::Mate(3), Bound::Exact, &["e2e4"]),
    );
    let v = state.eval_bar_view();
    assert_eq!(v.value, Some(EvalValue::Mate(3)));
    assert_eq!(v.label.as_deref(), Some("+M3"));
    assert!(close(v.fill.unwrap(), 1.0) && v.label_on_white);
    info(
        &mut state,
        run,
        report_score(&pos, 1, 1, 21, Score::Mate(-2), Bound::Exact, &["e2e4"]),
    );
    let v = state.eval_bar_view();
    assert_eq!(v.value, Some(EvalValue::Mate(-2)));
    assert_eq!(v.label.as_deref(), Some("-M2"));
    assert!(close(v.fill.unwrap(), 0.0) && !v.label_on_white);
}

#[test]
fn bounds_keep_the_value_and_decorate_the_label() {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    started(&mut state, run, 1);
    let pos = here(&state);
    for (bound, label) in [
        (Bound::Lower, "\u{2265}+0.50"),
        (Bound::Upper, "\u{2264}+0.50"),
        (Bound::Exact, "+0.50"),
    ] {
        info(
            &mut state,
            run,
            report_score(&pos, 1, 1, 12, Score::Cp(50), bound, &["e2e4"]),
        );
        let v = state.eval_bar_view();
        assert_eq!(v.bound, bound);
        assert_eq!(v.label.as_deref(), Some(label));
        assert!(close(v.fill.unwrap(), 0.55));
    }
}

#[test]
fn label_side_follows_who_is_ahead() {
    let (mut state, run) = engine_at(200);
    assert!(state.eval_bar_view().label_on_white);
    line(&mut state, run, 1, 1, -200, &["e2e4"]);
    assert!(!state.eval_bar_view().label_on_white);
}

#[test]
fn label_can_be_turned_off_and_ticks_on() {
    let (mut state, _run) = engine_at(100);
    set_eval(&mut state, |p| p.label = false);
    assert!(state.eval_bar_view().label.is_none());
    assert!(state.eval_bar_view().ticks.is_empty());
    set_eval(&mut state, |p| p.ticks = true);
    assert_eq!(state.eval_bar_view().ticks.len(), 9);
    set_eval(&mut state, |p| p.log = true);
    assert_eq!(state.eval_bar_view().ticks.len(), 9);
}

#[test]
fn scale_and_log_changes_redraw_immediately() {
    let (mut state, _run) = engine_at(100);
    assert!(close(fill_of(&state).unwrap(), 0.6));
    set_eval(&mut state, |p| p.scale = 10);
    assert!(close(fill_of(&state).unwrap(), 0.55));
    set_eval(&mut state, |p| p.scale = 1);
    assert!(close(fill_of(&state).unwrap(), 1.0));
    set_eval(&mut state, |p| p.log = true);
    assert!(close(fill_of(&state).unwrap(), 0.625));
}

#[test]
fn flipping_the_board_flips_the_bar_but_not_the_fill() {
    let (mut state, _run) = engine_at(100);
    assert!(!state.eval_bar_view().flipped);
    let before = fill_of(&state);
    msg(&mut state, Action::FlipBoard);
    assert!(state.board_flipped);
    assert!(state.eval_bar_view().flipped);
    assert_eq!(fill_of(&state), before);
    msg(&mut state, Action::FlipBoard);
    assert!(!state.eval_bar_view().flipped);
}

#[test]
fn an_empty_bar_also_knows_the_orientation() {
    let mut state = AppState::default();
    bar_on(&mut state);
    msg(&mut state, Action::FlipBoard);
    assert!(state.eval_bar_view().flipped);
    assert_eq!(fill_of(&state), None);
}

#[test]
fn a_stopped_analysis_returns_to_the_comment() {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    comment(&mut state, "[%eval -2.00]");
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, 300, &["e2e4"]);
    assert_eq!(state.eval_bar_view().source, EvalSource::Engine(run));
    msg(&mut state, AnalysisMessage::Toggle(run));
    let v = state.eval_bar_view();
    assert_eq!(v.source, EvalSource::Comment);
    assert!(close(v.fill.unwrap(), 0.3), "not the last engine score");
    assert!(!v.stale);
}

#[test]
fn a_stopped_analysis_without_comment_empties_the_bar() {
    let (mut state, run) = engine_at(300);
    msg(&mut state, AnalysisMessage::Toggle(run));
    assert_eq!(fill_of(&state), None);
    assert_eq!(state.eval_bar_view().source, EvalSource::None);
}

fn crash(state: &mut AppState, run: mychess_app::engines::RunId) {
    event(
        state,
        run,
        EngineEvent::Exited(ExitInfo {
            reason: ExitReason::Crashed,
            status: None,
            stderr_tail: vec!["boom".into()],
        }),
    );
}

#[test]
fn a_failed_engine_returns_to_the_comment_and_never_shows_its_last_score() {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    comment(&mut state, "[%eval 1.50]");
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, -400, &["e2e4"]);
    assert!(close(fill_of(&state).unwrap(), 0.1));
    crash(&mut state, run);
    let v = state.eval_bar_view();
    assert_eq!(v.source, EvalSource::Comment);
    assert!(close(v.fill.unwrap(), 0.65));
}

#[test]
fn a_failed_engine_without_comment_leaves_an_empty_bar() {
    let (mut state, run) = engine_at(-400);
    crash(&mut state, run);
    assert_eq!(fill_of(&state), None);
}

#[test]
fn ending_the_run_returns_to_the_comment() {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    comment(&mut state, "[%eval 0.50]");
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, 400, &["e2e4"]);
    msg(&mut state, Message::Engine(EngineMessage::Stop(run)));
    assert_eq!(state.eval_bar_view().source, EvalSource::Comment);
}

#[test]
fn a_new_position_fades_the_old_value_until_the_first_report() {
    let (mut state, run) = engine_at(100);
    assert!(!state.eval_bar_view().stale);
    play(&mut state, "e2e4");
    {
        let v = state.eval_bar_view();
        assert_eq!(v.source, EvalSource::Engine(run), "no blinking to empty");
        assert!(v.stale, "the value is about 1.e4's predecessor");
        assert!(close(v.fill.unwrap(), 0.6));
    }
    started(&mut state, run, 2);
    assert!(
        state.eval_bar_view().stale,
        "still nothing for the new position"
    );
    assert!(close(fill_of(&state).unwrap(), 0.6));
    line(&mut state, run, 2, 1, -50, &["e7e5"]);
    let v = state.eval_bar_view();
    assert!(!v.stale);
    assert!(close(v.fill.unwrap(), 0.45));
}

#[test]
fn going_back_to_an_analysed_position_is_stale_until_the_engine_reports_again() {
    let (mut state, run) = engine_at(100);
    play(&mut state, "e2e4");
    started(&mut state, run, 2);
    line(&mut state, run, 2, 1, -50, &["e7e5"]);
    nav(&mut state, Nav::Back(1));
    assert!(state.eval_bar_view().stale);
    started(&mut state, run, 3);
    line(&mut state, run, 3, 1, 90, &["e2e4"]);
    assert!(!state.eval_bar_view().stale);
}

#[test]
fn reports_of_an_old_search_do_not_unfade_the_bar() {
    let (mut state, run) = engine_at(100);
    play(&mut state, "e2e4");
    started(&mut state, run, 2);
    // A late report of search 1, about the start position.
    let start = Position::new();
    info(&mut state, run, report(&start, 1, 1, 12, 777, &["e2e4"]));
    assert!(state.eval_bar_view().stale);
}

#[test]
fn a_locked_engine_does_not_feed_the_bar_away_from_its_position() {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    play(&mut state, "e2e4");
    comment(&mut state, "[%eval -1.00]");
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, 300, &["e7e5"]);
    assert_eq!(state.eval_bar_view().source, EvalSource::Engine(run));
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    nav(&mut state, Nav::Back(1));
    let v = state.eval_bar_view();
    assert_ne!(
        v.source,
        EvalSource::Engine(run),
        "cursor left the locked position"
    );
    assert_eq!(v.fill, None, "the start has no comment");
    nav(&mut state, Nav::Forward(1));
    let v = state.eval_bar_view();
    assert_eq!(
        v.source,
        EvalSource::Engine(run),
        "back at the locked position"
    );
    assert!(close(v.fill.unwrap(), 0.8));
    assert!(!v.stale);
}

#[test]
fn a_locked_engine_falls_back_to_the_comment_of_the_other_position() {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    play(&mut state, "e2e4");
    comment(&mut state, "[%eval 2.00]");
    play(&mut state, "e7e5");
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, -600, &["g1f3"]);
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    nav(&mut state, Nav::Back(1));
    let v = state.eval_bar_view();
    assert_eq!(v.source, EvalSource::Comment);
    assert!(close(v.fill.unwrap(), 0.7));
}

fn terminal_state(moves: &[&str]) -> (AppState, mychess_app::engines::RunId) {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    started(&mut state, run, 1);
    play_all(&mut state, moves);
    (state, run)
}

#[test]
fn checkmate_shows_the_result_from_the_engine_with_label_m0() {
    // Fool's mate: Black wins.
    let (state, run) = terminal_state(&["f2f3", "e7e5", "g2g4", "d8h4"]);
    let v = state.eval_bar_view();
    assert_eq!(v.source, EvalSource::Engine(run));
    assert_eq!(
        v.value,
        Some(EvalValue::Terminal(TerminalResult::BlackWins))
    );
    assert!(close(v.fill.unwrap(), 0.0));
    assert_eq!(v.label.as_deref(), Some("M0"));
}

#[test]
fn checkmate_of_black_fills_the_bar_for_white() {
    // Scholar's mate: White wins.
    let (state, _run) = terminal_state(&["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6", "h5f7"]);
    let v = state.eval_bar_view();
    assert_eq!(
        v.value,
        Some(EvalValue::Terminal(TerminalResult::WhiteWins))
    );
    assert!(close(v.fill.unwrap(), 1.0));
}

#[test]
fn stalemate_is_half_and_half_with_an_equals_label() {
    let (mut state, run) = analysing();
    bar_on(&mut state);
    started(&mut state, run, 1);
    let stalemate = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    state.session.replace_game(Game::from_position(stalemate));
    // The cursor has to be told: any navigation refreshes the engine target.
    nav(&mut state, Nav::Start);
    let v = state.eval_bar_view();
    assert_eq!(v.value, Some(EvalValue::Terminal(TerminalResult::Draw)));
    assert!(close(v.fill.unwrap(), 0.5));
    assert_eq!(v.label.as_deref(), Some("="));
}

#[test]
fn a_terminal_position_takes_the_result_only_from_the_engine() {
    let mut state = AppState::default();
    bar_on(&mut state);
    play_all(&mut state, &["f2f3", "e7e5", "g2g4", "d8h4"]);
    assert_eq!(fill_of(&state), None, "no engine and no comment: empty");
    comment(&mut state, "[%eval -3.00]");
    let v = state.eval_bar_view();
    assert_eq!(v.source, EvalSource::Comment);
    assert!(matches!(v.value, Some(EvalValue::Pawns(_))));
}

#[test]
fn only_the_main_engine_feeds_the_bar() {
    let (mut state, a, b) = two_runs();
    bar_on(&mut state);
    started(&mut state, a, 1);
    started(&mut state, b, 1);
    line(&mut state, b, 1, 1, -300, &["e2e4"]);
    // The main engine (first) has nothing yet: the second must not be shown.
    assert_ne!(state.eval_bar_view().source, EvalSource::Engine(b));
    line(&mut state, a, 1, 1, 100, &["e2e4"]);
    assert_eq!(state.eval_bar_view().source, EvalSource::Engine(a));
    assert!(close(fill_of(&state).unwrap(), 0.6));
    line(&mut state, b, 1, 1, -400, &["e2e4"]);
    assert!(close(fill_of(&state).unwrap(), 0.6), "B is ignored");
}

#[test]
fn when_the_main_engine_stops_the_next_one_becomes_the_source() {
    let (mut state, a, b) = two_runs();
    bar_on(&mut state);
    started(&mut state, a, 1);
    started(&mut state, b, 1);
    line(&mut state, a, 1, 1, 100, &["e2e4"]);
    line(&mut state, b, 1, 1, -300, &["e2e4"]);
    msg(&mut state, AnalysisMessage::Toggle(a));
    let v = state.eval_bar_view();
    assert_eq!(v.source, EvalSource::Engine(b));
    assert!(close(v.fill.unwrap(), 0.2));
}

#[test]
fn a_report_that_changes_nothing_leaves_the_view_equal_and_sends_nothing() {
    let (mut state, run) = engine_at(100);
    let before = state.eval_bar_view().clone();
    let pos = here(&state);
    for _ in 0..50 {
        let fx = {
            let r = report(&pos, 1, 1, 12, 100, &["e2e4"]);
            let mut fx = Vec::new();
            fx.extend(event_fx(&mut state, run, r));
            fx
        };
        assert!(
            fx.iter()
                .all(|e| !matches!(e, mychess_app::Effect::EngineCommand { .. })),
            "no engine command for a repeated report"
        );
    }
    assert_eq!(state.eval_bar_view(), &before);
}

fn event_fx(
    state: &mut AppState,
    run: mychess_app::engines::RunId,
    info: mychess_engine::SearchInfo,
) -> Vec<mychess_app::Effect> {
    event(state, run, EngineEvent::Info(info))
}

#[test]
fn deeper_report_with_the_same_score_keeps_the_same_fill() {
    let (mut state, run) = engine_at(100);
    let pos = here(&state);
    info(&mut state, run, report(&pos, 1, 1, 30, 100, &["e2e4"]));
    assert!(close(fill_of(&state).unwrap(), 0.6));
}

#[test]
fn toggling_the_bar_off_empties_the_view_and_on_rebuilds_it() {
    let (mut state, _run) = engine_at(100);
    bar(&mut state, EvalBarMessage::ToggleBar);
    assert!(!eval_prefs(&state).show);
    assert_eq!(fill_of(&state), None);
    bar(&mut state, EvalBarMessage::ToggleBar);
    assert!(eval_prefs(&state).show);
    assert!(close(fill_of(&state).unwrap(), 0.6));
}

#[test]
fn hostile_hammering_of_reports_navigation_and_prefs_does_not_panic() {
    let (mut state, run) = analysing();
    enable_all(&mut state);
    started(&mut state, run, 1);
    play_all(&mut state, &["e2e4", "e7e5", "g1f3"]);
    for i in 0..40i32 {
        line_any(
            &mut state,
            run,
            1 + u64::from(i.unsigned_abs() % 3),
            1,
            i * 97 - 1500,
        );
        nav(
            &mut state,
            if i % 2 == 0 {
                Nav::Back(1)
            } else {
                Nav::Forward(1)
            },
        );
        set_eval(&mut state, |p| p.scale = u8::try_from(i % 14).unwrap());
        msg(&mut state, Action::FlipBoard);
        let _ = state.eval_bar_view();
        let _ = state.engine_arrows();
        let _ = state.traffic_lights();
    }
    let _ = PrefChange::TrafficLights;
}
