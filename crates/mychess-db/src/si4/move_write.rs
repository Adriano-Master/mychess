// Derived from Scid vs PC 4.27, src/game.cpp (encodeKing, encodeKnight,
// encodeRook, encodeBishop, encodeQueen, encodePawn).
// Copyright (c) 2000-2003 Shane Hudson. All rights reserved (notice of
// game.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The move encoder, inverse of [`decode_move`] (`docs/formatos/si4.md`,
//! section 9.4).
//!
//! A move is `(pieceNum << 4) | v`: the piece number is the index of the
//! moving piece in the piece list of the side to move, and `v` depends on the
//! type of the piece. The piece number is found here from the squares and the
//! list (the core never provides it). Every encoding is decoded again and
//! compared with the move, so a move that the codec cannot represent (a rook
//! that moves diagonally, a pawn that jumps three squares) is an error and
//! never a wrong byte.

use mychess_core::{Role, Square};

use super::moves::decode_move;
use super::position::ListPosition;
use super::raw::RawMove;

/// The bytes of one encoded move: one byte, or two for a queen diagonal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodedMove {
    bytes: [u8; 2],
    len: usize,
}

impl EncodedMove {
    /// The bytes to write.
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&[])
    }
}

/// King difference (-9..=9) to the code `v`; 0 means "not a king move".
const KING_CODE: [u8; 19] = [1, 2, 3, 0, 0, 0, 0, 9, 4, 0, 5, 10, 0, 0, 0, 0, 6, 7, 8];
/// Knight difference (-17..=17) to the code `v`; 0 means "not a knight move".
const KNIGHT_CODE: [u8; 35] = [
    1, 0, 2, 0, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0, 0,
    7, 0, 8,
];

fn index(sq: Square) -> i32 {
    // A square is always in 0..64.
    i32::try_from(sq.to_u32()).unwrap_or(0)
}

fn file_of(sq: Square) -> u8 {
    sq.file() as u8
}

fn rank_of(sq: Square) -> u8 {
    sq.rank() as u8
}

/// Encodes `mv` for the side to move of `pos`. The move is not applied.
///
/// Castling is the two-square king move. Errors have a static reason: no
/// piece of the side to move on the origin square, a promotion that is not
/// of a pawn to a queen, rook, bishop or knight, or a move shape the codec
/// cannot represent for that piece.
pub fn encode_move(pos: &ListPosition, mv: RawMove) -> Result<EncodedMove, &'static str> {
    let piece = pos
        .piece_at(mv.from)
        .filter(|p| p.color == pos.turn())
        .ok_or("no piece of the side to move on the origin square")?;
    let number = pos
        .mover_index(mv.from)
        .ok_or("piece list does not hold the moving piece")?;
    if mv.promotion.is_some() && piece.role != Role::Pawn {
        return Err("promotion of a piece that is not a pawn");
    }
    let diff = index(mv.to) - index(mv.from);
    let same_rank = rank_of(mv.from) == rank_of(mv.to);
    let mut second = None;
    let v = match piece.role {
        Role::King => {
            if diff == 0 {
                0
            } else {
                lookup(&KING_CODE, diff + 9).ok_or("invalid king move")?
            }
        }
        Role::Knight => lookup(&KNIGHT_CODE, diff + 17).ok_or("invalid knight move")?,
        Role::Rook => rook_code(mv, same_rank),
        Role::Bishop => {
            let rank_diff = i32::from(rank_of(mv.to)) - i32::from(rank_of(mv.from));
            let file_diff = i32::from(file_of(mv.to)) - i32::from(file_of(mv.from));
            file_of(mv.to) + if rank_diff * file_diff < 0 { 8 } else { 0 }
        }
        Role::Queen => {
            if same_rank || file_of(mv.from) == file_of(mv.to) {
                rook_code(mv, same_rank)
            } else {
                // A rook-like move to the origin square marks the diagonal,
                // and the destination follows in a second byte (+ 64).
                second = Some(u8::try_from(mv.to.to_u32() + 64).map_err(|_| "square")?);
                file_of(mv.from)
            }
        }
        Role::Pawn => pawn_code(mv, diff)?,
    };
    let first = (number << 4) | v;
    let encoded = match second {
        Some(b) => EncodedMove {
            bytes: [first, b],
            len: 2,
        },
        None => EncodedMove {
            bytes: [first, 0],
            len: 1,
        },
    };
    // The codec must give the same move back.
    let (back, used) = decode_move(pos, encoded.as_bytes()).map_err(|e| e.reason())?;
    if back != mv || used != encoded.len {
        return Err("move cannot be represented by the move codec");
    }
    Ok(encoded)
}

fn lookup(table: &[u8], at: i32) -> Option<u8> {
    let code = usize::try_from(at)
        .ok()
        .and_then(|i| table.get(i))
        .copied()?;
    (code != 0).then_some(code)
}

fn rook_code(mv: RawMove, same_rank: bool) -> u8 {
    if same_rank {
        file_of(mv.to)
    } else {
        8 + rank_of(mv.to)
    }
}

fn pawn_code(mv: RawMove, diff: i32) -> Result<u8, &'static str> {
    let base = match diff.abs() {
        16 => {
            if mv.promotion.is_some() {
                return Err("a double pawn step cannot promote");
            }
            return Ok(15);
        }
        7 => 0,
        8 => 1,
        9 => 2,
        _ => return Err("invalid pawn move"),
    };
    let promotion = match mv.promotion {
        None => 0,
        Some(Role::Queen) => 3,
        Some(Role::Rook) => 6,
        Some(Role::Bishop) => 9,
        Some(Role::Knight) => 12,
        Some(_) => return Err("a pawn promotes to a queen, rook, bishop or knight"),
    };
    Ok(base + promotion)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(from: Square, to: Square) -> RawMove {
        RawMove {
            from,
            to,
            promotion: None,
        }
    }

    fn bytes(pos: &ListPosition, m: RawMove) -> Vec<u8> {
        encode_move(pos, m).unwrap().as_bytes().to_vec()
    }

    #[test]
    fn encodes_the_example_of_the_format_document() {
        let mut pos = ListPosition::standard();
        let e4 = mv(Square::E2, Square::E4);
        assert_eq!(bytes(&pos, e4), vec![0xCF]);
        pos.make(e4).unwrap();
        assert_eq!(bytes(&pos, mv(Square::E7, Square::E5)), vec![0xCF]);
    }

    #[test]
    fn every_legal_first_move_round_trips_through_the_decoder() {
        let pos = ListPosition::standard();
        let mut count = 0;
        for from in 0..64usize {
            for to in 0..64usize {
                let (Ok(from), Ok(to)) = (Square::try_from(from), Square::try_from(to)) else {
                    continue;
                };
                if let Ok(enc) = encode_move(&pos, mv(from, to)) {
                    let (back, _) = decode_move(&pos, enc.as_bytes()).unwrap();
                    assert_eq!(back, mv(from, to));
                    count += 1;
                }
            }
        }
        // 16 pawn moves, 4 knight moves and the king and pieces that the
        // codec can describe even if they are blocked (it checks no legality).
        assert!(count >= 20, "{count}");
    }

    #[test]
    fn queen_diagonals_take_two_bytes_and_promotions_are_coded() {
        let pos = ListPosition::from_fen("4k3/P7/8/8/3Q4/8/8/4K3 w - - 0 1").unwrap();
        let diagonal = bytes(&pos, mv(Square::D4, Square::H8));
        assert_eq!(diagonal.len(), 2);
        assert_eq!(diagonal[1], 63 + 64);
        let promo = |role| {
            bytes(
                &pos,
                RawMove {
                    from: Square::A7,
                    to: Square::A8,
                    promotion: Some(role),
                },
            )
        };
        // White list: king, then the pawn (a7) and the queen in reading order.
        let first = promo(Role::Queen)[0];
        assert_eq!(first & 15, 4);
        assert_eq!(promo(Role::Rook)[0] & 15, 7);
        assert_eq!(promo(Role::Bishop)[0] & 15, 10);
        assert_eq!(promo(Role::Knight)[0] & 15, 13);
    }

    #[test]
    fn castling_is_the_two_square_king_move() {
        let pos = ListPosition::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        assert_eq!(bytes(&pos, mv(Square::E1, Square::G1)), vec![0x0A]);
        assert_eq!(bytes(&pos, mv(Square::E1, Square::C1)), vec![0x09]);
        // The null move is the king staying on its square.
        assert_eq!(bytes(&pos, mv(Square::E1, Square::E1)), vec![0x00]);
    }

    #[test]
    fn impossible_moves_are_errors() {
        let pos = ListPosition::standard();
        // Rook moving diagonally.
        assert!(encode_move(&pos, mv(Square::A1, Square::B2)).is_err());
        // Pawn three squares.
        assert!(encode_move(&pos, mv(Square::E2, Square::E5)).is_err());
        // No piece, or the opponent's piece.
        assert!(encode_move(&pos, mv(Square::E4, Square::E5)).is_err());
        assert!(encode_move(&pos, mv(Square::E7, Square::E5)).is_err());
        // Promotion by a knight; promotion to a king.
        let promo = |from, to, role| RawMove {
            from,
            to,
            promotion: Some(role),
        };
        assert!(encode_move(&pos, promo(Square::G1, Square::F3, Role::Queen)).is_err());
        assert!(encode_move(&pos, promo(Square::E2, Square::E3, Role::King)).is_err());
        assert!(encode_move(&pos, promo(Square::E2, Square::E4, Role::Queen)).is_err());
    }
}
