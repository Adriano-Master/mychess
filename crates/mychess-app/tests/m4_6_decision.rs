//! Contract tests of the pure part of the automatic annotation (spec M4-6,
//! rules 1-12 and the Scid's `addAnnotation`, `scoreToNag`, `addScore`,
//! `markExercise`, `appendTag`, `bookAnnotation`).
//!
//! The expectations come from the spec and from
//! `reference/scid_vs_pc-4.27/tcl/tools/analysis.tcl`, not from the code.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::field_reassign_with_default,
    clippy::manual_assert_eq,
    clippy::many_single_char_names,
    clippy::type_complexity,
    clippy::unnecessary_literal_bound,
    clippy::too_many_lines
)]

use std::collections::HashSet;
use std::time::Duration;

use mychess_app::{
    AnnotateAction as Action, AnnotateNotice, AnnotateOptions, AnnotateOutcome, AnnotateReport,
    AnnotateSide, AnnotateStep as Step, AnnotationError, BookProbe, BookVerdict, Control,
    Informant, PositionEval, ScoreFormat, WithScore, WithVars, append_tag, classify_book_move,
    decide_annotation, exercise_comment, exercise_difficulty, exercise_move, format_score_text,
    is_obvious, last_book_move_comment, out_of_book_comment, score_to_nag, skips_side,
};
use mychess_core::{Color, Game, Move, Nag, NodeId, Position};
use rstest::rstest;

// ---- fixture ----

struct Fx {
    parent: NodeId,
    node: NodeId,
    played: Move,
    alt: Move,
    alt2: Move,
}

fn fx() -> Fx {
    let mut game = Game::new();
    let root = game.root();
    let start = Position::new();
    let played = start.parse_uci("a2a3").unwrap();
    let alt = start.parse_uci("e2e4").unwrap();
    let alt2 = start.parse_uci("d2d4").unwrap();
    let node = game.add_move(root, played).unwrap();
    Fx {
        parent: root,
        node,
        played,
        alt,
        alt2,
    }
}

fn eval(score: f32, pv: &[Move]) -> PositionEval {
    PositionEval {
        score,
        mate: None,
        pv: pv.to_vec(),
        depth: 13,
        second: None,
    }
}

fn mate_eval(n: i32, pv: &[Move]) -> PositionEval {
    PositionEval {
        score: if n > 0 {
            327.5 - n as f32 / 100.0
        } else {
            -327.5 - n as f32 / 100.0
        },
        mate: Some(n),
        pv: pv.to_vec(),
        depth: 13,
        second: None,
    }
}

fn step(f: &Fx, mover: Color, prev: PositionEval, cur: PositionEval) -> Step {
    Step {
        node: f.node,
        parent: f.parent,
        mover,
        prev,
        cur,
        played: f.played,
        is_last: false,
        ply: 5,
        non_standard_start: false,
        cur_no_moves: false,
        prev_nag: None,
        suppress_score: false,
        wanted_depth: 13,
    }
}

fn decide(opts: &AnnotateOptions, s: &Step) -> mychess_app::Decision {
    decide_annotation(opts, &Informant::default(), s)
}

fn nags_on(actions: &[Action], node: NodeId) -> Vec<u16> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Nag { node: n, nag } if *n == node => Some(u16::from(nag.value())),
            _ => None,
        })
        .collect()
}

fn scores(actions: &[Action]) -> Vec<String> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Score { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn variations(actions: &[Action]) -> Vec<(NodeId, Vec<Move>, Option<String>, Option<u16>)> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Variation {
                parent,
                pv,
                comment,
                end_nag,
            } => Some((
                *parent,
                pv.clone(),
                comment.clone(),
                end_nag.map(|n| u16::from(n.value())),
            )),
            _ => None,
        })
        .collect()
}

fn has_candidate(actions: &[Action]) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, Action::ExerciseCandidate { .. }))
}

fn opts() -> AnnotateOptions {
    AnnotateOptions::default()
}

// ---- spec table, examples 1-10 ----

#[test]
fn white_blunder_gets_question_mark_both_scores_diagram_and_line() {
    let f = fx();
    let s = step(&f, Color::White, eval(0.30, &[f.alt]), eval(-1.80, &[]));
    let d = decide(&opts(), &s);
    // `?` then `-/+` on the move; the diagram on the parent.
    assert_eq!(nags_on(&d.actions, f.node), vec![2, 17]);
    assert_eq!(nags_on(&d.actions, f.parent), vec![201]);
    assert!(has_candidate(&d.actions), "`?` is a candidate exercise");
    assert_eq!(scores(&d.actions), vec!["-1.80 / +0.30".to_owned()]);
    // `0.30 >= -0.5`: the line ends in `=` (10).
    assert_eq!(
        variations(&d.actions),
        vec![(f.parent, vec![f.alt], None, Some(10))]
    );
}

#[test]
fn small_loss_writes_only_the_single_score() {
    let f = fx();
    let s = step(&f, Color::White, eval(0.30, &[f.alt]), eval(0.10, &[]));
    let d = decide(&opts(), &s);
    assert_eq!(scores(&d.actions), vec!["[%eval +0.10]".to_owned()]);
    assert_eq!(d.actions.len(), 1);
}

#[test]
fn black_blunder_gets_double_question_mark_and_the_white_nag() {
    let f = fx();
    let s = step(&f, Color::Black, eval(-0.20, &[f.alt]), eval(3.40, &[]));
    let d = decide(&opts(), &s);
    assert_eq!(nags_on(&d.actions, f.node), vec![4, 18]);
    assert!(has_candidate(&d.actions));
    assert_eq!(scores(&d.actions), vec!["+3.40 / -0.20".to_owned()]);
}

#[test]
fn a_loss_from_plus_six_to_plus_four_is_judged_with_the_default_cutoff() {
    let f = fx();
    let s = step(&f, Color::White, eval(6.00, &[f.alt]), eval(4.00, &[]));
    let d = decide(&opts(), &s);
    assert_eq!(nags_on(&d.actions, f.node), vec![2, 18], "`?` and `+-`");
    assert_eq!(nags_on(&d.actions, f.parent), vec![201]);
}

#[test]
fn a_decided_game_is_not_judged_when_black_gains_from_plus_six() {
    let f = fx();
    let mut o = opts();
    o.cutoff = 8.0;
    let s = step(&f, Color::Black, eval(6.00, &[f.alt]), eval(9.00, &[]));
    let d = decide(&o, &s);
    assert_eq!(scores(&d.actions), vec!["[%eval +9.00]".to_owned()]);
    assert_eq!(d.actions.len(), 1, "no nag, no line, no diagram");
}

#[test]
fn the_same_move_with_the_default_cutoff_is_not_even_considered() {
    let f = fx();
    let s = step(&f, Color::Black, eval(6.00, &[f.alt]), eval(9.00, &[]));
    let d = decide(&opts(), &s);
    assert_eq!(d.actions.len(), 1);
    assert_eq!(scores(&d.actions), vec!["[%eval +9.00]".to_owned()]);
}

#[test]
fn a_game_lost_before_the_white_move_is_annulled_symmetrically() {
    let f = fx();
    let mut o = opts();
    o.cutoff = 8.0;
    let s = step(&f, Color::White, eval(-6.00, &[f.alt]), eval(-9.00, &[]));
    let d = decide(&o, &s);
    assert_eq!(d.actions.len(), 1);
}

#[test]
fn notbest_has_no_decided_game_annulment() {
    // Scid 1557-1562: the annulment is only in the other branch.
    let f = fx();
    let mut o = opts();
    o.cutoff = 10.0;
    o.with_vars = WithVars::NotBest;
    let s = step(&f, Color::White, eval(-6.00, &[f.alt]), eval(-9.00, &[]));
    let d = decide(&o, &s);
    assert!(variations(&d.actions).len() == 1, "{:?}", d.actions);
    assert_eq!(nags_on(&d.actions, f.parent), vec![201]);
}

#[test]
fn a_missed_mate_in_three_gets_the_mate_comment_and_a_double_question_mark() {
    let f = fx();
    let s = step(&f, Color::White, mate_eval(3, &[f.alt]), eval(0.0, &[]));
    let d = decide(&opts(), &s);
    assert!(nags_on(&d.actions, f.node).contains(&4), "{:?}", d.actions);
    let v = variations(&d.actions);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].2.as_deref(), Some("Mate in 3..."));
    assert_eq!(nags_on(&d.actions, f.parent), vec![201]);
}

#[test]
fn allmoves_writes_mate_in_n_without_the_ellipsis() {
    // Scid 1606-1608 versus 1655-1657.
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::AllMoves;
    let s = step(&f, Color::White, mate_eval(3, &[f.alt]), eval(0.0, &[]));
    let d = decide(&o, &s);
    let v = variations(&d.actions);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].2.as_deref(), Some("Mate in 3"));
}

#[rstest]
#[case::mate_in_nine_is_candidate(9, true)]
#[case::mate_in_ten_is_beyond_depth_minus_three(10, false)]
#[case::mate_in_one(1, true)]
fn missed_mate_only_counts_below_depth_minus_three(#[case] mate: i32, #[case] flagged: bool) {
    let f = fx();
    let mut o = opts();
    // Isolate the mate rule: without it the loss from 327 to 0 would be an
    // error anyway, so compare with the option off.
    o.missed_mates = true;
    let s = step(&f, Color::White, mate_eval(mate, &[f.alt]), eval(0.0, &[]));
    let with = variations(&decide(&o, &s).actions);
    assert_eq!(with.len(), 1, "a loss of 327 pawns is an error either way");
    assert_eq!(with[0].2.is_some(), flagged, "mate in {mate}");
}

#[test]
fn missed_mates_off_means_no_mate_comment() {
    let f = fx();
    let mut o = opts();
    o.missed_mates = false;
    let s = step(&f, Color::White, mate_eval(3, &[f.alt]), eval(0.0, &[]));
    let d = decide(&o, &s);
    assert_eq!(variations(&d.actions)[0].2, None);
}

#[rstest]
#[case::mate_lost(None, true)]
#[case::longer_mate_is_a_shorter_mate_missed(Some(3), true)]
#[case::same_distance_after_the_move_is_still_longer(Some(4), true)]
#[case::mate_kept_one_shorter_is_fine(Some(2), false)]
#[case::mate_kept_much_shorter_is_fine(Some(1), false)]
fn white_missed_or_longer_mate_table(#[case] cur_mate: Option<i32>, #[case] flagged: bool) {
    let f = fx();
    let mut o = opts();
    o.cutoff = 10.0;
    let cur = match cur_mate {
        Some(n) => mate_eval(n, &[]),
        None => eval(327.0, &[]),
    };
    let s = step(&f, Color::White, mate_eval(3, &[f.alt]), cur);
    let d = decide(&o, &s);
    let comment = variations(&d.actions).first().and_then(|v| v.2.clone());
    assert_eq!(comment.is_some(), flagged, "{:?}", d.actions);
}

#[rstest]
#[case::mate_lost(None, true)]
#[case::longer(Some(-3), true)]
#[case::shorter_is_fine(Some(-2), false)]
fn black_missed_mate_table(#[case] cur_mate: Option<i32>, #[case] flagged: bool) {
    let f = fx();
    let mut o = opts();
    o.cutoff = 10.0;
    let cur = match cur_mate {
        Some(n) => mate_eval(n, &[]),
        None => eval(-327.0, &[]),
    };
    let s = step(&f, Color::Black, mate_eval(-3, &[f.alt]), cur);
    let d = decide(&o, &s);
    let comment = variations(&d.actions).first().and_then(|v| v.2.clone());
    assert_eq!(comment.is_some(), flagged, "{:?}", d.actions);
}

#[test]
fn missed_mate_depth_threshold_is_the_wanted_depth_not_the_time() {
    // Spec rule 4: `D` is the depth per move, also under a time limit.
    let f = fx();
    let mut s = step(&f, Color::White, mate_eval(6, &[f.alt]), eval(0.0, &[]));
    s.wanted_depth = 8; // 6 < 8 - 3 is false
    let d = decide(&opts(), &s);
    assert_eq!(variations(&d.actions)[0].2, None);
    s.wanted_depth = 13;
    let d = decide(&opts(), &s);
    assert_eq!(variations(&d.actions)[0].2.as_deref(), Some("Mate in 6..."));
}

// ---- rules 1-3 ----

#[test]
fn side_filter_skips_the_other_sides_move_and_only_that_one() {
    let f = fx();
    let mut o = opts();
    o.side = AnnotateSide::White;
    let black = step(&f, Color::Black, eval(0.3, &[f.alt]), eval(5.0, &[]));
    let d = decide(&o, &black);
    assert!(d.actions.is_empty());
    let white = step(&f, Color::White, eval(0.3, &[f.alt]), eval(0.2, &[]));
    assert!(!decide(&o, &white).actions.is_empty());
    o.side = AnnotateSide::Black;
    assert!(decide(&o, &white).actions.is_empty());
    assert!(!decide(&o, &black).actions.is_empty());
    assert!(skips_side(&o, Color::White));
    assert!(!skips_side(&o, Color::Black));
    assert!(!skips_side(&opts(), Color::White));
}

#[test]
fn the_expected_move_gets_only_the_single_score() {
    let f = fx();
    let s = step(
        &f,
        Color::White,
        eval(0.3, &[f.played, f.alt]),
        eval(-4.0, &[]),
    );
    let d = decide(&opts(), &s);
    assert_eq!(scores(&d.actions), vec!["[%eval -4.00]".to_owned()]);
    assert_eq!(d.actions.len(), 1, "no error is even looked for");
}

#[test]
fn the_expected_move_with_score_blunders_writes_nothing() {
    let f = fx();
    let mut o = opts();
    o.with_score = WithScore::Blunders;
    let s = step(&f, Color::White, eval(0.3, &[f.played]), eval(0.2, &[]));
    assert!(decide(&o, &s).actions.is_empty());
}

#[test]
fn the_expected_move_rule_does_not_apply_to_the_last_move() {
    // Scid 1486: `![sc_pos isAt vend]`. The line that repeats the move is
    // not a variation (D883) but the error is still found.
    let f = fx();
    let mut s = step(&f, Color::White, eval(0.3, &[f.played]), eval(-3.0, &[]));
    s.is_last = true;
    let d = decide(&opts(), &s);
    assert!(nags_on(&d.actions, f.node).contains(&4), "{:?}", d.actions);
    assert_eq!(nags_on(&d.actions, f.parent), vec![201]);
    assert!(variations(&d.actions).is_empty(), "D883: no repeated move");
}

#[test]
fn the_expected_move_rule_does_not_apply_with_allmoves() {
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::AllMoves;
    let s = step(
        &f,
        Color::White,
        eval(0.3, &[f.played, f.alt]),
        eval(0.3, &[]),
    );
    let d = decide(&o, &s);
    assert!(!nags_on(&d.actions, f.node).is_empty(), "the `=` nag");
}

#[test]
fn a_line_that_starts_with_the_played_move_is_not_a_variation() {
    // D883 (a deviation from the Scid, which would repeat the move).
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::AllMoves;
    let s = step(
        &f,
        Color::White,
        eval(0.3, &[f.played, f.alt]),
        eval(0.3, &[]),
    );
    assert!(variations(&decide(&o, &s).actions).is_empty());
}

#[test]
fn a_set_up_start_has_no_delta_for_its_first_move() {
    let f = fx();
    let mut s = step(&f, Color::White, eval(0.3, &[f.alt]), eval(-3.0, &[]));
    s.non_standard_start = true;
    s.ply = 1;
    let d = decide(&opts(), &s);
    assert_eq!(d.actions.len(), 1, "{:?}", d.actions);
    s.ply = 2;
    assert!(d.actions.len() < decide(&opts(), &s).actions.len());
    s.ply = 1;
    s.non_standard_start = false;
    assert!(decide(&opts(), &s).actions.len() > 1);
}

// ---- rules 5, 6, 7, 8 ----

#[rstest]
#[case::loss_equal_to_the_threshold_is_no_error(0.0, -1.0, false)]
#[case::just_above_the_threshold(0.0, -1.01, true)]
#[case::float_noise_case_1(1.3, 0.3, false)]
#[case::float_noise_case_2(0.7, -0.3, false)]
#[case::float_noise_case_3(3.3, 2.3, false)]
fn the_threshold_is_exact_in_centipawns(#[case] prev: f32, #[case] cur: f32, #[case] err: bool) {
    let f = fx();
    let s = step(&f, Color::White, eval(prev, &[f.alt]), eval(cur, &[]));
    let d = decide(&opts(), &s);
    assert_eq!(!variations(&d.actions).is_empty(), err, "{:?}", d.actions);
}

#[test]
fn the_threshold_is_exact_for_every_centipawn_pair_at_exactly_one_pawn() {
    // D881: `cur - prev` exactly equal to the threshold is not an error, for
    // all the values (floating point noise must not push it over).
    let f = fx();
    for cp in -400i32..=400 {
        let prev = cp as f32 / 100.0;
        let cur = (cp - 100) as f32 / 100.0;
        let white = decide(
            &opts(),
            &step(&f, Color::White, eval(prev, &[f.alt]), eval(cur, &[])),
        );
        assert!(
            variations(&white.actions).is_empty(),
            "white, {prev:+.2} -> {cur:+.2}: {:?}",
            white.actions
        );
        let black = decide(
            &opts(),
            &step(&f, Color::Black, eval(cur, &[f.alt]), eval(prev, &[])),
        );
        assert!(
            variations(&black.actions).is_empty(),
            "black, {cur:+.2} -> {prev:+.2}: {:?}",
            black.actions
        );
    }
}

#[test]
fn one_centipawn_above_the_threshold_is_an_error_for_every_value() {
    let f = fx();
    for cp in -400i32..=400 {
        let prev = cp as f32 / 100.0;
        let cur = (cp - 101) as f32 / 100.0;
        let d = decide(
            &opts(),
            &step(&f, Color::White, eval(prev, &[f.alt]), eval(cur, &[])),
        );
        assert_eq!(variations(&d.actions).len(), 1, "{prev:+.2} -> {cur:+.2}");
    }
}

#[test]
fn the_cutoff_is_strict_and_in_centipawns() {
    let f = fx();
    // |prev| = 5.00 and |cur| = 5.00 are not below the cutoff: not judged.
    let d = decide(
        &opts(),
        &step(&f, Color::White, eval(7.0, &[f.alt]), eval(5.0, &[])),
    );
    assert_eq!(d.actions.len(), 1, "{:?}", d.actions);
    // 4.99 is below: judged.
    let d = decide(
        &opts(),
        &step(&f, Color::White, eval(7.0, &[f.alt]), eval(4.99, &[])),
    );
    assert!(variations(&d.actions).len() == 1);
}

#[test]
fn a_swing_across_zero_is_judged_even_beyond_the_cutoff() {
    // Scid 1546: `abs(delta) > abs(score) && score*prevscore < 0`.
    let f = fx();
    let d = decide(
        &opts(),
        &step(&f, Color::White, eval(6.0, &[f.alt]), eval(-7.0, &[])),
    );
    assert_eq!(variations(&d.actions).len(), 1);
    // Same sign, same size of fall: not judged.
    let d = decide(
        &opts(),
        &step(&f, Color::White, eval(13.0, &[f.alt]), eval(6.0, &[])),
    );
    assert_eq!(d.actions.len(), 1);
}

#[test]
fn notbest_any_loss_is_an_error_even_one_centipawn() {
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::NotBest;
    let d = decide(
        &o,
        &step(&f, Color::White, eval(0.30, &[f.alt]), eval(0.29, &[])),
    );
    assert_eq!(variations(&d.actions).len(), 1);
    assert_eq!(scores(&d.actions), vec!["+0.29 / +0.30".to_owned()]);
    assert_eq!(nags_on(&d.actions, f.parent), vec![201]);
    // A gain is not.
    let d = decide(
        &o,
        &step(&f, Color::White, eval(0.30, &[f.alt]), eval(0.31, &[])),
    );
    assert_eq!(d.actions.len(), 1);
    // For Black a rise is the loss.
    let d = decide(
        &o,
        &step(&f, Color::Black, eval(0.30, &[f.alt]), eval(0.31, &[])),
    );
    assert_eq!(variations(&d.actions).len(), 1);
}

#[test]
fn without_variations_blunder_scores_are_double_only_on_errors() {
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::No;
    o.with_score = WithScore::Blunders;
    let err = decide(
        &o,
        &step(&f, Color::White, eval(0.3, &[f.alt]), eval(-1.8, &[])),
    );
    assert_eq!(scores(&err.actions), vec!["-1.80 / +0.30".to_owned()]);
    assert_eq!(err.actions.len(), 1, "no nag, no line");
    let ok = decide(
        &o,
        &step(&f, Color::White, eval(0.3, &[f.alt]), eval(0.2, &[])),
    );
    assert!(ok.actions.is_empty());
}

#[rstest]
#[case(WithScore::AllMoves, 1)]
#[case(WithScore::Blunders, 0)]
#[case(WithScore::Variations, 0)]
#[case(WithScore::No, 0)]
fn without_variations_only_all_moves_scores_quiet_moves(#[case] ws: WithScore, #[case] n: usize) {
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::No;
    o.with_score = ws;
    let d = decide(
        &o,
        &step(&f, Color::White, eval(0.3, &[f.alt]), eval(0.2, &[])),
    );
    assert_eq!(d.actions.len(), n);
}

#[test]
fn variations_score_mode_writes_no_double_score_without_variations() {
    // Scid 1570-1574: only `allmoves` and `blunders` write something.
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::No;
    o.with_score = WithScore::Variations;
    let d = decide(
        &o,
        &step(&f, Color::White, eval(0.3, &[f.alt]), eval(-3.0, &[])),
    );
    assert!(d.actions.is_empty());
}

#[rstest]
#[case::within_the_threshold(0.9, &[])]
#[case::dubious_boundary_is_included_in_the_threshold_rule(1.5, &[6])]
#[case::poor(1.51, &[2])]
#[case::poor_upper(3.0, &[2])]
#[case::blunder(3.01, &[4])]
fn allmoves_error_nag_by_loss(#[case] loss: f32, #[case] expected: &[u16]) {
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::AllMoves;
    // Keep the position equal after the move so that only the error nag is
    // written besides the evaluation nag: prev_nag is set to that.
    let prev = eval(0.0, &[f.alt]);
    let cur = eval(-loss, &[]);
    let mut s = step(&f, Color::White, prev, cur);
    s.prev_nag = score_to_nag(-loss, &Informant::default());
    let d = decide(&o, &s);
    assert_eq!(
        nags_on(&d.actions, f.node),
        expected.to_vec(),
        "{:?}",
        d.actions
    );
}

#[test]
fn allmoves_dubious_needs_the_loss_above_the_threshold() {
    // With the blunder threshold at 0.3 a loss of 0.4 is above it but not
    // above the `?!` limit (0.5): no nag; 0.6 is `?!`.
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::AllMoves;
    o.blunder = 0.3;
    for (loss, expected) in [(0.4_f32, vec![]), (0.5, vec![]), (0.6, vec![6])] {
        let mut s = step(&f, Color::White, eval(0.0, &[f.alt]), eval(-loss, &[]));
        s.prev_nag = score_to_nag(-loss, &Informant::default());
        assert_eq!(
            nags_on(&decide(&o, &s).actions, f.node),
            expected,
            "loss {loss}"
        );
    }
}

#[test]
fn allmoves_writes_score_nag_and_a_line_ending_with_the_previous_nag() {
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::AllMoves;
    o.with_score = WithScore::Variations; // only `no` suppresses here
    let d = decide(
        &o,
        &step(&f, Color::White, eval(2.0, &[f.alt]), eval(1.9, &[])),
    );
    assert_eq!(scores(&d.actions), vec!["[%eval +1.90]".to_owned()]);
    assert_eq!(nags_on(&d.actions, f.node), vec![16]);
    assert_eq!(
        variations(&d.actions),
        vec![(f.parent, vec![f.alt], None, Some(16))]
    );
    assert!(nags_on(&d.actions, f.parent).is_empty(), "no diagram");
    o.with_score = WithScore::No;
    assert!(
        scores(
            &decide(
                &o,
                &step(&f, Color::White, eval(2.0, &[f.alt]), eval(1.9, &[]))
            )
            .actions
        )
        .is_empty()
    );
}

#[test]
fn allmoves_does_not_repeat_the_same_evaluation_nag() {
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::AllMoves;
    let mut s = step(&f, Color::White, eval(0.3, &[f.alt]), eval(0.2, &[]));
    let first = decide(&o, &s);
    assert_eq!(nags_on(&first.actions, f.node), vec![10]);
    assert_eq!(first.prev_nag, Nag::new(10).ok());
    s.prev_nag = first.prev_nag;
    let second = decide(&o, &s);
    assert!(nags_on(&second.actions, f.node).is_empty());
    assert_eq!(second.prev_nag, Nag::new(10).ok());
    // A different evaluation is written again.
    s.cur = eval(2.0, &[]);
    assert_eq!(nags_on(&decide(&o, &s).actions, f.node), vec![16]);
}

#[test]
fn error_nags_are_not_subject_to_the_repeat_suppression() {
    let f = fx();
    let mut o = opts();
    o.with_vars = WithVars::AllMoves;
    let mut s = step(&f, Color::White, eval(0.0, &[f.alt]), eval(-2.0, &[]));
    s.prev_nag = Nag::new(17).ok();
    let d = decide(&o, &s);
    assert_eq!(
        nags_on(&d.actions, f.node),
        vec![2],
        "`?` stays, `-/+` omitted"
    );
}

#[test]
fn the_blunders_mode_writes_the_evaluation_nag_every_time_and_keeps_prev_nag() {
    let f = fx();
    let mut s = step(&f, Color::White, eval(0.3, &[f.alt]), eval(-1.8, &[]));
    s.prev_nag = Nag::new(17).ok();
    let d = decide(&opts(), &s);
    assert_eq!(nags_on(&d.actions, f.node), vec![2, 17]);
    assert_eq!(d.prev_nag, Nag::new(17).ok());
}

#[test]
fn d832_rule_8_follows_the_scid_in_a_won_game_that_stays_won() {
    // analysis.tcl 1619-1672: the `if` that tests `score > ++-` closes before
    // `scoreToNag`, `addNag D` and the variation, so they are written in both
    // cases. Only `?!`/`?`/`??` and `markExercise` stay inside the `else`.
    let f = fx();
    let mut o = opts();
    o.cutoff = 10.0;
    let s = step(&f, Color::White, eval(8.0, &[f.alt]), eval(6.0, &[]));
    let d = decide(&o, &s);
    assert_eq!(scores(&d.actions), vec!["+6.00 / +8.00".to_owned()]);
    assert_eq!(nags_on(&d.actions, f.node), vec![18], "only `+-`, no `?`");
    assert!(!has_candidate(&d.actions), "no exercise");
    assert_eq!(nags_on(&d.actions, f.parent), vec![201]);
    assert_eq!(variations(&d.actions).len(), 1);
    // Mirror for Black.
    let s = step(&f, Color::Black, eval(-8.0, &[f.alt]), eval(-6.0, &[]));
    let d = decide(&o, &s);
    assert_eq!(nags_on(&d.actions, f.node), vec![19]);
    assert_eq!(nags_on(&d.actions, f.parent), vec![201]);
    assert_eq!(variations(&d.actions).len(), 1);
}

#[test]
fn quiet_moves_in_blunders_mode_score_only_with_all_moves() {
    let f = fx();
    let mut o = opts();
    let s = step(&f, Color::White, eval(0.3, &[f.alt]), eval(0.2, &[]));
    assert_eq!(decide(&o, &s).actions.len(), 1);
    for ws in [WithScore::Blunders, WithScore::Variations, WithScore::No] {
        o.with_score = ws;
        assert!(decide(&o, &s).actions.is_empty(), "{ws:?}");
    }
}

#[test]
fn an_error_in_blunders_mode_with_score_no_has_no_score_but_the_rest() {
    let f = fx();
    let mut o = opts();
    o.with_score = WithScore::No;
    let d = decide(
        &o,
        &step(&f, Color::White, eval(0.3, &[f.alt]), eval(-1.8, &[])),
    );
    assert!(scores(&d.actions).is_empty());
    assert_eq!(nags_on(&d.actions, f.node), vec![2, 17]);
}

// ---- rules 9-12 ----

#[test]
fn a_suppressed_score_is_not_written() {
    let f = fx();
    let mut s = step(&f, Color::White, eval(0.3, &[f.alt]), eval(-1.8, &[]));
    s.suppress_score = true;
    let d = decide(&opts(), &s);
    assert!(scores(&d.actions).is_empty());
    assert_eq!(nags_on(&d.actions, f.node), vec![2, 17], "only the score");
}

#[test]
fn a_position_without_moves_gets_a_single_score_even_for_a_double_one() {
    let f = fx();
    let mut s = step(&f, Color::White, eval(3.0, &[f.alt]), eval(0.0, &[]));
    s.cur_no_moves = true;
    s.is_last = true;
    let d = decide(&opts(), &s);
    assert_eq!(scores(&d.actions), vec!["[%eval +0.00]".to_owned()]);
}

#[test]
fn a_position_without_moves_respects_score_no() {
    let f = fx();
    let mut o = opts();
    o.with_score = WithScore::No;
    let mut s = step(&f, Color::White, eval(3.0, &[f.alt]), eval(0.0, &[]));
    s.cur_no_moves = true;
    assert!(scores(&decide(&o, &s).actions).is_empty());
}

// ---- texts ----

#[test]
fn score_text_in_the_three_formats_and_mate() {
    assert_eq!(format_score_text(1.5, None, ScoreFormat::Plain), "+1.50");
    assert_eq!(format_score_text(-1.5, None, ScoreFormat::Plain), "-1.50");
    assert_eq!(
        format_score_text(1.5, None, ScoreFormat::Bracket),
        "[% +1.50]"
    );
    assert_eq!(
        format_score_text(1.5, None, ScoreFormat::Eval),
        "[%eval +1.50]"
    );
    assert_eq!(format_score_text(0.0, None, ScoreFormat::Plain), "+0.00");
    assert_eq!(
        format_score_text(0.0, None, ScoreFormat::Eval),
        "[%eval +0.00]"
    );
    assert_eq!(
        format_score_text(327.47, Some(3), ScoreFormat::Eval),
        "[%eval #3]"
    );
    assert_eq!(
        format_score_text(-327.47, Some(-3), ScoreFormat::Eval),
        "[%eval #-3]"
    );
    assert_eq!(
        format_score_text(327.47, Some(3), ScoreFormat::Plain),
        "+327.47"
    );
    assert_eq!(
        format_score_text(-327.47, Some(-3), ScoreFormat::Bracket),
        "[% -327.47]"
    );
}

#[test]
fn the_written_eval_reads_back_with_the_core_parser() {
    for (score, mate) in [
        (1.5_f32, None),
        (-0.03, None),
        (327.47, Some(3)),
        (-327.46, Some(-4)),
    ] {
        let text = format_score_text(score, mate, ScoreFormat::Eval);
        let read = mychess_core::eval_of(&text).expect("eval");
        match mate {
            None => assert!((read - score).abs() < 0.006, "{text}"),
            Some(n) => assert_eq!(read.is_sign_positive(), n > 0, "{text}"),
        }
    }
}

#[test]
fn the_nth_score_text_uses_the_mover_independent_both_format() {
    // Rule 12: `both` is `"%+.2f / %+.2f"`, always plain, even in `Eval`.
    let f = fx();
    for fmt in [ScoreFormat::Plain, ScoreFormat::Bracket, ScoreFormat::Eval] {
        let mut o = opts();
        o.score_format = fmt;
        let d = decide(
            &o,
            &step(&f, Color::White, eval(0.3, &[f.alt]), eval(-1.8, &[])),
        );
        assert_eq!(
            scores(&d.actions),
            vec!["-1.80 / +0.30".to_owned()],
            "{fmt:?}"
        );
    }
}

// ---- scoreToNag ----

#[rstest]
#[case(3.0, 18)]
#[case(2.99, 16)]
#[case(1.5, 16)]
#[case(1.49, 14)]
#[case(0.5, 14)]
#[case(0.49, 10)]
#[case(0.0, 10)]
#[case(-0.49, 10)]
#[case(-0.5, 10)]
#[case(-0.51, 15)]
#[case(-1.49, 15)]
#[case(-1.5, 17)]
#[case(-2.99, 17)]
#[case(-3.0, 19)]
#[case(327.47, 18)]
#[case(-327.47, 19)]
fn score_to_nag_follows_the_scids_threshold_order(#[case] score: f32, #[case] nag: u16) {
    let got = score_to_nag(score, &Informant::default()).map(|n| u16::from(n.value()));
    assert_eq!(got, Some(nag), "score {score}");
}

#[test]
fn score_to_nag_uses_the_configured_thresholds() {
    let inf = Informant {
        slight: 1.0,
        moderate: 2.0,
        decisive: 4.0,
        ..Informant::default()
    };
    let n = |s: f32| score_to_nag(s, &inf).map(|n| u16::from(n.value()));
    assert_eq!(n(0.99), Some(10));
    assert_eq!(n(1.0), Some(14));
    assert_eq!(n(2.0), Some(16));
    assert_eq!(n(4.0), Some(18));
    assert_eq!(n(-1.0), Some(10));
    assert_eq!(n(-1.01), Some(15));
    assert_eq!(n(-2.0), Some(17));
    assert_eq!(n(-4.0), Some(19));
}

#[test]
fn informant_defaults_are_the_scids() {
    let i = Informant::default();
    assert_eq!(
        [
            i.interesting,
            i.poor,
            i.blunder,
            i.dubious,
            i.slight,
            i.moderate,
            i.decisive,
            i.won
        ],
        [0.5, 1.5, 3.0, 0.5, 0.5, 1.5, 3.0, 5.5]
    );
}

// ---- appendTag ----

#[test]
fn append_tag_follows_the_scid() {
    assert_eq!(append_tag(None, "Stockfish"), "Stockfish");
    assert_eq!(append_tag(None, "  Stockfish  "), "Stockfish");
    assert_eq!(append_tag(Some("A"), "B"), "A , B");
    assert_eq!(append_tag(Some("A , B"), "B"), "A , B");
    assert_eq!(append_tag(Some("A , B"), "C"), "A , B , C");
}

#[test]
fn append_tag_keeps_the_scids_substring_quirk() {
    // `string match "*$value*"`: "113" already contains "13".
    assert_eq!(append_tag(Some("113"), "13"), "113");
    assert_eq!(append_tag(Some("10"), "13"), "10 , 13");
}

// ---- options and ranges ----

#[test]
fn options_defaults_are_the_scids_except_the_score_format() {
    let o = AnnotateOptions::default();
    assert_eq!(o.control, Control::Depth);
    assert_eq!(o.depth, 13);
    assert_eq!(o.movetime, Duration::from_millis(5000));
    assert!((o.blunder - 1.0).abs() < f32::EPSILON);
    assert!((o.cutoff - 5.0).abs() < f32::EPSILON);
    assert_eq!(o.with_score, WithScore::AllMoves);
    assert_eq!(o.with_vars, WithVars::Blunders);
    assert!(o.missed_mates && o.var_at_end && o.add_tag);
    assert_eq!(o.side, AnnotateSide::All);
    assert_eq!(o.score_format, ScoreFormat::Eval);
    assert_eq!(o.opening_only, None);
    assert!(!o.variations && !o.mark_exercises);
}

#[test]
fn ranges_follow_the_dialog_of_the_scid() {
    assert_eq!(mychess_app::DEPTH_RANGE, 10..=100);
    assert_eq!(mychess_app::MOVETIME_RANGE_MS, 100..=300_000);
    assert_eq!(mychess_app::BLUNDER_RANGE, 0.1..=3.0);
    assert_eq!(mychess_app::CUTOFF_RANGE, 3.0..=10.0);
    assert_eq!(mychess_app::OPENING_MOVES_RANGE, 5..=20);
    assert_eq!(mychess_app::DEFAULT_OPENING_MOVES, 10);
    assert_eq!(mychess_app::INFORMANT_RANGE, 0.0..=9.9);
}

#[test]
fn normalizing_clamps_hostile_values() {
    let o = AnnotateOptions {
        depth: u32::MAX,
        movetime: Duration::MAX,
        blunder: f32::NEG_INFINITY,
        cutoff: f32::NAN,
        opening_only: Some(255),
        ..AnnotateOptions::default()
    }
    .normalized();
    assert_eq!(o.depth, 100);
    assert_eq!(o.movetime, Duration::from_secs(300));
    assert!((o.blunder - 0.1).abs() < 1e-6);
    assert!(
        (o.cutoff - 5.0).abs() < f32::EPSILON,
        "NaN becomes the default"
    );
    assert_eq!(o.opening_only, Some(20));
    let low = AnnotateOptions {
        depth: 0,
        movetime: Duration::ZERO,
        opening_only: Some(0),
        ..AnnotateOptions::default()
    }
    .normalized();
    assert_eq!(low.depth, 10);
    assert_eq!(low.movetime, Duration::from_millis(100));
    assert_eq!(low.opening_only, Some(5));
    let inf = Informant {
        won: f32::NAN,
        slight: -1.0,
        decisive: f32::INFINITY,
        ..Informant::default()
    }
    .normalized();
    assert!((inf.won - 5.5).abs() < f32::EPSILON);
    assert!(inf.slight.abs() < f32::EPSILON);
    assert!((inf.decisive - 9.9).abs() < 1e-6);
}

#[test]
fn the_delay_preference_becomes_the_time_per_move() {
    assert_eq!(
        AnnotateOptions::with_delay_ms(5000).movetime,
        Duration::from_secs(5)
    );
    assert_eq!(
        AnnotateOptions::with_delay_ms(0).movetime,
        Duration::from_millis(100)
    );
    assert_eq!(
        AnnotateOptions::with_delay_ms(u32::MAX).movetime,
        Duration::from_secs(300)
    );
}

// ---- hostile input to the pure functions ----

fn survive(hostile: &[f32], mate: i32) {
    let f = fx();
    for vars in [
        WithVars::Blunders,
        WithVars::NotBest,
        WithVars::AllMoves,
        WithVars::No,
    ] {
        for &a in hostile {
            for &b in hostile {
                let mut o = opts();
                o.with_vars = vars;
                let mut s = step(&f, Color::White, eval(a, &[f.alt]), eval(b, &[]));
                s.prev.mate = Some(mate);
                s.cur.mate = Some(mate.wrapping_neg());
                drop(decide(&o, &s));
                s.mover = Color::Black;
                s.wanted_depth = 0;
                drop(decide(&o, &s));
                let _ = score_to_nag(a, &Informant::default());
                for fmt in [ScoreFormat::Plain, ScoreFormat::Bracket, ScoreFormat::Eval] {
                    drop(format_score_text(a, Some(mate), fmt));
                }
            }
        }
    }
}

#[test]
fn decide_survives_every_score_an_engine_can_report() {
    // `Score::Cp(i32::MAX)` is 21474836.0 pawns; NaN cannot come from an
    // engine but must not panic either.
    let extremes = [
        f32::NAN,
        21_474_836.0,
        -21_474_836.0,
        327.5,
        -327.5,
        0.0,
        -0.0,
        1e6,
    ];
    survive(&extremes, i32::MIN);
    survive(&extremes, i32::MAX);
    survive(&extremes, 0);
}

#[test]
fn decide_survives_non_finite_and_saturated_scores() {
    survive(
        &[f32::INFINITY, f32::NEG_INFINITY, f32::MAX, f32::MIN, 1e30],
        3,
    );
}

#[test]
fn empty_pv_means_no_variation_and_the_rest_still_applies() {
    // Rule 16.
    let f = fx();
    let d = decide(
        &opts(),
        &step(&f, Color::White, eval(0.3, &[]), eval(-1.8, &[])),
    );
    assert!(variations(&d.actions).is_empty());
    assert_eq!(nags_on(&d.actions, f.node), vec![2, 17]);
    assert_eq!(nags_on(&d.actions, f.parent), vec![201]);
    let _ = f.alt2;
}

// ---- exercises (markExercise) ----

fn ev(score: f32, second: Option<f32>, pv: &[Move]) -> PositionEval {
    PositionEval {
        score,
        mate: None,
        pv: pv.to_vec(),
        depth: 13,
        second,
    }
}

fn shot(prev: f32, cur: f32, second: Option<f32>, side: Color) -> Option<Move> {
    let start = Position::new();
    let e4 = start.parse_uci("e2e4").unwrap();
    exercise_move(
        &Informant::default(),
        &ev(prev, None, &[]),
        &ev(cur, second, &[e4]),
        side,
    )
}

#[rstest]
#[case::clear_shot(0.3, 3.0, Some(0.5), Color::White, true)]
#[case::gain_below_moderate(0.3, 1.7, Some(-3.0), Color::White, false)]
#[case::gain_exactly_moderate(0.0, 1.5, Some(-3.0), Color::White, true)]
#[case::already_won_before(6.0, 9.0, Some(0.0), Color::White, false)]
#[case::decided_and_less_than_double(4.0, 7.9, Some(0.0), Color::White, false)]
#[case::decided_and_exactly_double(3.5, 7.0, Some(0.0), Color::White, true)]
#[case::second_line_too_close(0.3, 3.0, Some(1.6), Color::White, false)]
#[case::second_line_gap_exactly_one_and_a_half(0.3, 3.0, Some(1.5), Color::White, true)]
#[case::no_second_line(0.3, 3.0, None, Color::White, false)]
#[case::two_winning_moves(0.3, 6.0, Some(4.0), Color::White, false)]
#[case::second_exactly_decisive(0.3, 6.0, Some(3.0), Color::White, true)]
#[case::best_move_still_loses_for_white(0.3, -2.0, Some(-5.0), Color::White, false)]
#[case::best_move_loses_exactly_moderate(3.0, -1.5, Some(-5.0), Color::White, true)]
#[case::best_move_still_loses_for_black(-0.3, 2.0, Some(5.0), Color::Black, false)]
#[case::black_shot(-0.3, -3.0, Some(-0.5), Color::Black, true)]
fn mark_exercise_filters(
    #[case] prev: f32,
    #[case] cur: f32,
    #[case] second: Option<f32>,
    #[case] side: Color,
    #[case] marked: bool,
) {
    assert_eq!(shot(prev, cur, second, side).is_some(), marked);
}

#[test]
fn an_exercise_without_a_best_line_move_is_not_marked() {
    let r = exercise_move(
        &Informant::default(),
        &ev(0.3, None, &[]),
        &ev(3.0, Some(0.5), &[]),
        Color::White,
    );
    assert_eq!(r, None);
}

#[test]
fn obvious_move_needs_the_same_move_at_every_depth() {
    let start = Position::new();
    let e4 = start.parse_uci("e2e4").unwrap();
    let d4 = start.parse_uci("d2d4").unwrap();
    assert!(is_obvious(e4, &[Some(e4), Some(e4), Some(e4)]));
    assert!(!is_obvious(e4, &[Some(e4), Some(d4), Some(e4)]));
    assert!(!is_obvious(e4, &[Some(e4), None, Some(e4)]));
    assert!(
        !is_obvious(e4, &[]),
        "no answers: nothing proves it obvious"
    );
}

#[test]
fn difficulty_is_two_plus_the_index_of_the_last_miss() {
    let start = Position::new();
    let e4 = start.parse_uci("e2e4").unwrap();
    let d4 = start.parse_uci("d2d4").unwrap();
    let all = [Some(e4); 8];
    assert_eq!(exercise_difficulty(e4, &all), 1);
    let mut miss_last = all;
    miss_last[7] = Some(d4);
    assert_eq!(exercise_difficulty(e4, &miss_last), 9);
    let mut miss_first = all;
    miss_first[0] = Some(d4);
    assert_eq!(exercise_difficulty(e4, &miss_first), 2);
    let mut interleaved = all;
    interleaved[1] = None;
    interleaved[3] = Some(d4);
    assert_eq!(
        exercise_difficulty(e4, &interleaved),
        5,
        "later hits do not matter"
    );
    assert_eq!(exercise_difficulty(e4, &[Some(d4); 8]), 9);
}

#[test]
fn exercise_comment_has_one_decimal() {
    assert_eq!(exercise_comment(5, 0.0, -8.0), "****D5 0.0->-8.0");
    assert_eq!(exercise_comment(1, 0.3, 2.94), "****D1 0.3->2.9");
}

#[test]
fn exercise_comment_rounds_like_tcl_doubles() {
    // Scid: `format %.1f` over `double(cp) / 100.0`. 15 cp is 0.1499999...
    // in double precision, so it prints `0.1`; 45 cp is 0.4500000...01 and
    // prints `0.5`. A single-precision value rounds both the other way.
    let cp = |c: i32| c as f32 / 100.0;
    assert_eq!(exercise_comment(2, cp(15), cp(45)), "****D2 0.1->0.5");
    assert_eq!(exercise_comment(2, cp(-15), cp(-45)), "****D2 -0.1->-0.5");
}

#[test]
fn the_tables_of_the_probes_are_the_scids() {
    assert_eq!(
        mychess_app::DIFFICULTY_TIMES_MS,
        [1, 2, 5, 10, 50, 100, 200, 1000]
    );
    assert_eq!(mychess_app::OBVIOUS_DEPTHS, [1, 2, 3]);
}

// ---- the book hook ----

struct Fixed(Vec<(String, Vec<&'static str>)>);

impl BookProbe for Fixed {
    fn moves(&self, position: &Position) -> Vec<Move> {
        let fen = position.fen();
        self.0
            .iter()
            .find(|(f, _)| *f == fen)
            .map(|(_, ms)| {
                ms.iter()
                    .filter_map(|u| position.parse_uci(u).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn name(&self) -> &str {
        "Fixed"
    }
}

#[test]
fn the_book_classifies_in_last_and_out_of_book_like_the_scid() {
    let start = Position::new();
    let e4 = start.parse_uci("e2e4").unwrap();
    let d4 = start.parse_uci("d2d4").unwrap();
    let a3 = start.parse_uci("a2a3").unwrap();
    let mut after = start.clone();
    after.play(e4).unwrap();
    let book = Fixed(vec![
        (start.fen(), vec!["e2e4", "d2d4"]),
        (after.fen(), vec!["e7e5"]),
    ]);
    assert_eq!(classify_book_move(&book, &start, e4), BookVerdict::InBook);
    // d4 is in the list but the position after it has no book moves.
    assert_eq!(
        classify_book_move(&book, &start, d4),
        BookVerdict::LastBookMove
    );
    assert_eq!(
        classify_book_move(&book, &start, a3),
        BookVerdict::OutOfBook(vec!["e4".to_owned(), "d4".to_owned()])
    );
    assert_eq!(book.name(), "Fixed");
}

#[test]
fn book_comments_are_written_in_english_like_the_scid() {
    assert_eq!(
        out_of_book_comment("Fixed", &["e4".to_owned(), "c4".to_owned()]),
        "Move out of book (Fixed: e4 c4)"
    );
    assert_eq!(last_book_move_comment("Fixed"), "Last book move (Fixed)");
}

#[test]
fn out_of_book_comment_without_book_moves_omits_the_list() {
    // analysis.tcl 1283-1287: with no previous book moves the text is
    // "Move out of book (<book>)", without the colon.
    assert_eq!(
        out_of_book_comment("Fixed", &[]),
        "Move out of book (Fixed)"
    );
}

// ---- notices ----

#[test]
fn notice_keys_are_unique_stable_and_prefixed() {
    let all = [
        AnnotateNotice::AlreadyRunning,
        AnnotateNotice::NoEngine,
        AnnotateNotice::TrialMode,
        AnnotateNotice::NothingToAnnotate,
        AnnotateNotice::Busy,
        AnnotateNotice::Finished {
            plies: 1,
            variations: 2,
        },
        AnnotateNotice::Cancelled { plies: 1 },
        AnnotateNotice::GameChanged { plies: 1 },
        AnnotateNotice::EngineFailed { plies: 1 },
        AnnotateNotice::NoScore { plies: 1 },
    ];
    let keys: Vec<&str> = all.iter().map(AnnotateNotice::message_key).collect();
    let unique: HashSet<&&str> = keys.iter().collect();
    assert_eq!(unique.len(), keys.len(), "{keys:?}");
    assert!(keys.iter().all(|k| k.starts_with("annotate-notice-")));
    // The payload does not change the key.
    assert_eq!(
        AnnotateNotice::Finished {
            plies: 9,
            variations: 9
        }
        .message_key(),
        AnnotateNotice::Finished {
            plies: 1,
            variations: 2
        }
        .message_key()
    );
    assert_eq!(keys[0], "annotate-notice-already-running");
    assert_eq!(keys[4], "annotate-notice-busy");
}

#[test]
fn every_outcome_maps_to_a_notice_with_its_numbers() {
    let report = |outcome| AnnotateReport {
        outcome,
        plies: 7,
        total: 40,
        variations: 2,
        exercises: 1,
    };
    assert_eq!(
        AnnotateNotice::of_report(&report(AnnotateOutcome::Finished)),
        AnnotateNotice::Finished {
            plies: 7,
            variations: 2
        }
    );
    assert_eq!(
        AnnotateNotice::of_report(&report(AnnotateOutcome::Cancelled)),
        AnnotateNotice::Cancelled { plies: 7 }
    );
    assert_eq!(
        AnnotateNotice::of_report(&report(AnnotateOutcome::GameChanged)),
        AnnotateNotice::GameChanged { plies: 7 }
    );
    assert_eq!(
        AnnotateNotice::of_report(&report(AnnotateOutcome::Failed(
            AnnotationError::EngineFailed
        ))),
        AnnotateNotice::EngineFailed { plies: 7 }
    );
    assert_eq!(
        AnnotateNotice::of_report(&report(AnnotateOutcome::Failed(AnnotationError::NoScore))),
        AnnotateNotice::NoScore { plies: 7 }
    );
    assert_eq!(
        AnnotateNotice::of_report(&report(AnnotateOutcome::Failed(
            AnnotationError::NothingToAnnotate
        ))),
        AnnotateNotice::NothingToAnnotate
    );
}

// ---- no moves score ----

#[test]
fn no_moves_scores_are_327_for_mate_and_zero_for_stalemate() {
    let mated = Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
        .unwrap();
    assert_eq!(PositionEval::no_moves(&mated).score, -327.0);
    // Black mated: +327 (analysis.tcl 2682-2696).
    let black_mated = Position::from_fen("k7/1Q6/1K6/8/8/8/8/8 b - - 0 1").unwrap();
    assert!(black_mated.legal_moves().is_empty() && black_mated.is_check());
    assert_eq!(PositionEval::no_moves(&black_mated).score, 327.0);
    let stalemate = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    assert_eq!(PositionEval::no_moves(&stalemate).score, 0.0);
    assert_eq!(mychess_app::NO_MOVES_PAWNS, 327.0);
}

// ---- properties ----

mod properties {
    use super::*;
    use proptest::prelude::*;

    fn rank(nag: u16) -> i32 {
        // The order of `scoreToNag`, worst for White first.
        match nag {
            19 => 0,
            17 => 1,
            15 => 2,
            10 => 3,
            14 => 4,
            16 => 5,
            18 => 6,
            other => panic!("unexpected nag {other}"),
        }
    }

    fn vars() -> impl Strategy<Value = WithVars> {
        prop_oneof![
            Just(WithVars::Blunders),
            Just(WithVars::NotBest),
            Just(WithVars::AllMoves),
            Just(WithVars::No)
        ]
    }

    fn scores() -> impl Strategy<Value = WithScore> {
        prop_oneof![
            Just(WithScore::AllMoves),
            Just(WithScore::Blunders),
            Just(WithScore::Variations),
            Just(WithScore::No)
        ]
    }

    proptest! {
        #[test]
        fn score_to_nag_is_monotone_in_the_score(a in -40_000i32..40_000, b in -40_000i32..40_000) {
            let inf = Informant::default();
            let n = |cp: i32| score_to_nag(cp as f32 / 100.0, &inf).map(|n| u16::from(n.value())).unwrap();
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            // `-0.50` is `=` but `-0.51` is `-=`: still monotone.
            prop_assert!(rank(n(lo)) <= rank(n(hi)), "{lo} -> {}, {hi} -> {}", n(lo), n(hi));
        }

        #[test]
        fn decisions_only_touch_the_move_and_its_parent(
            prev in -1500i32..1500,
            cur in -1500i32..1500,
            white in any::<bool>(),
            v in vars(),
            ws in scores(),
            cutoff in 3.0f32..10.0,
            blunder in 0.1f32..3.0,
            has_pv in any::<bool>(),
            last in any::<bool>(),
        ) {
            let f = fx();
            let mut o = opts();
            o.with_vars = v;
            o.with_score = ws;
            o.cutoff = cutoff;
            o.blunder = blunder;
            let pv: Vec<Move> = if has_pv { vec![f.alt] } else { Vec::new() };
            let mover = if white { Color::White } else { Color::Black };
            let mut s = step(&f, mover, eval(prev as f32 / 100.0, &pv), eval(cur as f32 / 100.0, &[]));
            s.is_last = last;
            let d = decide(&o, &s);
            for action in &d.actions {
                match action {
                    Action::Score { node, .. } | Action::Exercise { node, .. } | Action::ExerciseCandidate { node } | Action::EndComment { node, .. } => prop_assert_eq!(*node, f.node),
                    Action::Nag { node, nag } => {
                        prop_assert!(*node == f.node || (*node == f.parent && nag.value() == 201));
                    }
                    Action::Variation { parent, pv: line, .. } => {
                        prop_assert_eq!(*parent, f.parent);
                        prop_assert_eq!(line, &pv);
                    }
                    Action::InitialComment { .. } => prop_assert!(false, "not for a move"),
                }
            }
            // A candidate exercise always comes with `?` or `??`.
            if has_candidate(&d.actions) {
                let n = nags_on(&d.actions, f.node);
                prop_assert!(n.contains(&2) || n.contains(&4), "{n:?}");
            }
            // At most one score and one variation per move.
            prop_assert!(scores_len(&d.actions) <= 1);
            prop_assert!(variations(&d.actions).len() <= 1);
            // No annotation of the other side, ever.
            let mut w = o;
            w.side = if white { AnnotateSide::Black } else { AnnotateSide::White };
            prop_assert!(decide(&w, &s).actions.is_empty());
        }

        #[test]
        fn scores_in_the_decision_are_the_same_for_equal_inputs(prev in -900i32..900, cur in -900i32..900) {
            let f = fx();
            let s = step(&f, Color::White, eval(prev as f32 / 100.0, &[f.alt]), eval(cur as f32 / 100.0, &[]));
            prop_assert_eq!(decide(&opts(), &s), decide(&opts(), &s));
        }

        #[test]
        fn append_tag_never_loses_the_old_text_and_is_idempotent(old in "[A-Za-z0-9 ,]{0,12}", new in "[A-Za-z0-9]{1,8}") {
            let once = append_tag(Some(&old), &new);
            prop_assert!(once.contains(&old));
            prop_assert!(once.contains(&new));
            prop_assert_eq!(append_tag(Some(&once), &new), once);
        }
    }

    fn scores_len(actions: &[Action]) -> usize {
        super::scores(actions).len()
    }
}
