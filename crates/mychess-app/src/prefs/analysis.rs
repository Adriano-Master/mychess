//! Preferences of the analysis engines (spec M4-3, D431; Scid:
//! `tcl/start.tcl` 420-435, `analysis(logMax)`, `maxPly`, `lowPriority`...).

use std::time::Duration;

use serde::Serialize;

use crate::engines::RunDefaults;

/// Largest protocol log accepted, in lines.
pub const MAX_LOG_LINES: u32 = 100_000;
/// Largest PV depth limit accepted, in plies.
pub const MAX_PV_PLY: u32 = 255;
/// Largest interval between two `info` updates, in milliseconds.
pub const MAX_INFO_INTERVAL_MS: u32 = 5000;
/// Smallest interval between two `info` updates, in milliseconds: `0` would
/// turn the coalescing off and flood the front-end (D732).
pub const MIN_INFO_INTERVAL_MS: u32 = 10;

/// How the analysis engines run and how their lines are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AnalysisPrefs {
    /// Size of the protocol log of each engine; `0` turns it off
    /// (Scid `analysis(logMax)`).
    pub log_lines: u32,
    /// Put the engine name in the comments it adds (Scid `logName`).
    pub name_in_comments: bool,
    /// Longest PV shown, in plies; `0` is unlimited (Scid `maxPly`).
    pub max_ply: u32,
    /// Run the engines at low CPU priority (on by default on Windows, as in
    /// the Scid).
    pub low_priority: bool,
    /// Wrap long PV lines (Scid `wrapPV`).
    pub wrap_pv: bool,
    /// Show the extra `info` fields (Scid `showEngineInfo`).
    pub show_info: bool,
    /// Minimum time between two updates of the same line, in milliseconds.
    pub info_interval_ms: u32,
}

impl Default for AnalysisPrefs {
    fn default() -> AnalysisPrefs {
        AnalysisPrefs {
            log_lines: 5000,
            name_in_comments: true,
            max_ply: 0,
            low_priority: cfg!(windows),
            wrap_pv: false,
            show_info: false,
            info_interval_ms: 50,
        }
    }
}

impl AnalysisPrefs {
    /// The same preferences with every number inside its limit.
    #[must_use]
    pub fn sanitized(mut self) -> AnalysisPrefs {
        self.log_lines = self.log_lines.min(MAX_LOG_LINES);
        self.max_ply = self.max_ply.min(MAX_PV_PLY);
        self.info_interval_ms = self
            .info_interval_ms
            .clamp(MIN_INFO_INTERVAL_MS, MAX_INFO_INTERVAL_MS);
        self
    }

    /// The settings every engine run starts with.
    pub fn run_defaults(&self) -> RunDefaults {
        RunDefaults {
            low_priority: self.low_priority,
            log_lines: usize::try_from(self.log_lines).unwrap_or(usize::MAX),
            info_interval: Duration::from_millis(u64::from(self.info_interval_ms)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{PrefChange, Preferences, PrefsIssue, load_preferences, render_preferences};

    #[test]
    fn defaults_follow_d431() {
        let p = AnalysisPrefs::default();
        assert_eq!((p.log_lines, p.max_ply, p.info_interval_ms), (5000, 0, 50));
        assert!(p.name_in_comments && !p.wrap_pv && !p.show_info);
        assert_eq!(p.low_priority, cfg!(windows));
        assert_eq!(*Preferences::default().analysis, p);
    }

    #[test]
    fn old_file_without_the_section_is_valid() {
        let loaded = load_preferences("[board]\nanimation_ms = 100\n");
        assert_eq!(*loaded.prefs.analysis, AnalysisPrefs::default());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn values_round_trip() {
        let prefs = Preferences {
            analysis: Box::new(AnalysisPrefs {
                log_lines: 0,
                name_in_comments: false,
                max_ply: 12,
                low_priority: !cfg!(windows),
                wrap_pv: true,
                show_info: true,
                info_interval_ms: 200,
            }),
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("[analysis]"), "{text}");
        let loaded = load_preferences(&text);
        assert_eq!(loaded.prefs.analysis, prefs.analysis);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn bad_and_wild_values_reset_or_clamp_one_by_one() {
        let loaded = load_preferences(
            "[analysis]\nlog_lines = 99999999\nwrap_pv = 3\nmax_ply = \"deep\"\nshow_info = true\n",
        );
        let a = *loaded.prefs.analysis;
        assert_eq!(a.log_lines, MAX_LOG_LINES);
        assert!(!a.wrap_pv && a.show_info);
        assert_eq!(a.max_ply, 0);
        assert_eq!(loaded.issues.len(), 3, "{:?}", loaded.issues);
        assert!(loaded.issues.contains(&PrefsIssue::Clamped {
            key: "analysis.log_lines".to_owned()
        }));
    }

    #[test]
    fn a_zero_info_interval_is_raised_to_the_minimum() {
        let mut prefs = Preferences::default();
        PrefChange::Analysis(AnalysisPrefs {
            info_interval_ms: 0,
            ..AnalysisPrefs::default()
        })
        .apply(&mut prefs);
        assert_eq!(prefs.analysis.info_interval_ms, MIN_INFO_INTERVAL_MS);
        let loaded = load_preferences("[analysis]\ninfo_interval_ms = 0\n");
        assert_eq!(loaded.prefs.analysis.info_interval_ms, MIN_INFO_INTERVAL_MS);
    }

    #[test]
    fn change_is_limited_and_run_defaults_follow() {
        let mut prefs = Preferences::default();
        let wild = AnalysisPrefs {
            log_lines: u32::MAX,
            max_ply: u32::MAX,
            info_interval_ms: u32::MAX,
            low_priority: true,
            ..AnalysisPrefs::default()
        };
        PrefChange::Analysis(wild).apply(&mut prefs);
        assert_eq!(prefs.analysis.log_lines, MAX_LOG_LINES);
        assert_eq!(prefs.analysis.max_ply, MAX_PV_PLY);
        let defaults = prefs.analysis.run_defaults();
        assert!(defaults.low_priority);
        assert_eq!(defaults.log_lines, MAX_LOG_LINES as usize);
        assert_eq!(
            defaults.info_interval,
            Duration::from_millis(u64::from(MAX_INFO_INTERVAL_MS))
        );
    }
}
