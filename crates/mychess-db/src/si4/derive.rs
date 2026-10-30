// Derived from Scid vs PC 4.27, src/game.cpp (Game::Encode, MakeHomePawnList),
// src/index.cpp, src/index.h (IndexEntry), src/matsig.cpp and src/stored.cpp.
// Copyright (c) 1999-2003 Shane Hudson. All rights reserved (notices of
// game.cpp and index.cpp); Scid vs PC contributors. Scid is distributed under
// the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Data derived from a game for its index entry
//! (`docs/specs/m3-2-escrita-si4.md`, rules 33 to 38): the final material
//! signature, the home-pawn data, the number of half moves, the promotion
//! flags, the stored-line code, the 4-bit counts, the event date and the
//! rating fields.

use std::sync::OnceLock;

use mychess_core::{Color, MAX_ELO, PartialDate, Position, Role, SanMode, Square};

use super::error::{Result, Si4Error};
use super::position::ListPosition;
use super::raw::RawMove;
use super::stored_lines::STORED_LINES;

/// The scale decoded from a 4-bit count code (`DecodeCount`).
const COUNT_SCALE: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 15, 20, 30, 40, 50];

/// The home-pawn signature of the standard start position (`HPSIG_StdStart`).
const HPSIG_STD_START: u16 = 0xFFFF;

/// Encodes a count of variations, comments or NAGs in 4 bits
/// (`IndexEntry::EncodeCount`): exact up to 10, then 10 stands for 11-12, 11
/// for 13-17, 12 for 18-24, 13 for 25-34, 14 for 35-44 and 15 for 45 or more.
pub fn encode_count(x: u32) -> u8 {
    match x {
        0..=10 => u8::try_from(x).unwrap_or(10),
        11..=12 => 10,
        13..=17 => 11,
        18..=24 => 12,
        25..=34 => 13,
        35..=44 => 14,
        _ => 15,
    }
}

/// The value a 4-bit count code stands for (`DecodeCount`): 0-10, 15, 20, 30,
/// 40 or 50.
pub fn decode_count(code: u8) -> u8 {
    COUNT_SCALE
        .get(usize::from(code & 15))
        .copied()
        .unwrap_or(0)
}

/// The 12-bit event-date field of the `Dates` word (`IndexEntry::SetEventDate`).
///
/// The event year is stored as an offset of -3 to +3 from the game year, so
/// an event date outside that range is lost, and so is every event date when
/// the game year is 0, 1 or 2 (the unsigned `dyear - 3` of the original wraps
/// around). An unknown event year gives 0, even if the month or day is known.
pub fn event_date_field(game: PartialDate, event: PartialDate) -> u32 {
    let game_year = u32::from(game.year().unwrap_or(0));
    let event_year = u32::from(event.year().unwrap_or(0));
    if event_year == 0 || game_year < 3 || event_year < game_year - 3 || event_year > game_year + 3
    {
        return 0;
    }
    let month = u32::from(event.month().unwrap_or(0));
    let day = u32::from(event.day().unwrap_or(0));
    // `event_year + 4 - game_year` is 1 to 7.
    let offset = (event_year + 4 - game_year) & 7;
    (month << 5) | day | (offset << 9)
}

/// The 16-bit rating field: 12 bits of Elo (limited to 4000, decision D281)
/// and the rating type in the high 4 bits.
pub fn rating_field(elo: u16, rating_type: u8) -> u16 {
    (elo.min(MAX_ELO) & 0x0FFF) | (u16::from(rating_type & 15) << 12)
}

fn role_count(pos: &ListPosition, color: Color, role: Role) -> u32 {
    let n = pos
        .piece_squares(color)
        .iter()
        .filter(|&&sq| pos.piece_at(sq).is_some_and(|p| p.role == role))
        .count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The 24-bit material signature of a position (`matsig_Make`): pawns in 4
/// bits and every other piece in 2 bits (counts above 3 are capped), with
/// the layout of `MaterialSignature::from_bits`.
pub fn material_signature(pos: &ListPosition) -> u32 {
    let mut sig = 0u32;
    for (color, pawn, knight, bishop, rook, queen) in [
        (Color::White, 12, 16, 18, 20, 22),
        (Color::Black, 0, 4, 6, 8, 10),
    ] {
        sig |= role_count(pos, color, Role::Pawn) << pawn;
        sig |= role_count(pos, color, Role::Knight).min(3) << knight;
        sig |= role_count(pos, color, Role::Bishop).min(3) << bishop;
        sig |= role_count(pos, color, Role::Rook).min(3) << rook;
        sig |= role_count(pos, color, Role::Queen).min(3) << queen;
    }
    sig & 0x00FF_FFFF
}

/// The home-pawn signature (`Position::GetHPSig`): bit 15 is the white pawn
/// on a2 ... bit 8 on h2, bit 7 the black pawn on a7 ... bit 0 on h7.
pub fn home_pawn_signature(pos: &ListPosition) -> u16 {
    let mut sig = 0u16;
    for file in 0..8usize {
        for (color, rank_base, high_bit) in [(Color::White, 8usize, 15usize), (Color::Black, 48, 7)]
        {
            let is_home_pawn = Square::try_from(rank_base + file)
                .ok()
                .and_then(|sq| pos.piece_at(sq))
                .is_some_and(|p| p.color == color && p.role == Role::Pawn);
            if is_home_pawn {
                sig |= 1 << (high_bit - file);
            }
        }
    }
    sig
}

/// Everything the index keeps that depends on the main line of a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainLineData {
    /// The material signature of the final position (24 bits).
    pub final_mat_sig: u32,
    /// The number of half moves of the main line (the entry keeps it modulo
    /// 1024, decision D278).
    pub num_half_moves: u32,
    /// The 9 bytes of home-pawn data: the count of changes, then the changes
    /// in half bytes. All zeros for a game with its own start position.
    pub home_pawn_data: [u8; 9],
    /// Whether the main line has a promotion.
    pub promotions: bool,
    /// Whether the main line has a promotion to something other than a queen.
    pub under_promotions: bool,
    /// The code of the longest stored line the game starts with (0 = none, or
    /// the game has its own start position).
    pub stored_line_code: u8,
}

/// Replays `moves` (the main line of a game) from `start` and computes the
/// data of the entry (`Game::MakeHomePawnList` and `Game::Encode`).
///
/// `standard_start` tells whether the game starts from the standard
/// position; only then the home-pawn list and the stored line are computed.
/// The final material and the number of half moves are exact in both cases
/// (the original leaves stale values in the index for games with their own
/// start position; see `docs/formatos/si4.md`, section 3).
pub fn derive_mainline(
    start: &ListPosition,
    standard_start: bool,
    moves: &[RawMove],
) -> Result<MainLineData> {
    let mut pos = start.clone();
    let mut home_pawn_data = [0u8; 9];
    let mut changes = 0usize;
    let mut hp_old = HPSIG_STD_START;
    let mut promotions = false;
    let mut under_promotions = false;
    let mut ply = 0u32;
    let mut half = false;

    let mut record_change = |pos: &ListPosition, hp_old: &mut u16, data: &mut [u8; 9]| {
        let hp_new = home_pawn_signature(pos);
        if hp_new == *hp_old {
            return;
        }
        // `log2` of the bits that left the home squares (unsigned difference,
        // as in the original).
        let change = u8::try_from(
            u32::from(*hp_old)
                .wrapping_sub(u32::from(hp_new))
                .checked_ilog2()
                .unwrap_or(0),
        )
        .unwrap_or(0);
        // A pawn leaves its home square only once, so at most 16 changes fit.
        if changes < 16 {
            let slot = 1 + changes / 2;
            if let Some(byte) = data.get_mut(slot) {
                *byte |= if half { change & 15 } else { change << 4 };
            }
        }
        half = !half;
        changes += 1;
        *hp_old = hp_new;
    };

    if standard_start {
        record_change(&pos, &mut hp_old, &mut home_pawn_data);
    }
    for mv in moves {
        ply += 1;
        if let Some(role) = mv.promotion {
            promotions = true;
            under_promotions |= role != Role::Queen;
        }
        pos.make(*mv)
            .map_err(|reason| Si4Error::UnencodableGame { ply, reason })?;
        if standard_start {
            record_change(&pos, &mut hp_old, &mut home_pawn_data);
        }
    }
    if standard_start && let Some(first) = home_pawn_data.first_mut() {
        // At most 16 changes were counted.
        *first = u8::try_from(changes.min(16)).unwrap_or(16);
    }
    let stored_line_code = if standard_start {
        let squares: Vec<(Square, Square)> = moves.iter().map(|m| (m.from, m.to)).collect();
        stored_line_code(&squares)
    } else {
        0
    };
    Ok(MainLineData {
        final_mat_sig: material_signature(&pos),
        num_half_moves: ply,
        home_pawn_data,
        promotions,
        under_promotions,
        stored_line_code,
    })
}

/// The moves of every stored line as `(from, to)` squares, with castling as
/// the two-square king move (decision D288). Built once from the SAN text.
fn stored_line_moves() -> &'static [Vec<(Square, Square)>] {
    static TABLE: OnceLock<Vec<Vec<(Square, Square)>>> = OnceLock::new();
    TABLE.get_or_init(|| STORED_LINES.iter().map(|text| parse_line(text)).collect())
}

/// Parses `1.e4 e5 2.Nf3` into squares; a line that cannot be read gives an
/// empty list, which never matches.
fn parse_line(text: &str) -> Vec<(Square, Square)> {
    let mut pos = Position::new();
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        let san = token.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
        let Ok(parsed) = pos.parse_san(san, SanMode::Strict) else {
            return Vec::new();
        };
        let (Some(from), Some(to)) = (parsed.mv.from(), parsed.mv.to()) else {
            return Vec::new();
        };
        if pos.play(parsed.mv).is_err() {
            return Vec::new();
        }
        out.push((from, to));
    }
    out
}

/// The code (1-based) of the longest stored line that is a prefix of `moves`,
/// or 0 (`Game::Encode`): the first of equal length wins, and a line longer
/// than the game does not match.
pub fn stored_line_code(moves: &[(Square, Square)]) -> u8 {
    let mut best = 0usize;
    let mut code = 0u8;
    for (i, line) in stored_line_moves().iter().enumerate() {
        if line.len() > best && moves.starts_with(line) {
            best = line.len();
            code = u8::try_from(i + 1).unwrap_or(0);
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(from: Square, to: Square) -> RawMove {
        RawMove {
            from,
            to,
            promotion: None,
        }
    }

    #[test]
    fn counts_follow_the_boundaries_of_the_scale() {
        let cases = [
            (0, 0),
            (10, 10),
            (11, 10),
            (12, 10),
            (13, 11),
            (17, 11),
            (18, 12),
            (24, 12),
            (25, 13),
            (34, 13),
            (35, 14),
            (44, 14),
            (45, 15),
            (u32::MAX, 15),
        ];
        for (n, code) in cases {
            assert_eq!(encode_count(n), code, "n={n}");
        }
        let scale: Vec<u8> = (0..16).map(decode_count).collect();
        assert_eq!(scale, COUNT_SCALE);
        for code in 0..16 {
            assert_eq!(encode_count(u32::from(decode_count(code))), code);
        }
    }

    #[test]
    fn event_date_follows_set_event_date() {
        let d = |y, m, dd| PartialDate::new(Some(y), Some(m), Some(dd)).unwrap();
        // Same year: offset 4.
        assert_eq!(
            event_date_field(d(2000, 1, 1), d(2000, 3, 4)),
            (4 << 9) | (3 << 5) | 4
        );
        // Offsets 1 and 7 are the limits.
        assert_eq!(event_date_field(d(2000, 1, 1), d(1997, 1, 1)) >> 9, 1);
        assert_eq!(event_date_field(d(2000, 1, 1), d(2003, 1, 1)) >> 9, 7);
        assert_eq!(event_date_field(d(2000, 1, 1), d(1996, 1, 1)), 0);
        assert_eq!(event_date_field(d(2000, 1, 1), d(2004, 1, 1)), 0);
        // Unknown event year: nothing, even with a month.
        let month_only = PartialDate::new(None, Some(5), None).unwrap();
        assert_eq!(event_date_field(d(2000, 1, 1), month_only), 0);
        // Game years 0, 1 and 2 always lose the event date; 3 keeps it.
        for year in 0..=2 {
            let game = PartialDate::new(Some(year), None, None).unwrap();
            assert_eq!(event_date_field(game, d(1, 1, 1)), 0, "year {year}");
        }
        assert_eq!(event_date_field(d(3, 1, 1), d(1, 2, 3)) >> 9, 2);
        assert_eq!(event_date_field(PartialDate::UNKNOWN, d(1, 1, 1)), 0);
    }

    #[test]
    fn rating_fields_limit_the_elo() {
        assert_eq!(rating_field(2700, 3), (3 << 12) | 2700);
        assert_eq!(rating_field(4000, 0), 4000);
        assert_eq!(rating_field(4001, 0), 4000);
        assert_eq!(rating_field(u16::MAX, 15), (15 << 12) | 4000);
    }

    #[test]
    fn the_standard_position_has_full_material_and_pawns() {
        let pos = ListPosition::standard();
        // 8 pawns, 2 knights, 2 bishops, 2 rooks and 1 queen per side.
        let side = (8 << 12) | (2 << 16) | (2 << 18) | (2 << 20) | (1 << 22);
        assert_eq!(material_signature(&pos), side | (side >> 12));
        assert_eq!(home_pawn_signature(&pos), 0xFFFF);
    }

    #[test]
    fn home_pawn_changes_are_log2_of_the_bits_lost() {
        // 1.e4 e5 2.Nf3 Nc6 3.d4 exd4: e2 (bit 11), e7 (bit 3), d2 (bit 12),
        // then the black pawn e5 captured is not on a home square.
        let moves = [
            raw(Square::E2, Square::E4),
            raw(Square::E7, Square::E5),
            raw(Square::G1, Square::F3),
            raw(Square::B8, Square::C6),
            raw(Square::D2, Square::D4),
            raw(Square::E5, Square::D4),
        ];
        let data = derive_mainline(&ListPosition::standard(), true, &moves).unwrap();
        assert_eq!(data.num_half_moves, 6);
        assert_eq!(data.home_pawn_data[0], 3);
        assert_eq!(data.home_pawn_data[1], (11 << 4) | 3);
        assert_eq!(data.home_pawn_data[2], 12 << 4);
        assert!(!data.promotions && !data.under_promotions);
        // 1.e4 e5 2.Nf3 is not a stored line prefix of the game that matters
        // here; the code is whatever the table gives, and not 0.
        assert_ne!(data.stored_line_code, 0);
    }

    #[test]
    fn games_with_their_own_start_have_no_pawn_list_or_stored_line() {
        let start = ListPosition::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let promo = RawMove {
            from: Square::A7,
            to: Square::A8,
            promotion: Some(Role::Knight),
        };
        let data = derive_mainline(&start, false, &[promo]).unwrap();
        assert_eq!(data.home_pawn_data, [0; 9]);
        assert_eq!(data.stored_line_code, 0);
        assert!(data.promotions && data.under_promotions);
        assert_eq!(data.num_half_moves, 1);
        // White: king and a knight; black: king.
        assert_eq!(data.final_mat_sig, 1 << 16);
    }

    #[test]
    fn an_impossible_move_is_an_error_with_its_ply() {
        let err = derive_mainline(
            &ListPosition::standard(),
            true,
            &[raw(Square::E2, Square::E4), raw(Square::E3, Square::E4)],
        );
        assert!(matches!(err, Err(Si4Error::UnencodableGame { ply: 2, .. })));
    }

    #[test]
    fn the_stored_line_table_is_complete_and_matches_the_longest_line() {
        let table = stored_line_moves();
        assert_eq!(table.len(), 254);
        assert!(
            table.iter().all(|l| !l.is_empty()),
            "every stored line must parse"
        );
        // Code 1 is "1.b3": a game that starts with it.
        assert_eq!(stored_line_code(&[(Square::B2, Square::B3)]), 1);
        // "1.c4 c5 2.Nf3" (code 4) beats "1.c4" and "1.c4 c5".
        let line = [
            (Square::C2, Square::C4),
            (Square::C7, Square::C5),
            (Square::G1, Square::F3),
            (Square::B8, Square::C6),
        ];
        assert_eq!(stored_line_code(&line), 4);
        assert_eq!(stored_line_code(&line[..2]), 3);
        assert_eq!(stored_line_code(&[]), 0);
        assert_eq!(stored_line_code(&[(Square::A2, Square::A3)]), 0);
    }
}
