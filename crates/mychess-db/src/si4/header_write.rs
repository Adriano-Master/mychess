// Derived from Scid vs PC 4.27, src/index.cpp, src/index.h (Index header).
// Copyright (c) 1999-2002 Shane Hudson. All rights reserved (notice of
// index.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Writing the 182-byte `.si4` header (`docs/formatos/si4.md`, section 2;
//! `docs/specs/m3-2-escrita-si4.md`, rules 20 to 24).
//!
//! The writer keeps the raw bytes of the header and patches only the fields
//! it changes, so everything else in the header is written back unchanged.

use super::error::{Result, Si4Error};
use super::header::{
    CUSTOM_FLAG_COUNT, CUSTOM_FLAG_SLOT, CUSTOM_FLAG_TEXT, CUSTOM_FLAGS_OFFSET, DESCRIPTION_LEN,
    DESCRIPTION_OFFSET, HEADER_LEN, Header, MAGIC, MAX_GAMES, SUPPORTED_VERSION,
};

/// The longest description, in bytes (`SCID_DESC_LENGTH`).
pub const MAX_DESCRIPTION_LEN: usize = DESCRIPTION_LEN - 1;

/// The raw 182 bytes of a `.si4` header with setters for the fields that
/// can be edited. [`HeaderBytes::new`] gives the header of an empty database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderBytes {
    bytes: [u8; HEADER_LEN],
}

impl HeaderBytes {
    /// The header of a new empty database: magic, version 400, the given
    /// type, 0 games, autoload "first game" (raw 2), empty description and
    /// empty custom flag descriptions (`Index::Init`).
    pub fn new(base_type: u32) -> HeaderBytes {
        let mut header = HeaderBytes {
            bytes: [0; HEADER_LEN],
        };
        header.put(0, MAGIC);
        header.put(8, &SUPPORTED_VERSION.to_be_bytes());
        header.set_base_type(base_type);
        header.put_u24(17, 2);
        header
    }

    /// Wraps the first 182 bytes of a `.si4` file, after checking them with
    /// [`Header::parse`] (magic and version).
    pub fn from_bytes(data: &[u8]) -> Result<HeaderBytes> {
        Header::parse(data)?;
        let mut header = HeaderBytes {
            bytes: [0; HEADER_LEN],
        };
        if let Some(raw) = data.get(..HEADER_LEN) {
            header.bytes.copy_from_slice(raw);
        }
        Ok(header)
    }

    /// The 182 bytes to write.
    pub fn as_bytes(&self) -> &[u8; HEADER_LEN] {
        &self.bytes
    }

    /// The decoded header.
    pub fn parse(&self) -> Result<Header> {
        Header::parse(&self.bytes)
    }

    /// Sets the database type (`baseType`), kept as given even if it is 32 or
    /// more.
    pub fn set_base_type(&mut self, base_type: u32) {
        self.put(10, &base_type.to_be_bytes());
    }

    /// Sets the number of games (3 bytes; truncated to 24 bits like Scid).
    pub fn set_num_games(&mut self, count: u32) {
        self.put_u24(14, count);
    }

    /// Sets the game to load when the database is opened: `0` is none, `1`
    /// the first game, up to `MAX_GAMES` (the last one). The raw field is
    /// `game + 1` (`SetAutoLoad`). Validation against the number of games is
    /// up to the caller (decision D283).
    pub fn set_auto_load(&mut self, game: u32) -> Result<()> {
        if game > MAX_GAMES {
            return Err(Si4Error::InvalidAutoLoad {
                value: game,
                limit: MAX_GAMES,
            });
        }
        self.put_u24(17, game + 1);
        Ok(())
    }

    /// Sets the description (`Index::SetDescription`): at most 107 bytes,
    /// ending at the first NUL or newline (the newline is dropped). The
    /// 108-byte field is cleared first.
    pub fn set_description(&mut self, text: &str) {
        let mut field = [0u8; DESCRIPTION_LEN];
        for (slot, &byte) in field
            .iter_mut()
            .zip(text.as_bytes().iter().take(MAX_DESCRIPTION_LEN))
        {
            if byte == 0 || byte == b'\n' {
                break;
            }
            *slot = byte;
        }
        self.put(DESCRIPTION_OFFSET, &field);
    }

    /// Sets the description of custom flag `number` (1 to 6): at most 8 bytes,
    /// cut at a character boundary (decision D282), zero padded, with the
    /// 9th byte 0 (`SetCustomFlagDesc`).
    pub fn set_custom_flag_description(&mut self, number: u8, text: &str) -> Result<()> {
        let index = usize::from(number)
            .checked_sub(1)
            .filter(|&i| i < CUSTOM_FLAG_COUNT)
            .ok_or(Si4Error::InvalidCustomFlag(number))?;
        let mut end = text.len().min(CUSTOM_FLAG_TEXT);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut field = [0u8; CUSTOM_FLAG_SLOT];
        for (slot, &byte) in field
            .iter_mut()
            .zip(text.as_bytes().iter().take(end).take_while(|&&b| b != 0))
        {
            *slot = byte;
        }
        self.put(CUSTOM_FLAGS_OFFSET + index * CUSTOM_FLAG_SLOT, &field);
        Ok(())
    }

    fn put(&mut self, offset: usize, data: &[u8]) {
        let end = offset + data.len();
        if let Some(dst) = self.bytes.get_mut(offset..end) {
            dst.copy_from_slice(data);
        }
    }

    fn put_u24(&mut self, offset: usize, value: u32) {
        let bytes = value.to_be_bytes();
        self.put(offset, bytes.get(1..).unwrap_or(&[]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_header_is_the_scid_one() {
        let h = HeaderBytes::new(1);
        let b = h.as_bytes();
        assert_eq!(&b[..8], b"Scid.si\0");
        assert_eq!(&b[8..10], &[0x01, 0x90]);
        assert_eq!(&b[10..14], &[0, 0, 0, 1]);
        assert_eq!(&b[14..17], &[0, 0, 0]);
        assert_eq!(&b[17..20], &[0, 0, 2]);
        assert!(b[20..].iter().all(|&x| x == 0));
        let parsed = h.parse().unwrap();
        assert_eq!((parsed.base_type(), parsed.num_games()), (1, 0));
    }

    #[test]
    fn description_stops_at_newline_and_at_107_bytes() {
        let mut h = HeaderBytes::new(0);
        h.set_description("first\nsecond");
        assert_eq!(h.parse().unwrap().description(), b"first");
        h.set_description(&"x".repeat(200));
        let b = h.as_bytes();
        assert_eq!(b[DESCRIPTION_OFFSET + 106], b'x');
        assert_eq!(b[DESCRIPTION_OFFSET + 107], 0);
        assert_eq!(h.parse().unwrap().description().len(), 107);
    }

    #[test]
    fn custom_flag_description_is_cut_at_a_character_boundary() {
        let mut h = HeaderBytes::new(0);
        // 7 ASCII bytes plus a 2-byte letter: the letter does not fit.
        h.set_custom_flag_description(2, "abcdefgé").unwrap();
        let parsed = h.parse().unwrap();
        assert_eq!(parsed.custom_flag_descriptions()[1], b"abcdefg");
        assert_eq!(parsed.custom_flag_descriptions()[0], b"");
        assert!(matches!(
            h.set_custom_flag_description(7, "x"),
            Err(Si4Error::InvalidCustomFlag(7))
        ));
        assert!(matches!(
            h.set_custom_flag_description(0, "x"),
            Err(Si4Error::InvalidCustomFlag(0))
        ));
    }

    #[test]
    fn auto_load_is_stored_plus_one() {
        let mut h = HeaderBytes::new(0);
        h.set_auto_load(0).unwrap();
        assert_eq!(&h.as_bytes()[17..20], &[0, 0, 1]);
        h.set_auto_load(MAX_GAMES).unwrap();
        assert_eq!(&h.as_bytes()[17..20], &[0xFF, 0xFF, 0xFF]);
        assert!(h.set_auto_load(MAX_GAMES + 1).is_err());
    }

    #[test]
    fn other_header_bytes_are_preserved() {
        let mut raw = HeaderBytes::new(7).as_bytes().to_vec();
        raw[100] = 0xAA; // garbage after the description's NUL
        let mut h = HeaderBytes::from_bytes(&raw).unwrap();
        h.set_num_games(5);
        h.set_base_type(9);
        assert_eq!(h.as_bytes()[100], 0xAA);
        assert_eq!(h.parse().unwrap().num_games(), 5);
        assert!(HeaderBytes::from_bytes(&raw[..100]).is_err());
    }
}
