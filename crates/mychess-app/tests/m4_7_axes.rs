//! Contract tests of the axes, tick marks and click mapping (M4-7).

// Test code: exact float comparisons are intended (values are built from small
// integers) and the casts are bounded by the tiny sizes used here.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::field_reassign_with_default,
    clippy::float_cmp,
    clippy::format_push_string,
    clippy::single_match_else,
    clippy::type_complexity,
    clippy::unnecessary_wraps,
    clippy::useless_vec
)]

mod m4_7_common;

use m4_7_common::game_with;
use mychess_app::graphs::{
    Axis, GraphKind, GraphPrefs, MAX_TICKS, ply_at_x, score_graph_view, time_step, x_of_ply,
};
use proptest::prelude::*;
use rstest::rstest;

fn axis(min: f32, max: f32, step: f32) -> Axis {
    Axis {
        min,
        max,
        label_step: step,
        minor_step: step,
        major_step: Some(step),
    }
}

fn x_axis_of(plies: usize) -> Axis {
    let comments = vec!["[%eval 1]"; plies];
    let g = game_with(&comments);
    score_graph_view(
        &g,
        GraphPrefs {
            kind: GraphKind::Score,
            ..GraphPrefs::default()
        },
    )
    .x_axis
}

#[rstest]
#[case(0, 0.5)]
#[case(1, 1.0)]
#[case(2, 1.5)]
#[case(3, 2.0)]
#[case(10, 5.5)]
fn x_of_ply_matches_scid(#[case] ply: usize, #[case] x: f32) {
    assert_eq!(x_of_ply(ply), x);
}

#[rstest]
#[case(0.0, Some(0))]
#[case(0.5, Some(0))]
#[case(0.74, Some(0))]
#[case(0.75, Some(1))]
#[case(1.0, Some(1))]
#[case(1.24, Some(1))]
#[case(1.5, Some(2))]
#[case(2.0, Some(3))]
#[case(4.5, Some(8))]
#[case(100.0, None)]
#[case(-0.01, None)]
#[case(f32::NAN, None)]
#[case(f32::INFINITY, None)]
#[case(f32::NEG_INFINITY, None)]
fn click_maps_x_to_ply(#[case] x: f32, #[case] want: Option<usize>) {
    let a = x_axis_of(12);
    assert_eq!(ply_at_x(&a, x, 12), want, "x = {x}");
}

#[test]
fn click_is_clamped_to_the_number_of_plies() {
    let a = axis(0.0, 50.0, 5.0);
    assert_eq!(ply_at_x(&a, 50.0, 7), Some(7));
    assert_eq!(ply_at_x(&a, 20.0, 7), Some(7));
    assert_eq!(ply_at_x(&a, 2.0, 0), Some(0));
    assert_eq!(ply_at_x(&a, 0.0, 0), Some(0));
}

#[test]
fn click_on_the_axis_limits_is_accepted_and_just_outside_is_not() {
    let a = x_axis_of(12);
    assert_eq!(ply_at_x(&a, a.min, 12), Some(0));
    assert!(ply_at_x(&a, a.max, 12).is_some());
    assert_eq!(ply_at_x(&a, a.max + 0.01, 12), None);
    assert_eq!(ply_at_x(&a, a.min - 0.01, 12), None);
}

#[test]
fn clicking_on_each_bar_goes_to_its_ply() {
    let comments = vec!["[%eval 1]"; 30];
    let g = game_with(&comments);
    let v = score_graph_view(
        &g,
        GraphPrefs {
            kind: GraphKind::Score,
            ..GraphPrefs::default()
        },
    );
    for (i, &(x, _)) in v.bars.iter().enumerate() {
        assert_eq!(ply_at_x(&v.x_axis, x, v.plies), Some(i + 1));
    }
}

#[test]
fn click_with_degenerate_axis_does_not_panic() {
    // A NaN-bounded axis cannot be built by the view; only no-panic matters.
    let a = axis(f32::NAN, f32::NAN, 1.0);
    assert!(ply_at_x(&a, 1.0, 5).is_none_or(|p| p <= 5));
    let a = axis(5.0, 1.0, 1.0);
    assert_eq!(ply_at_x(&a, 3.0, 5), None);
}

// ----------------------------------------------------------------- ticks

#[test]
fn ticks_have_no_floating_point_drift() {
    let a = axis(0.0, 1000.0, 0.1);
    let ticks: Vec<f32> = a.ticks(0.1).collect();
    assert_eq!(ticks.len(), 10_001);
    for (k, t) in ticks.iter().enumerate() {
        assert_eq!(*t, k as f32 * 0.1_f32, "tick {k}");
    }
}

#[test]
fn integer_steps_are_exact_over_long_axes() {
    let a = axis(0.0, 15_000.0, 5.0);
    let ticks: Vec<f32> = a.ticks(5.0).collect();
    assert_eq!(ticks.len(), 3001);
    assert!(ticks.iter().enumerate().all(|(k, t)| *t == (k * 5) as f32));
}

#[test]
fn ticks_include_negative_positions_and_zero_in_order() {
    let a = axis(-3.0, 6.0, 1.0);
    let t: Vec<f32> = a.ticks(5.0).collect();
    assert_eq!(t, vec![0.0, 5.0]);
    let t: Vec<f32> = a.minor_ticks().collect();
    assert_eq!(t.first(), Some(&-3.0));
    assert_eq!(t.last(), Some(&6.0));
    assert_eq!(t.len(), 10);
}

#[test]
fn ticks_are_capped() {
    let a = axis(0.0, f32::MAX, 1.0);
    assert_eq!(a.minor_ticks().count(), MAX_TICKS);
    let a = axis(-f32::MAX, f32::MAX, 1.0);
    assert!(a.minor_ticks().count() <= MAX_TICKS);
    let a = axis(0.0, 1e12, 0.001);
    assert!(a.minor_ticks().count() <= MAX_TICKS);
}

#[test]
fn ticks_with_invalid_step_or_range_are_empty() {
    let a = axis(0.0, 10.0, 1.0);
    for step in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(a.ticks(step).count(), 0, "step {step}");
    }
    assert_eq!(axis(10.0, 0.0, 1.0).minor_ticks().count(), 0);
    assert_eq!(axis(f32::NAN, 5.0, 1.0).minor_ticks().count(), 0);
    assert_eq!(axis(0.0, f32::INFINITY, 1.0).minor_ticks().count(), 0);
}

#[test]
fn major_ticks_are_empty_without_a_major_step() {
    let mut a = axis(0.0, 10.0, 1.0);
    a.major_step = None;
    assert_eq!(a.major_ticks().count(), 0);
}

#[test]
fn x_axis_of_the_view_has_grid_every_one_and_highlight_every_five() {
    let a = x_axis_of(20);
    assert_eq!(a.minor_ticks().count(), a.max as usize + 1);
    assert_eq!(
        a.major_ticks().collect::<Vec<_>>(),
        (0..=(a.max as usize / 5))
            .map(|k| (k * 5) as f32)
            .collect::<Vec<_>>()
    );
}

#[rstest]
#[case(0, 2.0)]
#[case(20, 2.0)]
#[case(21, 5.0)]
#[case(50, 5.0)]
#[case(51, 10.0)]
#[case(100, 10.0)]
#[case(101, 20.0)]
#[case(500, 20.0)]
#[case(501, 50.0)]
#[case(1000, 50.0)]
#[case(1001, 100.0)]
#[case(2000, 100.0)]
#[case(2001, 200.0)]
#[case(u32::MAX, 200.0)]
fn time_step_table(#[case] max_emt: u32, #[case] step: f32) {
    assert_eq!(time_step(max_emt), step);
}

proptest! {
    #[test]
    fn ply_at_x_never_panics_and_is_bounded(x in any::<f32>(), plies in 0usize..50_000) {
        let a = x_axis_of(3);
        if let Some(p) = ply_at_x(&a, x, plies) {
            prop_assert!(p <= plies);
        }
        if !x.is_finite() {
            prop_assert_eq!(ply_at_x(&a, x, plies), None);
        }
    }

    #[test]
    fn click_round_trips_on_every_bar(ply in 1usize..40_000) {
        let a = Axis { min: 0.0, max: x_of_ply(ply) + 5.0, label_step: 5.0, minor_step: 1.0, major_step: Some(5.0) };
        prop_assert_eq!(ply_at_x(&a, x_of_ply(ply), ply), Some(ply));
    }

    #[test]
    fn ticks_are_increasing_bounded_and_inside(
        min in -1e5f32..1e5,
        len in 0.0f32..1e5,
        step in 0.001f32..1000.0,
    ) {
        let a = axis(min, min + len, step);
        let t: Vec<f32> = a.ticks(step).collect();
        prop_assert!(t.len() <= MAX_TICKS);
        prop_assert!(t.windows(2).all(|w| w[0] < w[1]));
        // A tick may sit one `f32` rounding away from the end of the axis: the
        // spacing of the floats grows with the magnitude of the axis.
        let eps = 1e-3 * step + 4.0 * f32::EPSILON * a.min.abs().max(a.max.abs());
        for v in t {
            prop_assert!(v >= a.min - eps && v <= a.max + eps, "{v} outside {a:?}");
        }
    }

    #[test]
    fn time_step_is_monotonic(a in any::<u32>(), b in any::<u32>()) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(time_step(lo) <= time_step(hi));
    }
}
