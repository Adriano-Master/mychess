//! Axes, tick tables and the click-to-ply mapping.

use super::series::SCORE_LIMIT;

/// Most labels, in the Scid-sized range, an axis aims at; beyond it the
/// steps grow along the 1-2-5 progression (D771).
const MAX_LABELS: f32 = 20.0;
/// Most light grid lines an axis aims at (D771).
const MAX_LINES: f32 = 200.0;

/// Upper bound on the ticks an [`Axis`] iterator yields, so a corrupt range
/// can never allocate or loop for long.
pub const MAX_TICKS: usize = 20_000;

/// The x coordinate of the point after `ply` halfmoves: `ply * 0.5 + 0.5`.
pub fn x_of_ply(ply: usize) -> f32 {
    ply as f32 * 0.5 + 0.5
}

/// Maps the x coordinate of a click to a main-line ply (Scid's
/// `round(x * 2) - 1`, `graphs.tcl` `Move`), clamped to `0..=plies`.
///
/// Returns `None` for a non finite `x` or one outside `[axis.min, axis.max]`
/// (a click outside the plot is ignored). The result is the argument of the
/// "go to main-line ply" navigation.
pub fn ply_at_x(axis: &Axis, x: f32, plies: usize) -> Option<usize> {
    // Affirmative form: a NaN `x` or NaN limit fails the comparison.
    if !(x.is_finite() && x >= axis.min && x <= axis.max) {
        return None;
    }
    let ply = (x * 2.0).round() - 1.0;
    if ply <= 0.0 {
        return Some(0);
    }
    Some((ply as usize).min(plies))
}

/// A numeric axis with its grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    /// Lower bound.
    pub min: f32,
    /// Upper bound (always greater than `min`).
    pub max: f32,
    /// Distance between labelled ticks.
    pub label_step: f32,
    /// Distance between the light grid lines.
    pub minor_step: f32,
    /// Distance between the highlighted grid lines (`steelblue` in Scid).
    pub major_step: Option<f32>,
}

impl Axis {
    /// The positions `k * step` inside the axis, in increasing order, at most
    /// [`MAX_TICKS`]. Empty for a non positive or non finite `step`.
    pub fn ticks(&self, step: f32) -> impl Iterator<Item = f32> + use<> {
        let ok = step.is_finite() && step > 0.0 && self.min.is_finite() && self.max.is_finite();
        let first = if ok {
            (self.min / step).ceil() as i64
        } else {
            0
        };
        let last = if ok {
            (self.max / step).floor() as i64
        } else {
            -1
        };
        let count = if last >= first {
            usize::try_from(i128::from(last) - i128::from(first) + 1).unwrap_or(usize::MAX)
        } else {
            0
        };
        (0..count.min(MAX_TICKS)).map(move |i| (first + i as i64) as f32 * step)
    }

    /// Positions of the labels.
    pub fn label_ticks(&self) -> impl Iterator<Item = f32> + use<> {
        self.ticks(self.label_step)
    }

    /// Positions of the light grid lines.
    pub fn minor_ticks(&self) -> impl Iterator<Item = f32> + use<> {
        self.ticks(self.minor_step)
    }

    /// Positions of the highlighted grid lines (none without a major step).
    pub fn major_ticks(&self) -> impl Iterator<Item = f32> + use<> {
        self.ticks(self.major_step.unwrap_or(0.0))
    }

    /// The x axis: from 0, wide enough for a bar centred on `max_x` (at
    /// least 5). Up to 100 the steps are Scid's (grid every 1, labels and
    /// highlight every 5); beyond, the steps follow the 1-2-5 progression so
    /// that there are at most 20 labels and 200 grid lines (D771).
    pub(super) fn x(max_x: f32) -> Axis {
        let max_x = if max_x.is_finite() {
            max_x.max(0.0)
        } else {
            0.0
        };
        let raw = max_x + 0.5;
        let label = pick_125((raw / MAX_LABELS).max(5.0));
        let max = ((raw / label).ceil() * label).max(label);
        // The light lines divide the label step.
        let mut minor = pick_125((max / MAX_LINES).max(1.0));
        while minor < label && (label / minor).fract() != 0.0 {
            minor = pick_125(minor * 1.01);
        }
        Axis {
            min: 0.0,
            max,
            label_step: label,
            minor_step: minor.min(label),
            major_step: Some(label),
        }
    }

    /// The score y axis: whole pawns, at least `-0.9..=0.9` visible (so
    /// `-1..=1`), at most +-10, light lines every 1 and highlighted every 5.
    pub(super) fn score(min: f32, max: f32) -> Axis {
        let min = if min.is_finite() { min } else { 0.0 };
        let max = if max.is_finite() { max } else { 0.0 };
        Axis {
            min: min.min(-0.9).floor().max(-SCORE_LIMIT),
            max: max.max(0.9).ceil().min(SCORE_LIMIT),
            label_step: 1.0,
            minor_step: 1.0,
            major_step: Some(5.0),
        }
    }

    /// The time y axis (seconds): the step of [`time_step`] computed from
    /// `max_emt`, from the multiple of the step below `min_y` (0 unless the
    /// data is negative, as in a `Score Combo` with few scores that keeps the
    /// time step; Scid fits the axis to the data, `graphs.tcl:612-639`) up to
    /// the multiple above `max_y`.
    pub(super) fn time_fitting(max_emt: u32, min_y: f32, max_y: f32) -> Axis {
        let step = scaled_time_step(max_emt);
        let max_y = if max_y.is_finite() {
            max_y.max(0.0)
        } else {
            0.0
        };
        let min_y = if min_y.is_finite() {
            min_y.min(0.0)
        } else {
            0.0
        };
        Axis {
            min: (min_y / step).floor() * step,
            max: ((max_y / step).ceil() * step).max(step),
            label_step: step,
            // Scid draws a line per second while the step is 2.
            minor_step: if step <= 2.0 { 1.0 } else { step },
            major_step: None,
        }
    }
}

/// Smallest value of the 1-2-5 progression (`1, 2, 5, 10, 20, ...`) that is
/// at least `target`.
fn pick_125(target: f32) -> f32 {
    let mut decade = 1.0f32;
    loop {
        for m in [1.0, 2.0, 5.0] {
            let v = m * decade;
            if v >= target || !v.is_finite() {
                return v;
            }
        }
        decade *= 10.0;
    }
}

/// [`time_step`] continued past 2000 along the 1-2-5 progression (200 up to
/// 5000, 500 up to 10000, 1000 up to 20000, ...), so a corrupt `%emt` cannot
/// produce thousands of labels (D771; Scid keeps 200).
pub(super) fn scaled_time_step(max_emt: u32) -> f32 {
    let max = u64::from(max_emt);
    let (mut step, mut limit) = (200u64, 5000u64);
    let mut i = 0u32;
    while max > limit {
        // Steps 200, 500, 1000, 2000, 5000, ... ; limits 5000, 10000, 20000, 50000, ...
        step = if i.is_multiple_of(3) {
            step * 5 / 2
        } else {
            step * 2
        };
        limit = if i % 3 == 2 { limit * 5 / 2 } else { limit * 2 };
        i += 1;
    }
    if max_emt > 2000 {
        step as f32
    } else {
        time_step(max_emt)
    }
}

/// The label step of the time axis for the largest move time `max_emt`
/// (seconds): 2, then 5 (> 20), 10 (> 50), 20 (> 100), 50 (> 500), 100
/// (> 1000) and 200 (> 2000), as `graphs.tcl:612-639`. The axes use
/// `scaled_time_step`, which continues the progression above 5000.
pub fn time_step(max_emt: u32) -> f32 {
    match max_emt {
        0..=20 => 2.0,
        21..=50 => 5.0,
        51..=100 => 10.0,
        101..=500 => 20.0,
        501..=1000 => 50.0,
        1001..=2000 => 100.0,
        _ => 200.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rstest::rstest;

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
    fn time_steps(#[case] emt: u32, #[case] step: f32) {
        assert_eq!(time_step(emt), step);
        if emt <= 5000 {
            assert_eq!(Axis::time_fitting(emt, 0.0, 0.0).label_step, step);
        }
    }

    #[rstest]
    #[case(5000, 200.0)]
    #[case(5001, 500.0)]
    #[case(10_000, 500.0)]
    #[case(10_001, 1000.0)]
    #[case(20_001, 2000.0)]
    #[case(50_001, 5000.0)]
    #[case(100_001, 10_000.0)]
    fn time_step_continues_the_progression(#[case] emt: u32, #[case] step: f32) {
        assert_eq!(scaled_time_step(emt), step);
    }

    #[test]
    fn huge_axes_keep_few_ticks() {
        for emt in [5001, 100_000, 359_999_999, u32::MAX] {
            let a = Axis::time_fitting(emt, 0.0, emt as f32);
            assert!(a.label_ticks().count() <= 26, "{emt}");
            assert!(a.minor_ticks().count() <= 26, "{emt}");
        }
        for max_x in [0.0, 99.0, 100.0, 150.0, 1000.0, 10_000.0, 1e6] {
            let a = Axis::x(max_x);
            assert!(a.label_ticks().count() <= 21, "{max_x}");
            assert!(a.minor_ticks().count() <= 201, "{max_x}");
            assert!(a.major_ticks().count() <= 21, "{max_x}");
            assert!(a.max >= max_x + 0.5);
        }
        // Scid's steps while the axis is small.
        let a = Axis::x(99.0);
        assert_eq!(
            (a.label_step, a.minor_step, a.major_step),
            (5.0, 1.0, Some(5.0))
        );
    }

    #[test]
    fn click_with_nan_limits_is_ignored() {
        let a = Axis {
            min: f32::NAN,
            max: f32::NAN,
            label_step: 5.0,
            minor_step: 1.0,
            major_step: None,
        };
        assert_eq!(ply_at_x(&a, 1.0, 10), None);
    }

    #[test]
    fn time_fitting_extends_below_zero_keeping_the_step() {
        let a = Axis::time_fitting(2, -3.0, 2.0);
        assert_eq!((a.min, a.max, a.label_step), (-4.0, 2.0, 2.0));
        let b = Axis::time_fitting(30, -3.0, 12.0);
        assert_eq!((b.min, b.max, b.label_step), (-5.0, 15.0, 5.0));
        assert_eq!(Axis::time_fitting(2, f32::NAN, 1.0).min, 0.0);
    }

    #[test]
    fn score_axis_minimum_and_limits() {
        let a = Axis::score(0.0, 0.0);
        assert_eq!((a.min, a.max), (-1.0, 1.0));
        let a = Axis::score(-3.2, 7.1);
        assert_eq!((a.min, a.max), (-4.0, 8.0));
        let a = Axis::score(-99.0, 99.0);
        assert_eq!((a.min, a.max), (-10.0, 10.0));
        let a = Axis::score(f32::NAN, f32::INFINITY);
        assert_eq!((a.min, a.max), (-1.0, 1.0));
    }

    #[test]
    fn grid_ticks() {
        let a = Axis::score(-3.0, 6.0);
        assert_eq!(a.major_ticks().collect::<Vec<_>>(), vec![0.0, 5.0]);
        assert_eq!(a.minor_ticks().count(), 10);
        assert_eq!(Axis::time_fitting(5, 0.0, 0.0).major_ticks().count(), 0);
        let x = Axis::x(12.0);
        assert_eq!((x.min, x.max), (0.0, 15.0));
        assert_eq!(Axis::x(f32::NAN).max, 5.0);
        let t = Axis::time_fitting(10, 0.0, 7.0);
        assert_eq!(t.max, 8.0);
        assert_eq!(t.minor_ticks().count(), 9);
    }

    #[test]
    fn ticks_are_capped_and_safe() {
        let a = Axis {
            min: 0.0,
            max: f32::MAX,
            label_step: 1.0,
            minor_step: 1.0,
            major_step: None,
        };
        assert_eq!(a.minor_ticks().count(), MAX_TICKS);
        assert_eq!(a.ticks(0.0).count(), 0);
        assert_eq!(a.ticks(f32::NAN).count(), 0);
        assert_eq!(a.ticks(-1.0).count(), 0);
    }

    #[test]
    fn click_mapping() {
        let x = Axis::x(10.0);
        assert_eq!(ply_at_x(&x, 0.0, 20), Some(0));
        assert_eq!(ply_at_x(&x, 0.5, 20), Some(0));
        assert_eq!(ply_at_x(&x, 1.0, 20), Some(1));
        assert_eq!(ply_at_x(&x, 1.5, 20), Some(2));
        assert_eq!(ply_at_x(&x, 1.4, 20), Some(2));
        assert_eq!(ply_at_x(&x, 10.0, 7), Some(7));
        assert_eq!(ply_at_x(&x, -0.1, 20), None);
        assert_eq!(ply_at_x(&x, x.max + 0.1, 20), None);
        assert_eq!(ply_at_x(&x, f32::NAN, 20), None);
    }

    proptest! {
        #[test]
        fn click_round_trip(ply in 0usize..100_000) {
            let axis = Axis::x(x_of_ply(ply));
            prop_assert_eq!(ply_at_x(&axis, x_of_ply(ply), ply), Some(ply));
        }

        #[test]
        fn click_is_monotonic_and_bounded(a in 0.0f32..5000.0, b in 0.0f32..5000.0, n in 0usize..9000) {
            let axis = Axis::x(5000.0);
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let (pl, ph) = (ply_at_x(&axis, lo, n), ply_at_x(&axis, hi, n));
            prop_assert!(pl <= ph);
            prop_assert!(ph.is_none_or(|p| p <= n));
        }

        #[test]
        fn x_is_strictly_increasing(ply in 0usize..1_000_000) {
            prop_assert!(x_of_ply(ply) < x_of_ply(ply + 1));
        }

        #[test]
        fn time_step_is_monotonic(a in any::<u32>(), b in any::<u32>()) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(time_step(lo) <= time_step(hi));
        }

        #[test]
        fn axes_are_sane(min in -1e6f32..1e6, max in -1e6f32..1e6, emt in any::<u32>()) {
            let s = Axis::score(min, max);
            prop_assert!(s.min < s.max && s.min >= -10.0 && s.max <= 10.0);
            let t = Axis::time_fitting(emt, 0.0, max);
            prop_assert!(t.min < t.max);
        }
    }
}
