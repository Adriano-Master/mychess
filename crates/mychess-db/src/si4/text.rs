//! Text conversion at the border (decision D256).
//!
//! Names, tags and comments are raw bytes inside the SI4 files. When they
//! become `String`s the rule is: UTF-8 if the bytes are valid UTF-8, otherwise
//! Windows-1252 (the superset of Latin-1 used by old databases). The writer
//! (slice M3-2) must write new names as UTF-8 and preserve inherited bytes.

use std::borrow::Cow;

/// Converts raw SI4 text bytes to a string: UTF-8 when valid, else
/// Windows-1252.
///
/// ```
/// use mychess_db::si4::bytes_to_string;
///
/// assert_eq!(bytes_to_string("Kasparov".as_bytes()), "Kasparov");
/// assert_eq!(bytes_to_string("Pérez".as_bytes()), "Pérez");
/// assert_eq!(bytes_to_string(b"P\xe9rez"), "Pérez"); // Windows-1252
/// assert_eq!(bytes_to_string(b"\x80"), "\u{20ac}"); // euro sign
/// ```
pub fn bytes_to_string(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => {
            encoding_rs::WINDOWS_1252
                .decode_without_bom_handling(bytes)
                .0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_utf8_is_kept_borrowed() {
        assert!(matches!(
            bytes_to_string("São Paulo".as_bytes()),
            Cow::Borrowed("São Paulo")
        ));
    }

    #[test]
    fn invalid_utf8_falls_back_to_windows_1252() {
        assert_eq!(
            bytes_to_string(&[0x4D, 0xFC, 0x6C, 0x6C, 0x65, 0x72]),
            "Müller"
        );
        assert_eq!(bytes_to_string(&[0xC3]), "\u{c3}");
        assert_eq!(
            bytes_to_string(&[0x80, 0x93, 0x94]),
            "\u{20ac}\u{201c}\u{201d}"
        );
    }

    #[test]
    fn empty_input_gives_empty_string() {
        assert_eq!(bytes_to_string(&[]), "");
    }
}
