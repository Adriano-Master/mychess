//! The value the bar shows and its mapping to a fraction of the bar (spec
//! M4-5, D453, D454; Scid `board.tcl` `evalbar_linlog_eval2pos`,
//! `update_evalbar_linear`, `update_evalbar_linlog`).

use mychess_core::MATE_SCORE;
use mychess_engine::{Bound, Score};

use super::prefs::EVAL_SCALE_RANGE;
use crate::analysis::format_score;

/// The result of a position without moves (D450).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalResult {
    /// Black is checkmated.
    WhiteWins,
    /// White is checkmated.
    BlackWins,
    /// Stalemate.
    Draw,
}

/// What the bar shows, from White's point of view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvalValue {
    /// A score in pawns (a comment, or a centipawn score).
    Pawns(f32),
    /// A mate in so many moves; negative when Black mates.
    Mate(i32),
    /// The game is over.
    Terminal(TerminalResult),
}

impl EvalValue {
    /// The value as a score in pawns for the mapping: a mate saturates
    /// (`327.xx`), the result of a game is at the ends or in the middle.
    pub fn pawns(self) -> f32 {
        match self {
            EvalValue::Pawns(p) => p,
            EvalValue::Mate(n) => Score::Mate(n).pawns(),
            EvalValue::Terminal(TerminalResult::WhiteWins) => MATE_SCORE,
            EvalValue::Terminal(TerminalResult::BlackWins) => -MATE_SCORE,
            EvalValue::Terminal(TerminalResult::Draw) => 0.0,
        }
    }

    /// The text next to the bar: `+0.35`, `-1.20`, `M3`/`-M3`, with `\u{2265}`
    /// or `\u{2264}` for a bound; `M0` and `=` for a finished game.
    pub fn label(self, bound: Bound) -> String {
        match self {
            EvalValue::Pawns(p) => {
                // NaN and infinities have no meaning: they read as 0.00.
                if !p.is_finite() {
                    return format_score(Score::Cp(0), bound);
                }
                // The comment of a mate has no distance: `#`. The comment
                // reader encodes `[%eval #N]` as exactly `MATE_SCORE`
                // (D830), so only that value is a mate; `[%eval 400]` is
                // an ordinary (huge) score.
                if p.abs() == MATE_SCORE {
                    return if p < 0.0 { "-#" } else { "#" }.to_owned();
                }
                // Clamped to the range of an i32 after the product; the
                // cast saturates by the semantics of `as` for floats.
                #[allow(clippy::cast_possible_truncation)]
                let cp = (p.clamp(-1.0e7, 1.0e7) * 100.0).round() as i32;
                format_score(Score::Cp(cp), bound)
            }
            EvalValue::Mate(n) => format_score(Score::Mate(n), bound),
            EvalValue::Terminal(TerminalResult::Draw) => "=".to_owned(),
            EvalValue::Terminal(_) => "M0".to_owned(),
        }
    }
}

/// Position of a score on the logarithmic scale, in `-2.0..=2.0` (Scid
/// `evalbar_linlog_eval2pos`: 3.16 is about a minor piece, 5 a rook, 12.6
/// the end of the bar).
fn log_position(eval: f32) -> f32 {
    if (-2.0..=2.0).contains(&eval) {
        return eval / 2.0;
    }
    let magnitude = ((f64::from(eval.abs()) * 1000.0 * 3.16).log10() - 3.0) * 1.2489;
    #[allow(clippy::cast_possible_truncation)]
    let wlen = (magnitude.min(2.0)) as f32;
    if eval < 0.0 { -wlen } else { wlen }
}

/// The share of the bar that is White's, from `0.0` to `1.0` (viewed from
/// the bottom, as the board is seen by White). `scale` is limited to
/// `1..=10`; a NaN or an infinity gives `0.5` (rule 1 of the spec).
pub fn bar_fill(value: EvalValue, scale: u8, log: bool) -> f32 {
    fill_of_pawns(value.pawns(), scale, log)
}

fn fill_of_pawns(eval: f32, scale: u8, log: bool) -> f32 {
    if !eval.is_finite() {
        return 0.5;
    }
    let fill = if log {
        (log_position(eval.clamp(-1.0e6, 1.0e6)) + 2.0) / 4.0
    } else {
        let s = f32::from(scale.clamp(EVAL_SCALE_RANGE.0, EVAL_SCALE_RANGE.1));
        (s + eval.clamp(-s, s)) / (2.0 * s)
    };
    fill.clamp(0.0, 1.0)
}

/// Positions of the ticks along the bar, from `0.0` to `1.0` (`evalTicks`):
/// the integers `-scale+1..scale-1` in the linear mapping, and
/// `-9 -5 -3 -1 0 1 3 5 9` in the logarithmic one.
pub fn bar_ticks(scale: u8, log: bool) -> Vec<f32> {
    if log {
        [-9.0_f32, -5.0, -3.0, -1.0, 0.0, 1.0, 3.0, 5.0, 9.0]
            .iter()
            .map(|e| fill_of_pawns(*e, scale, true))
            .collect()
    } else {
        let s = i32::from(scale.clamp(EVAL_SCALE_RANGE.0, EVAL_SCALE_RANGE.1));
        (1 - s..s)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let e = i as f32;
                fill_of_pawns(e, scale, false)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn fill(e: f32, scale: u8, log: bool) -> f32 {
        bar_fill(EvalValue::Pawns(e), scale, log)
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn linear_table() {
        assert!(near(fill(0.0, 5, false), 0.5));
        assert!(near(fill(1.0, 5, false), 0.6));
        assert!(near(fill(-1.0, 5, false), 0.4));
        assert!(near(fill(5.0, 5, false), 1.0));
        assert!(near(fill(9.0, 5, false), 1.0));
        assert!(near(fill(-327.0, 5, false), 0.0));
        assert!(near(fill(1.0, 1, false), 1.0));
        assert!(near(fill(1.0, 10, false), 0.55));
        // Out of range scales are limited.
        assert!(near(fill(1.0, 0, false), fill(1.0, 1, false)));
        assert!(near(fill(1.0, 99, false), fill(1.0, 10, false)));
    }

    #[test]
    fn log_table_follows_the_scid_formula() {
        assert!(near(fill(0.0, 5, true), 0.5));
        assert!(near(fill(0.5, 5, true), 0.5625));
        assert!(near(fill(2.0, 5, true), 0.75));
        assert!(near(fill(-2.0, 5, true), 0.25));
        assert!(near(fill(3.16, 5, true), (1.248 + 2.0) / 4.0));
        assert!(near(fill(5.0, 5, true), (1.497 + 2.0) / 4.0));
        assert!(near(fill(12.6, 5, true), 0.9995));
        assert!(near(fill(327.0, 5, true), 1.0));
        assert!(near(fill(-327.0, 5, true), 0.0));
    }

    #[test]
    fn hostile_values_do_not_panic() {
        for log in [false, true] {
            assert!(near(fill(f32::NAN, 5, log), 0.5));
            assert!(near(fill(f32::INFINITY, 5, log), 0.5));
            assert!(near(fill(f32::NEG_INFINITY, 5, log), 0.5));
            assert!(near(fill(32_700.0, 5, log), 1.0));
            assert!(near(fill(-32_700.0, 5, log), 0.0));
        }
        // An absurd distance keeps its sign and stays near the middle.
        assert!(bar_fill(EvalValue::Mate(i32::MIN), 5, false) < 0.5);
        assert!(bar_fill(EvalValue::Mate(i32::MAX), 5, false) > 0.5);
    }

    #[test]
    fn mates_and_results_saturate() {
        assert!(near(bar_fill(EvalValue::Mate(3), 5, false), 1.0));
        assert!(near(bar_fill(EvalValue::Mate(-3), 5, true), 0.0));
        let t = |r| bar_fill(EvalValue::Terminal(r), 5, false);
        assert!(near(t(TerminalResult::WhiteWins), 1.0));
        assert!(near(t(TerminalResult::BlackWins), 0.0));
        assert!(near(t(TerminalResult::Draw), 0.5));
    }

    #[test]
    fn ticks_follow_the_scid() {
        let linear = bar_ticks(3, false);
        assert_eq!(linear.len(), 5);
        assert!(near(linear[2], 0.5));
        assert!(near(linear[0], 1.0 / 6.0));
        let log = bar_ticks(5, true);
        assert_eq!(log.len(), 9);
        assert!(near(log[4], 0.5));
        assert!(log.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn labels() {
        let l = |v: EvalValue| v.label(Bound::Exact);
        assert_eq!(l(EvalValue::Pawns(0.35)), "+0.35");
        assert_eq!(l(EvalValue::Pawns(-1.2)), "-1.20");
        assert_eq!(l(EvalValue::Pawns(327.0)), "#");
        assert_eq!(l(EvalValue::Pawns(-327.0)), "-#");
        assert_eq!(l(EvalValue::Pawns(f32::NAN)), "+0.00");
        assert_eq!(l(EvalValue::Pawns(f32::INFINITY)), "+0.00");
        // Only the mate sentinel is a mate (D830): 400 is a plain score.
        assert_eq!(l(EvalValue::Pawns(400.0)), "+400.00");
        assert_eq!(l(EvalValue::Pawns(-400.0)), "-400.00");
        assert_eq!(l(EvalValue::Mate(3)), "+M3");
        assert_eq!(l(EvalValue::Mate(-3)), "-M3");
        assert_eq!(l(EvalValue::Terminal(TerminalResult::Draw)), "=");
        assert_eq!(l(EvalValue::Terminal(TerminalResult::WhiteWins)), "M0");
        assert_eq!(
            EvalValue::Pawns(0.5).label(Bound::Lower),
            "\u{2265}+0.50".to_owned()
        );
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
        fn any_float_stays_inside_the_bar(bits: u32, scale: u8, log: bool) {
            let f = fill(f32::from_bits(bits), scale, log);
            prop_assert!((0.0..=1.0).contains(&f));
        }
    }
}
