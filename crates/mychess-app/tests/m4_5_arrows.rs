//! M4-5 contract tests of the PV arrows (D456, D457, D821), the redraw rule of
//! the sequence mode (Scid `analysis.tcl` 3896-3900) and the MultiPV traffic
//! lights (D458, D822). The spec and `analysis.tcl` 3799-3921 are the oracle.
#![allow(
    clippy::assert_is_empty,
    clippy::bool_assert_comparison,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp
)]

mod m4_5_common;

use m4_5_common::*;
use mychess_app::engines::RunId;
use mychess_app::{
    AnalysisMessage, AppState, ArrowKind, ArrowMode, EvalBarMessage, Nav, SequenceSide,
    TrafficLightPrefs, traffic_light_color,
};
use mychess_core::{Color, Move, Position};
use mychess_engine::{Bound, Score};

const E4_E5_NF3: [&str; 3] = ["e2e4", "e7e5", "g1f3"];

fn arrows_on(state: &mut AppState) {
    set_arrows(state, |p| p.enabled = true);
}

fn lights_on(state: &mut AppState) {
    pref(
        state,
        mychess_app::PrefChange::TrafficLights(TrafficLightPrefs { show: true }),
    );
}

/// Engine analysing the start position with these first moves as lines.
fn arrows_with(firsts: &[(i32, &str)]) -> (AppState, RunId) {
    let n = u8::try_from(firsts.len()).unwrap().max(1);
    let (mut state, run) = with_lines(n, firsts);
    arrows_on(&mut state);
    (state, run)
}

fn labels(state: &AppState) -> Vec<Option<u8>> {
    state.engine_arrows().iter().map(|a| a.label).collect()
}

fn sequence_state(pv: &[&str], len: u8) -> (AppState, RunId) {
    let (mut state, run) = analysing();
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 1));
    started(&mut state, run, 1);
    let pos = here(&state);
    info(&mut state, run, report(&pos, 1, 1, 10, 30, pv));
    set_arrows(&mut state, |p| {
        p.enabled = true;
        p.mode = ArrowMode::Sequence;
        p.sequence_len = len;
    });
    (state, run)
}

/// The message of a click on the arrow of line `line` that is on the board
/// now; with no such arrow, a click on a1-a1 (which is never an arrow).
fn click(state: &AppState, run: RunId, line: u8) -> EvalBarMessage {
    let a1 = "a1".parse().unwrap();
    let (from, to) = state
        .engine_arrows()
        .iter()
        .find(|a| a.rank == line)
        .map_or((a1, a1), |a| (a.from, a.to));
    EvalBarMessage::PlayArrow {
        run,
        line,
        from,
        to,
    }
}

const PV5: [&str; 5] = ["e2e4", "e7e5", "g1f3", "b8c6", "f1c4"];

// ---------------------------------------------------------------- which engine

#[test]
fn arrows_are_off_by_default_even_with_lines() {
    let (state, _run) = with_lines(2, &[(30, "e2e4"), (20, "d2d4")]);
    assert!(state.engine_arrows().is_empty());
    assert_eq!(state.arrows_engine(), None);
}

#[test]
fn enabling_without_marking_uses_the_main_engine() {
    let (mut state, run) = with_lines(1, &[(30, "e2e4")]);
    arrows_on(&mut state);
    assert_eq!(state.arrows_engine(), Some(run));
    assert_eq!(arrow_texts(&state), ["e2e4"]);
}

#[test]
fn the_arrows_button_marks_and_unmarks_the_run() {
    let (mut state, run) = with_lines(1, &[(30, "e2e4")]);
    bar(&mut state, EvalBarMessage::ToggleArrows(run));
    assert!(arrow_prefs(&state).enabled);
    assert_eq!(state.arrows_engine(), Some(run));
    assert_eq!(arrow_texts(&state), ["e2e4"]);
    bar(&mut state, EvalBarMessage::ToggleArrows(run));
    assert!(!arrow_prefs(&state).enabled);
    assert_eq!(state.arrows_engine(), None);
    assert!(state.engine_arrows().is_empty());
}

#[test]
fn the_last_engine_marked_wins_and_the_others_are_ignored() {
    let (mut state, a, b) = two_runs();
    started(&mut state, a, 1);
    started(&mut state, b, 1);
    line(&mut state, a, 1, 1, 30, &["e2e4"]);
    line(&mut state, b, 1, 1, 30, &["d2d4"]);
    bar(&mut state, EvalBarMessage::ToggleArrows(a));
    assert_eq!(arrow_texts(&state), ["e2e4"]);
    bar(&mut state, EvalBarMessage::ToggleArrows(b));
    assert_eq!(state.arrows_engine(), Some(b));
    assert_eq!(arrow_texts(&state), ["d2d4"]);
    bar(&mut state, EvalBarMessage::ToggleArrows(b));
    assert_eq!(state.arrows_engine(), None);
}

#[test]
fn marking_a_run_that_does_not_exist_is_ignored() {
    let (mut state, _run) = with_lines(1, &[(30, "e2e4")]);
    bar(&mut state, EvalBarMessage::ToggleArrows(RunId(9999)));
    assert!(!arrow_prefs(&state).enabled);
    assert_eq!(state.arrows_engine(), None);
}

#[test]
fn a_marked_engine_that_stops_hands_the_arrows_to_the_main_engine() {
    let (mut state, a, b) = two_runs();
    started(&mut state, a, 1);
    started(&mut state, b, 1);
    line(&mut state, a, 1, 1, 30, &["e2e4"]);
    line(&mut state, b, 1, 1, 30, &["d2d4"]);
    bar(&mut state, EvalBarMessage::ToggleArrows(b));
    msg(&mut state, AnalysisMessage::Toggle(b));
    assert_eq!(state.arrows_engine(), Some(a));
    assert_eq!(arrow_texts(&state), ["e2e4"]);
}

#[test]
fn a_failed_engine_draws_nothing() {
    let (mut state, run) = arrows_with(&[(30, "e2e4")]);
    event(
        &mut state,
        run,
        mychess_engine::EngineEvent::Exited(mychess_engine::ExitInfo {
            reason: mychess_engine::ExitReason::Crashed,
            status: None,
            stderr_tail: Vec::new(),
        }),
    );
    assert!(state.engine_arrows().is_empty());
    assert!(state.traffic_lights().is_empty());
}

// ----------------------------------------------------------------- Arrows mode

#[test]
fn one_line_gives_a_single_best_arrow_without_number() {
    let (state, _run) = arrows_with(&[(30, "e2e4")]);
    let a = state.engine_arrows();
    assert_eq!(a.len(), 1);
    assert_eq!(
        (a[0].kind, a[0].label, a[0].rank),
        (ArrowKind::Best, None, 1)
    );
    assert_eq!(a[0].side, Color::White);
    assert_eq!(a[0].from.to_string(), "e2");
    assert_eq!(a[0].to.to_string(), "e4");
}

#[test]
fn two_lines_are_not_numbered() {
    let (state, _run) = arrows_with(&[(30, "e2e4"), (20, "d2d4")]);
    assert_eq!(arrow_texts(&state), ["e2e4", "d2d4"]);
    assert_eq!(labels(&state), [None, None]);
    let kinds: Vec<_> = state.engine_arrows().iter().map(|a| a.kind).collect();
    assert_eq!(kinds, [ArrowKind::Best, ArrowKind::Other]);
}

#[test]
fn three_or_more_lines_number_the_secondary_arrows_from_2() {
    let (state, _run) = arrows_with(&[(30, "e2e4"), (20, "d2d4"), (10, "g1f3")]);
    assert_eq!(labels(&state), [None, Some(2), Some(3)]);
    let (state, _run) = arrows_with(&[
        (30, "e2e4"),
        (20, "d2d4"),
        (10, "g1f3"),
        (5, "c2c4"),
        (0, "b1c3"),
    ]);
    assert_eq!(labels(&state), [None, Some(2), Some(3), Some(4), Some(5)]);
    let ranks: Vec<_> = state.engine_arrows().iter().map(|a| a.rank).collect();
    assert_eq!(ranks, [1, 2, 3, 4, 5]);
}

#[test]
fn numbering_can_be_turned_off_by_the_action_and_the_preference() {
    let (mut state, _run) = arrows_with(&[(30, "e2e4"), (20, "d2d4"), (10, "g1f3")]);
    bar(&mut state, EvalBarMessage::ToggleArrowNumbers);
    assert!(!arrow_prefs(&state).number_lines);
    assert_eq!(labels(&state), [None, None, None]);
    set_arrows(&mut state, |p| p.number_lines = true);
    assert_eq!(labels(&state), [None, Some(2), Some(3)]);
}

#[test]
fn arrows_follow_the_line_rank_whatever_the_report_order() {
    let (mut state, run) = analysing();
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    arrows_on(&mut state);
    started(&mut state, run, 1);
    line(&mut state, run, 1, 3, 10, &["g1f3"]);
    line(&mut state, run, 1, 1, 30, &["e2e4"]);
    line(&mut state, run, 1, 2, 20, &["d2d4"]);
    assert_eq!(arrow_texts(&state), ["e2e4", "d2d4", "g1f3"]);
}

#[test]
fn a_missing_middle_line_keeps_the_numbering_of_the_line() {
    let (mut state, run) = analysing();
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 4));
    arrows_on(&mut state);
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, 30, &["e2e4"]);
    line(&mut state, run, 1, 3, 10, &["g1f3"]);
    line(&mut state, run, 1, 4, 5, &["c2c4"]);
    assert_eq!(arrow_texts(&state), ["e2e4", "g1f3", "c2c4"]);
    assert_eq!(labels(&state), [None, Some(3), Some(4)]);
}

#[test]
fn two_lines_with_the_same_first_move_draw_one_arrow() {
    let (state, _run) = arrows_with(&[(30, "e2e4"), (20, "e2e4")]);
    assert_eq!(arrow_texts(&state), ["e2e4"]);
}

#[test]
fn arrows_of_black_carry_the_side_to_move() {
    let (mut state, run) = analysing();
    arrows_on(&mut state);
    play(&mut state, "e2e4");
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, -20, &["e7e5"]);
    assert_eq!(arrow_texts(&state), ["e7e5"]);
    assert_eq!(state.engine_arrows()[0].side, Color::Black);
}

#[test]
fn castling_is_drawn_as_the_king_move_to_its_destination() {
    let (mut state, run) = analysing();
    arrows_on(&mut state);
    play_all(
        &mut state,
        &["e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "f8c5"],
    );
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, 30, &["e1g1"]);
    assert_eq!(arrow_texts(&state), ["e1g1"], "king from e1 to g1");
}

#[test]
fn castling_in_the_sequence_has_the_king_destination_for_both_sides() {
    let (mut state, run) = analysing();
    play_all(
        &mut state,
        &["e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "f8c5"],
    );
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, 30, &["e1g1", "g8f6", "d2d3", "e8g8"]);
    set_arrows(&mut state, |p| {
        p.enabled = true;
        p.mode = ArrowMode::Sequence;
        p.sequence_len = 8;
    });
    assert_eq!(arrow_texts(&state), ["e1g1", "g8f6", "d2d3", "e8g8"]);
}

#[test]
fn a_null_move_first_in_a_secondary_line_draws_nothing_and_does_not_panic() {
    let (mut state, run) = analysing();
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 2));
    arrows_on(&mut state);
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, 30, &["e2e4"]);
    let pos = here(&state);
    let mut r = report(&pos, 1, 2, 12, 20, &[]);
    r.pv = vec![Move::Null];
    info(&mut state, run, r);
    assert_eq!(arrow_texts(&state), ["e2e4"]);
}

#[test]
fn arrows_vanish_when_the_cursor_leaves_the_position_of_the_lines() {
    let (mut state, run) = arrows_with(&[(30, "e2e4"), (20, "d2d4")]);
    assert_eq!(state.engine_arrows().len(), 2);
    play(&mut state, "e2e4");
    assert!(
        state.engine_arrows().is_empty(),
        "old lines are about another position"
    );
    started(&mut state, run, 2);
    assert!(state.engine_arrows().is_empty());
    line(&mut state, run, 2, 1, -10, &["e7e5"]);
    assert_eq!(arrow_texts(&state), ["e7e5"]);
}

#[test]
fn a_locked_engine_draws_only_on_its_locked_position() {
    let (mut state, run) = analysing();
    enable_all(&mut state);
    play(&mut state, "e2e4");
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, -20, &["e7e5"]);
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    assert_eq!(arrow_texts(&state), ["e7e5"]);
    nav(&mut state, Nav::Back(1));
    assert!(state.engine_arrows().is_empty());
    assert!(state.traffic_lights().is_empty());
    nav(&mut state, Nav::Forward(1));
    assert_eq!(arrow_texts(&state), ["e7e5"]);
    assert_eq!(state.traffic_lights().len(), 1);
}

#[test]
fn flipping_the_board_does_not_change_the_arrows() {
    let (mut state, _run) = arrows_with(&[(30, "e2e4"), (20, "d2d4"), (10, "g1f3")]);
    let before = state.engine_arrows().to_vec();
    msg(&mut state, mychess_app::Action::FlipBoard);
    assert_eq!(state.engine_arrows(), before.as_slice());
}

#[test]
fn turning_arrows_off_clears_them_at_once() {
    let (mut state, _run) = arrows_with(&[(30, "e2e4")]);
    set_arrows(&mut state, |p| p.enabled = false);
    assert!(state.engine_arrows().is_empty());
}

// --------------------------------------------------------------- Sequence mode

#[test]
fn sequence_draws_the_first_k_moves_numbered_with_alternating_sides() {
    let (state, _run) = sequence_state(&PV5, 3);
    let a = state.engine_arrows();
    assert_eq!(arrow_texts(&state), ["e2e4", "e7e5", "g1f3"]);
    assert_eq!(labels(&state), [Some(1), Some(2), Some(3)]);
    let sides: Vec<_> = a.iter().map(|x| x.side).collect();
    assert_eq!(sides, [Color::White, Color::Black, Color::White]);
    let kinds: Vec<_> = a.iter().map(|x| x.kind).collect();
    assert_eq!(
        kinds,
        [
            ArrowKind::Sequence(0),
            ArrowKind::Sequence(1),
            ArrowKind::Sequence(2)
        ]
    );
}

#[test]
fn sequence_len_bounds_the_arrow_count() {
    for (len, want) in [(1u8, 1usize), (2, 2), (5, 5), (8, 5)] {
        let (state, _run) = sequence_state(&PV5, len);
        assert_eq!(state.engine_arrows().len(), want, "len {len}");
    }
}

#[test]
fn sequence_of_a_short_pv_is_not_padded() {
    let (state, _run) = sequence_state(&["e2e4"], 3);
    assert_eq!(arrow_texts(&state), ["e2e4"]);
}

#[test]
fn sequence_sides_white_black_and_both() {
    let (mut state, _run) = sequence_state(&PV5, 8);
    set_arrows(&mut state, |p| p.sequence_side = SequenceSide::White);
    assert_eq!(arrow_texts(&state), ["e2e4", "g1f3", "f1c4"]);
    assert_eq!(labels(&state), [Some(1), Some(2), Some(3)]);
    set_arrows(&mut state, |p| p.sequence_side = SequenceSide::Black);
    assert_eq!(arrow_texts(&state), ["e7e5", "b8c6"]);
    assert_eq!(labels(&state), [Some(1), Some(2)]);
    set_arrows(&mut state, |p| p.sequence_side = SequenceSide::Both);
    assert_eq!(state.engine_arrows().len(), 5);
}

#[test]
fn sequence_count_applies_to_the_filtered_moves() {
    // Scid `boardVisualizeMoves`: `mc` counts only the moves that pass the
    // submode filter.
    let (mut state, _run) = sequence_state(&PV5, 2);
    set_arrows(&mut state, |p| p.sequence_side = SequenceSide::White);
    assert_eq!(arrow_texts(&state), ["e2e4", "g1f3"]);
}

#[test]
fn cycling_the_sequence_side_visits_three_states_and_comes_back() {
    let (mut state, _run) = sequence_state(&PV5, 3);
    let mut seen = vec![arrow_prefs(&state).sequence_side];
    for _ in 0..3 {
        bar(&mut state, EvalBarMessage::CycleSequenceSide);
        seen.push(arrow_prefs(&state).sequence_side);
    }
    assert_eq!(seen[0], seen[3]);
    assert!(seen[0] != seen[1] && seen[1] != seen[2] && seen[0] != seen[2]);
}

#[test]
fn cycling_the_mode_switches_between_arrows_and_sequence() {
    let (mut state, _run) = arrows_with(&[(30, "e2e4"), (20, "d2d4")]);
    assert_eq!(arrow_prefs(&state).mode, ArrowMode::Arrows);
    bar(&mut state, EvalBarMessage::CycleArrowMode);
    assert_eq!(arrow_prefs(&state).mode, ArrowMode::Sequence);
    assert!(
        state
            .engine_arrows()
            .iter()
            .all(|a| matches!(a.kind, ArrowKind::Sequence(_)))
    );
    bar(&mut state, EvalBarMessage::CycleArrowMode);
    assert_eq!(arrow_prefs(&state).mode, ArrowMode::Arrows);
    assert_eq!(state.engine_arrows().len(), 2);
}

#[test]
fn sequence_uses_the_main_line_only() {
    let (mut state, run) = analysing();
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 2));
    started(&mut state, run, 1);
    let pos = here(&state);
    info(
        &mut state,
        run,
        report(&pos, 1, 2, 10, 5, &["d2d4", "d7d5"]),
    );
    info(
        &mut state,
        run,
        report(&pos, 1, 1, 10, 30, &["e2e4", "e7e5"]),
    );
    set_arrows(&mut state, |p| {
        p.enabled = true;
        p.mode = ArrowMode::Sequence;
    });
    assert_eq!(arrow_texts(&state), ["e2e4", "e7e5"]);
}

#[test]
fn sequence_leaves_the_null_move_out_but_keeps_the_sides() {
    let (mut state, run) = analysing();
    started(&mut state, run, 1);
    let pos = here(&state);
    let mut r = report(&pos, 1, 1, 10, 30, &["e2e4"]);
    // 1.e4 -- 2.d4: the null move is Black's, so the next one is White's.
    let mut after = pos.clone();
    after.play(r.pv[0]).unwrap();
    after.play(Move::Null).unwrap();
    let d4 = after.parse_uci("d2d4").unwrap();
    r.pv = vec![r.pv[0], Move::Null, d4];
    info(&mut state, run, r);
    set_arrows(&mut state, |p| {
        p.enabled = true;
        p.mode = ArrowMode::Sequence;
        p.sequence_len = 8;
    });
    assert_eq!(arrow_texts(&state), ["e2e4", "d2d4"]);
    let sides: Vec<_> = state.engine_arrows().iter().map(|a| a.side).collect();
    assert_eq!(sides, [Color::White, Color::White]);
}

#[test]
fn sequence_ignores_a_continuation_that_is_not_legal() {
    // The slice 1 validation keeps illegal moves out; defend anyway.
    let (mut state, run) = analysing();
    started(&mut state, run, 1);
    let pos = here(&state);
    let mut r = report(&pos, 1, 1, 10, 30, &["e2e4"]);
    r.pv.push(r.pv[0]); // 1.e4 e4?? is illegal for Black
    info(&mut state, run, r);
    set_arrows(&mut state, |p| {
        p.enabled = true;
        p.mode = ArrowMode::Sequence;
        p.sequence_len = 8;
    });
    assert!(
        state.engine_arrows().len() <= 1,
        "drew an illegal move: {:?}",
        arrow_texts(&state)
    );
}

// ---------------------------------------------- redraw rule (analysis.tcl 3896)

fn deep_report(state: &mut AppState, run: RunId, depth: u32, score: Score, pv: &[&str]) {
    let pos = here(state);
    info(
        state,
        run,
        report_score(&pos, 1, 1, depth, score, Bound::Exact, pv),
    );
}

#[test]
fn sequence_is_not_redrawn_when_the_pv_shrinks_to_half_or_less_at_the_same_depth() {
    let (mut state, run) = sequence_state(&PV5[..4], 8);
    assert_eq!(state.engine_arrows().len(), 4);
    // seen 4 moves, depth 10: 2 > 4/2 is false, depth is the same.
    deep_report(&mut state, run, 10, Score::Cp(30), &PV5[..2]);
    assert_eq!(state.engine_arrows().len(), 4, "kept");
    // 3 > 4/2: redrawn with the new PV.
    deep_report(&mut state, run, 10, Score::Cp(30), &PV5[..3]);
    assert_eq!(state.engine_arrows().len(), 3);
}

#[test]
fn sequence_is_redrawn_when_the_depth_grows_by_two_but_not_by_one() {
    let (mut state, run) = sequence_state(&PV5[..4], 8);
    deep_report(&mut state, run, 11, Score::Cp(30), &PV5[..1]);
    assert_eq!(state.engine_arrows().len(), 4, "depth +1 does not redraw");
    deep_report(&mut state, run, 12, Score::Cp(30), &PV5[..1]);
    assert_eq!(state.engine_arrows().len(), 1, "depth +2 redraws");
}

#[test]
fn sequence_is_redrawn_when_a_mate_is_reported() {
    let (mut state, run) = sequence_state(&PV5[..4], 8);
    deep_report(&mut state, run, 10, Score::Mate(5), &PV5[..1]);
    assert_eq!(state.engine_arrows().len(), 1);
}

#[test]
fn the_sequence_redraws_on_a_new_position_and_on_a_new_preference() {
    let (mut state, run) = sequence_state(&PV5[..4], 8);
    set_arrows(&mut state, |p| p.sequence_len = 2);
    assert_eq!(
        state.engine_arrows().len(),
        2,
        "a preference redraws at once"
    );
    play(&mut state, "e2e4");
    assert!(state.engine_arrows().is_empty());
    started(&mut state, run, 2);
    line(&mut state, run, 2, 1, 0, &["e7e5"]);
    assert_eq!(
        arrow_texts(&state),
        ["e7e5"],
        "first report of the new position"
    );
}

// --------------------------------------------------------------------- clicks

fn three_lines_state() -> (AppState, RunId) {
    let (mut state, run) = with_lines(3, &[(30, "e2e4"), (20, "d2d4"), (10, "g1f3")]);
    arrows_on(&mut state);
    (state, run)
}

fn mainline(state: &AppState) -> Vec<String> {
    let game = state.session.game();
    let mut pos = game.start_position().clone();
    game.mainline()
        .filter(|id| *id != game.root())
        .map(|id| {
            let mv = game.mv(id).unwrap();
            let u = pos.uci(mv);
            pos.play(mv).unwrap();
            u
        })
        .collect()
}

#[test]
fn clicking_an_arrow_plays_the_first_move_of_that_line() {
    let (mut state, run) = three_lines_state();
    let m = click(&state, run, 2);
    bar(&mut state, m);
    assert_eq!(mainline(&state), ["d2d4"]);
}

#[test]
fn clicking_the_best_arrow_plays_line_1() {
    let (mut state, run) = three_lines_state();
    let m = click(&state, run, 1);
    bar(&mut state, m);
    assert_eq!(mainline(&state), ["e2e4"]);
}

#[test]
fn click_to_play_off_makes_the_arrows_inert() {
    let (mut state, run) = three_lines_state();
    set_arrows(&mut state, |p| p.click_to_play = false);
    let m = click(&state, run, 1);
    bar(&mut state, m);
    assert!(mainline(&state).is_empty());
}

#[test]
fn clicking_a_line_that_does_not_exist_or_a_ghost_run_does_nothing() {
    let (mut state, run) = three_lines_state();
    let m = click(&state, run, 9);
    bar(&mut state, m);
    let m = click(&state, run, 0);
    bar(&mut state, m);
    let m = click(&state, run, u8::MAX);
    bar(&mut state, m);
    let EvalBarMessage::PlayArrow { line, from, to, .. } = click(&state, run, 1) else {
        unreachable!()
    };
    let ghost = EvalBarMessage::PlayArrow {
        run: RunId(4242),
        line,
        from,
        to,
    };
    bar(&mut state, ghost);
    assert!(mainline(&state).is_empty());
}

#[test]
fn clicking_when_the_lines_are_about_another_position_does_nothing() {
    let (mut state, run) = three_lines_state();
    // The arrow was on the board before the move; the click arrives late.
    let m = click(&state, run, 1);
    play(&mut state, "c2c4");
    bar(&mut state, m);
    assert_eq!(mainline(&state), ["c2c4"], "the stale arrow is not played");
}

#[test]
fn clicking_with_a_stopped_analysis_does_nothing() {
    let (mut state, run) = three_lines_state();
    let m = click(&state, run, 1);
    msg(&mut state, AnalysisMessage::Toggle(run));
    bar(&mut state, m);
    assert!(mainline(&state).is_empty());
}

#[test]
fn clicking_with_a_locked_run_away_from_its_position_does_nothing() {
    let (mut state, run) = analysing();
    arrows_on(&mut state);
    play(&mut state, "e2e4");
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, -20, &["e7e5"]);
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    let m = click(&state, run, 1);
    nav(&mut state, Nav::Back(1));
    bar(&mut state, m);
    assert_eq!(mainline(&state), ["e2e4"]);
}

#[test]
fn the_click_takes_the_path_of_add_move_even_over_an_existing_continuation() {
    // Two identical states: one clicks the arrow, the other uses "Add move".
    let make = || {
        let (mut state, run) = analysing();
        play(&mut state, "e2e4");
        nav(&mut state, Nav::Start);
        started(&mut state, run, 1);
        line(&mut state, run, 1, 1, 30, &["d2d4"]);
        (state, run)
    };
    let (mut a, ra) = make();
    let (mut b, rb) = make();
    arrows_on(&mut a);
    let m = click(&a, ra, 1);
    bar(&mut a, m);
    msg(&mut b, AnalysisMessage::AddMove(rb));
    assert_eq!(mainline(&a), mainline(&b));
    assert_eq!(a.session.position().fen(), b.session.position().fen());
    assert_eq!(a.notice, b.notice);
}

#[test]
fn the_click_clears_the_arrows_of_the_old_position() {
    let (mut state, run) = three_lines_state();
    let m = click(&state, run, 1);
    bar(&mut state, m);
    assert!(state.engine_arrows().is_empty());
}

#[test]
fn a_click_is_refused_for_another_run_or_other_squares_or_with_arrows_off() {
    let (mut state, run) = three_lines_state();
    let EvalBarMessage::PlayArrow { line, from, to, .. } = click(&state, run, 1) else {
        unreachable!()
    };
    // The squares of line 2 do not belong to line 1.
    let EvalBarMessage::PlayArrow {
        from: from2,
        to: to2,
        ..
    } = click(&state, run, 2)
    else {
        unreachable!()
    };
    let wrong = EvalBarMessage::PlayArrow {
        run,
        line,
        from: from2,
        to: to2,
    };
    bar(&mut state, wrong);
    assert!(mainline(&state).is_empty());
    set_arrows(&mut state, |p| p.enabled = false);
    bar(
        &mut state,
        EvalBarMessage::PlayArrow {
            run,
            line,
            from,
            to,
        },
    );
    assert!(mainline(&state).is_empty(), "arrows off");
}

#[test]
fn only_the_first_move_of_a_sequence_is_playable() {
    let (mut state, run) = sequence_state(&PV5, 8);
    let arrows: Vec<_> = state.engine_arrows().to_vec();
    let second = arrows
        .iter()
        .find(|a| a.kind == ArrowKind::Sequence(1))
        .copied()
        .unwrap();
    bar(
        &mut state,
        EvalBarMessage::PlayArrow {
            run,
            line: second.rank,
            from: second.from,
            to: second.to,
        },
    );
    assert!(mainline(&state).is_empty(), "ply 1 is not playable");
    let first = arrows[0];
    assert_eq!(first.kind, ArrowKind::Sequence(0));
    bar(
        &mut state,
        EvalBarMessage::PlayArrow {
            run,
            line: first.rank,
            from: first.from,
            to: first.to,
        },
    );
    assert_eq!(mainline(&state), ["e2e4"]);
}

// --------------------------------------------------------------- traffic lights

fn hexes(state: &AppState) -> Vec<String> {
    state
        .traffic_lights()
        .iter()
        .map(|l| l.color.hex())
        .collect()
}

#[test]
fn lights_are_hidden_by_default_and_follow_the_toggle() {
    let (mut state, _run) = with_lines(1, &[(100, "e2e4")]);
    assert!(state.traffic_lights().is_empty());
    bar(&mut state, EvalBarMessage::ToggleTrafficLights);
    assert!(state.prefs.engine_view.traffic_lights.show);
    assert_eq!(state.traffic_lights().len(), 1);
    bar(&mut state, EvalBarMessage::ToggleTrafficLights);
    assert!(state.traffic_lights().is_empty());
}

#[test]
fn lights_have_one_strip_per_line_in_rank_order_with_the_scid_colours() {
    let (mut state, _run) = with_lines(
        5,
        &[
            (100, "e2e4"),
            (0, "d2d4"),
            (-100, "g1f3"),
            (500, "c2c4"),
            (-500, "b1c3"),
        ],
    );
    lights_on(&mut state);
    let ranks: Vec<_> = state.traffic_lights().iter().map(|l| l.rank).collect();
    assert_eq!(ranks, [1, 2, 3, 4, 5]);
    assert_eq!(
        hexes(&state),
        ["#c9f800", "#f8f800", "#f8c900", "#09f800", "#f80900"]
    );
}

#[test]
fn a_single_line_is_a_single_strip() {
    let (mut state, _run) = with_lines(1, &[(25, "e2e4")]);
    lights_on(&mut state);
    assert_eq!(hexes(&state), ["#edf800"]);
}

#[test]
fn lights_need_at_least_one_line_with_a_move() {
    let (mut state, run) = analysing();
    lights_on(&mut state);
    started(&mut state, run, 1);
    assert!(state.traffic_lights().is_empty());
}

#[test]
fn lights_are_sorted_by_rank_whatever_the_report_order() {
    let (mut state, run) = analysing();
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    lights_on(&mut state);
    started(&mut state, run, 1);
    line(&mut state, run, 1, 3, -100, &["g1f3"]);
    line(&mut state, run, 1, 1, 100, &["e2e4"]);
    line(&mut state, run, 1, 2, 0, &["d2d4"]);
    assert_eq!(hexes(&state), ["#c9f800", "#f8f800", "#f8c900"]);
}

#[test]
fn mate_scores_use_the_full_colours() {
    let (mut state, run) = analysing();
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 2));
    lights_on(&mut state);
    started(&mut state, run, 1);
    let pos = here(&state);
    info(
        &mut state,
        run,
        report_score(&pos, 1, 1, 20, Score::Mate(3), Bound::Exact, &["e2e4"]),
    );
    info(
        &mut state,
        run,
        report_score(&pos, 1, 2, 20, Score::Mate(-3), Bound::Exact, &["d2d4"]),
    );
    assert_eq!(hexes(&state), ["#09f800", "#f80900"]);
}

#[test]
fn lights_use_whites_point_of_view_with_black_to_move() {
    let (mut state, run) = analysing();
    lights_on(&mut state);
    play(&mut state, "e2e4");
    started(&mut state, run, 1);
    line(&mut state, run, 1, 1, -100, &["e7e5"]);
    assert_eq!(hexes(&state), [traffic_light_color(-1.0).hex()]);
    assert_eq!(hexes(&state), ["#f8c900"]);
}

#[test]
fn lights_do_not_need_the_arrows_and_vanish_with_a_new_position() {
    let (mut state, run) = with_lines(1, &[(100, "e2e4")]);
    lights_on(&mut state);
    assert!(!arrow_prefs(&state).enabled);
    assert_eq!(state.traffic_lights().len(), 1);
    play(&mut state, "e2e4");
    assert!(state.traffic_lights().is_empty());
    started(&mut state, run, 2);
    line(&mut state, run, 2, 1, 0, &["e7e5"]);
    assert_eq!(hexes(&state), ["#f8f800"]);
}

#[test]
fn lights_follow_the_marked_engine_otherwise_the_main_one() {
    let (mut state, a, b) = two_runs();
    started(&mut state, a, 1);
    started(&mut state, b, 1);
    line(&mut state, a, 1, 1, 100, &["e2e4"]);
    line(&mut state, b, 1, 1, -100, &["e2e4"]);
    lights_on(&mut state);
    assert_eq!(hexes(&state), ["#c9f800"], "main engine");
    bar(&mut state, EvalBarMessage::ToggleArrows(b));
    assert_eq!(hexes(&state), ["#f8c900"], "the marked one");
    bar(&mut state, EvalBarMessage::ToggleArrows(b));
    assert_eq!(hexes(&state), ["#c9f800"], "back to the main engine");
}

#[test]
fn lights_hold_up_under_hostile_input() {
    let (mut state, run) = analysing();
    enable_all(&mut state);
    started(&mut state, run, 1);
    for cp in [i32::MAX, i32::MIN, 0, 32_700, -32_700] {
        let pos = here(&state);
        let mut r = report(&pos, 1, 1, 12, 0, &["e2e4"]);
        r.score = Some((Score::Cp(cp), Bound::Exact));
        info(&mut state, run, r);
        let _ = state.traffic_lights();
        let _ = state.eval_bar_view();
    }
    let pos = here(&state);
    let mut r = report(&pos, 1, 200, 12, 0, &["e2e4"]);
    r.multipv = u8::MAX;
    info(&mut state, run, r);
    let _ = (E4_E5_NF3, Position::new());
}

#[test]
fn sequence_follows_a_pv_whose_tail_changed_at_the_same_depth_and_score() {
    // Scid 3896: `llength moves > seen/2` redraws; the arrows must show the
    // moves of the PV last reported, not an older one with the same shape.
    let (mut state, run) = sequence_state(&["e2e4", "e7e5", "g1f3"], 8);
    assert_eq!(arrow_texts(&state), ["e2e4", "e7e5", "g1f3"]);
    deep_report(
        &mut state,
        run,
        10,
        Score::Cp(30),
        &["e2e4", "c7c5", "g1f3"],
    );
    assert_eq!(
        arrow_texts(&state),
        ["e2e4", "c7c5", "g1f3"],
        "stale tail: the cache key ignores the moves after the first"
    );
}
