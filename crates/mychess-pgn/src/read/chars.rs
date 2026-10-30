//! Byte classes shared by the chunker and the lexers.
//!
//! The chunker and the movetext lexer must agree, byte for byte, on where a
//! word ends and which words are game results; keeping the rules here makes
//! that hard to break.

use mychess_core::GameResult;

/// Whitespace as the Scid and PGN understand it (ASCII, including line
/// breaks and the vertical tab).
pub(crate) fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Whitespace that does not end a line.
pub(crate) fn is_blank(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | 0x0b | 0x0c)
}

/// Line break bytes (`\r` and `\n`).
pub(crate) fn is_eol(b: u8) -> bool {
    matches!(b, b'\n' | b'\r')
}

/// Bytes that end a movetext word: white space, the comment and variation
/// delimiters, `$` (start of a NAG) and the Latin-1 no-break space (the
/// UTF-8 one is `C2 A0`, see [`find_word_end`]).
pub(crate) fn is_word_delim(b: u8) -> bool {
    is_space(b) || matches!(b, b'{' | b'}' | b'(' | b')' | b';' | b'$' | 0xa0)
}

/// Finds the end of the word that starts at `word_start`, searching from
/// `from` (`word_start < from <= data.len()`; earlier bytes are known to hold
/// no delimiter). `None` when the word may continue in data not yet read.
///
/// A `C2 A0` pair (UTF-8 no-break space) counts as one delimiter. An `A0`
/// byte that ends any other UTF-8 character (`C3 A0` is `a` with a grave
/// accent) is part of the word; a lone `A0` is the Latin-1 no-break space.
pub(crate) fn find_word_end(data: &[u8], word_start: usize, from: usize) -> Option<usize> {
    let mut from = from;
    loop {
        let tail = data.get(from..)?;
        let i = from + tail.iter().position(|&b| is_word_delim(b))?;
        if data[i] != 0xa0 {
            return Some(i);
        }
        // The multi-byte UTF-8 character that ends with this `A0`, if any.
        let start = (2..=4usize)
            .filter_map(|n| i.checked_sub(n - 1))
            .find(|&s| s >= word_start && std::str::from_utf8(&data[s..=i]).is_ok());
        match start {
            Some(s) if data[s] == 0xc2 && s - word_start >= 1 => return Some(s),
            Some(s) if data[s] == 0xc2 => return Some(i),
            Some(_) => from = i + 1,
            None => return Some(i),
        }
    }
}

/// Length of the no-break space (or other white space) at the start of
/// `data`, or 0.
pub(crate) fn space_len(data: &[u8]) -> usize {
    match data {
        [b, ..] if is_space(*b) || *b == 0xa0 => 1,
        [0xc2, 0xa0, ..] => 2,
        _ => 0,
    }
}

/// Index of the first byte of `data` that is not white space (NBSP included,
/// see [`space_len`]), or `None` when there is none. `data` must start at a
/// character boundary.
pub(crate) fn first_non_space(data: &[u8]) -> Option<usize> {
    let mut i = 0;
    loop {
        let rest = data.get(i..)?;
        if rest.is_empty() {
            return None;
        }
        match space_len(rest) {
            0 => return Some(i),
            n => i += n,
        }
    }
}

/// Length of the blank (not a line break) at the start of `data` for the
/// purpose of finding the first token of a line: ASCII blanks and the
/// no-break space, in Latin-1 (`A0`) or UTF-8 (`C2 A0`) form. 0 if none.
pub(crate) fn blank_len(data: &[u8]) -> usize {
    match data {
        [b, ..] if is_blank(*b) || *b == 0xa0 => 1,
        [0xc2, 0xa0, ..] => 2,
        _ => 0,
    }
}

/// Recognises a whole word as a game result: `1-0`, `0-1`, `1/2-1/2`, `1/2`,
/// `*`, `1:0`, `0:1`, `1/2:1/2`, and `½-½` in UTF-8 or Latin-1.
pub(crate) fn result_token(word: &[u8]) -> Option<GameResult> {
    match word {
        b"1-0" | b"1:0" => Some(GameResult::White),
        b"0-1" | b"0:1" => Some(GameResult::Black),
        b"1/2-1/2" | b"1/2" | b"1/2:1/2" | b"\xc2\xbd-\xc2\xbd" | b"\xbd-\xbd" => {
            Some(GameResult::Draw)
        }
        b"*" => Some(GameResult::Unknown),
        _ => None,
    }
}

/// Counts line breaks (`\n`, `\r\n` and a lone `\r` each count once).
///
/// `prev_cr` carries the "previous byte was `\r`" state across calls so that
/// a `\r\n` split between two slices counts once.
pub(crate) fn count_breaks(bytes: &[u8], prev_cr: &mut bool) -> u64 {
    if bytes.is_empty() {
        return 0;
    }
    if !*prev_cr && !bytes.contains(&b'\r') {
        // Fast path (auto-vectorised): LF-only data. No `bytecount` crate for
        // a single call site.
        #[allow(clippy::naive_bytecount)]
        let n = bytes.iter().filter(|&&b| b == b'\n').count() as u64;
        return n;
    }
    let mut n = 0;
    for &b in bytes {
        match b {
            b'\n' => {
                if !*prev_cr {
                    n += 1;
                }
                *prev_cr = false;
            }
            b'\r' => {
                n += 1;
                *prev_cr = true;
            }
            _ => *prev_cr = false,
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(b"1-0", Some(GameResult::White))]
    #[case(b"0-1", Some(GameResult::Black))]
    #[case(b"1/2-1/2", Some(GameResult::Draw))]
    #[case(b"1/2", Some(GameResult::Draw))]
    #[case(b"*", Some(GameResult::Unknown))]
    #[case(b"1:0", Some(GameResult::White))]
    #[case(b"0:1", Some(GameResult::Black))]
    #[case(b"1/2:1/2", Some(GameResult::Draw))]
    #[case("½-½".as_bytes(), Some(GameResult::Draw))]
    #[case(b"\xbd-\xbd", Some(GameResult::Draw))]
    #[case(b"1-0x", None)]
    #[case(b"0-0", None)]
    #[case(b"e4", None)]
    fn recognises_results(#[case] word: &[u8], #[case] expected: Option<GameResult>) {
        assert_eq!(result_token(word), expected);
    }

    #[test]
    fn counts_each_break_style_once() {
        for (text, n) in [
            (&b"a\nb\nc"[..], 2),
            (b"a\r\nb\r\nc", 2),
            (b"a\rb\rc", 2),
            (b"a\r\n\r\nb", 2),
            (b"", 0),
        ] {
            let mut cr = false;
            assert_eq!(count_breaks(text, &mut cr), n, "{text:?}");
        }
    }

    #[test]
    fn crlf_split_across_slices_counts_once() {
        let mut cr = false;
        let total = count_breaks(b"a\r", &mut cr) + count_breaks(b"\nb", &mut cr);
        assert_eq!(total, 1);
    }

    #[test]
    fn word_end_handles_utf8_nbsp() {
        let data = b"e4\xc2\xa0e5";
        assert_eq!(find_word_end(data, 0, 1), Some(2));
        assert_eq!(find_word_end(b"e4 e5", 0, 1), Some(2));
        assert_eq!(find_word_end(b"e4", 0, 1), None);
        assert_eq!(space_len(b"\xc2\xa0x"), 2);
        assert_eq!(space_len(b"\xa0x"), 1);
        assert_eq!(space_len(b"x"), 0);
    }

    #[test]
    fn leading_space_includes_nbsp_but_not_a_grave() {
        assert_eq!(first_non_space(b"\xc2\xa0 \xa0\n[x"), Some(5));
        assert_eq!(first_non_space("\u{e0}".as_bytes()), Some(0));
        assert_eq!(first_non_space(b" \xc2\xa0"), None);
        assert_eq!(first_non_space(b""), None);
        assert_eq!(blank_len(b"\xc2\xa0["), 2);
        assert_eq!(blank_len(b"\xa0["), 1);
        assert_eq!(blank_len(b"\n"), 0);
    }
}
