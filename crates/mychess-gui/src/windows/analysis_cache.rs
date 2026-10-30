//! What the analysis panel formats from its view of an execution, kept
//! between frames (spec M4-4, D930).
//!
//! The engine reports about twenty times a second and `view` runs for each
//! report, but the history and the log change far less often than that.
//! Formatting them (Fluent arguments, scores, `format!` per entry) is the
//! costly part of a frame, so the strings are built once and reused while the
//! data they came from is the same. The cache is not state: it never affects
//! what is drawn, only how much work it takes.

use std::cell::RefCell;

use mychess_app::engines::RunId;
use mychess_app::{AnalysisView, HistoryLine};

use crate::i18n::Language;

/// How many executions keep a cache (the oldest one is dropped).
const MAX_RUNS: usize = 8;

/// The history of one execution, formatted.
#[derive(Debug)]
struct HistoryEntry {
    run: RunId,
    language: Language,
    /// The lines the strings were made from.
    source: Vec<HistoryLine>,
    lines: Vec<String>,
}

/// The shown tail of the log of one execution, formatted.
#[derive(Debug)]
struct LogCacheEntry {
    run: RunId,
    filter: String,
    /// `(len, first seq, last seq)` of the whole log: `seq` is never reused,
    /// so the same triple means the same entries.
    stamp: (usize, Option<u64>, Option<u64>),
    lines: Vec<String>,
}

/// Replaces the entry that `same` finds, or adds `fresh` (dropping the oldest
/// entry past [`MAX_RUNS`]).
fn upsert<T>(list: &mut Vec<T>, fresh: T, same: impl Fn(&T) -> bool) {
    if let Some(slot) = list.iter_mut().find(|e| same(e)) {
        *slot = fresh;
        return;
    }
    if list.len() >= MAX_RUNS {
        list.remove(0);
    }
    list.push(fresh);
}

#[derive(Debug, Default)]
struct Inner {
    history: Vec<HistoryEntry>,
    log: Vec<LogCacheEntry>,
    history_formats: u64,
    log_formats: u64,
}

/// The formatted strings of the panel (see the module docs). Cloning gives an
/// empty cache and two caches are always equal: it is not part of the state.
#[derive(Debug, Default)]
pub struct ViewCache(RefCell<Inner>);

impl Clone for ViewCache {
    fn clone(&self) -> ViewCache {
        ViewCache::default()
    }
}

impl PartialEq for ViewCache {
    fn eq(&self, _: &ViewCache) -> bool {
        true
    }
}

impl Eq for ViewCache {}

impl ViewCache {
    /// How many history lines were formatted so far (for tests).
    pub fn history_formats(&self) -> u64 {
        self.0.borrow().history_formats
    }

    /// How many times the log was formatted so far (for tests).
    pub fn log_formats(&self) -> u64 {
        self.0.borrow().log_formats
    }

    /// The strings of `shown` (the history lines drawn), made by `format`
    /// only when `shown` differs from the last call for this execution and
    /// language.
    pub fn history_lines(
        &self,
        run: RunId,
        language: Language,
        shown: &[HistoryLine],
        mut format: impl FnMut(&HistoryLine) -> String,
    ) -> Vec<String> {
        let mut inner = self.0.borrow_mut();
        if let Some(entry) = inner
            .history
            .iter()
            .find(|e| e.run == run && e.language == language && e.source == shown)
        {
            return entry.lines.clone();
        }
        let lines: Vec<String> = shown.iter().map(&mut format).collect();
        inner.history_formats += lines.len() as u64;
        let fresh = HistoryEntry {
            run,
            language,
            source: shown.to_vec(),
            lines: lines.clone(),
        };
        upsert(&mut inner.history, fresh, |e| {
            e.run == run && e.language == language
        });
        lines
    }

    /// The newest `limit` entries of the log of `view` that match its filter,
    /// as `"<prefix><text>"` lines, oldest first. The filter is walked once,
    /// and only when the log or the filter changed.
    pub fn log_lines(&self, run: RunId, view: &AnalysisView, limit: usize) -> Vec<String> {
        let stamp = (
            view.log.len(),
            view.log.first().map(|e| e.seq),
            view.log.last().map(|e| e.seq),
        );
        let mut inner = self.0.borrow_mut();
        if let Some(entry) = inner
            .log
            .iter()
            .find(|e| e.run == run && e.stamp == stamp && e.filter == view.log_filter)
        {
            return entry.lines.clone();
        }
        let matches: Vec<_> = view.filtered_log().collect();
        let tail = &matches[matches.len().saturating_sub(limit)..];
        let lines: Vec<String> = tail
            .iter()
            .map(|entry| {
                let prefix = entry.kind.prefix();
                let mut line = String::with_capacity(prefix.len() + entry.text.len());
                line.push_str(prefix);
                line.push_str(&entry.text);
                line
            })
            .collect();
        inner.log_formats += 1;
        let fresh = LogCacheEntry {
            run,
            filter: view.log_filter.clone(),
            stamp,
            lines: lines.clone(),
        };
        upsert(&mut inner.log, fresh, |e| e.run == run);
        lines
    }
}

#[cfg(test)]
mod tests {
    use mychess_engine::{Bound, LogEntry, LogKind, Score};

    use super::*;

    fn line(depth: u32, text: &str) -> HistoryLine {
        HistoryLine {
            depth,
            score: Score::Cp(10),
            bound: Bound::Exact,
            pv: Vec::new(),
            time_ms: 1000,
            text: text.to_owned(),
        }
    }

    fn lines(n: u32) -> Vec<HistoryLine> {
        (0..n).map(|d| line(d, "1.e4")).collect()
    }

    fn fmt(l: &HistoryLine) -> String {
        format!("{} {}", l.depth, l.text)
    }

    #[test]
    fn the_same_history_is_not_formatted_again() {
        let cache = ViewCache::default();
        let shown = lines(5);
        let first = cache.history_lines(RunId(1), Language::EnUs, &shown, fmt);
        assert_eq!(cache.history_formats(), 5);
        let second = cache.history_lines(RunId(1), Language::EnUs, &shown, fmt);
        assert_eq!(cache.history_formats(), 5, "same data: no new formatting");
        assert_eq!(first, second);
        assert_eq!(first[3], "3 1.e4");
    }

    #[test]
    fn a_changed_history_a_language_or_a_run_format_again() {
        let cache = ViewCache::default();
        let mut shown = lines(3);
        cache.history_lines(RunId(1), Language::EnUs, &shown, fmt);
        shown[2].text = "1.d4".to_owned();
        let out = cache.history_lines(RunId(1), Language::EnUs, &shown, fmt);
        assert_eq!(out[2], "2 1.d4", "a changed text is not served stale");
        assert_eq!(cache.history_formats(), 6);
        cache.history_lines(RunId(1), Language::PtBr, &shown, fmt);
        assert_eq!(cache.history_formats(), 9);
        cache.history_lines(RunId(2), Language::PtBr, &shown, fmt);
        assert_eq!(cache.history_formats(), 12);
        // Both runs stay cached.
        cache.history_lines(RunId(1), Language::PtBr, &shown, fmt);
        cache.history_lines(RunId(2), Language::PtBr, &shown, fmt);
        assert_eq!(cache.history_formats(), 12);
    }

    fn view_with_log(count: u64, filter: &str) -> AnalysisView {
        let mut view = AnalysisView::new(RunId(1), 1, &mychess_app::AnalysisPrefs::default());
        view.log = (0..count)
            .map(|seq| LogEntry {
                seq,
                kind: LogKind::Received,
                text: format!("line {seq}"),
            })
            .collect();
        view.log_filter = filter.to_owned();
        view
    }

    #[test]
    fn the_log_is_formatted_once_per_change_and_keeps_the_newest_matches() {
        let cache = ViewCache::default();
        let mut view = view_with_log(10, "");
        let out = cache.log_lines(RunId(1), &view, 3);
        assert_eq!(out, ["< line 7", "< line 8", "< line 9"]);
        cache.log_lines(RunId(1), &view, 3);
        assert_eq!(cache.log_formats(), 1);
        view.log_filter = "line 1".to_owned();
        assert_eq!(cache.log_lines(RunId(1), &view, 3).len(), 1);
        assert_eq!(cache.log_formats(), 2);
        // A shifted log of the same length is a different log.
        let mut shifted = view_with_log(10, "");
        for e in &mut shifted.log {
            e.seq += 5;
            e.text = format!("line {}", e.seq);
        }
        let out = cache.log_lines(RunId(1), &shifted, 2);
        assert_eq!(out, ["< line 13", "< line 14"]);
    }
}
