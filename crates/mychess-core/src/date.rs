//! PGN dates with unknown parts.
//!
//! A [`PartialDate`] is a year (`0..=2047`), month (`1..=12`) and day
//! (`1..=31`), each of which may be unknown. It prints as `YYYY.MM.DD` with
//! `????`/`??` for unknown parts. Like Scid, the value `0` means "unknown" and
//! is normalised to `None`, and the day is not checked against the month.

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

use crate::error::clip;

/// Largest year a [`PartialDate`] can hold (Scid stores the year in 11 bits).
pub const MAX_YEAR: u16 = 2047;

/// Why a date is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum DateError {
    /// The year is above [`MAX_YEAR`].
    #[error("year {0} is out of range (0..=2047)")]
    Year(u16),
    /// The month is above 12.
    #[error("month {0} is out of range (1..=12)")]
    Month(u8),
    /// The day is above 31.
    #[error("day {0} is out of range (1..=31)")]
    Day(u8),
    /// The text is not a `YYYY.MM.DD` date (see [`PartialDate::is_valid_str`]).
    #[error("invalid date text {text:?}")]
    Syntax {
        /// The offending text, clipped.
        text: String,
    },
}

/// A date whose year, month and day may each be unknown.
///
/// Ordering is chronological with unknown parts first (as in Scid), because
/// the fields are compared in year, month, day order and `None < Some`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PartialDate {
    year: Option<u16>,
    month: Option<u8>,
    day: Option<u8>,
}

impl PartialDate {
    /// The completely unknown date, `????.??.??`.
    pub const UNKNOWN: PartialDate = PartialDate {
        year: None,
        month: None,
        day: None,
    };

    /// Creates a date from its parts. A part equal to `0` is unknown.
    pub fn new(
        year: Option<u16>,
        month: Option<u8>,
        day: Option<u8>,
    ) -> Result<PartialDate, DateError> {
        let year = year.filter(|y| *y != 0);
        let month = month.filter(|m| *m != 0);
        let day = day.filter(|d| *d != 0);
        if let Some(y) = year.filter(|y| *y > MAX_YEAR) {
            return Err(DateError::Year(y));
        }
        if let Some(m) = month.filter(|m| *m > 12) {
            return Err(DateError::Month(m));
        }
        if let Some(d) = day.filter(|d| *d > 31) {
            return Err(DateError::Day(d));
        }
        Ok(PartialDate { year, month, day })
    }

    /// The year, if known.
    pub fn year(self) -> Option<u16> {
        self.year
    }

    /// The month (`1..=12`), if known.
    pub fn month(self) -> Option<u8> {
        self.month
    }

    /// The day (`1..=31`), if known.
    pub fn day(self) -> Option<u8> {
        self.day
    }

    /// Whether every part is unknown.
    pub fn is_unknown(self) -> bool {
        self == Self::UNKNOWN
    }

    /// Reads a date the way Scid's `date_EncodeFromString` does; never fails.
    ///
    /// Each segment (separated by `.`) is read as leading digits, so `?` and
    /// junk count as `0` (unknown). A year above 2047 becomes unknown but the
    /// month and day are still read (`3000.05.17` gives `????.05.17`); a
    /// month above 12 keeps only the year; a day above 31 keeps year and
    /// month.
    pub fn parse_lenient(text: &str) -> PartialDate {
        let mut segments = text.split('.');
        let mut next = || segments.next().map_or(0, leading_number);
        let year = next();
        let mut date = PartialDate {
            year: if year > u32::from(MAX_YEAR) {
                None
            } else {
                to_part(year)
            },
            ..Self::UNKNOWN
        };
        let month = next();
        if month > 12 {
            return date;
        }
        date.month = to_part(month);
        let day = next();
        if day > 31 {
            return date;
        }
        date.day = to_part(day);
        date
    }

    /// Packs the date as Scid's 32-bit `dateT`: `year << 9 | month << 5 | day`,
    /// with an unknown part stored as 0.
    pub fn to_scid_u32(self) -> u32 {
        u32::from(self.year.unwrap_or(0)) << 9
            | u32::from(self.month.unwrap_or(0)) << 5
            | u32::from(self.day.unwrap_or(0))
    }

    /// Unpacks a Scid `dateT`; `None` if a field is out of range (year above
    /// 2047, month above 12, day above 31) or bits above the year are set.
    pub fn from_scid_u32(value: u32) -> Option<PartialDate> {
        let year = u16::try_from(value >> 9).ok()?;
        let month = u8::try_from((value >> 5) & 0xF).ok()?;
        let day = u8::try_from(value & 0x1F).ok()?;
        PartialDate::new(Some(year), Some(month), Some(day)).ok()
    }

    /// Whether `text` is a well-formed date, as Scid's `date_ValidString`.
    ///
    /// One to three segments separated by `.`; each is either only digits or
    /// only `?` (never mixed, possibly empty), with year <= 2047, month <= 12
    /// and day <= 31. The day is not checked against the month.
    pub fn is_valid_str(text: &str) -> bool {
        const MAX: [u32; 3] = [2047, 12, 31];
        if text.split('.').count() > 3 {
            return false;
        }
        for (segment, max) in text.split('.').zip(MAX) {
            let digits = segment.bytes().filter(u8::is_ascii_digit).count();
            let questions = segment.bytes().filter(|b| *b == b'?').count();
            if digits + questions != segment.len() || (digits > 0 && questions > 0) {
                return false;
            }
            if digits > 0 && leading_number(segment) > max {
                return false;
            }
        }
        true
    }

    /// The date without trailing unknown parts, as Scid's `strTrimDate`:
    /// `2024.05.??` gives `2024.05` and `2024.??.??` gives `2024`.
    pub fn short(self) -> String {
        let full = self.to_string();
        if self.day.is_some() {
            return full;
        }
        if self.month.is_some() {
            return full[..7].to_owned();
        }
        full[..4].to_owned()
    }
}

/// Value of the leading ASCII digits of `s` (saturating); `0` if none.
fn leading_number(s: &str) -> u32 {
    s.bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0u32, |acc, b| {
            acc.saturating_mul(10).saturating_add(u32::from(b - b'0'))
        })
}

fn to_part<T: TryFrom<u32>>(value: u32) -> Option<T> {
    if value == 0 {
        None
    } else {
        T::try_from(value).ok()
    }
}

impl fmt::Display for PartialDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.year {
            Some(y) => write!(f, "{y:04}")?,
            None => f.write_str("????")?,
        }
        match self.month {
            Some(m) => write!(f, ".{m:02}")?,
            None => f.write_str(".??")?,
        }
        match self.day {
            Some(d) => write!(f, ".{d:02}"),
            None => f.write_str(".??"),
        }
    }
}

impl FromStr for PartialDate {
    type Err = DateError;

    /// Strict reading: the text must satisfy [`PartialDate::is_valid_str`].
    fn from_str(text: &str) -> Result<PartialDate, DateError> {
        if Self::is_valid_str(text) {
            Ok(Self::parse_lenient(text))
        } else {
            Err(DateError::Syntax { text: clip(text) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_round_trips() {
        for text in [
            "????.??.??",
            "2024.??.??",
            "2024.05.??",
            "2024.05.17",
            "0987.12.31",
        ] {
            let d: PartialDate = text.parse().unwrap();
            assert_eq!(d.to_string(), text);
        }
        assert_eq!(PartialDate::UNKNOWN.to_string(), "????.??.??");
    }

    #[test]
    fn new_validates_and_normalises_zero() {
        assert_eq!(
            PartialDate::new(Some(2048), None, None),
            Err(DateError::Year(2048))
        );
        assert_eq!(
            PartialDate::new(None, Some(13), None),
            Err(DateError::Month(13))
        );
        assert_eq!(
            PartialDate::new(None, None, Some(32)),
            Err(DateError::Day(32))
        );
        assert_eq!(
            PartialDate::new(Some(0), Some(0), Some(0)),
            Ok(PartialDate::UNKNOWN)
        );
    }

    #[test]
    fn lenient_parsing() {
        let d = |t| PartialDate::parse_lenient(t).to_string();
        assert_eq!(d("2024.05.17"), "2024.05.17");
        assert_eq!(d("2024"), "2024.??.??");
        assert_eq!(d("2024.??.17"), "2024.??.17");
        assert_eq!(d("????.??.??"), "????.??.??");
        assert_eq!(d("3000.05.17"), "????.05.17");
        assert_eq!(d("2048.05.17"), "????.05.17");
        assert_eq!(d("3000.13.17"), "????.??.??");
        assert_eq!(d("3000"), "????.??.??");
        assert_eq!(d("2024.13.17"), "2024.??.??");
        assert_eq!(d("2024.05.40"), "2024.05.??");
        assert_eq!(d(""), "????.??.??");
        assert_eq!(d("99999999999999.1.1"), "????.01.01");
    }

    #[test]
    fn scid_u32_packing() {
        let d: PartialDate = "2024.05.17".parse().unwrap();
        assert_eq!(d.to_scid_u32(), (2024 << 9) + (5 << 5) + 17);
        assert_eq!(PartialDate::UNKNOWN.to_scid_u32(), 0);
        assert_eq!(PartialDate::from_scid_u32(0), Some(PartialDate::UNKNOWN));
        for text in ["2024.05.17", "????.05.??", "2047.12.31", "2024.??.??"] {
            let d: PartialDate = text.parse().unwrap();
            assert_eq!(PartialDate::from_scid_u32(d.to_scid_u32()), Some(d));
        }
        assert_eq!(PartialDate::from_scid_u32(2048 << 9), None);
        assert_eq!(PartialDate::from_scid_u32(13 << 5), None);
        assert_eq!(PartialDate::from_scid_u32(15 << 5), None);
        assert_eq!(PartialDate::from_scid_u32(u32::MAX), None);
    }

    #[test]
    fn validity_rules() {
        for ok in [
            "",
            "2024",
            "2024.05",
            "2024.05.17",
            "????.??.??",
            "2024.??.??",
            "????.05.??",
            "2047.12.31",
            "2024.02.31",
        ] {
            assert!(PartialDate::is_valid_str(ok), "{ok}");
        }
        for bad in [
            "2048.01.01",
            "2024.13.01",
            "2024.01.32",
            "20?4.01.01",
            "2024.01.01.01",
            "abcd",
            "2024-05-17",
            "2024.0?.01",
            " 2024",
        ] {
            assert!(!PartialDate::is_valid_str(bad), "{bad}");
        }
        assert!("2024.13.01".parse::<PartialDate>().is_err());
    }

    #[test]
    fn short_form() {
        let s = |t: &str| t.parse::<PartialDate>().unwrap().short();
        assert_eq!(s("2024.05.17"), "2024.05.17");
        assert_eq!(s("2024.05.??"), "2024.05");
        assert_eq!(s("2024.??.??"), "2024");
        assert_eq!(s("????.??.??"), "????");
        assert_eq!(s("????.05.??"), "????.05");
    }

    #[test]
    fn unknown_sorts_first() {
        let a: PartialDate = "2024.??.??".parse().unwrap();
        let b: PartialDate = "2024.01.??".parse().unwrap();
        let c: PartialDate = "2024.01.02".parse().unwrap();
        assert!(PartialDate::UNKNOWN < a && a < b && b < c);
    }
}
