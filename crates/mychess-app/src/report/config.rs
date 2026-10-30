//! The configuration of the report (spec M4-10, D1422, D1425, D2100-D2105):
//! how hard the engine looks at each position, the limits of the classes of
//! move and the marks on the board. Persisted in the `[report_config]` table
//! of the preferences.

use std::time::Duration;

use super::{ReportPreset, ReportThresholds};
use crate::annotate::{AnnotateOptions, Control, DEPTH_RANGE, MOVETIME_RANGE_MS};
use crate::engines::RunId;

/// The most a limit of the scale may be, in tenths of a percentage point.
pub const LIMIT_MAX_TENTHS: u16 = 1000;

/// The user's choices for the report, as saved in the preferences.
///
/// The limits of the loss scale are kept in tenths of a percentage point
/// (`5` is 0.5 points), so the file has no floating point and reads back
/// exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ReportConfig {
    /// Limit each search by time instead of by depth.
    pub by_time: bool,
    /// The depth of each search when `by_time` is off.
    pub depth: u32,
    /// The time of each search, in milliseconds, when `by_time` is on.
    pub movetime_ms: u32,
    /// Up to this loss a move is `Best` (D1425).
    pub best_tenths: u16,
    /// Up to this loss: `Excellent`.
    pub excellent_tenths: u16,
    /// Up to this loss: `Good`.
    pub good_tenths: u16,
    /// Up to this loss: `Inaccuracy`.
    pub inaccuracy_tenths: u16,
    /// Up to this loss: `Mistake`; above it, `Blunder`.
    pub mistake_tenths: u16,
    /// Draw the class of the move on its destination square (D1434, D2111).
    pub board_marks: bool,
}

impl Default for ReportConfig {
    fn default() -> ReportConfig {
        ReportConfig {
            by_time: false,
            depth: ReportPreset::Standard.depth(),
            movetime_ms: 2000,
            best_tenths: 5,
            excellent_tenths: 20,
            good_tenths: 50,
            inaccuracy_tenths: 100,
            mistake_tenths: 200,
            board_marks: true,
        }
    }
}

impl ReportConfig {
    /// The same choices with every number inside its range and the limits
    /// in order (`best <= excellent <= good <= inaccuracy <= mistake`): a
    /// limit raised past the next ones carries them along.
    #[must_use]
    pub fn sanitized(mut self) -> ReportConfig {
        self.depth = self.depth.clamp(*DEPTH_RANGE.start(), *DEPTH_RANGE.end());
        let ms =
            u64::from(self.movetime_ms).clamp(*MOVETIME_RANGE_MS.start(), *MOVETIME_RANGE_MS.end());
        // The range ends at 300 000: it fits.
        self.movetime_ms = u32::try_from(ms).unwrap_or(u32::MAX);
        let mut floor = 0;
        for limit in [
            &mut self.best_tenths,
            &mut self.excellent_tenths,
            &mut self.good_tenths,
            &mut self.inaccuracy_tenths,
            &mut self.mistake_tenths,
        ] {
            *limit = (*limit).clamp(floor, LIMIT_MAX_TENTHS);
            floor = *limit;
        }
        self
    }

    /// The thresholds of the classification; the special classes keep their
    /// defaults.
    pub fn thresholds(&self) -> ReportThresholds {
        let tenths = |v: u16| f32::from(v) / 10.0;
        ReportThresholds {
            best: tenths(self.best_tenths),
            excellent: tenths(self.excellent_tenths),
            good: tenths(self.good_tenths),
            inaccuracy: tenths(self.inaccuracy_tenths),
            mistake: tenths(self.mistake_tenths),
            ..ReportThresholds::default()
        }
    }

    /// The options of the run that match this configuration: a limit by
    /// depth or by time and nothing that edits the game (D1447).
    pub(crate) fn options(&self) -> AnnotateOptions {
        AnnotateOptions {
            control: if self.by_time {
                Control::Time
            } else {
                Control::Depth
            },
            depth: self.depth,
            movetime: Duration::from_millis(u64::from(self.movetime_ms)),
            missed_mates: false,
            var_at_end: false,
            opening_only: None,
            add_tag: false,
            variations: false,
            mark_exercises: false,
            batch: false,
            ..AnnotateOptions::default()
        }
        .normalized()
    }

    /// This configuration with the depth of `preset`.
    #[must_use]
    pub fn with_preset(mut self, preset: ReportPreset) -> ReportConfig {
        self.by_time = false;
        self.depth = preset.depth();
        self
    }
}

/// The "Configure report" dialog being edited (D2100).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportDialog {
    /// The choices on show.
    pub config: ReportConfig,
    /// The engine of the run: a running one, or `None` for the default of
    /// the annotation (the first running, else the one of F2).
    pub engine: Option<RunId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_limits_of_the_spec() {
        let config = ReportConfig::default();
        assert_eq!(config.thresholds(), ReportThresholds::default());
        assert_eq!(config.sanitized(), config);
        assert_eq!(config.options().depth, 16);
    }

    #[test]
    fn sanitizing_keeps_the_limits_in_order_and_in_range() {
        let config = ReportConfig {
            best_tenths: 30,
            excellent_tenths: 10,
            good_tenths: 5000,
            inaccuracy_tenths: 0,
            mistake_tenths: 7,
            depth: 1,
            movetime_ms: u32::MAX,
            ..ReportConfig::default()
        }
        .sanitized();
        assert_eq!(
            [
                config.best_tenths,
                config.excellent_tenths,
                config.good_tenths,
                config.inaccuracy_tenths,
                config.mistake_tenths
            ],
            [30, 30, 1000, 1000, 1000]
        );
        assert_eq!(config.depth, *DEPTH_RANGE.start());
        assert_eq!(u64::from(config.movetime_ms), *MOVETIME_RANGE_MS.end());
    }

    #[test]
    fn the_options_follow_the_kind_of_limit() {
        let by_time = ReportConfig {
            by_time: true,
            movetime_ms: 1500,
            ..ReportConfig::default()
        };
        let options = by_time.options();
        assert_eq!(options.control, Control::Time);
        assert_eq!(options.movetime, Duration::from_millis(1500));
        assert!(!options.add_tag && !options.variations);
        let deep = ReportConfig::default().with_preset(ReportPreset::Deep);
        assert_eq!(deep.options().control, Control::Depth);
        assert_eq!(deep.options().depth, 22);
    }
}
