//! The 16-bit Polyglot move encoding (spec D1104).
//!
//! Bits 0-2 destination file, 3-5 destination rank, 6-8 origin file, 9-11
//! origin rank, 12-14 promotion piece (1 = knight ... 4 = queen). Castling is
//! written as "king takes own rook" (`e1h1`, `e1a1`, `e8h8`, `e8a8`).
//!
//! Scid: `src/polyglot/move.cpp`, `move_from_polyglot` / `move_to_polyglot`.

use mychess_core::{CastlingMode, Move, Position, Role, Square};

/// Encodes `m` as a Polyglot move, `None` for the null move.
///
/// Castling uses the king and rook squares, so the result is the same for
/// standard and Chess960 positions; the book itself only applies to standard
/// chess.
pub fn encode_move(m: Move) -> Option<u16> {
    let (from, to, promotion) = match m {
        Move::Null => return None,
        Move::Normal {
            from,
            to,
            promotion,
            ..
        } => (from, to, promotion),
        Move::EnPassant { from, to } => (from, to, None),
        Move::Castle {
            king_from,
            rook_from,
        } => (king_from, rook_from, None),
    };
    let promo = match promotion {
        None => 0u16,
        Some(Role::Knight) => 1,
        Some(Role::Bishop) => 2,
        Some(Role::Rook) => 3,
        Some(Role::Queen) => 4,
        Some(_) => return None,
    };
    Some(coords(to) | (coords(from) << 6) | (promo << 12))
}

/// File in bits 0-2 and rank in bits 3-5.
fn coords(sq: Square) -> u16 {
    let file = u16::from(u8::from(sq.file()));
    let rank = u16::from(u8::from(sq.rank()));
    file | (rank << 3)
}

/// Finds the legal move of `pos` whose encoding is `code`.
///
/// Returns `None` for `0`, for codes that name no legal move (including a
/// castling written with the king's destination square instead of the rook's)
/// and for every non-standard position.
pub fn decode_move(pos: &Position, code: u16) -> Option<Move> {
    if code == 0 || pos.castling_mode() != CastlingMode::Standard {
        return None;
    }
    pos.legal_moves()
        .into_iter()
        .find(|m| encode_move(*m) == Some(code))
}

/// The legal moves of `pos` paired with their Polyglot codes, generated once
/// so that many codes can be resolved without regenerating the moves. Empty
/// for non-standard positions.
pub(crate) fn legal_code_map(pos: &Position) -> Vec<(u16, Move)> {
    if pos.castling_mode() != CastlingMode::Standard {
        return Vec::new();
    }
    pos.legal_moves()
        .into_iter()
        .filter_map(|m| Some((encode_move(m)?, m)))
        .collect()
}

/// Looks `code` up in a map from [`legal_code_map`] (`0` never matches).
pub(crate) fn lookup_code(map: &[(u16, Move)], code: u16) -> Option<Move> {
    if code == 0 {
        return None;
    }
    map.iter().find(|(c, _)| *c == code).map(|(_, m)| *m)
}

/// The Polyglot move as long algebraic text (`e2e4`, `e1h1`, `e7e8q`), for
/// diagnostics. Returns `None` for `0`.
pub fn code_to_text(code: u16) -> Option<String> {
    if code == 0 {
        return None;
    }
    let square = |bits: u16| -> String {
        let file = char::from(b'a' + (bits & 7) as u8);
        let rank = char::from(b'1' + ((bits >> 3) & 7) as u8);
        format!("{file}{rank}")
    };
    let promo = match (code >> 12) & 7 {
        1 => "n",
        2 => "b",
        3 => "r",
        4 => "q",
        _ => "",
    };
    Some(format!("{}{}{promo}", square(code >> 6), square(code)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_legal_move_round_trips() {
        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1",
            "r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R b KQkq - 0 1",
            "3n4/4P3/8/8/8/8/8/k6K w - - 0 1",
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        ];
        for fen in fens {
            let pos = Position::from_fen(fen).expect("fen");
            for m in pos.legal_moves() {
                let code = encode_move(m).expect("encodable");
                assert_eq!(decode_move(&pos, code), Some(m), "{fen} {m:?}");
            }
        }
    }

    #[test]
    fn castling_is_king_takes_rook() {
        let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").expect("fen");
        let short = pos.parse_uci("e1g1").expect("castle");
        assert_eq!(encode_move(short), Some(0x0107)); // e1h1
        assert_eq!(code_to_text(0x0107).as_deref(), Some("e1h1"));
        // The king-destination form is not accepted.
        assert_eq!(decode_move(&pos, 0x0106), None);
    }

    #[test]
    fn promotion_and_invalid_codes() {
        let pos = Position::from_fen("3n4/4P3/8/8/8/8/8/k6K w - - 0 1").expect("fen");
        // exd8=Q: e7 (file 4, rank 6) to d8 (file 3, rank 7).
        let code = 3 | (7 << 3) | (4 << 6) | (6 << 9) | (4 << 12);
        assert!(decode_move(&pos, code).is_some_and(Move::is_promotion));
        assert_eq!(decode_move(&pos, 0), None);
        assert_eq!(decode_move(&pos, 0xffff), None);
        assert_eq!(encode_move(Move::Null), None);
    }
}
