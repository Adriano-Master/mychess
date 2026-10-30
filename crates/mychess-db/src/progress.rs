//! Progress reporting and cancellation of long operations (decision D329).
//!
//! Searches and imports run off the interface thread. They report progress
//! through a [`ProgressSink`] and stop when the [`CancelToken`] they were
//! given is set. Both are checked at the same points, driven by `Ticker`:
//! every N items or every 100 ms, whichever comes first.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Receives the progress of a long operation.
///
/// Implemented for any `FnMut(u64, u64)`, so a closure works as a sink.
pub trait ProgressSink {
    /// `done` items out of `total` have been processed.
    fn progress(&mut self, done: u64, total: u64);
}

impl<F: FnMut(u64, u64)> ProgressSink for F {
    fn progress(&mut self, done: u64, total: u64) {
        self(done, total);
    }
}

/// A sink that ignores progress.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl ProgressSink for NoProgress {
    fn progress(&mut self, _done: u64, _total: u64) {}
}

/// A shared flag that asks an operation to stop. Clones share the flag.
///
/// ```
/// use mychess_db::CancelToken;
///
/// let token = CancelToken::new();
/// let worker_side = token.clone();
/// assert!(!worker_side.is_cancelled());
/// token.cancel();
/// assert!(worker_side.is_cancelled());
/// ```
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A token that is not cancelled.
    pub fn new() -> CancelToken {
        CancelToken::default()
    }

    /// Asks the operation to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether [`CancelToken::cancel`] was called.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Clears the flag so the token can be reused.
    pub fn reset(&self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

/// Number of items searched between progress reports (Scid: 5000).
pub const SEARCH_CHECK_EVERY: u32 = 5000;

/// Time between progress reports, besides the item count (D329).
pub const CHECK_INTERVAL: Duration = Duration::from_millis(100);

/// How many items pass between clock readings.
const CLOCK_STRIDE: u32 = 16;

/// Decides when an operation reports progress and looks at its token.
#[derive(Debug)]
pub(crate) struct Ticker {
    every: u32,
    interval: Duration,
    since_report: u32,
    since_clock: u32,
    last: Instant,
}

impl Ticker {
    /// A ticker that fires every `every` items or every `interval`.
    pub(crate) fn new(every: u32, interval: Duration) -> Ticker {
        Ticker {
            every: every.max(1),
            interval,
            since_report: 0,
            since_clock: 0,
            last: Instant::now(),
        }
    }

    /// Counts one item; `true` when it is time to report and check the token.
    pub(crate) fn tick(&mut self) -> bool {
        self.since_report += 1;
        self.since_clock += 1;
        let mut fire = self.since_report >= self.every;
        if !fire && self.since_clock >= CLOCK_STRIDE {
            self.since_clock = 0;
            fire = self.last.elapsed() >= self.interval;
        }
        if fire {
            self.since_report = 0;
            self.since_clock = 0;
            self.last = Instant::now();
        }
        fire
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticker_fires_every_n_items() {
        let mut t = Ticker::new(3, Duration::MAX);
        let fired: Vec<bool> = (0..7).map(|_| t.tick()).collect();
        assert_eq!(fired, [false, false, true, false, false, true, false]);
    }

    #[test]
    fn ticker_fires_on_time_even_with_few_items() {
        let mut t = Ticker::new(1_000_000, Duration::ZERO);
        let fired = (0..CLOCK_STRIDE).filter(|_| t.tick()).count();
        assert_eq!(fired, 1);
    }

    #[test]
    fn closures_are_sinks() {
        let mut seen = Vec::new();
        {
            let mut sink = |d, t| seen.push((d, t));
            ProgressSink::progress(&mut sink, 1, 2);
        }
        assert_eq!(seen, [(1, 2)]);
    }

    #[test]
    fn token_clones_share_the_flag() {
        let a = CancelToken::new();
        let b = a.clone();
        b.cancel();
        assert!(a.is_cancelled());
        a.reset();
        assert!(!b.is_cancelled());
    }
}
