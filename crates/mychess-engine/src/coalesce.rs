//! Rate limiting of `info` reports (spec m4-1, D412).
//!
//! Engines write thousands of `info` lines per second; the interface needs a
//! few per second. The coalescer lets through at most one report per
//! `(search, multipv)` in each interval. In between, the newest report is
//! kept (merged with the pending one, so a `currmove` line does not erase the
//! last score), and it is released when the interval is over or when
//! something more important has to be delivered first.

use std::collections::HashMap;
use std::time::Duration;

use tokio::time::Instant;

use crate::protocol::{SearchId, SearchInfo};

type Key = (SearchId, u8);

/// See the [module documentation](self).
#[derive(Debug)]
pub(crate) struct Coalescer {
    interval: Duration,
    last_emit: HashMap<Key, Instant>,
    /// Pending reports in arrival order, at most one per key.
    pending: Vec<SearchInfo>,
}

impl Coalescer {
    pub(crate) fn new(interval: Duration) -> Coalescer {
        Coalescer {
            interval,
            last_emit: HashMap::new(),
            pending: Vec::new(),
        }
    }

    fn key(info: &SearchInfo) -> Key {
        (info.search, info.multipv)
    }

    /// Offers a report. Returns it (merged with the pending one) when it may
    /// be delivered now; otherwise keeps it.
    pub(crate) fn push(&mut self, info: SearchInfo, now: Instant) -> Option<SearchInfo> {
        if self.interval.is_zero() {
            return Some(info);
        }
        let key = Self::key(&info);
        // A new search makes the bookkeeping of older ones useless.
        self.last_emit.retain(|k, _| k.0 == key.0);
        let merged = match self.pending.iter().position(|p| Self::key(p) == key) {
            Some(i) => merge(self.pending.remove(i), info),
            None => info,
        };
        let due = self
            .last_emit
            .get(&key)
            .is_none_or(|t| now.saturating_duration_since(*t) >= self.interval);
        if due {
            self.last_emit.insert(key, now);
            Some(merged)
        } else {
            self.pending.push(merged);
            None
        }
    }

    /// When the earliest pending report becomes deliverable.
    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.pending
            .iter()
            .filter_map(|p| {
                self.last_emit
                    .get(&Self::key(p))
                    .map(|t| *t + self.interval)
            })
            .min()
    }

    /// Takes the pending reports whose interval is over.
    pub(crate) fn take_due(&mut self, now: Instant) -> Vec<SearchInfo> {
        let mut due = Vec::new();
        let mut keep = Vec::new();
        for info in self.pending.drain(..) {
            let key = Self::key(&info);
            let ready = self
                .last_emit
                .get(&key)
                .is_none_or(|t| now.saturating_duration_since(*t) >= self.interval);
            if ready {
                self.last_emit.insert(key, now);
                due.push(info);
            } else {
                keep.push(info);
            }
        }
        self.pending = keep;
        due
    }

    /// Takes every pending report, in arrival order.
    pub(crate) fn take_all(&mut self, now: Instant) -> Vec<SearchInfo> {
        let all: Vec<SearchInfo> = std::mem::take(&mut self.pending);
        for info in &all {
            self.last_emit.insert(Self::key(info), now);
        }
        all
    }

    /// Forgets everything pending (reports of a search nobody wants any more).
    pub(crate) fn clear(&mut self) {
        self.pending.clear();
    }
}

/// Merges a newer report into an older one of the same line.
///
/// Statistics take the newer value when present. Score, WDL and PV travel
/// together: when the newer report has a PV they all come from it; otherwise
/// the older ones stay.
fn merge(old: SearchInfo, new: SearchInfo) -> SearchInfo {
    let has_pv = !new.pv.is_empty();
    SearchInfo {
        search: new.search,
        multipv: new.multipv,
        depth: new.depth.or(old.depth),
        seldepth: new.seldepth.or(old.seldepth),
        time_ms: new.time_ms.or(old.time_ms),
        nodes: new.nodes.or(old.nodes),
        nps: new.nps.or(old.nps),
        hashfull_permille: new.hashfull_permille.or(old.hashfull_permille),
        cpuload_permille: new.cpuload_permille.or(old.cpuload_permille),
        tbhits: new.tbhits.or(old.tbhits),
        score: if has_pv {
            new.score
        } else {
            new.score.or(old.score)
        },
        wdl: if has_pv { new.wdl } else { new.wdl.or(old.wdl) },
        pv: if has_pv { new.pv } else { old.pv },
        pv_truncated: if has_pv {
            new.pv_truncated
        } else {
            old.pv_truncated
        },
        currmove: new.currmove.or(old.currmove),
        currmovenumber: new.currmovenumber.or(old.currmovenumber),
        string: new.string.or(old.string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Bound, Score};

    fn info(search: u64, multipv: u8, depth: u32) -> SearchInfo {
        SearchInfo {
            search: SearchId(search),
            multipv,
            depth: Some(depth),
            ..SearchInfo::default()
        }
    }

    #[test]
    fn first_report_passes_and_later_ones_wait_for_the_interval() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(Duration::from_millis(50));
        assert!(c.push(info(1, 1, 1), t0).is_some());
        assert!(
            c.push(info(1, 1, 2), t0 + Duration::from_millis(10))
                .is_none()
        );
        assert!(
            c.push(info(1, 1, 3), t0 + Duration::from_millis(20))
                .is_none()
        );
        // Another line is independent.
        assert!(
            c.push(info(1, 2, 1), t0 + Duration::from_millis(20))
                .is_some()
        );
        assert_eq!(c.next_due(), Some(t0 + Duration::from_millis(50)));
        assert_eq!(c.take_due(t0 + Duration::from_millis(49)).len(), 0);
        let due = c.take_due(t0 + Duration::from_millis(50));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].depth, Some(3), "the last value wins");
        assert_eq!(c.next_due(), None);
    }

    #[test]
    fn take_all_keeps_arrival_order() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(Duration::from_millis(50));
        for mpv in 1..=3 {
            assert!(c.push(info(1, mpv, 1), t0).is_some());
        }
        for mpv in [3, 1, 2] {
            assert!(c.push(info(1, mpv, 2), t0).is_none());
        }
        let all = c.take_all(t0);
        assert_eq!(all.iter().map(|i| i.multipv).collect::<Vec<_>>(), [3, 1, 2]);
    }

    #[test]
    fn zero_interval_lets_everything_through() {
        let mut c = Coalescer::new(Duration::ZERO);
        let t0 = Instant::now();
        assert!(c.push(info(1, 1, 1), t0).is_some());
        assert!(c.push(info(1, 1, 2), t0).is_some());
    }

    #[test]
    fn a_stats_only_report_does_not_erase_the_score_and_pv() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(Duration::from_millis(50));
        assert!(c.push(info(1, 1, 1), t0).is_some());
        let mut full = info(1, 1, 5);
        full.score = Some((Score::Cp(10), Bound::Exact));
        full.pv = vec![mychess_core::Move::Null];
        assert!(c.push(full, t0 + Duration::from_millis(1)).is_none());
        let mut stats = info(1, 1, 6);
        stats.nodes = Some(9);
        assert!(c.push(stats, t0 + Duration::from_millis(2)).is_none());
        let merged = c.take_all(t0).remove(0);
        assert_eq!(merged.depth, Some(6));
        assert_eq!(merged.nodes, Some(9));
        assert_eq!(merged.score, Some((Score::Cp(10), Bound::Exact)));
        assert_eq!(merged.pv.len(), 1);
    }
}

#[cfg(test)]
mod contract_tests;
