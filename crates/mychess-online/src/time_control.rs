//! Speed classification and validation of time controls (spec m8-3, D1314).
//!
//! The Board API only takes rapid and classical for seeks and blitz or slower
//! for direct challenges. The classification follows Lichess: estimated game
//! duration `limit_s + 40 * increment_s`.

use serde::Deserialize;

/// Lichess speed category ("speed" in the API, `perf` for the rating).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum Speed {
    /// Estimated duration under 30 s.
    UltraBullet,
    /// Under 3 minutes.
    Bullet,
    /// Under 8 minutes.
    Blitz,
    /// Under 25 minutes.
    Rapid,
    /// 25 minutes or more.
    Classical,
    /// Days per move.
    Correspondence,
    /// A speed this client does not know.
    #[serde(other)]
    Unknown,
}

/// Why a time control is refused before touching the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TimeControlError {
    /// Minutes or increment outside the allowed range.
    #[error("time or increment out of range")]
    OutOfRange,
    /// The challenge clock limit is not one of the values Lichess accepts.
    #[error("unsupported clock limit")]
    BadLimit,
    /// The estimated speed is faster than the Board API allows.
    #[error("too fast for the Board API ({0:?})")]
    TooFast(Speed),
}

/// Classifies a clock (`limit` and `increment` in seconds) as Lichess does.
#[must_use]
pub fn classify(limit_secs: u32, increment_secs: u32) -> Speed {
    let estimated = u64::from(limit_secs) + 40 * u64::from(increment_secs);
    match estimated {
        0..30 => Speed::UltraBullet,
        30..180 => Speed::Bullet,
        180..480 => Speed::Blitz,
        480..1500 => Speed::Rapid,
        _ => Speed::Classical,
    }
}

/// Validates a real-time seek: minutes 0-180, increment 0-180 s, rapid or
/// classical only. Returns the speed.
pub fn validate_seek(minutes: u32, increment_secs: u32) -> Result<Speed, TimeControlError> {
    if minutes > 180 || increment_secs > 180 {
        return Err(TimeControlError::OutOfRange);
    }
    let speed = classify(minutes * 60, increment_secs);
    match speed {
        Speed::Rapid | Speed::Classical => Ok(speed),
        other => Err(TimeControlError::TooFast(other)),
    }
}

/// Validates a direct challenge: `limit` in seconds (0, 15, 30, 45, 60, 90 or a
/// multiple of 60 up to 3 hours), increment 0-60 s, blitz or slower.
pub fn validate_challenge(limit_secs: u32, increment_secs: u32) -> Result<Speed, TimeControlError> {
    if limit_secs > 10_800 || increment_secs > 60 {
        return Err(TimeControlError::OutOfRange);
    }
    let allowed = matches!(limit_secs, 0 | 15 | 30 | 45 | 90) || limit_secs.is_multiple_of(60);
    if !allowed {
        return Err(TimeControlError::BadLimit);
    }
    let speed = classify(limit_secs, increment_secs);
    match speed {
        Speed::Blitz | Speed::Rapid | Speed::Classical => Ok(speed),
        other => Err(TimeControlError::TooFast(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_table() {
        let cases = [
            (0, 0, Speed::UltraBullet),
            (15, 0, Speed::UltraBullet),
            (60, 0, Speed::Bullet),
            (120, 1, Speed::Bullet),
            (180, 0, Speed::Blitz),
            (300, 3, Speed::Blitz),
            (420, 0, Speed::Blitz),
            (480, 0, Speed::Rapid),
            (600, 0, Speed::Rapid),
            (900, 10, Speed::Rapid),
            (1500, 0, Speed::Classical),
            (1800, 20, Speed::Classical),
        ];
        for (limit, inc, want) in cases {
            assert_eq!(classify(limit, inc), want, "{limit}+{inc}");
        }
    }

    #[test]
    fn seek_accepts_rapid_and_classical_only() {
        assert_eq!(validate_seek(10, 0), Ok(Speed::Rapid));
        assert_eq!(validate_seek(10, 5), Ok(Speed::Rapid));
        assert_eq!(validate_seek(15, 10), Ok(Speed::Rapid));
        assert_eq!(validate_seek(30, 20), Ok(Speed::Classical));
        assert_eq!(
            validate_seek(3, 0),
            Err(TimeControlError::TooFast(Speed::Blitz))
        );
        assert_eq!(
            validate_seek(5, 3),
            Err(TimeControlError::TooFast(Speed::Blitz))
        );
        assert_eq!(
            validate_seek(1, 0),
            Err(TimeControlError::TooFast(Speed::Bullet))
        );
        assert_eq!(validate_seek(181, 0), Err(TimeControlError::OutOfRange));
        assert_eq!(validate_seek(10, 181), Err(TimeControlError::OutOfRange));
    }

    #[test]
    fn challenge_accepts_blitz_or_slower() {
        assert_eq!(validate_challenge(180, 0), Ok(Speed::Blitz));
        assert_eq!(validate_challenge(300, 3), Ok(Speed::Blitz));
        assert_eq!(validate_challenge(600, 0), Ok(Speed::Rapid));
        assert_eq!(
            validate_challenge(60, 0),
            Err(TimeControlError::TooFast(Speed::Bullet))
        );
        assert_eq!(validate_challenge(75, 0), Err(TimeControlError::BadLimit));
        assert_eq!(
            validate_challenge(600, 61),
            Err(TimeControlError::OutOfRange)
        );
        assert_eq!(
            validate_challenge(10_860, 0),
            Err(TimeControlError::OutOfRange)
        );
    }
}
