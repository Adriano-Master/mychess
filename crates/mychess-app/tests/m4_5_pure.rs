//! M4-5 contract tests of the pure mappings: bar fill (linear and
//! logarithmic, D453), ticks, labels (D454) and the traffic light colours
//! (D458). Derived from the spec and `board.tcl` 3196-3205 and 3353-3369.
#![allow(
    clippy::assert_is_empty,
    clippy::bool_assert_comparison,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp
)]

use mychess_app::{EvalValue, TerminalResult, bar_fill, bar_ticks, traffic_light_color};
use mychess_engine::Bound;
use proptest::prelude::*;
use rstest::rstest;

fn fill(e: f32, scale: u8, log: bool) -> f32 {
    bar_fill(EvalValue::Pawns(e), scale, log)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 2e-3
}

#[rstest]
#[case(0.0, 5, 0.5)]
#[case(0.5, 5, 0.55)]
#[case(-0.5, 5, 0.45)]
#[case(1.0, 5, 0.6)]
#[case(2.0, 5, 0.7)]
#[case(3.16, 5, 0.816)]
#[case(5.0, 5, 1.0)]
#[case(9.0, 5, 1.0)]
#[case(-12.6, 5, 0.0)]
#[case(327.0, 5, 1.0)]
#[case(-327.0, 5, 0.0)]
#[case(0.5, 1, 0.75)]
#[case(1.0, 1, 1.0)]
#[case(-2.0, 1, 0.0)]
#[case(1.0, 10, 0.55)]
#[case(3.16, 10, 0.658)]
#[case(9.0, 10, 0.95)]
#[case(32_700.0, 10, 1.0)]
#[case(-32_700.0, 10, 0.0)]
fn linear_fill_follows_the_formula(#[case] eval: f32, #[case] scale: u8, #[case] expected: f32) {
    let got = fill(eval, scale, false);
    assert!(
        close(got, expected),
        "eval {eval} scale {scale}: {got} != {expected}"
    );
}

#[rstest]
#[case(0.0, 0.5)]
#[case(0.5, 0.5625)]
#[case(1.0, 0.625)]
#[case(2.0, 0.75)]
#[case(-2.0, 0.25)]
#[case(3.16, 0.812)]
#[case(5.0, 0.874)]
#[case(-5.0, 0.126)]
#[case(9.0, 0.954)]
#[case(12.6, 0.9995)]
#[case(-12.6, 0.0005)]
#[case(327.0, 1.0)]
#[case(-327.0, 0.0)]
#[case(32_700.0, 1.0)]
#[case(-32_700.0, 0.0)]
fn log_fill_follows_the_scid_formula(#[case] eval: f32, #[case] expected: f32) {
    // The scale does not matter in the logarithmic mapping.
    for scale in [1u8, 5, 10] {
        let got = fill(eval, scale, true);
        assert!(
            close(got, expected),
            "eval {eval} scale {scale}: {got} != {expected}"
        );
    }
}

#[test]
fn log_fill_is_continuous_at_the_two_pawn_knee() {
    let below = fill(1.999, 5, true);
    let above = fill(2.001, 5, true);
    assert!((above - below).abs() < 0.01, "{below} vs {above}");
}

#[test]
fn zero_is_half_in_both_mappings_and_for_every_scale() {
    for scale in 1..=10u8 {
        for log in [false, true] {
            assert_eq!(fill(0.0, scale, log), 0.5, "scale {scale} log {log}");
        }
    }
}

#[test]
fn scale_outside_the_range_is_limited() {
    assert!(close(fill(1.0, 0, false), fill(1.0, 1, false)));
    assert!(close(fill(1.0, 11, false), fill(1.0, 10, false)));
    assert!(close(fill(1.0, 255, false), fill(1.0, 10, false)));
}

#[test]
fn mates_saturate_and_results_use_the_ends_or_the_middle() {
    for log in [false, true] {
        assert!(close(bar_fill(EvalValue::Mate(1), 5, log), 1.0));
        assert!(close(bar_fill(EvalValue::Mate(30), 5, log), 1.0));
        assert!(close(bar_fill(EvalValue::Mate(-1), 5, log), 0.0));
        assert!(close(bar_fill(EvalValue::Mate(-30), 5, log), 0.0));
        let t = |r| bar_fill(EvalValue::Terminal(r), 5, log);
        assert!(close(t(TerminalResult::WhiteWins), 1.0));
        assert!(close(t(TerminalResult::BlackWins), 0.0));
        assert!(close(t(TerminalResult::Draw), 0.5));
    }
}

#[test]
fn absurd_mate_distances_do_not_panic_and_keep_their_side() {
    for log in [false, true] {
        assert!(bar_fill(EvalValue::Mate(i32::MIN), 5, log) <= 0.5);
        assert!(bar_fill(EvalValue::Mate(i32::MAX), 5, log) >= 0.5);
        assert!(bar_fill(EvalValue::Mate(0), 5, log).is_finite());
    }
}

#[test]
fn nan_falls_to_the_middle() {
    for log in [false, true] {
        assert_eq!(fill(f32::NAN, 5, log), 0.5);
    }
}

/// Rule 1 of the spec says NaN *and infinities* fall to 0.5.
#[test]
fn infinities_fall_to_the_middle_per_spec_rule_1() {
    for log in [false, true] {
        assert_eq!(fill(f32::INFINITY, 5, log), 0.5);
        assert_eq!(fill(f32::NEG_INFINITY, 5, log), 0.5);
    }
}

#[test]
fn infinities_stay_inside_the_bar() {
    for log in [false, true] {
        for e in [f32::INFINITY, f32::NEG_INFINITY] {
            assert!((0.0..=1.0).contains(&fill(e, 5, log)));
        }
    }
}

#[test]
fn linear_ticks_are_the_inner_integers_of_the_scale() {
    // scale 3: -2 -1 0 1 2 -> 1/6 .. 5/6
    let t = bar_ticks(3, false);
    let want = [1.0 / 6.0, 2.0 / 6.0, 0.5, 4.0 / 6.0, 5.0 / 6.0];
    assert_eq!(t.len(), want.len());
    for (a, b) in t.iter().zip(want) {
        assert!(close(*a, b), "{t:?}");
    }
    assert_eq!(
        bar_ticks(1, false),
        vec![0.5],
        "scale 1 has only the 0 tick"
    );
    assert_eq!(bar_ticks(5, false).len(), 9);
    assert_eq!(bar_ticks(10, false).len(), 19);
}

#[test]
fn log_ticks_sit_at_the_scid_values() {
    let t = bar_ticks(5, true);
    let values = [-9.0f32, -5.0, -3.0, -1.0, 0.0, 1.0, 3.0, 5.0, 9.0];
    assert_eq!(t.len(), values.len());
    for (tick, e) in t.iter().zip(values) {
        assert!(close(*tick, fill(e, 5, true)), "tick for {e}");
    }
    assert_eq!(
        bar_ticks(1, true),
        bar_ticks(10, true),
        "scale is irrelevant"
    );
}

#[test]
fn ticks_are_sorted_and_symmetric() {
    for scale in 1..=10u8 {
        for log in [false, true] {
            let t = bar_ticks(scale, log);
            assert!(t.windows(2).all(|w| w[0] < w[1]), "scale {scale}");
            for (a, b) in t.iter().zip(t.iter().rev()) {
                assert!(close(*a + *b, 1.0));
            }
        }
    }
}

#[test]
fn ticks_with_hostile_scales_do_not_panic() {
    for scale in [0u8, 11, 255] {
        for log in [false, true] {
            assert!(
                bar_ticks(scale, log)
                    .iter()
                    .all(|t| (0.0..=1.0).contains(t))
            );
        }
    }
}

#[rstest]
#[case(EvalValue::Pawns(0.35), Bound::Exact, "+0.35")]
#[case(EvalValue::Pawns(-1.2), Bound::Exact, "-1.20")]
#[case(EvalValue::Pawns(0.0), Bound::Exact, "+0.00")]
#[case(EvalValue::Mate(3), Bound::Exact, "+M3")]
#[case(EvalValue::Mate(-3), Bound::Exact, "-M3")]
#[case(EvalValue::Pawns(327.0), Bound::Exact, "#")]
#[case(EvalValue::Pawns(-327.0), Bound::Exact, "-#")]
#[case(EvalValue::Terminal(TerminalResult::Draw), Bound::Exact, "=")]
#[case(EvalValue::Terminal(TerminalResult::WhiteWins), Bound::Exact, "M0")]
#[case(EvalValue::Terminal(TerminalResult::BlackWins), Bound::Exact, "M0")]
#[case(EvalValue::Pawns(0.5), Bound::Lower, "\u{2265}+0.50")]
#[case(EvalValue::Pawns(-0.5), Bound::Upper, "\u{2264}-0.50")]
fn labels(#[case] value: EvalValue, #[case] bound: Bound, #[case] text: &str) {
    assert_eq!(value.label(bound), text);
}

#[test]
fn label_of_a_nan_or_infinity_does_not_panic() {
    for p in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.0e30] {
        drop(EvalValue::Pawns(p).label(Bound::Exact));
    }
}

#[rstest]
#[case(0.0, "#f8f800")]
#[case(0.2, "#f8f800")]
// Scid `elseif {$eval > -0.2}` (board.tcl 3359): -0.2 is already red.
#[case(-0.2, "#f8f000")]
#[case(0.1, "#f8f800")]
#[case(0.25, "#edf800")]
#[case(1.0, "#c9f800")]
#[case(5.0, "#09f800")]
#[case(9.0, "#09f800")]
#[case(327.0, "#09f800")]
#[case(-0.25, "#f8ed00")]
#[case(-1.0, "#f8c900")]
#[case(-5.0, "#f80900")]
#[case(-9.0, "#f80900")]
#[case(-327.0, "#f80900")]
fn traffic_light_table_of_the_spec(#[case] eval: f32, #[case] hex: &str) {
    assert_eq!(traffic_light_color(eval).hex(), hex);
}

#[test]
fn traffic_light_threshold_is_the_f32_literal_and_the_next_float_flips_it() {
    let up = 0.2f32.next_up();
    let down = (-0.2f32).next_down();
    assert_eq!(traffic_light_color(0.2f32).hex(), "#f8f800");
    assert_eq!(traffic_light_color(-0.2f32).hex(), "#f8f000");
    let c = traffic_light_color(up);
    assert_eq!((c.g, c.b), (0xf8, 0), "just above 0.2 is green");
    assert!(c.r >= 0xee && c.r < 0xf8, "{}", c.hex());
    let c = traffic_light_color(down);
    assert_eq!((c.r, c.b), (0xf8, 0), "just below -0.2 is red");
    assert!(c.g >= 0xee && c.g < 0xf8, "{}", c.hex());
}

#[test]
fn traffic_light_hostile_values_do_not_panic() {
    for e in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MAX,
        f32::MIN,
    ] {
        let _ = traffic_light_color(e);
    }
    // Rule 1: infinities carry no advantage colour.
    assert_eq!(traffic_light_color(f32::INFINITY).hex(), "#f8f800");
    assert_eq!(traffic_light_color(f32::NEG_INFINITY).hex(), "#f8f800");
}

proptest! {
    #[test]
    fn fill_is_bounded_monotonic_and_symmetric(
        a in -400.0f32..400.0, b in -400.0f32..400.0, scale in 0u8..=12, log: bool,
    ) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let (flo, fhi) = (fill(lo, scale, log), fill(hi, scale, log));
        prop_assert!((0.0..=1.0).contains(&flo) && (0.0..=1.0).contains(&fhi));
        prop_assert!(flo <= fhi + 1e-6);
        prop_assert!((fill(a, scale, log) + fill(-a, scale, log) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn any_bit_pattern_stays_inside_the_bar(bits: u32, scale: u8, log: bool) {
        let f = fill(f32::from_bits(bits), scale, log);
        prop_assert!((0.0..=1.0).contains(&f));
    }

    #[test]
    fn linear_fill_is_the_exact_formula_inside_the_scale(
        e in -10.0f32..10.0, scale in 1u8..=10,
    ) {
        let s = f32::from(scale);
        let want = (s + e.clamp(-s, s)) / (2.0 * s);
        prop_assert!((fill(e, scale, false) - want).abs() < 1e-5);
    }

    #[test]
    fn mate_fill_depends_only_on_the_side(n in -5000i32..5000, log: bool) {
        prop_assume!(n != 0);
        let f = bar_fill(EvalValue::Mate(n), 5, log);
        let ok = if n > 0 { f > 0.999 } else { f < 0.001 };
        prop_assert!(ok, "fill {}", f);
    }

    #[test]
    fn traffic_light_never_panics_and_keeps_its_shape(e in any::<f32>()) {
        let c = traffic_light_color(e);
        prop_assert!(c.b == 0);
        prop_assert!(c.r == 0xf8 || c.g == 0xf8);
    }

    #[test]
    fn traffic_light_is_mirrored_for_the_two_sides(e in 0.21f32..400.0) {
        let g = traffic_light_color(e);
        let r = traffic_light_color(-e);
        prop_assert_eq!((g.r, g.g), (r.g, r.r));
    }
}
