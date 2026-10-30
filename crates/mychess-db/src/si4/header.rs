// Derived from Scid vs PC 4.27, src/index.cpp (Index header).
// Copyright (c) 1999-2002 Shane Hudson. All rights reserved (notice of
// index.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The 182-byte header of the `.si4` file (`docs/formatos/si4.md`, sections 2
//! and 6).

use super::bytes::{read_u16_be, read_u24_be, read_u32_be};
use super::error::{Result, Si4Error, Si4File};

/// Size of the `.si4` header in bytes.
pub const HEADER_LEN: usize = 182;
/// Size of one index entry in bytes.
pub const ENTRY_LEN: usize = 47;
/// The only supported format version.
pub const SUPPORTED_VERSION: u16 = 400;

/// The largest number of games in a database (`MAX_GAMES`, 3-byte counter
/// whose value 2^24 - 1 is reserved by the autoload field).
pub const MAX_GAMES: u32 = 16_777_214;

pub(crate) const MAGIC: &[u8; 8] = b"Scid.si\0";
pub(crate) const DESCRIPTION_OFFSET: usize = 20;
pub(crate) const DESCRIPTION_LEN: usize = 108;
pub(crate) const CUSTOM_FLAGS_OFFSET: usize = 128;
pub(crate) const CUSTOM_FLAG_SLOT: usize = 9;
pub(crate) const CUSTOM_FLAG_TEXT: usize = 8;
pub(crate) const CUSTOM_FLAG_COUNT: usize = 6;

/// Names of the database types 0..=31 (`docs/formatos/si4.md`, section 6).
const BASE_TYPE_NAMES: [&str; 32] = [
    "Unknown",
    "Temporary database",
    "Clipbase",
    "PGN file",
    "My games",
    "Large database",
    "Correspondence chess",
    "Computer Chess",
    "Index of games",
    "Player collection",
    "Tournament",
    "Internet",
    "GM games",
    "IM games",
    "Blitz games",
    "Tactics",
    "End games",
    "Openings for White",
    "Openings for Black",
    "Openings for either color",
    "Theory: 1.c4",
    "Theory: 1.d4",
    "Theory: 1.d4 d5",
    "Theory: QGD",
    "Theory: Slav",
    "Theory: 1.d4 Nf6",
    "Theory: 1.e4",
    "Theory: Sicilian",
    "Theory: Sicilian 1.e4 c5 2.Nd3 d6 3.d4",
    "Theory: Caro-Kann",
    "Theory: French",
    "Theory: Open Games",
];

/// The decoded `.si4` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    base_type: u32,
    num_games: u32,
    auto_load_raw: u32,
    description: Vec<u8>,
    custom_flag_descriptions: [Vec<u8>; CUSTOM_FLAG_COUNT],
}

impl Header {
    /// Parses the first 182 bytes of a `.si4` file.
    ///
    /// Errors: fewer than 182 bytes is [`Si4Error::Truncated`]; a wrong magic
    /// is [`Si4Error::BadMagic`]; a version below 400 is
    /// [`Si4Error::OldVersion`] and above 400 is [`Si4Error::NewerVersion`].
    /// The size of the file against `num_games` is checked by the caller.
    pub fn parse(data: &[u8]) -> Result<Header> {
        let truncated = Si4Error::Truncated {
            file: Si4File::Index,
            offset: 0,
            need: HEADER_LEN as u64,
        };
        let Some(raw) = data.get(..HEADER_LEN) else {
            return Err(truncated);
        };
        if raw.get(..MAGIC.len()) != Some(&MAGIC[..]) {
            return Err(Si4Error::BadMagic {
                file: Si4File::Index,
            });
        }
        let version = read_u16_be(raw, 8).ok_or_else(truncated_header)?;
        match version.cmp(&SUPPORTED_VERSION) {
            std::cmp::Ordering::Less => return Err(Si4Error::OldVersion { found: version }),
            std::cmp::Ordering::Greater => return Err(Si4Error::NewerVersion { found: version }),
            std::cmp::Ordering::Equal => {}
        }
        let base_type = read_u32_be(raw, 10).ok_or_else(truncated_header)?;
        let num_games = read_u24_be(raw, 14).ok_or_else(truncated_header)?;
        let auto_load_raw = read_u24_be(raw, 17).ok_or_else(truncated_header)?;
        let description = raw
            .get(DESCRIPTION_OFFSET..DESCRIPTION_OFFSET + DESCRIPTION_LEN)
            .map(until_nul)
            .ok_or_else(truncated_header)?;
        let mut custom_flag_descriptions: [Vec<u8>; CUSTOM_FLAG_COUNT] = Default::default();
        for (i, slot) in custom_flag_descriptions.iter_mut().enumerate() {
            let start = CUSTOM_FLAGS_OFFSET + i * CUSTOM_FLAG_SLOT;
            let text = raw
                .get(start..start + CUSTOM_FLAG_TEXT)
                .ok_or_else(truncated_header)?;
            *slot = until_nul(text);
        }
        Ok(Header {
            base_type,
            num_games,
            auto_load_raw,
            description,
            custom_flag_descriptions,
        })
    }

    /// The raw database type (`baseType`), preserved even when `>= 32`.
    pub fn base_type(&self) -> u32 {
        self.base_type
    }

    /// The English name of the database type, or `None` for values `>= 32`.
    pub fn base_type_name(&self) -> Option<&'static str> {
        usize::try_from(self.base_type)
            .ok()
            .and_then(|i| BASE_TYPE_NAMES.get(i))
            .copied()
    }

    /// The number of games declared by the header.
    pub fn num_games(&self) -> u32 {
        self.num_games
    }

    /// The raw `autoLoad` value, exactly as stored.
    pub fn auto_load_raw(&self) -> u32 {
        self.auto_load_raw
    }

    /// The game to load when the database is opened (base 1), or `None` for
    /// "no game". Raw 0 and 2 mean game 1, raw 1 means none and raw `k >= 2`
    /// means game `k - 1`; the result is limited to the number of games, and
    /// an empty database has none (`docs/formatos/si4.md`, section 2).
    pub fn auto_load(&self) -> Option<u32> {
        if self.num_games == 0 {
            return None;
        }
        let game = match self.auto_load_raw {
            0 => 1,
            1 => return None,
            k => k - 1,
        };
        Some(game.min(self.num_games))
    }

    /// The description text, as raw bytes up to the first NUL.
    pub fn description(&self) -> &[u8] {
        &self.description
    }

    /// The descriptions of the six custom flags, as raw bytes (the first byte
    /// is the letter shown in the game list).
    pub fn custom_flag_descriptions(&self) -> &[Vec<u8>; 6] {
        &self.custom_flag_descriptions
    }
}

fn truncated_header() -> Si4Error {
    Si4Error::Truncated {
        file: Si4File::Index,
        offset: 0,
        need: HEADER_LEN as u64,
    }
}

fn until_nul(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().take_while(|&b| b != 0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_bytes(version: u16, games: u32, auto: u32) -> Vec<u8> {
        let mut data = vec![0u8; HEADER_LEN];
        data[..8].copy_from_slice(MAGIC);
        data[8..10].copy_from_slice(&version.to_be_bytes());
        data[10..14].copy_from_slice(&5u32.to_be_bytes());
        data[14..17].copy_from_slice(&games.to_be_bytes()[1..]);
        data[17..20].copy_from_slice(&auto.to_be_bytes()[1..]);
        data[20..26].copy_from_slice(b"hello\0");
        data[128..131].copy_from_slice(b"Abc");
        data
    }

    #[test]
    fn parses_a_valid_header() {
        let h = Header::parse(&header_bytes(400, 1234, 2)).unwrap();
        assert_eq!(h.num_games(), 1234);
        assert_eq!(h.base_type(), 5);
        assert_eq!(h.base_type_name(), Some("Large database"));
        assert_eq!(h.description(), b"hello");
        assert_eq!(h.custom_flag_descriptions()[0], b"Abc");
        assert_eq!(h.custom_flag_descriptions()[1], Vec::<u8>::new());
    }

    #[test]
    fn rejects_short_files_wrong_magic_and_versions() {
        let good = header_bytes(400, 1, 2);
        assert!(matches!(
            Header::parse(&good[..181]),
            Err(Si4Error::Truncated { need: 182, .. })
        ));
        let mut bad = good.clone();
        bad[0] = b'X';
        assert!(matches!(
            Header::parse(&bad),
            Err(Si4Error::BadMagic { .. })
        ));
        let mut bad = good.clone();
        bad[7] = 1;
        assert!(matches!(
            Header::parse(&bad),
            Err(Si4Error::BadMagic { .. })
        ));
        assert!(matches!(
            Header::parse(&header_bytes(300, 1, 2)),
            Err(Si4Error::OldVersion { found: 300 })
        ));
        assert!(matches!(
            Header::parse(&header_bytes(401, 1, 2)),
            Err(Si4Error::NewerVersion { found: 401 })
        ));
    }

    #[test]
    fn auto_load_follows_the_scid_rule() {
        let load = |games, raw| {
            Header::parse(&header_bytes(400, games, raw))
                .unwrap()
                .auto_load()
        };
        assert_eq!(load(10, 0), Some(1));
        assert_eq!(load(10, 1), None);
        assert_eq!(load(10, 2), Some(1));
        assert_eq!(load(10, 3), Some(2));
        assert_eq!(load(10, 11), Some(10));
        assert_eq!(load(10, 500), Some(10));
        assert_eq!(load(0, 2), None);
    }

    #[test]
    fn unknown_base_type_is_preserved_without_a_name() {
        let mut data = header_bytes(400, 1, 2);
        data[10..14].copy_from_slice(&99u32.to_be_bytes());
        let h = Header::parse(&data).unwrap();
        assert_eq!(h.base_type(), 99);
        assert_eq!(h.base_type_name(), None);
    }
}
