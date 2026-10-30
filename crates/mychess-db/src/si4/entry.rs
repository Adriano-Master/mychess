// Derived from Scid vs PC 4.27, src/index.cpp, src/index.h (IndexEntry).
// Copyright (c) 1999-2002 Shane Hudson. All rights reserved (notice of
// index.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The 47-byte index entry (`docs/formatos/si4.md`, sections 3 to 5).

use bitflags::bitflags;
use mychess_core::{Eco, Elo, GameResult, MAX_ELO, PartialDate, Rating, RatingType};

use super::bytes::{read_u16_be, read_u32_be};
use super::error::IndexEntryError;
use super::header::ENTRY_LEN;
use super::names::{NameBase, NameKind};

bitflags! {
    /// The 22 per-game flags (`docs/formatos/si4.md`, section 5).
    ///
    /// Bits 0-15 come from the `Flags` field of the entry; bits 16-21 (the
    /// custom flags) come from bits 0-5 of `Length_High`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct GameFlags: u32 {
        /// The game starts from its own position (`START`).
        const START = 1 << 0;
        /// The game has a promotion (`PROMO`).
        const PROMOTION = 1 << 1;
        /// The game has an under-promotion (`UPROMO`).
        const UNDER_PROMOTION = 1 << 2;
        /// Marked for deletion (`D`).
        const DELETE = 1 << 3;
        /// White opening (`W`).
        const WHITE_OPENING = 1 << 4;
        /// Black opening (`B`).
        const BLACK_OPENING = 1 << 5;
        /// Middlegame (`M`).
        const MIDDLEGAME = 1 << 6;
        /// Endgame (`E`).
        const ENDGAME = 1 << 7;
        /// Novelty (`N`).
        const NOVELTY = 1 << 8;
        /// Pawn structure (`P`).
        const PAWN_STRUCTURE = 1 << 9;
        /// Tactics (`T`).
        const TACTICS = 1 << 10;
        /// Kingside play (`K`).
        const KINGSIDE = 1 << 11;
        /// Queenside play (`Q`).
        const QUEENSIDE = 1 << 12;
        /// Brilliancy (`!`).
        const BRILLIANCY = 1 << 13;
        /// Blunder (`?`).
        const BLUNDER = 1 << 14;
        /// User flag (`U`).
        const USER = 1 << 15;
        /// Custom flag 1.
        const CUSTOM1 = 1 << 16;
        /// Custom flag 2.
        const CUSTOM2 = 1 << 17;
        /// Custom flag 3.
        const CUSTOM3 = 1 << 18;
        /// Custom flag 4.
        const CUSTOM4 = 1 << 19;
        /// Custom flag 5.
        const CUSTOM5 = 1 << 20;
        /// Custom flag 6.
        const CUSTOM6 = 1 << 21;
    }
}

/// The scale of the 4-bit variation, comment and NAG counts
/// (`docs/formatos/si4.md`, section 3).
const COUNT_SCALE: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 15, 20, 30, 40, 50];

/// The material left in the final position of a game, from the 24-bit
/// signature: pawns 0-15, other pieces 0-3 (`docs/formatos/si4.md`, section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MaterialSignature {
    /// White pawns.
    pub white_pawns: u8,
    /// White knights (capped at 3).
    pub white_knights: u8,
    /// White bishops (capped at 3).
    pub white_bishops: u8,
    /// White rooks (capped at 3).
    pub white_rooks: u8,
    /// White queens (capped at 3).
    pub white_queens: u8,
    /// Black pawns.
    pub black_pawns: u8,
    /// Black knights (capped at 3).
    pub black_knights: u8,
    /// Black bishops (capped at 3).
    pub black_bishops: u8,
    /// Black rooks (capped at 3).
    pub black_rooks: u8,
    /// Black queens (capped at 3).
    pub black_queens: u8,
}

impl MaterialSignature {
    /// Unpacks the low 24 bits of `FinalMatSig`.
    pub fn from_bits(sig: u32) -> MaterialSignature {
        // Each field is masked to at most 4 bits, so the narrowing is exact.
        let field = |shift: u32, mask: u32| u8::try_from((sig >> shift) & mask).unwrap_or(0);
        MaterialSignature {
            black_pawns: field(0, 0xF),
            black_knights: field(4, 0x3),
            black_bishops: field(6, 0x3),
            black_rooks: field(8, 0x3),
            black_queens: field(10, 0x3),
            white_pawns: field(12, 0xF),
            white_knights: field(16, 0x3),
            white_bishops: field(18, 0x3),
            white_rooks: field(20, 0x3),
            white_queens: field(22, 0x3),
        }
    }
}

/// One entry of the index: the data of a game that is searched and listed
/// without reading the `.sg4` record.
///
/// Name IDs are indices into the [`NameBase`]; "unknown" values of the
/// original format (0) are `None`/[`PartialDate::UNKNOWN`] here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    /// Offset of the record in the `.sg4` file.
    pub offset: u32,
    /// Length of the record in bytes (17 bits, at most 131,071).
    pub length: u32,
    /// The 22 flags.
    pub flags: GameFlags,
    /// Bit 6 of `Length_High`, unused by Scid and preserved for the writer.
    pub length_spare_bit: bool,
    /// ID of the White player name (20 bits).
    pub white_id: u32,
    /// ID of the Black player name (20 bits).
    pub black_id: u32,
    /// ID of the event name (19 bits).
    pub event_id: u32,
    /// ID of the site name (19 bits).
    pub site_id: u32,
    /// ID of the round name (18 bits).
    pub round_id: u32,
    /// Approximate number of variations (scale 0-10, 15, 20, 30, 40, 50).
    pub variation_count: u8,
    /// Approximate number of comments (same scale).
    pub comment_count: u8,
    /// Approximate number of NAGs (same scale).
    pub nag_count: u8,
    /// The result stored in the index.
    pub result: GameResult,
    /// Raw Scid ECO code (0 = none); see [`IndexEntry::eco`].
    pub eco_code: u16,
    /// Date of the game.
    pub date: PartialDate,
    /// Date of the event (only within 3 years of the game date).
    pub event_date: PartialDate,
    /// White Elo (0 = none), limited to 4000.
    pub white_elo: u16,
    /// Raw rating type of White (0-15); see [`IndexEntry::white_rating`].
    pub white_rating_type: u8,
    /// Black Elo (0 = none), limited to 4000.
    pub black_elo: u16,
    /// Raw rating type of Black (0-15).
    pub black_rating_type: u8,
    /// The 24-bit material signature of the final position, raw.
    pub final_mat_sig: u32,
    /// The stored-line code (high 8 bits of `FinalMatSig`), raw.
    pub stored_line_code: u8,
    /// Number of half moves of the main line (10 bits, at most 1023).
    pub num_half_moves: u16,
    /// The 9 bytes of home-pawn data, raw (the first one masked to 6 bits).
    pub home_pawn_data: [u8; 9],
}

impl IndexEntry {
    /// Decodes an entry from its 47 bytes.
    ///
    /// The only value rejected here is a result above 3. Name IDs are
    /// checked against the name base by [`IndexEntry::verify_names`]. Dates
    /// with an impossible month become partially unknown; an Elo above 4000
    /// is limited to 4000.
    pub fn parse(data: &[u8]) -> Result<IndexEntry, IndexEntryError> {
        let (entry, result_ok) = Self::parse_lenient(data)?;
        if result_ok {
            Ok(entry)
        } else {
            Err(IndexEntryError::new("result out of range"))
        }
    }

    /// Like [`IndexEntry::parse`] but a result above 3 is read as unknown;
    /// the flag tells whether the result was valid. Used when opening a
    /// database, which does not reject games for their result (D262).
    pub(crate) fn parse_lenient(data: &[u8]) -> Result<(IndexEntry, bool), IndexEntryError> {
        const SHORT: &str = "entry shorter than 47 bytes";
        let short = IndexEntryError::new(SHORT);
        let Some(raw) = data.get(..ENTRY_LEN) else {
            return Err(short);
        };
        let u8_at = |i: usize| raw.get(i).copied().ok_or(short);
        let u16_at = |i: usize| read_u16_be(raw, i).ok_or(short);
        let u32_at = |i: usize| read_u32_be(raw, i).ok_or(short);

        let length_high = u8_at(6)?;
        let flags_low = u16_at(7)?;
        let white_black_high = u32::from(u8_at(9)?);
        let event_site_round_high = u32::from(u8_at(14)?);
        let var_counts = u16_at(21)?;
        let dates = u32_at(25)?;
        let white_elo_raw = u16_at(29)?;
        let black_elo_raw = u16_at(31)?;
        let final_mat = u32_at(33)?;
        let byte38 = u8_at(38)?;

        let (result, result_ok) = match var_counts >> 12 {
            0 => (GameResult::Unknown, true),
            1 => (GameResult::White, true),
            2 => (GameResult::Black, true),
            3 => (GameResult::Draw, true),
            _ => (GameResult::Unknown, false),
        };
        let mut home_pawn_data = [0u8; 9];
        for (i, slot) in home_pawn_data.iter_mut().enumerate() {
            *slot = u8_at(38 + i)?;
        }
        if let Some(first) = home_pawn_data.first_mut() {
            *first &= 63;
        }
        let game_date_bits = dates & 0x000F_FFFF;
        let date = date_from_bits(game_date_bits);
        let flags = GameFlags::from_bits_retain(
            u32::from(flags_low) | (u32::from(length_high & 0x3F) << 16),
        );
        let count = |shift: u32| COUNT_SCALE[usize::from((var_counts >> shift) & 15)];

        let entry = IndexEntry {
            offset: u32_at(0)?,
            length: u32::from(u16_at(4)?) + (u32::from(length_high & 0x80) << 9),
            flags,
            length_spare_bit: length_high & 0x40 != 0,
            white_id: (white_black_high >> 4) << 16 | u32::from(u16_at(10)?),
            black_id: (white_black_high & 0xF) << 16 | u32::from(u16_at(12)?),
            event_id: (event_site_round_high >> 5) << 16 | u32::from(u16_at(15)?),
            site_id: ((event_site_round_high >> 2) & 7) << 16 | u32::from(u16_at(17)?),
            round_id: (event_site_round_high & 3) << 16 | u32::from(u16_at(19)?),
            variation_count: count(0),
            comment_count: count(4),
            nag_count: count(8),
            result,
            eco_code: u16_at(23)?,
            date,
            event_date: event_date_from_bits(dates >> 20, date),
            white_elo: (white_elo_raw & 0x0FFF).min(MAX_ELO),
            white_rating_type: (white_elo_raw >> 12) as u8,
            black_elo: (black_elo_raw & 0x0FFF).min(MAX_ELO),
            black_rating_type: (black_elo_raw >> 12) as u8,
            final_mat_sig: final_mat & 0x00FF_FFFF,
            stored_line_code: (final_mat >> 24) as u8,
            num_half_moves: u16::from(u8_at(37)?) | (u16::from(byte38 >> 6) << 8),
            home_pawn_data,
        };
        Ok((entry, result_ok))
    }

    /// Checks that every name ID exists in `names` (`docs/formatos/si4.md`,
    /// section 4). Returns the reason of the first ID out of range.
    pub fn verify_names(&self, names: &NameBase) -> Result<(), IndexEntryError> {
        let checks = [
            (
                NameKind::Player,
                self.white_id,
                "white player id out of range",
            ),
            (
                NameKind::Player,
                self.black_id,
                "black player id out of range",
            ),
            (NameKind::Event, self.event_id, "event id out of range"),
            (NameKind::Site, self.site_id, "site id out of range"),
            (NameKind::Round, self.round_id, "round id out of range"),
        ];
        for (kind, id, reason) in checks {
            if id >= names.num_names(kind) {
                return Err(IndexEntryError::new(reason));
            }
        }
        Ok(())
    }

    /// The ECO code, if the entry has a valid one.
    pub fn eco(&self) -> Option<Eco> {
        Eco::from_scid_code(self.eco_code)
    }

    /// The rating of White, if the Elo is above 0 and the type is known
    /// (types 7-15 are reserved and give `None`).
    pub fn white_rating(&self) -> Option<Rating> {
        rating(self.white_elo, self.white_rating_type)
    }

    /// The rating of Black, see [`IndexEntry::white_rating`].
    pub fn black_rating(&self) -> Option<Rating> {
        rating(self.black_elo, self.black_rating_type)
    }

    /// The material signature of the final position, unpacked.
    pub fn final_material(&self) -> MaterialSignature {
        MaterialSignature::from_bits(self.final_mat_sig)
    }

    /// The number of home-pawn changes recorded (0-16).
    pub fn home_pawn_change_count(&self) -> u8 {
        self.home_pawn_data.first().map_or(0, |b| (b & 63).min(16))
    }

    /// The home-pawn changes (`log2` of the change of the pawn signature,
    /// 0-15), the first one in the high nibble of byte 1.
    pub fn home_pawn_changes(&self) -> impl Iterator<Item = u8> + '_ {
        let count = usize::from(self.home_pawn_change_count());
        self.home_pawn_data
            .iter()
            .skip(1)
            .flat_map(|&b| [b >> 4, b & 15])
            .take(count)
    }
}

fn rating(elo: u16, kind: u8) -> Option<Rating> {
    if elo == 0 {
        return None;
    }
    let kind = *RatingType::ALL.get(usize::from(kind))?;
    Some(Rating::new(Elo::new(elo).ok()?, kind))
}

/// Decodes a Scid 20-bit date (`year << 9 | month << 5 | day`, 0 = unknown).
/// A month above 12 is treated as unknown.
pub(crate) fn date_from_bits(bits: u32) -> PartialDate {
    let year = u16::try_from((bits >> 9) & 0x7FF).ok();
    let month = u8::try_from((bits >> 5) & 0xF).ok().filter(|&m| m <= 12);
    let day = u8::try_from(bits & 0x1F).ok();
    PartialDate::new(year, month, day).unwrap_or(PartialDate::UNKNOWN)
}

/// Decodes the 12-bit event date relative to the game date
/// (`docs/formatos/si4.md`, section 3).
fn event_date_from_bits(bits: u32, game_date: PartialDate) -> PartialDate {
    let offset = (bits >> 9) & 7;
    if offset == 0 {
        return PartialDate::UNKNOWN;
    }
    let game_year = i32::from(game_date.year().unwrap_or(0));
    // `offset` is at most 7, so the sum fits easily in an `i32`.
    let year = game_year + i32::try_from(offset).unwrap_or(0) - 4;
    let year = u16::try_from(year).ok().filter(|&y| y != 0);
    let month = u8::try_from((bits >> 5) & 0xF).ok().filter(|&m| m <= 12);
    let day = u8::try_from(bits & 0x1F).ok();
    PartialDate::new(year, month, day).unwrap_or(PartialDate::UNKNOWN)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An entry with every field at a distinct, checkable value.
    fn raw_entry() -> [u8; ENTRY_LEN] {
        let mut e = [0u8; ENTRY_LEN];
        e[0..4].copy_from_slice(&0x0102_0304u32.to_be_bytes());
        e[4..6].copy_from_slice(&0x1234u16.to_be_bytes());
        e[6] = 0x80 | 0x40 | 0x05; // length bit 16, spare bit, custom 1 and 3
        e[7..9].copy_from_slice(&0x8009u16.to_be_bytes()); // USER | START | DELETE-ish
        e[9] = 0xAB; // white high 0xA, black high 0xB
        e[10..12].copy_from_slice(&0x1111u16.to_be_bytes());
        e[12..14].copy_from_slice(&0x2222u16.to_be_bytes());
        e[14] = (0b101 << 5) | (0b011 << 2) | 0b10; // event 5, site 3, round 2
        e[15..17].copy_from_slice(&0x3333u16.to_be_bytes());
        e[17..19].copy_from_slice(&0x4444u16.to_be_bytes());
        e[19..21].copy_from_slice(&0x5555u16.to_be_bytes());
        e[21..23].copy_from_slice(&((3u16 << 12) | (12 << 8) | (11 << 4) | 2).to_be_bytes());
        e[23..25].copy_from_slice(&132u16.to_be_bytes()); // A01
        let game_date = (2001u32 << 9) | (3 << 5) | 15;
        let event_date = (5u32 << 9) | (2 << 5) | 10; // offset 5 -> year + 1
        e[25..29].copy_from_slice(&((event_date << 20) | game_date).to_be_bytes());
        e[29..31].copy_from_slice(&((3u16 << 12) | 2750).to_be_bytes());
        e[31..33].copy_from_slice(&((5u16 << 12) | 4095).to_be_bytes());
        e[33..37].copy_from_slice(&0xAB12_3456u32.to_be_bytes());
        e[37] = 0xFF;
        e[38] = 0xC0 | 0x05; // half moves bits 8-9 = 3, 5 pawn changes
        e[39..47].copy_from_slice(&[0x12, 0x34, 0x56, 0, 0, 0, 0, 0]);
        e
    }

    #[test]
    fn decodes_every_field() {
        let e = IndexEntry::parse(&raw_entry()).unwrap();
        assert_eq!(e.offset, 0x0102_0304);
        assert_eq!(e.length, 0x1234 + 0x1_0000);
        assert!(e.length_spare_bit);
        assert!(e.flags.contains(GameFlags::USER | GameFlags::START));
        assert!(e.flags.contains(GameFlags::DELETE));
        assert!(e.flags.contains(GameFlags::CUSTOM1 | GameFlags::CUSTOM3));
        assert!(!e.flags.contains(GameFlags::CUSTOM2));
        assert_eq!(e.white_id, 0xA_1111);
        assert_eq!(e.black_id, 0xB_2222);
        assert_eq!(e.event_id, (5 << 16) | 0x3333);
        assert_eq!(e.site_id, (3 << 16) | 0x4444);
        assert_eq!(e.round_id, (2 << 16) | 0x5555);
        assert_eq!(
            (e.variation_count, e.comment_count, e.nag_count),
            (2, 15, 20)
        );
        assert_eq!(e.result, GameResult::Draw);
        assert_eq!(e.eco().map(|c| c.to_string()), Some("A01".to_owned()));
        assert_eq!(
            (e.date.year(), e.date.month(), e.date.day()),
            (Some(2001), Some(3), Some(15))
        );
        assert_eq!(
            (
                e.event_date.year(),
                e.event_date.month(),
                e.event_date.day()
            ),
            (Some(2002), Some(2), Some(10))
        );
        assert_eq!(e.white_elo, 2750);
        assert_eq!(e.white_rating().map(|r| r.kind), Some(RatingType::Iccf));
        assert_eq!(e.black_elo, 4000, "Elo above 4000 is limited");
        assert_eq!(e.black_rating().map(|r| r.kind), Some(RatingType::Dwz));
        assert_eq!(e.final_mat_sig, 0x12_3456);
        assert_eq!(e.stored_line_code, 0xAB);
        assert_eq!(
            e.num_half_moves, 0x3FF,
            "bits 8-9 come from HomePawnData[0]"
        );
        assert_eq!(e.home_pawn_data[0], 5);
        assert_eq!(e.home_pawn_change_count(), 5);
        assert_eq!(
            e.home_pawn_changes().collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn all_twenty_two_flags_can_be_set() {
        let mut raw = raw_entry();
        raw[6] = 0xFF;
        raw[7] = 0xFF;
        raw[8] = 0xFF;
        let e = IndexEntry::parse(&raw).unwrap();
        assert_eq!(e.flags.bits().count_ones(), 22);
        assert_eq!(e.flags, GameFlags::all());
        assert_eq!(e.length, 0x1234 + 0x1_0000);
    }

    #[test]
    fn maximum_ids_and_length() {
        let mut raw = raw_entry();
        raw[4..6].copy_from_slice(&[0xFF, 0xFF]);
        raw[6] = 0x80;
        raw[9] = 0xFF;
        raw[10..14].copy_from_slice(&[0xFF; 4]);
        raw[14] = 0xFF;
        raw[15..21].copy_from_slice(&[0xFF; 6]);
        let e = IndexEntry::parse(&raw).unwrap();
        assert_eq!(e.length, 131_071);
        assert_eq!((e.white_id, e.black_id), (1_048_575, 1_048_575));
        assert_eq!((e.event_id, e.site_id), (524_287, 524_287));
        assert_eq!(e.round_id, 262_143);
    }

    #[test]
    fn rejects_result_above_three_and_short_input() {
        for result in 4u16..=15 {
            let mut raw = raw_entry();
            raw[21..23].copy_from_slice(&(result << 12).to_be_bytes());
            assert_eq!(
                IndexEntry::parse(&raw).map_err(|e| e.reason()),
                Err("result out of range")
            );
            assert_eq!(
                IndexEntry::parse_lenient(&raw).map(|(e, ok)| (e.result, ok)),
                Ok((GameResult::Unknown, false))
            );
        }
        assert!(IndexEntry::parse(&raw_entry()[..46]).is_err());
    }

    #[test]
    fn event_date_offsets_and_absence() {
        let game_date = (2000u32 << 9) | (6 << 5) | 1;
        for off in 0u32..=7 {
            let mut raw = raw_entry();
            let event = (off << 9) | (1 << 5) | 2;
            raw[25..29].copy_from_slice(&((event << 20) | game_date).to_be_bytes());
            let e = IndexEntry::parse(&raw).unwrap();
            if off == 0 {
                assert!(e.event_date.is_unknown());
            } else {
                let year = 2000 + i32::try_from(off).unwrap() - 4;
                assert_eq!(e.event_date.year().map(i32::from), Some(year));
                assert_eq!(
                    (e.event_date.month(), e.event_date.day()),
                    (Some(1), Some(2))
                );
            }
        }
    }

    #[test]
    fn unknown_date_parts_and_invalid_month() {
        let mut raw = raw_entry();
        raw[25..29].copy_from_slice(&((1999u32 << 9) | (14 << 5)).to_be_bytes());
        let e = IndexEntry::parse(&raw).unwrap();
        assert_eq!(
            (e.date.year(), e.date.month(), e.date.day()),
            (Some(1999), None, None)
        );
        raw[25..29].copy_from_slice(&0u32.to_be_bytes());
        assert!(IndexEntry::parse(&raw).unwrap().date.is_unknown());
    }

    #[test]
    fn eco_codes_basic_and_extended() {
        let mut raw = raw_entry();
        for (code, text) in [(1u16, "A00"), (132, "A01"), (1 + 131 * 499, "E99")] {
            raw[23..25].copy_from_slice(&code.to_be_bytes());
            let e = IndexEntry::parse(&raw).unwrap();
            assert_eq!(e.eco().map(Eco::basic_string), Some(text.to_owned()));
        }
        // B90 with extension "a" and digit 2: 1 + 131 * 190 + 1 + 5 * 0 + 2.
        raw[23..25].copy_from_slice(&((1 + 131 * 190 + 1 + 2) as u16).to_be_bytes());
        let e = IndexEntry::parse(&raw).unwrap();
        assert_eq!(e.eco().map(|c| c.to_string()), Some("B90a2".to_owned()));
        raw[23..25].copy_from_slice(&0u16.to_be_bytes());
        assert_eq!(IndexEntry::parse(&raw).unwrap().eco(), None);
    }

    #[test]
    fn count_scale_and_reserved_rating_types() {
        let mut raw = raw_entry();
        let counts: Vec<u8> = (0u16..16)
            .map(|c| {
                raw[21..23].copy_from_slice(&c.to_be_bytes());
                IndexEntry::parse(&raw).unwrap().variation_count
            })
            .collect();
        assert_eq!(counts, COUNT_SCALE.to_vec());
        raw[29..31].copy_from_slice(&((9u16 << 12) | 2000).to_be_bytes());
        let e = IndexEntry::parse(&raw).unwrap();
        assert_eq!(
            (e.white_elo, e.white_rating_type, e.white_rating()),
            (2000, 9, None)
        );
    }

    #[test]
    fn material_signature_unpacks_the_start_position() {
        let sig = (1 << 22)
            | (1 << 10)
            | (2 << 20)
            | (2 << 8)
            | (2 << 18)
            | (2 << 6)
            | (2 << 16)
            | (2 << 4)
            | (8 << 12)
            | 8;
        let m = MaterialSignature::from_bits(sig);
        assert_eq!((m.white_pawns, m.black_pawns, m.white_queens), (8, 8, 1));
        assert_eq!((m.white_rooks, m.black_bishops, m.black_knights), (2, 2, 2));
    }

    #[test]
    fn home_pawn_changes_are_capped_at_sixteen() {
        let mut raw = raw_entry();
        raw[38] = 63;
        raw[39..47].copy_from_slice(&[0x01; 8]);
        let e = IndexEntry::parse(&raw).unwrap();
        assert_eq!(e.home_pawn_change_count(), 16);
        assert_eq!(e.home_pawn_changes().count(), 16);
    }
}
