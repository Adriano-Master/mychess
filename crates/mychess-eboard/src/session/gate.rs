//! The ceiling of lines per second of a session (spec m8-10, D2759): a
//! device that floods the session must not flood the application with
//! messages and redraws.

use std::time::Duration;

use tokio::time::Instant;

/// Counts the lines of the current one-second window.
#[derive(Debug)]
pub(crate) struct LineGate {
    window_end: Instant,
    admitted: u32,
    omitted: u32,
    limit: u32,
}

const WINDOW: Duration = Duration::from_secs(1);

impl LineGate {
    pub(crate) fn new(now: Instant, limit: u32) -> LineGate {
        LineGate {
            window_end: now + WINDOW,
            admitted: 0,
            omitted: 0,
            limit,
        }
    }

    fn roll(&mut self, now: Instant) {
        if now >= self.window_end {
            self.window_end = now + WINDOW;
            self.admitted = 0;
        }
    }

    /// Whether the line that arrived at `now` is read; a refused line is
    /// counted.
    pub(crate) fn admit(&mut self, now: Instant) -> bool {
        self.roll(now);
        if self.admitted < self.limit {
            self.admitted += 1;
            true
        } else {
            self.omitted = self.omitted.saturating_add(1);
            false
        }
    }

    /// When the count of dropped lines is to be told (the end of the
    /// window), if any was dropped.
    pub(crate) fn report_due(&self) -> Option<Instant> {
        (self.omitted > 0).then_some(self.window_end)
    }

    /// The number of lines dropped, once the window that dropped them is
    /// over; the count starts again.
    pub(crate) fn flush(&mut self, now: Instant) -> Option<u32> {
        if self.omitted > 0 && now >= self.window_end {
            self.roll(now);
            return Some(std::mem::take(&mut self.omitted));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_over_the_limit_are_dropped_and_counted_until_the_window_ends() {
        let t0 = Instant::now();
        let mut gate = LineGate::new(t0, 3);
        assert_eq!(gate.report_due(), None);
        let admitted = (0..10).filter(|_| gate.admit(t0)).count();
        assert_eq!(admitted, 3);
        assert_eq!(gate.report_due(), Some(t0 + WINDOW));
        assert_eq!(gate.flush(t0), None, "the window is still open");
        let later = t0 + WINDOW;
        assert_eq!(gate.flush(later), Some(7));
        assert_eq!(gate.flush(later), None, "told once");
        assert!(gate.admit(later), "a new window admits again");
    }

    #[test]
    fn a_quiet_device_is_never_limited() {
        let t0 = Instant::now();
        let mut gate = LineGate::new(t0, 2);
        for second in 0..20_u64 {
            let now = t0 + Duration::from_secs(second);
            assert!(gate.admit(now));
            assert!(gate.admit(now));
        }
        assert_eq!(gate.report_due(), None);
    }
}
