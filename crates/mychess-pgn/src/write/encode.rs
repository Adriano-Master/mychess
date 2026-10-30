//! Output encoding (spec D36).

use super::options::Encoding;

/// The UTF-8 byte order mark.
pub(crate) const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Appends `text` to `out` in the given encoding. The BOM is **not** added
/// here: it belongs to the start of the stream.
///
/// Latin-1 is ISO 8859-1 proper (not `windows-1252`, which is what
/// `encoding_rs` calls "latin1"): every character above U+00FF becomes one
/// `?`, as in Scid's converter, so `€` is `?` instead of byte `0x80`.
pub(crate) fn encode_into(text: &str, encoding: Encoding, out: &mut Vec<u8>) {
    match encoding {
        Encoding::Latin1 => {
            out.extend(
                text.chars()
                    .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')),
            );
        }
        // `Utf8Bom` differs from `Utf8` only by the stream prefix; unknown
        // future variants are written as UTF-8, the safe default.
        _ => out.extend_from_slice(text.as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn enc(text: &str, e: Encoding) -> Vec<u8> {
        let mut v = Vec::new();
        encode_into(text, e, &mut v);
        v
    }

    #[rstest]
    #[case("abc", Encoding::Utf8, b"abc".to_vec())]
    #[case("é", Encoding::Utf8, vec![0xC3, 0xA9])]
    #[case("é", Encoding::Utf8Bom, vec![0xC3, 0xA9])]
    #[case("é£ÿ", Encoding::Latin1, vec![0xE9, 0xA3, 0xFF])]
    #[case("€", Encoding::Latin1, b"?".to_vec())]
    #[case("a♟b", Encoding::Latin1, b"a?b".to_vec())]
    #[case("\u{1F600}x", Encoding::Latin1, b"?x".to_vec())]
    fn encodes(#[case] text: &str, #[case] e: Encoding, #[case] expected: Vec<u8>) {
        assert_eq!(enc(text, e), expected);
    }
}
