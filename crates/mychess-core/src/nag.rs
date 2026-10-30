//! Numeric annotation glyphs (NAGs).
//!
//! A [`Nag`] is a number in `1..=255` (the PGN standard defines `$1` to
//! `$255`; `$0` means "no annotation" and is not representable). Text
//! symbols (`!`, `+-`, `N`...) are read by [`Nag::parse`], which follows
//! Scid's `game_parseNag`; display symbols come from Scid's `nagtext.h`.

use std::borrow::Cow;
use std::fmt;
use std::num::NonZeroU8;

use thiserror::Error;

/// Why a number is not a valid [`Nag`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum NagError {
    /// NAG 0 means "no annotation" and is not a NAG.
    #[error("NAG 0 is not a valid annotation")]
    Zero,
    /// The value does not fit in the `1..=255` range of PGN.
    #[error("NAG {0} is out of range (1..=255)")]
    OutOfRange(u32),
}

/// A numeric annotation glyph, `$1` to `$255`.
///
/// `Display` prints the PGN numeric form (`$12`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Nag(NonZeroU8);

/// Display symbols of Scid's `evalNagsRegular` table (`nagtext.h`).
///
/// An empty string means the entry has no symbol (Scid shows `$n`). Entry 0
/// is the null annotation.
const GLYPHS: [&str; 216] = [
    "",
    "!",
    "?",
    "!!",
    "??",
    "!?",
    "?!",
    "forced",
    "□",
    "worst",
    "=",
    "=, quiet",
    "↹",
    "~",
    "+=",
    "=+",
    "+/-",
    "-/+",
    "+-",
    "-+",
    "+--",
    "--+",
    "⊙",
    "⊙",
    "◯",
    "◯",
    "◯◯",
    "◯◯",
    "◯◯◯",
    "◯◯◯",
    "↻",
    "↺",
    "↻↻",
    "↺↺",
    "↻↻↻",
    "↺↺↺",
    "↑",
    "↓",
    "⇑",
    "⇓",
    "→",
    "←",
    "&/-",
    "&/+",
    "=/&",
    "=/&",
    "+/&",
    "-/&",
    "⊞",
    "⊞",
    "⊞⊞",
    "⊞⊞",
    "⊞⊞⊞",
    "⊞⊞⊞",
    "⟩",
    "⟩",
    "⟫",
    "⟫",
    "⋙",
    "⋙",
    "⟨",
    "⟨",
    "⟪",
    "⟪",
    "⋘",
    "⋘",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "↗",
    "↖",
    "",
    "",
    "⇈",
    "⇊",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "⇄",
    "⇆",
    "⇄⇄",
    "⇆⇆",
    "⇄⇄⇄",
    "⇆⇆⇆",
    "⊕",
    "⊖",
    "⊕⊕",
    "⊖⊖",
    "△",
    "",
    "⌓",
    "",
    "R",
    "RR",
    "N",
    "×",
    "⊥",
    "⟺",
    "⇗",
    "◫",
    "",
    "◨",
    "⊶",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "ǁ",
    "⡁",
    "⚯",
    "⚮",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "D",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "—",
    "#",
    "⚨",
    "",
    "⌊",
    "⌋",
];

/// ASCII symbols accepted by [`Nag::parse`], one canonical spelling per NAG.
const ASCII: &[(u8, &str)] = &[
    (1, "!"),
    (2, "?"),
    (3, "!!"),
    (4, "??"),
    (5, "!?"),
    (6, "?!"),
    (8, "[]"),
    (10, "="),
    (13, "~"),
    (14, "+="),
    (15, "=+"),
    (16, "+/-"),
    (17, "-/+"),
    (18, "+-"),
    (19, "-+"),
    (20, "+--"),
    (21, "--+"),
    (22, "ZZ"),
    (23, "zz"),
    (40, "+>"),
    (41, "->"),
    (44, "=/&"),
    (48, "[+]"),
    (50, "[++]"),
    (54, ">"),
    (56, ">>"),
    (58, ">>>"),
    (60, "<"),
    (62, "<<"),
    (64, "<<<"),
    (130, "<+>"),
    (131, "<->"),
    (132, "<++>"),
    (133, "<-->"),
    (134, "<+++>"),
    (135, "<--->"),
    (140, "/\\"),
    (142, "(_)"),
    (144, "R"),
    (145, "RR"),
    (146, "N"),
    (147, "x"),
    (148, "_|_"),
    (149, "<=>"),
    (150, "/"),
    (151, "BB"),
    (153, "Bb"),
    (190, "||"),
    (192, "o-o"),
    (193, "oo"),
    (201, "D"),
    (210, "--"),
    (212, "o^"),
    (214, "|_"),
    (215, "_|"),
];

impl Nag {
    /// Creates a NAG from its number (`1..=255`).
    pub fn new(n: u16) -> Result<Nag, NagError> {
        Self::from_wide(u32::from(n))
    }

    fn from_wide(n: u32) -> Result<Nag, NagError> {
        let byte = u8::try_from(n).map_err(|_| NagError::OutOfRange(n))?;
        NonZeroU8::new(byte).map(Nag).ok_or(NagError::Zero)
    }

    /// The NAG number, `1..=255`.
    pub fn value(self) -> u8 {
        self.0.get()
    }

    /// Reads a NAG from text, as Scid's `game_parseNag` does.
    ///
    /// Accepts `$n` or plain digits (`n` in `1..=255`; larger numbers are
    /// rejected instead of wrapping modulo 256), and the symbols `!`, `?`,
    /// `!!`, `??`, `!?`, `?!`, `[]`, `=`, `~`, `+=`, `+/=`, `=+`, `=/+`, `+/-`,
    /// `-/+`, `+-`, `-+`, `+--`, `--+`, `ZZ`, `zz`, `[+]`, `[++]`, `>`, `>>`, `>>>`,
    /// `<`, `<<`, `<<<`, `<+>`, `<->`, `<++>`, `<-->`, `<+++>`, `<--->`, `/\\`,
    /// `(_)`, `R`, `RR`, `N`, `x...`, `_|_`, `<=>`, `/`, `BB`, `Bb`, `||`, `o-o`,
    /// `oo`, `D`, `--`, `o^`, `|_`, `_|`, `->`, `+>`, `=/&` and `~=`.
    ///
    /// Like Scid, several symbols are recognised by a prefix only (for example
    /// `!!x` reads as `!!`, and any text starting with `x` is `$147`); the
    /// caller is expected to hand over one PGN token. Anything else gives
    /// `None`.
    pub fn parse(text: &str) -> Option<Nag> {
        parse_nag(text.as_bytes())
    }

    /// The ASCII symbol that [`Nag::parse`] reads back to this NAG, if any
    /// (`1` gives `"!"`, `18` gives `"+-"`).
    pub fn ascii(self) -> Option<&'static str> {
        let v = self.value();
        ASCII.iter().find(|(n, _)| *n == v).map(|(_, s)| *s)
    }

    /// The display symbol from Scid's table (Unicode for many entries, such
    /// as `⊙` for zugzwang). NAGs without a symbol give `$n`.
    pub fn glyph(self) -> Cow<'static, str> {
        match GLYPHS.get(usize::from(self.value())) {
            Some(s) if !s.is_empty() => Cow::Borrowed(s),
            _ => Cow::Owned(format!("${}", self.value())),
        }
    }
}

impl fmt::Display for Nag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}", self.value())
    }
}

impl TryFrom<u8> for Nag {
    type Error = NagError;

    fn try_from(n: u8) -> Result<Nag, NagError> {
        Nag::new(u16::from(n))
    }
}

/// Port of `game_parseNag`. Reading past the end yields 0, like the NUL
/// terminator of the C string.
#[allow(clippy::too_many_lines)] // a flat port of one C function
fn parse_nag(s: &[u8]) -> Option<Nag> {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let nag = |n: u8| Nag::try_from(n).ok();
    let (c1, c2, c3) = (at(1), at(2), at(3));
    match at(0) {
        b'$' => parse_number(&s[1..]),
        b'0'..=b'9' => parse_number(s),
        b'!' => match c1 {
            0 => nag(1),
            b'!' => nag(3),
            b'?' => nag(5),
            _ => None,
        },
        b'?' => match c1 {
            0 => nag(2),
            b'?' => nag(4),
            b'!' => nag(6),
            _ => None,
        },
        b'+' => match (c1, c2) {
            (b'=', _) | (b'/', b'=') => nag(14),
            (b'-', 0) => nag(18),
            (b'>', _) => nag(40),
            (b'/', b'-') => nag(16),
            (b'-', b'-') => nag(20),
            _ => None,
        },
        b'=' => match (c1, c2) {
            (0, _) => nag(10),
            (b'+', _) | (b'/', b'+') => nag(15),
            (b'/', b'&') => nag(44),
            _ => None,
        },
        b'-' => match (c1, c2) {
            (b'+', _) => nag(19),
            (b'>', _) => nag(41),
            (b'/', b'+') => nag(17),
            (b'-', b'+') => nag(21),
            (b'-', 0) => nag(210),
            _ => None,
        },
        b'/' => match c1 {
            0 => nag(150),
            b'\\' => nag(140),
            _ => None,
        },
        b'R' => match c1 {
            0 => nag(144),
            b'R' => nag(145),
            _ => None,
        },
        b'z' => (c1 == b'z').then(|| nag(23)).flatten(),
        b'Z' => (c1 == b'Z').then(|| nag(22)).flatten(),
        b'B' => match c1 {
            b'B' => nag(151),
            b'b' => nag(153),
            _ => None,
        },
        b'o' => match (c1, c2, c3) {
            (b'-', b'o', _) => nag(192),
            (b'o', 0, _) => nag(193),
            (b'^', 0, _) => nag(212),
            _ => None,
        },
        b'(' => (c1 == b'_' && c2 == b')').then(|| nag(142)).flatten(),
        b'[' => match (c1, c2, c3) {
            (b']', 0, _) => nag(8),
            (b'+', b']', _) => nag(48),
            (b'+', b'+', b']') => nag(50),
            _ => None,
        },
        b'_' => match (c1, c2) {
            (b'|', b'_') => nag(148),
            (b'|', 0) => nag(215),
            _ => None,
        },
        b'|' => match c1 {
            b'|' => nag(190),
            b'_' => nag(214),
            _ => None,
        },
        b'>' => match (c1, c2) {
            (0, _) => nag(54),
            (b'>', 0) => nag(56),
            (b'>', b'>') => nag(58),
            _ => None,
        },
        b'<' => match (c1, c2, c3, at(4)) {
            (0, _, _, _) => nag(60),
            (b'<', 0, _, _) => nag(62),
            (b'<', b'<', 0, _) => nag(64),
            (b'=', b'>', 0, _) => nag(149),
            (b'+', b'>', 0, _) => nag(130),
            (b'-', b'>', 0, _) => nag(131),
            (b'+', b'+', b'>', 0) => nag(132),
            (b'-', b'-', b'>', 0) => nag(133),
            (b'+', b'+', b'+', b'>') => nag(134),
            (b'-', b'-', b'-', b'>') => nag(135),
            _ => None,
        },
        b'~' => {
            if c1 == b'=' {
                nag(44)
            } else {
                nag(13)
            }
        }
        b'x' => nag(147),
        b'N' if c1 == 0 => nag(146),
        b'D' if c1 == 0 => nag(201),
        _ => None,
    }
}

/// Reads the leading decimal digits of `s` (`strtoul` style) as a NAG.
fn parse_number(s: &[u8]) -> Option<Nag> {
    let mut value: u32 = 0;
    for &b in s.iter().take_while(|b| b.is_ascii_digit()) {
        value = value.saturating_mul(10).saturating_add(u32::from(b - b'0'));
    }
    Nag::from_wide(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> Option<u8> {
        Nag::parse(text).map(Nag::value)
    }

    #[test]
    fn new_validates_range() {
        assert_eq!(Nag::new(1).map(Nag::value), Ok(1));
        assert_eq!(Nag::new(255).map(Nag::value), Ok(255));
        assert_eq!(Nag::new(0), Err(NagError::Zero));
        assert_eq!(Nag::new(256), Err(NagError::OutOfRange(256)));
    }

    #[test]
    fn display_is_dollar_form() {
        assert_eq!(Nag::new(12).map(|n| n.to_string()), Ok("$12".to_owned()));
    }

    #[test]
    fn parses_numeric_forms() {
        assert_eq!(p("$12"), Some(12));
        assert_eq!(p("12"), Some(12));
        assert_eq!(p("$255"), Some(255));
        assert_eq!(p("$256"), None);
        assert_eq!(p("$0"), None);
        assert_eq!(p("$"), None);
        assert_eq!(p("99999999999999999999"), None);
    }

    #[test]
    fn parses_every_symbol_of_the_spec() {
        let table: &[(&str, u8)] = &[
            ("!", 1),
            ("?", 2),
            ("!!", 3),
            ("??", 4),
            ("!?", 5),
            ("?!", 6),
            ("[]", 8),
            ("=", 10),
            ("~", 13),
            ("+=", 14),
            ("+/=", 14),
            ("=+", 15),
            ("=/+", 15),
            ("+/-", 16),
            ("-/+", 17),
            ("+-", 18),
            ("-+", 19),
            ("+--", 20),
            ("--+", 21),
            ("ZZ", 22),
            ("zz", 23),
            ("[+]", 48),
            ("[++]", 50),
            (">", 54),
            (">>", 56),
            (">>>", 58),
            ("<", 60),
            ("<<", 62),
            ("<<<", 64),
            ("<+>", 130),
            ("<->", 131),
            ("<++>", 132),
            ("<-->", 133),
            ("<+++>", 134),
            ("<--->", 135),
            ("/\\", 140),
            ("(_)", 142),
            ("R", 144),
            ("RR", 145),
            ("N", 146),
            ("x", 147),
            ("x...", 147),
            ("_|_", 148),
            ("<=>", 149),
            ("/", 150),
            ("BB", 151),
            ("Bb", 153),
            ("||", 190),
            ("o-o", 192),
            ("oo", 193),
            ("D", 201),
            ("--", 210),
            ("o^", 212),
            ("|_", 214),
            ("_|", 215),
            ("->", 41),
            ("+>", 40),
            ("=/&", 44),
            ("~=", 44),
        ];
        for (text, n) in table {
            assert_eq!(p(text), Some(*n), "{text}");
        }
    }

    #[test]
    fn rejects_unknown_symbols() {
        for text in [
            "", "!x", "+", "+x", "=x", "-", "-x", "Q", "NN", "DD", "<x", "[", "(", "_", "|", "z",
            "o", "é",
        ] {
            assert_eq!(p(text), None, "{text:?}");
        }
    }

    #[test]
    fn ascii_round_trips_through_parse() {
        let mut seen = 0;
        for n in 1..=255u8 {
            let nag = Nag::try_from(n).unwrap();
            if let Some(sym) = nag.ascii() {
                assert_eq!(Nag::parse(sym), Some(nag), "{sym}");
                seen += 1;
            }
        }
        assert_eq!(seen, ASCII.len());
    }

    #[test]
    fn glyphs_have_dollar_fallback() {
        let g = |n| Nag::new(n).unwrap().glyph().into_owned();
        assert_eq!(g(1), "!");
        assert_eq!(g(7), "forced");
        assert_eq!(g(22), "⊙");
        assert_eq!(g(66), "$66");
        assert_eq!(g(201), "D");
        assert_eq!(g(216), "$216");
        assert_eq!(g(255), "$255");
    }

    /// FNV-1a/64 of the 216 glyphs (entry 0 empty, `$n` fallbacks included), each followed by a NUL byte,
    /// computed from Scid's `nagtext.h`. Guards against shifted or edited
    /// entries when the (git-ignored) `reference/` tree is absent.
    #[test]
    fn glyph_table_hash_is_unchanged() {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for n in 0..216u16 {
            let entry = if n == 0 {
                Cow::Borrowed("")
            } else {
                Nag::new(n).unwrap().glyph()
            };
            for byte in entry.bytes().chain([0]) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
        assert_eq!(GLYPHS.len(), 216);
        assert_eq!(hash, 0xa5f1_a99b_fd95_b6bc);
    }

    /// Compares all 216 entries with Scid's `nagtext.h`; fails if the
    /// (git-ignored) reference tree is missing, so it is ignored by default.
    #[test]
    #[ignore = "requires reference/scid_vs_pc-4.27 (run with --ignored)"]
    fn glyphs_match_scid_nagtext() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../reference/scid_vs_pc-4.27/src/nagtext.h"
        );
        let src = std::fs::read_to_string(path);
        assert!(src.is_ok(), "reference file not found: {path}");
        let src = src.unwrap_or_default();
        let start = src.find("evalNagsRegular").unwrap();
        let end = start + src[start..].find("};").unwrap();
        let mut entries = Vec::new();
        for line in src[start..end].lines() {
            let line = line.trim();
            if !line.starts_with('"') {
                continue;
            }
            let close = line[1..].find('"').unwrap() + 1;
            entries.push(&line[1..close]);
        }
        assert_eq!(entries.len(), 216);
        for (i, expected) in entries.iter().enumerate().skip(1) {
            let n = u16::try_from(i).unwrap();
            assert_eq!(Nag::new(n).unwrap().glyph(), *expected, "NAG {i}");
        }
    }

    #[test]
    fn hostile_input_does_not_panic() {
        for text in [
            "$\u{0}",
            "\u{0}",
            "<+++",
            "<---",
            "<+",
            "[+",
            "$99\u{1F600}",
            "x\u{1F600}",
        ] {
            let _ = Nag::parse(text);
        }
    }
}
