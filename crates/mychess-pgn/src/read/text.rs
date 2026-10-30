// Derived from Scid vs PC 4.27, src/pgnparse.cpp (standardPlayerName, standardDutchName and the comment white space rules).
// Copyright (c) 2001-2003 Shane Hudson. All rights reserved (notice of
// pgnparse.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Text helpers: decoding (D18), white space in comments (D20) and player
//! names (D24).

use std::borrow::Cow;

use encoding_rs::Encoding;

/// Decodes the text of a tag or comment: UTF-8 when valid, otherwise the
/// file's fallback encoding, otherwise `U+FFFD`.
pub(crate) struct Decoder {
    fallback: Option<&'static Encoding>,
    /// Whether the fallback encoding was used at least once.
    pub(crate) used_fallback: bool,
}

impl Decoder {
    pub(crate) fn new(fallback: Option<&'static Encoding>) -> Decoder {
        Decoder {
            fallback,
            used_fallback: false,
        }
    }

    pub(crate) fn decode<'b>(&mut self, bytes: &'b [u8]) -> Cow<'b, str> {
        match std::str::from_utf8(bytes) {
            Ok(s) => Cow::Borrowed(s),
            Err(_) => match self.fallback {
                Some(encoding) => {
                    self.used_fallback = true;
                    encoding.decode_without_bom_handling(bytes).0
                }
                None => String::from_utf8_lossy(bytes),
            },
        }
    }
}

/// The first `max` bytes of `bytes` for decoding. When the cut falls inside a
/// UTF-8 character (and only then) the partial character is dropped, so that
/// a long valid text is not sent to the fallback encoding because of the cut.
pub(crate) fn cut_for_decode(bytes: &[u8], max: usize) -> &[u8] {
    if bytes.len() <= max {
        return bytes;
    }
    let head = &bytes[..max];
    match std::str::from_utf8(head) {
        Err(e) if e.error_len().is_none() => &head[..e.valid_up_to()],
        _ => head,
    }
}

/// White space that `strSingleSpace` collapses.
fn is_collapsible(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

/// Runs of white space (line breaks included) become one space; the ends are
/// trimmed (Scid's `strSingleSpace`).
pub(crate) fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars() {
        if is_collapsible(c) {
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(c);
        }
    }
    out
}

/// Normalises `\r\n` and `\r` to `\n` and trims blank ends.
pub(crate) fn normalize_newlines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out.trim_matches(is_collapsible).to_owned()
}

/// Truncates `text` to at most `max` bytes, on a character boundary.
pub(crate) fn truncate_at_boundary(text: &mut String, max: usize) {
    if text.len() <= max {
        return;
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text.truncate(cut);
}

/// A copy of `text` of at most `max_chars` characters (`...` appended when
/// clipped), for diagnostics.
pub(crate) fn clip(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        None => text.to_owned(),
        Some((i, _)) => format!("{}...", &text[..i]),
    }
}

/// Reads the leading decimal digits of `text` (after blanks), saturating.
pub(crate) fn leading_number(text: &str) -> u32 {
    text.trim_start()
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0u32, |n, d| {
            n.saturating_mul(10).saturating_add(u32::from(d - b'0'))
        })
}

/// Splits a rating written after a player name, `Name (2800)`: the name must
/// not be empty and the parenthesis must follow a space (Scid's rule, applied
/// before normalisation, D24).
pub(crate) fn split_name_rating(value: &str) -> (&str, Option<u32>) {
    let b = value.as_bytes();
    let n = b.len();
    if n > 7
        && b[n - 1] == b')'
        && b[n - 5..n - 1].iter().all(u8::is_ascii_digit)
        && b[n - 6] == b'('
        && b[n - 7] == b' '
    {
        let elo = leading_number(&value[n - 5..]);
        return (&value[..n - 7], Some(elo));
    }
    (value, None)
}

/// Scid's `standardPlayerName`: one space after each comma, no leading blanks
/// and no trailing blanks, and the `van der / den / de` capitalisation.
pub(crate) fn standard_player_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 1);
    let mut after_comma = false;
    let mut start_spaces = true;
    for c in name.chars() {
        match c {
            ',' => {
                out.push(',');
                after_comma = true;
                start_spaces = false;
            }
            ' ' => {
                if !after_comma && !start_spaces {
                    out.push(' ');
                }
            }
            other => {
                start_spaces = false;
                if after_comma {
                    out.push(' ');
                }
                out.push(other);
                after_comma = false;
            }
        }
    }
    let trimmed = out.trim_end_matches([' ', '\t']).len();
    out.truncate(trimmed);
    // standardDutchName
    if out.starts_with("van ") {
        out.replace_range(..1, "V");
    }
    for prefix in ["Van Der ", "Van Den ", "Van De "] {
        if out.starts_with(prefix) {
            out.replace_range(4..5, "d");
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use encoding_rs::WINDOWS_1252;
    use rstest::rstest;

    use super::*;

    #[test]
    fn decoder_prefers_utf8_then_fallback_then_replacement() {
        let mut d = Decoder::new(Some(WINDOWS_1252));
        assert_eq!(d.decode("Müller £".as_bytes()), "Müller £");
        assert!(!d.used_fallback);
        assert_eq!(d.decode(b"M\xfcller \xa310 \x80"), "Müller £10 €");
        assert!(d.used_fallback);
        let mut utf8_only = Decoder::new(None);
        assert_eq!(utf8_only.decode(b"M\xfcller"), "M\u{fffd}ller");
        assert!(!utf8_only.used_fallback);
    }

    #[rstest]
    #[case("  a \n\t b  ", "a b")]
    #[case("one", "one")]
    #[case("", "")]
    #[case("\n\n", "")]
    #[case("a\u{a0}b", "a\u{a0}b")]
    fn collapses_whitespace(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(collapse_whitespace(input), expected);
    }

    #[test]
    fn normalizes_newlines() {
        assert_eq!(normalize_newlines(" a\r\nb\rc\n"), "a\nb\nc");
    }

    #[test]
    fn truncates_on_char_boundary() {
        let mut s = "aé".to_owned();
        truncate_at_boundary(&mut s, 2);
        assert_eq!(s, "a");
        let mut s = "abc".to_owned();
        truncate_at_boundary(&mut s, 10);
        assert_eq!(s, "abc");
        assert_eq!(clip("abcdef", 3), "abc...");
        assert_eq!(clip("ab", 3), "ab");
    }

    #[rstest]
    #[case("2800", 2800)]
    #[case(" 2800x", 2800)]
    #[case("abc", 0)]
    #[case("", 0)]
    #[case("99999999999999999999", u32::MAX)]
    fn reads_leading_numbers(#[case] input: &str, #[case] expected: u32) {
        assert_eq!(leading_number(input), expected);
    }

    #[rstest]
    #[case("Kasparov,G (2800)", "Kasparov,G", Some(2800))]
    #[case("X (9999)", "X", Some(9999))]
    #[case("X (0000)", "X", Some(0))]
    #[case("X(2800)", "X(2800)", None)]
    #[case(" (2800)", " (2800)", None)]
    #[case("X (280)", "X (280)", None)]
    #[case("X (28a0)", "X (28a0)", None)]
    #[case("Nom (2800)", "Nom", Some(2800))]
    fn splits_rating_from_name(#[case] input: &str, #[case] name: &str, #[case] elo: Option<u32>) {
        assert_eq!(split_name_rating(input), (name, elo));
    }

    #[rstest]
    #[case("Kasparov,G", "Kasparov, G")]
    #[case("Kasparov,   G", "Kasparov, G")]
    #[case("Kasparov, G  ", "Kasparov, G")]
    #[case("  Anand,V", "Anand, V")]
    #[case("Anand, V.", "Anand, V.")]
    #[case("van Gelder", "Van Gelder")]
    #[case("van der Wiel, J", "Van der Wiel, J")]
    #[case("Van Der Wiel", "Van der Wiel")]
    #[case("Van Den Doel", "Van den Doel")]
    #[case("Van De Kamp", "Van de Kamp")]
    #[case("vanity", "vanity")]
    #[case("Two  Spaces", "Two  Spaces")]
    #[case("Name,", "Name,")]
    fn normalizes_player_names(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(standard_player_name(input), expected);
    }
}
