// Derived from Scid vs PC 4.27, src/game.cpp (the move codec).
// Copyright (c) 2000-2003 Shane Hudson. All rights reserved (notice of
// game.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The move codec (`docs/formatos/si4.md`, section 9.4; Scid `game.cpp`,
//! `decodeMove` and the `decode*` functions).
//!
//! A move byte is `(pieceNum << 4) | v`. The piece is `list[pieceNum]` of the
//! side to move and its type comes from the board; `v` is interpreted per
//! piece type. The decoder does not check legality, only that the squares
//! exist.

use mychess_core::{Color, File, Rank, Role, Square};

use super::error::MoveDecodeError;
use super::position::ListPosition;
use super::raw::RawMove;

/// King offsets for `v = 1..=10`; 9 and 10 are the two castlings.
const KING_DELTAS: [i32; 10] = [-9, -8, -7, -1, 1, 7, 8, 9, -2, 2];
/// Knight offsets for `v = 1..=8`.
const KNIGHT_DELTAS: [i32; 8] = [-17, -15, -10, -6, 6, 10, 15, 17];
/// Pawn forward distance for `v = 0..=15`.
const PAWN_STEPS: [i32; 16] = [7, 8, 9, 7, 8, 9, 7, 8, 9, 7, 8, 9, 7, 8, 9, 16];

/// Decodes one move from the start of `bytes` for the side to move of `pos`.
///
/// Returns the move and the number of bytes it takes (1, or 2 for a queen
/// diagonal). The byte must not be one of the special tokens 11 to 15 (the
/// caller handles those). Errors have a static reason: no such piece in the
/// list, a code that is invalid for the piece, a second byte outside 64-127
/// or a destination off the board.
pub fn decode_move(pos: &ListPosition, bytes: &[u8]) -> Result<(RawMove, usize), MoveDecodeError> {
    let &first = bytes
        .first()
        .ok_or(MoveDecodeError::new("move byte missing"))?;
    let from = pos
        .mover_square(first >> 4)
        .ok_or(MoveDecodeError::new("piece number beyond the piece list"))?;
    let piece = pos
        .piece_at(from)
        .ok_or(MoveDecodeError::new("piece list points to an empty square"))?;
    let v = first & 15;
    let mut used = 1;
    let mut promotion = None;
    let to = match piece.role {
        Role::King => match v {
            0 => Some(from),
            1..=10 => offset(from, KING_DELTAS.get(usize::from(v) - 1).copied()),
            _ => return Err(MoveDecodeError::new("invalid king move code")),
        },
        Role::Knight => match v {
            1..=8 => offset(from, KNIGHT_DELTAS.get(usize::from(v) - 1).copied()),
            _ => return Err(MoveDecodeError::new("invalid knight move code")),
        },
        Role::Rook => rook_target(from, v),
        Role::Bishop => {
            let column = i32::from(v & 7);
            let diff = column - file_of(from);
            if v >= 8 {
                offset(from, Some(-7 * diff))
            } else {
                offset(from, Some(9 * diff))
            }
        }
        Role::Queen => {
            if v >= 8 || i32::from(v) != file_of(from) {
                rook_target(from, v)
            } else {
                let &second = bytes
                    .get(1)
                    .ok_or(MoveDecodeError::new("queen move second byte missing"))?;
                if !(64..=127).contains(&second) {
                    return Err(MoveDecodeError::new("queen move second byte out of range"));
                }
                used = 2;
                Square::try_from(second - 64).ok()
            }
        }
        Role::Pawn => {
            let step = PAWN_STEPS.get(usize::from(v)).copied().unwrap_or(8);
            promotion = match v {
                3..=5 => Some(Role::Queen),
                6..=8 => Some(Role::Rook),
                9..=11 => Some(Role::Bishop),
                12..=14 => Some(Role::Knight),
                _ => None,
            };
            offset(
                from,
                Some(if piece.color == Color::White {
                    step
                } else {
                    -step
                }),
            )
        }
    };
    let to = to.ok_or(MoveDecodeError::new("destination off the board"))?;
    Ok((
        RawMove {
            from,
            to,
            promotion,
        },
        used,
    ))
}

fn file_of(sq: Square) -> i32 {
    // A file index is 0..8.
    i32::from(sq.file() as u8)
}

fn offset(from: Square, delta: Option<i32>) -> Option<Square> {
    from.offset(delta?)
}

/// Rook-like target: `v >= 8` stays on the file and goes to rank `v - 8`;
/// otherwise stays on the rank and goes to file `v`.
fn rook_target(from: Square, v: u8) -> Option<Square> {
    if v >= 8 {
        Some(Square::from_coords(
            from.file(),
            Rank::try_from(v - 8).ok()?,
        ))
    } else {
        Some(Square::from_coords(File::try_from(v).ok()?, from.rank()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(pos: &ListPosition, bytes: &[u8]) -> (RawMove, usize) {
        decode_move(pos, bytes).unwrap()
    }

    fn mv(from: Square, to: Square) -> RawMove {
        RawMove {
            from,
            to,
            promotion: None,
        }
    }

    #[test]
    fn decodes_the_example_of_the_format_document() {
        // 1.e4 e5 = 00 00 CF CF 0F; here the two moves 0xCF.
        let mut pos = ListPosition::standard();
        let (m, n) = decode(&pos, &[0xCF]);
        assert_eq!((m, n), (mv(Square::E2, Square::E4), 1));
        pos.make(m).unwrap();
        let (m, _) = decode(&pos, &[0xCF]);
        assert_eq!(m, mv(Square::E7, Square::E5));
    }

    #[test]
    fn decodes_pawn_moves_and_promotions() {
        // White pawn on b7 (index 0 after the king) and black pieces around.
        let pos = ListPosition::from_fen("r1k5/1P6/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        // White list: [E1 (king), B7].
        let promo = |v: u8| decode(&pos, &[0x10 | v]).0;
        assert_eq!(promo(1), mv(Square::B7, Square::B8)); // forward
        assert_eq!(promo(0), mv(Square::B7, Square::A8)); // capture left
        assert_eq!(promo(2), mv(Square::B7, Square::C8)); // capture right
        for (v, role) in [
            (4, Role::Queen),
            (7, Role::Rook),
            (10, Role::Bishop),
            (13, Role::Knight),
        ] {
            let m = promo(v);
            assert_eq!((m.to, m.promotion), (Square::B8, Some(role)), "v={v}");
        }
    }

    #[test]
    fn black_pawn_moves_go_down_the_board() {
        let pos = ListPosition::from_fen("4k3/3p4/8/8/8/8/8/4K3 b - - 0 1").unwrap();
        // Black list: [E8, D7].
        assert_eq!(decode(&pos, &[0x11]).0, mv(Square::D7, Square::D6));
        assert_eq!(decode(&pos, &[0x1F]).0, mv(Square::D7, Square::D5));
        assert_eq!(decode(&pos, &[0x10]).0, mv(Square::D7, Square::E6)); // 7 down
        assert_eq!(decode(&pos, &[0x12]).0, mv(Square::D7, Square::C6)); // 9 down
    }

    #[test]
    fn decodes_king_moves_castling_and_null() {
        let pos = ListPosition::from_fen("4k3/8/8/8/4K3/8/8/8 w - - 0 1").unwrap();
        assert_eq!(decode(&pos, &[0x00]).0, mv(Square::E4, Square::E4));
        let targets = [
            Square::D3,
            Square::E3,
            Square::F3,
            Square::D4,
            Square::F4,
            Square::D5,
            Square::E5,
            Square::F5,
            Square::C4, // 9: -2
            Square::G4, // 10: +2
        ];
        for (i, target) in targets.into_iter().enumerate() {
            let v = u8::try_from(i + 1).unwrap();
            assert_eq!(decode(&pos, &[v]).0, mv(Square::E4, target), "v={v}");
        }
        assert!(decode_move(&pos, &[0x0B]).is_err());
        assert!(decode_move(&pos, &[0x0F]).is_err());
    }

    #[test]
    fn decodes_knight_moves() {
        let pos = ListPosition::from_fen("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1").unwrap();
        // White list: [E1, D4].
        let targets = [
            Square::C2,
            Square::E2,
            Square::B3,
            Square::F3,
            Square::B5,
            Square::F5,
            Square::C6,
            Square::E6,
        ];
        for (i, target) in targets.into_iter().enumerate() {
            let v = u8::try_from(i + 1).unwrap();
            assert_eq!(decode(&pos, &[0x10 | v]).0, mv(Square::D4, target), "v={v}");
        }
        assert!(decode_move(&pos, &[0x10]).is_err());
        assert!(decode_move(&pos, &[0x19]).is_err());
    }

    #[test]
    fn decodes_rook_moves_by_rank_and_file() {
        let pos = ListPosition::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
        // White list: [E1, A1].
        assert_eq!(decode(&pos, &[0x13]).0, mv(Square::A1, Square::D1)); // rank: file d
        assert_eq!(decode(&pos, &[0x1B]).0, mv(Square::A1, Square::A4)); // file: rank 4
    }

    #[test]
    fn decodes_bishop_diagonals_both_ways() {
        let pos = ListPosition::from_fen("4k3/8/8/8/3B4/8/8/4K3 w - - 0 1").unwrap();
        // d4 bishop: v < 8 goes up-right/down-left, v >= 8 up-left/down-right.
        assert_eq!(decode(&pos, &[0x16]).0, mv(Square::D4, Square::G7)); // file g
        assert_eq!(decode(&pos, &[0x11]).0, mv(Square::D4, Square::B2)); // file b, down-left
        assert_eq!(decode(&pos, &[0x19]).0, mv(Square::D4, Square::B6)); // file b, up-left
    }

    #[test]
    fn decodes_queen_rook_moves_and_two_byte_diagonals() {
        let pos = ListPosition::from_fen("4k3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        // White list: [E1, D1].
        assert_eq!(decode(&pos, &[0x14]).0, mv(Square::D1, Square::E1)); // rank: file e
        assert_eq!(decode(&pos, &[0x1C]).0, mv(Square::D1, Square::D5)); // file: rank 5
        // v == file of the queen (d = 3) announces a diagonal in two bytes.
        let (m, n) = decode(&pos, &[0x13, 64 + 28]); // square index 28 is e4
        assert_eq!((m.from, m.to, n), (Square::D1, Square::E4, 2));
        assert!(decode_move(&pos, &[0x13]).is_err(), "second byte missing");
        assert!(decode_move(&pos, &[0x13, 63]).is_err(), "below 64");
        assert!(decode_move(&pos, &[0x13, 128]).is_err(), "above 127");
    }

    #[test]
    fn rejects_piece_numbers_beyond_the_list_and_off_board_targets() {
        let pos = ListPosition::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert!(decode_move(&pos, &[0x10]).is_err());
        assert!(decode_move(&pos, &[]).is_err());
        // King on e1, delta -9 lands off the board.
        assert!(decode_move(&pos, &[0x01]).is_err());
    }
}
