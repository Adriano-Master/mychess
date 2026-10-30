//! ECO opening codes in Scid's extended form.
//!
//! A basic code is a letter `A`-`E` and two digits (`B91`). Scid extends it
//! with a letter `a`-`z` and an optional digit `1`-`4` (`B91a`, `E99b2`).
//! Internally Scid numbers the codes: none = 0, `A00` = 1, `A00a` = 2,
//! `A01` = 132 and so on; that number is exposed for the SI4 codec.

use std::fmt;
use std::num::NonZeroU16;
use std::str::FromStr;

use thiserror::Error;

use crate::error::clip;

/// Codes per basic ECO code: 26 letters x 5 (letter alone + digits 1-4) + 1.
const SUBCODES: u16 = 131;
/// Number of basic codes (`A00` to `E99`).
const BASIC_CODES: u16 = 500;

/// Why a text is not an ECO code.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum EcoError {
    /// The text is not `A`-`E` + digits (+ extension); see [`Eco`].
    #[error("invalid ECO code {text:?}")]
    Syntax {
        /// The offending text, clipped.
        text: String,
    },
}

/// An ECO code, with Scid's extensions. `Display` prints the extended form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Eco(NonZeroU16);

impl Eco {
    /// Builds an ECO code from Scid's internal number (`1..=65500`).
    /// Zero (no ECO) and numbers beyond `E99z4` give `None`.
    pub fn from_scid_code(code: u16) -> Option<Eco> {
        if code > BASIC_CODES * SUBCODES {
            return None;
        }
        NonZeroU16::new(code).map(Eco)
    }

    /// Scid's internal number: `A00` = 1, `A01` = 132.
    pub fn to_scid_code(self) -> u16 {
        self.0.get()
    }

    /// Scid-compatible `eco_FromString`: reads the longest valid ECO prefix
    /// of `text` and returns it with the unread rest, ignoring trailing junk
    /// (`"B90 Sicilian"` gives `B90` and `" Sicilian"`). Partial codes fill
    /// with zeros like [`FromStr`]; text that does not start with a valid
    /// code gives `None`. An extension letter is taken even when followed by
    /// something else (`"B91ax"` gives `B91a` and `"x"`), and a digit
    /// `1`-`4` after it is part of the code.
    ///
    /// Use [`str::parse`] for strict validation.
    pub fn parse_prefix(text: &str) -> Option<(Eco, &str)> {
        let b = text.as_bytes();
        let letter = match b.first()? {
            c @ b'A'..=b'E' => c - b'A',
            c @ b'a'..=b'e' => c - b'a',
            _ => return None,
        };
        let mut code = u16::from(letter) * 100 * SUBCODES;
        let mut used = 1;
        for weight in [10 * SUBCODES, SUBCODES] {
            match b.get(used) {
                None => return Eco::from_scid_code(code + 1).map(|e| (e, &text[used..])),
                Some(c) if c.is_ascii_digit() => {
                    code += u16::from(c - b'0') * weight;
                    used += 1;
                }
                Some(_) => return None,
            }
        }
        if let Some(e @ b'a'..=b'z') = b.get(used) {
            code += 1 + u16::from(e - b'a') * 5;
            used += 1;
            if let Some(n @ b'1'..=b'4') = b.get(used) {
                code += u16::from(n - b'0');
                used += 1;
            }
        }
        Eco::from_scid_code(code + 1).map(|e| (e, &text[used..]))
    }

    /// The code without the extension: `B91a` gives `B91`.
    #[must_use]
    pub fn basic(self) -> Eco {
        let base = (self.0.get() - 1) / SUBCODES * SUBCODES + 1;
        NonZeroU16::new(base).map_or(self, Eco)
    }

    /// The deepest code this one could contain (Scid's `eco_LastSubCode`,
    /// `misc.cpp`): `B91` gives `B91z4`, `B91a` gives `B91a4` and `B91a2`
    /// stays as it is. It is the upper end of an ECO range.
    #[must_use]
    pub fn last_sub_code(self) -> Eco {
        let mut offset = (self.0.get() - 1) % SUBCODES;
        let base = self.0.get() - 1 - offset;
        if offset == 0 {
            offset = 1 + 25 * 5;
        }
        // A letter without a digit (offset 1 modulo 5) takes the digit 4.
        if offset % 5 == 1 {
            offset += 4;
        }
        Eco::from_scid_code(base + offset + 1).unwrap_or(self)
    }

    /// Whether the code carries an extension (`B91a`).
    pub fn is_extended(self) -> bool {
        self.basic() != self
    }

    /// The basic form as text, `B91`.
    pub fn basic_string(self) -> String {
        let basic = (self.0.get() - 1) / SUBCODES;
        let letter = u8::try_from(basic / 100).map_or('?', |l| char::from(b'A' + l));
        format!("{letter}{:02}", basic % 100)
    }
}

impl fmt::Display for Eco {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.basic_string())?;
        let ext = (self.0.get() - 1) % SUBCODES;
        if ext > 0 {
            let ext = ext - 1;
            let letter = u8::try_from(ext / 5).map_or('?', |l| char::from(b'a' + l));
            write!(f, "{letter}")?;
            if !ext.is_multiple_of(5) {
                write!(f, "{}", ext % 5)?;
            }
        }
        Ok(())
    }
}

impl FromStr for Eco {
    type Err = EcoError;

    /// Reads `A00`, `b91`, `B91a`, `E99b2`; partial codes fill with zeros
    /// (`C` is `C00`, `C1` is `C10`). Anything else is an error.
    fn from_str(text: &str) -> Result<Eco, EcoError> {
        let err = || EcoError::Syntax { text: clip(text) };
        let b = text.as_bytes();
        let letter = match b.first() {
            Some(c @ b'A'..=b'E') => c - b'A',
            Some(c @ b'a'..=b'e') => c - b'a',
            _ => return Err(err()),
        };
        let mut code = u16::from(letter) * 100 * SUBCODES;
        let digit = |i: usize| {
            b.get(i)
                .filter(|c| c.is_ascii_digit())
                .map(|c| u16::from(c - b'0'))
        };
        match b.len() {
            1 => {}
            2 => code += digit(1).ok_or_else(err)? * 10 * SUBCODES,
            _ => {
                code += digit(1).ok_or_else(err)? * 10 * SUBCODES;
                code += digit(2).ok_or_else(err)? * SUBCODES;
                match &b[3..] {
                    [] => {}
                    [e @ b'a'..=b'z'] => code += 1 + u16::from(e - b'a') * 5,
                    [e @ b'a'..=b'z', n @ b'1'..=b'4'] => {
                        code += 1 + u16::from(e - b'a') * 5 + u16::from(n - b'0');
                    }
                    _ => return Err(err()),
                }
            }
        }
        Eco::from_scid_code(code + 1).ok_or_else(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eco(t: &str) -> Eco {
        t.parse().unwrap()
    }

    #[test]
    fn internal_codes() {
        assert_eq!(eco("A00").to_scid_code(), 1);
        assert_eq!(eco("A00a").to_scid_code(), 2);
        assert_eq!(eco("A01").to_scid_code(), 132);
        assert_eq!(eco("E99z4").to_scid_code(), 65500);
        assert_eq!(Eco::from_scid_code(0), None);
        assert_eq!(Eco::from_scid_code(65501), None);
        assert_eq!(Eco::from_scid_code(65500), Some(eco("E99z4")));
    }

    #[test]
    fn round_trips() {
        for t in ["A00", "B91", "B91a", "E99b2", "C10", "D05z4", "A00a1"] {
            assert_eq!(eco(t).to_string(), t);
        }
        for code in 1..=65500u16 {
            let e = Eco::from_scid_code(code).unwrap();
            assert_eq!(e.to_string().parse::<Eco>(), Ok(e), "{code}");
        }
    }

    #[test]
    fn partial_and_case() {
        assert_eq!(eco("C1"), eco("C10"));
        assert_eq!(eco("C"), eco("C00"));
        assert_eq!(eco("b91A".to_lowercase().as_str()), eco("B91a"));
        assert_eq!(eco("e99"), eco("E99"));
    }

    #[test]
    fn rejects_bad_text() {
        for t in [
            "", "F00", "Z", "1", "A0x", "AA0", "B91A", "B91a5", "B91a1x", "B91 ", "B91!", "A00ab",
            "é",
        ] {
            assert!(t.parse::<Eco>().is_err(), "{t:?}");
        }
    }

    #[test]
    fn prefix_parsing_ignores_trailing_junk() {
        let p = |t| Eco::parse_prefix(t).map(|(e, r)| (e.to_string(), r));
        assert_eq!(p("B90 Sicilian"), Some(("B90".to_owned(), " Sicilian")));
        assert_eq!(p("B91a"), Some(("B91a".to_owned(), "")));
        assert_eq!(p("E99b2x"), Some(("E99b2".to_owned(), "x")));
        assert_eq!(p("b91A"), Some(("B91".to_owned(), "A")));
        assert_eq!(p("B91a5"), Some(("B91a".to_owned(), "5")));
        assert_eq!(p("C1"), Some(("C10".to_owned(), "")));
        assert_eq!(p("C"), Some(("C00".to_owned(), "")));
        assert_eq!(p("C1x"), None);
        assert_eq!(p("F00"), None);
        assert_eq!(p(""), None);
        assert_eq!(p(" B90"), None);
        assert_eq!(p("B9é"), None);
        assert_eq!(p("B90é"), Some(("B90".to_owned(), "é")));
        // Strict parsing still rejects what the prefix reader tolerates.
        assert!("B90 Sicilian".parse::<Eco>().is_err());
    }

    #[test]
    fn last_sub_code_follows_scid() {
        assert_eq!(eco("B91").last_sub_code(), eco("B91z4"));
        assert_eq!(eco("B91a").last_sub_code(), eco("B91a4"));
        assert_eq!(eco("B91a2").last_sub_code(), eco("B91a2"));
        assert_eq!(eco("B91z").last_sub_code(), eco("B91z4"));
        assert_eq!(eco("E99z4").last_sub_code(), eco("E99z4"));
        assert_eq!(eco("A00").last_sub_code(), eco("A00z4"));
    }

    #[test]
    fn basic_form() {
        assert_eq!(eco("B91a2").basic(), eco("B91"));
        assert_eq!(eco("B91").basic(), eco("B91"));
        assert!(eco("B91a").is_extended());
        assert!(!eco("B91").is_extended());
        assert_eq!(eco("B91a2").basic_string(), "B91");
    }
}
