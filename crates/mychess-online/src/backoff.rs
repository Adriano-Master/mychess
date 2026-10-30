//! Reconnection delays (spec m8-3, D1333): exponential, 1, 2, 4, 8, 16 and 30 s
//! (cap), each with ±20% jitter.
//!
//! The jitter comes from a small xorshift generator seeded by the caller, so
//! tests are deterministic and the crate needs no `rand`.

use std::time::Duration;

/// Exponential backoff with jitter.
#[derive(Debug, Clone)]
pub struct Backoff {
    unit: Duration,
    attempt: u32,
    rng: u64,
}

impl Backoff {
    /// The nominal sequence in units: 1, 2, 4, 8, 16 and then the cap.
    const CAP_UNITS: u32 = 30;

    /// Standard backoff (1 s unit) seeded from the system clock.
    #[must_use]
    pub fn new() -> Self {
        Self::from_clock(Duration::from_secs(1))
    }

    /// Backoff with a custom unit and a jitter seed taken from the system clock.
    #[must_use]
    pub fn from_clock(unit: Duration) -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9E37_79B9_7F4A_7C15, |d| {
                u64::try_from(d.as_nanos() & u128::from(u64::MAX)).unwrap_or(1)
            });
        Self::with_unit(unit, seed)
    }

    /// Backoff with a custom unit (tests use milliseconds) and jitter seed.
    #[must_use]
    pub fn with_unit(unit: Duration, seed: u64) -> Self {
        Self {
            unit,
            attempt: 0,
            // xorshift must not start at zero.
            rng: seed | 1,
        }
    }

    /// Number of delays handed out since the last [`Backoff::reset`].
    #[must_use]
    pub fn attempts(&self) -> u32 {
        self.attempt
    }

    /// Forgets the failures (call after a healthy connection).
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// The nominal delay (without jitter) for the zero-based `attempt`.
    #[must_use]
    pub fn nominal(&self, attempt: u32) -> Duration {
        let units = 1u32
            .checked_shl(attempt)
            .map_or(Self::CAP_UNITS, |u| u.min(Self::CAP_UNITS));
        self.unit.saturating_mul(units)
    }

    /// Next delay, with jitter in `[0.8, 1.2]` of the nominal value.
    pub fn next_delay(&mut self) -> Duration {
        let nominal = self.nominal(self.attempt);
        self.attempt = self.attempt.saturating_add(1);
        // 0..=400 permille offset, minus 200: -20%..+20%.
        let permille = 800 + self.next_rand() % 401;
        let nanos = nominal.as_nanos() * u128::from(permille) / 1000;
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }

    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nominal_sequence_caps_at_thirty() {
        let b = Backoff::with_unit(Duration::from_secs(1), 7);
        let got: Vec<u64> = (0..8).map(|i| b.nominal(i).as_secs()).collect();
        assert_eq!(got, [1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(b.nominal(500).as_secs(), 30);
    }

    #[test]
    fn jitter_stays_within_twenty_percent() {
        let mut b = Backoff::with_unit(Duration::from_secs(1), 12345);
        for i in 0..40 {
            let nominal = b.nominal(i).as_secs_f64();
            let d = b.next_delay().as_secs_f64();
            assert!(
                d >= nominal * 0.8 - 1e-9 && d <= nominal * 1.2 + 1e-9,
                "{d} vs {nominal}"
            );
        }
        assert_eq!(b.attempts(), 40);
    }

    #[test]
    fn huge_unit_saturates_instead_of_panicking() {
        let b = Backoff::with_unit(Duration::MAX, 1);
        assert_eq!(b.nominal(10), Duration::MAX);
    }

    #[test]
    fn reset_restarts_the_sequence() {
        let mut b = Backoff::with_unit(Duration::from_millis(10), 1);
        let _ = b.next_delay();
        let _ = b.next_delay();
        b.reset();
        assert_eq!(b.attempts(), 0);
        assert!(b.next_delay() <= Duration::from_millis(12));
    }
}
