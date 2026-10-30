// Derived from Scid vs PC 4.27, src/pgnparse.cpp (the annotation symbol table).
// Copyright (c) 2001-2003 Shane Hudson. All rights reserved (notice of
// pgnparse.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Annotation glyph words (`N`, `RR`, `+-`, `!?`, ...), D22.

use mychess_core::Nag;

/// Bytes that appear in the ASCII spelling of a NAG.
fn is_glyph_byte(b: u8) -> bool {
    b.is_ascii_punctuation()
        || matches!(
            b,
            b'N' | b'R' | b'B' | b'b' | b'D' | b'Z' | b'z' | b'x' | b'o'
        )
}

/// Spellings `Nag::parse` accepts besides the canonical one.
const SYNONYMS: [&str; 3] = ["+/=", "=/+", "~="];

/// The NAG written exactly as `text` (the whole word), if any.
///
/// `oo`, `o-o` and `--` are NAGs in the Scid but read as castling and the
/// null move here: a move wins over a glyph.
pub(crate) fn glyph_exact(text: &[u8]) -> Option<Nag> {
    if text.is_empty() || text.len() > 6 || !text.iter().all(|&b| is_glyph_byte(b)) {
        return None;
    }
    let s = std::str::from_utf8(text).ok()?;
    let nag = Nag::parse(s)?;
    if matches!(nag.value(), 192 | 193 | 210) {
        return None;
    }
    (nag.ascii() == Some(s) || SYNONYMS.contains(&s)).then_some(nag)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("!", Some(1))]
    #[case("?", Some(2))]
    #[case("!!", Some(3))]
    #[case("??", Some(4))]
    #[case("!?", Some(5))]
    #[case("?!", Some(6))]
    #[case("=", Some(10))]
    #[case("+=", Some(14))]
    #[case("+/=", Some(14))]
    #[case("=+", Some(15))]
    #[case("=/+", Some(15))]
    #[case("+-", Some(18))]
    #[case("-+", Some(19))]
    #[case("=/&", Some(44))]
    #[case("N", Some(146))]
    #[case("R", Some(144))]
    #[case("RR", Some(145))]
    #[case("BB", Some(151))]
    #[case("Bb", Some(153))]
    #[case("D", Some(201))]
    #[case("ZZ", Some(22))]
    #[case("zz", Some(23))]
    #[case("x", Some(147))]
    #[case("--", None)]
    #[case("oo", None)]
    #[case("o-o", None)]
    #[case("!!!", None)]
    #[case("xyz", None)]
    #[case("Nf3", None)]
    #[case("e4", None)]
    #[case("", None)]
    #[case("B", None)]
    fn recognises_glyph_words(#[case] word: &str, #[case] expected: Option<u8>) {
        assert_eq!(
            glyph_exact(word.as_bytes()).map(Nag::value),
            expected,
            "{word}"
        );
    }
}
