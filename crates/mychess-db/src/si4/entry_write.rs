// Derived from Scid vs PC 4.27, src/index.cpp, src/index.h (IndexEntry::Write,
// CharToFlag, SetFlagStr).
// Copyright (c) 1999-2002 Shane Hudson. All rights reserved (notice of
// index.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Encoding of the 47-byte index entry (`docs/formatos/si4.md`, section 3) and
//! the letters of the user flags (section 5).

use mychess_core::{GameResult, PartialDate};

use super::derive::{encode_count, event_date_field, rating_field};
use super::entry::{GameFlags, IndexEntry};
use super::error::{Result, Si4Error};
use super::header::ENTRY_LEN;

/// The letters of the 19 flags a user can edit, in Scid's order.
pub const FLAG_LETTERS: &str = "DWBMENPTKQ!?U123456";

impl GameFlags {
    /// The 19 flags a user can set: everything except `START`, `PROMO` and
    /// `UPROMO`, which are derived from the game.
    pub const EDITABLE: GameFlags = GameFlags::from_bits_retain(0x003F_FFF8);

    /// The flags `START`, `PROMO` and `UPROMO`, set when the entry is built.
    pub const DERIVED: GameFlags = GameFlags::from_bits_retain(0x0000_0007);

    /// The flag of a letter of `DWBMENPTKQ!?U123456` (case does not matter),
    /// or `None` for any other character.
    ///
    /// Scid maps an unknown letter to `START` by accident; the MyChess
    /// refuses it (`docs/specs/m3-2-escrita-si4.md`, rule 18).
    pub fn from_letter(letter: char) -> Option<GameFlags> {
        let upper = letter.to_ascii_uppercase();
        let index = FLAG_LETTERS.chars().position(|c| c == upper)?;
        // The letters are in bit order starting at bit 3.
        GameFlags::from_bits(1u32 << (index + 3))
    }

    /// The flags named by a string of letters, as in `SetFlagStr`.
    pub fn from_letters(letters: &str) -> Result<GameFlags> {
        letters.chars().try_fold(GameFlags::empty(), |acc, c| {
            GameFlags::from_letter(c)
                .map(|f| acc | f)
                .ok_or(Si4Error::UnknownFlag(c))
        })
    }

    /// The letters of the editable flags that are set, in Scid's order
    /// (`GetFlagStr`).
    pub fn letters(self) -> String {
        FLAG_LETTERS
            .chars()
            .enumerate()
            .filter(|&(i, _)| self.bits() & (1u32 << (i + 3)) != 0)
            .map(|(_, c)| c)
            .collect()
    }
}

impl Default for IndexEntry {
    /// An entry as `IndexEntry::Init` leaves it: everything zero, no
    /// result, no dates.
    fn default() -> IndexEntry {
        IndexEntry {
            offset: 0,
            length: 0,
            flags: GameFlags::empty(),
            length_spare_bit: false,
            white_id: 0,
            black_id: 0,
            event_id: 0,
            site_id: 0,
            round_id: 0,
            variation_count: 0,
            comment_count: 0,
            nag_count: 0,
            result: GameResult::Unknown,
            eco_code: 0,
            date: PartialDate::UNKNOWN,
            event_date: PartialDate::UNKNOWN,
            white_elo: 0,
            white_rating_type: 0,
            black_elo: 0,
            black_rating_type: 0,
            final_mat_sig: 0,
            stored_line_code: 0,
            num_half_moves: 0,
            home_pawn_data: [0; 9],
        }
    }
}

impl IndexEntry {
    /// Encodes the entry in its 47 bytes (`IndexEntry::Write`), the inverse
    /// of [`IndexEntry::parse`].
    ///
    /// Counts are written with `encode_count` (the fields hold the decoded
    /// scale, so a parsed entry gives back its own code), the Elo is limited
    /// to 4000, and the number of half moves wraps around at 1024
    /// (decision D278). IDs are masked to their width.
    pub fn to_bytes(&self) -> [u8; ENTRY_LEN] {
        let mut out = [0u8; ENTRY_LEN];
        let mut put = |offset: usize, data: &[u8]| {
            if let Some(dst) = out.get_mut(offset..offset + data.len()) {
                dst.copy_from_slice(data);
            }
        };
        let flags = self.flags.bits();
        let length = self.length & 0x1_FFFF;
        // Bit 7: bit 16 of the length; bit 6: spare; bits 0-5: custom flags.
        let length_high = (u8::from(length > 0xFFFF) << 7)
            | (u8::from(self.length_spare_bit) << 6)
            | u8::try_from((flags >> 16) & 0x3F).unwrap_or(0);
        let low16 = |v: u32| u16::try_from(v & 0xFFFF).unwrap_or(0).to_be_bytes();
        let high =
            |v: u32, shift: u32, mask: u32| u8::try_from((v >> 16) & mask).unwrap_or(0) << shift;

        put(0, &self.offset.to_be_bytes());
        put(4, &low16(length));
        put(6, &[length_high]);
        put(7, &low16(flags));
        put(
            9,
            &[high(self.white_id, 4, 0xF) | high(self.black_id, 0, 0xF)],
        );
        put(10, &low16(self.white_id));
        put(12, &low16(self.black_id));
        put(
            14,
            &[high(self.event_id, 5, 7) | high(self.site_id, 2, 7) | high(self.round_id, 0, 3)],
        );
        put(15, &low16(self.event_id));
        put(17, &low16(self.site_id));
        put(19, &low16(self.round_id));

        let result = match self.result {
            GameResult::White => 1u16,
            GameResult::Black => 2,
            GameResult::Draw => 3,
            _ => 0,
        };
        let counts = u16::from(encode_count(u32::from(self.variation_count)))
            | (u16::from(encode_count(u32::from(self.comment_count))) << 4)
            | (u16::from(encode_count(u32::from(self.nag_count))) << 8)
            | (result << 12);
        put(21, &counts.to_be_bytes());
        put(23, &self.eco_code.to_be_bytes());
        let dates = (self.date.to_scid_u32() & 0x000F_FFFF)
            | (event_date_field(self.date, self.event_date) << 20);
        put(25, &dates.to_be_bytes());
        put(
            29,
            &rating_field(self.white_elo, self.white_rating_type).to_be_bytes(),
        );
        put(
            31,
            &rating_field(self.black_elo, self.black_rating_type).to_be_bytes(),
        );
        let mat = (self.final_mat_sig & 0x00FF_FFFF) | (u32::from(self.stored_line_code) << 24);
        put(33, &mat.to_be_bytes());
        let half_moves = self.num_half_moves & 0x3FF;
        put(37, &[u8::try_from(half_moves & 0xFF).unwrap_or(0)]);
        let mut hp = self.home_pawn_data;
        if let Some(first) = hp.first_mut() {
            *first = (*first & 0x3F) | (u8::try_from(half_moves >> 8).unwrap_or(0) << 6);
        }
        put(38, &hp);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_letters_map_to_bits() {
        assert_eq!(GameFlags::from_letter('D'), Some(GameFlags::DELETE));
        assert_eq!(GameFlags::from_letter('d'), Some(GameFlags::DELETE));
        assert_eq!(GameFlags::from_letter('U'), Some(GameFlags::USER));
        assert_eq!(GameFlags::from_letter('!'), Some(GameFlags::BRILLIANCY));
        assert_eq!(GameFlags::from_letter('?'), Some(GameFlags::BLUNDER));
        assert_eq!(GameFlags::from_letter('1'), Some(GameFlags::CUSTOM1));
        assert_eq!(GameFlags::from_letter('6'), Some(GameFlags::CUSTOM6));
        assert_eq!(GameFlags::from_letter('X'), None);
        assert_eq!(GameFlags::from_letter('7'), None);
        assert_eq!(FLAG_LETTERS.len(), 19);
        assert_eq!(GameFlags::EDITABLE.bits().count_ones(), 19);
        assert!(matches!(
            GameFlags::from_letters("DWX"),
            Err(Si4Error::UnknownFlag('X'))
        ));
        let all = GameFlags::from_letters(FLAG_LETTERS).unwrap();
        assert_eq!(all, GameFlags::EDITABLE);
        assert_eq!(all.letters(), FLAG_LETTERS);
        assert_eq!((GameFlags::DELETE | GameFlags::START).letters(), "D");
    }

    #[test]
    fn a_parsed_entry_encodes_to_the_same_bytes() {
        let mut raw = [0u8; ENTRY_LEN];
        raw[0..4].copy_from_slice(&0x0102_0304u32.to_be_bytes());
        raw[4..6].copy_from_slice(&0x1234u16.to_be_bytes());
        raw[6] = 0x80 | 0x40 | 0x05;
        raw[7..9].copy_from_slice(&0x8009u16.to_be_bytes());
        raw[9] = 0xAB;
        raw[10..12].copy_from_slice(&0x1111u16.to_be_bytes());
        raw[12..14].copy_from_slice(&0x2222u16.to_be_bytes());
        raw[14] = (0b101 << 5) | (0b011 << 2) | 0b10;
        raw[15..17].copy_from_slice(&0x3333u16.to_be_bytes());
        raw[17..19].copy_from_slice(&0x4444u16.to_be_bytes());
        raw[19..21].copy_from_slice(&0x5555u16.to_be_bytes());
        raw[21..23].copy_from_slice(&((3u16 << 12) | (12 << 8) | (11 << 4) | 2).to_be_bytes());
        raw[23..25].copy_from_slice(&132u16.to_be_bytes());
        let game = (2001u32 << 9) | (3 << 5) | 15;
        let event = (5u32 << 9) | (2 << 5) | 10;
        raw[25..29].copy_from_slice(&((event << 20) | game).to_be_bytes());
        raw[29..31].copy_from_slice(&((3u16 << 12) | 2750).to_be_bytes());
        raw[31..33].copy_from_slice(&((5u16 << 12) | 3999).to_be_bytes());
        raw[33..37].copy_from_slice(&0xAB12_3456u32.to_be_bytes());
        raw[37] = 0xFF;
        raw[38] = 0xC0 | 0x05;
        raw[39..47].copy_from_slice(&[0x12, 0x34, 0x56, 0, 0, 0, 0, 0]);
        let entry = IndexEntry::parse(&raw).unwrap();
        assert_eq!(entry.to_bytes(), raw);
    }

    #[test]
    fn half_moves_wrap_around_at_1024() {
        for (value, byte37, byte38_high) in [(255u16, 255u8, 0u8), (256, 0, 1), (1023, 255, 3)] {
            let e = IndexEntry {
                num_half_moves: value,
                ..IndexEntry::default()
            };
            let b = e.to_bytes();
            assert_eq!((b[37], b[38] >> 6), (byte37, byte38_high), "{value}");
        }
        // 1024 and 1300 are given modulo 1024 (the entry keeps 10 bits).
        let e = IndexEntry {
            num_half_moves: 1300,
            ..IndexEntry::default()
        };
        let b = e.to_bytes();
        assert_eq!(u16::from(b[37]) | (u16::from(b[38] >> 6) << 8), 276);
    }

    #[test]
    fn default_entry_is_all_zero_and_ids_are_split() {
        assert_eq!(IndexEntry::default().to_bytes(), [0u8; ENTRY_LEN]);
        let e = IndexEntry {
            white_id: 0xF_FFFF,
            black_id: 0x1_0000,
            event_id: 0x7_FFFF,
            site_id: 0x4_0000,
            round_id: 0x3_FFFF,
            ..IndexEntry::default()
        };
        let b = e.to_bytes();
        assert_eq!(b[9], 0xF1);
        assert_eq!(b[14], (7 << 5) | (4 << 2) | 3);
        let back = IndexEntry::parse(&b).unwrap();
        assert_eq!(
            (
                back.white_id,
                back.black_id,
                back.event_id,
                back.site_id,
                back.round_id
            ),
            (0xF_FFFF, 0x1_0000, 0x7_FFFF, 0x4_0000, 0x3_FFFF)
        );
    }

    #[test]
    fn length_uses_17_bits_and_preserves_the_spare_bit() {
        let e = IndexEntry {
            length: 131_071,
            length_spare_bit: true,
            flags: GameFlags::CUSTOM1 | GameFlags::CUSTOM6 | GameFlags::DELETE,
            ..IndexEntry::default()
        };
        let b = e.to_bytes();
        assert_eq!(&b[4..6], &[0xFF, 0xFF]);
        assert_eq!(b[6], 0x80 | 0x40 | 0x01 | 0x20);
        let back = IndexEntry::parse(&b).unwrap();
        assert_eq!(back.length, 131_071);
        assert!(back.length_spare_bit);
        assert_eq!(back.flags, e.flags);
    }
}
