//! The Polyglot Zobrist key of a position.

use mychess_core::{CastlingSide, Color, Position, Role};

use crate::random::{CASTLE_OFFSET, EN_PASSANT_OFFSET, RANDOM64, TURN_INDEX};

/// Index in [`RANDOM64`] of `value`, or 0 if somehow out of range (cannot
/// happen for indices built below, but indexing never panics).
fn random(index: usize) -> u64 {
    RANDOM64.get(index).copied().unwrap_or(0)
}

/// Polyglot piece kind: `2 * role + 1` for white, `2 * role` for black, with
/// roles ordered pawn, knight, bishop, rook, queen, king.
fn piece_kind(color: Color, role: Role) -> usize {
    let role_index = match role {
        Role::Pawn => 0,
        Role::Knight => 1,
        Role::Bishop => 2,
        Role::Rook => 3,
        Role::Queen => 4,
        Role::King => 5,
    };
    2 * role_index + usize::from(color == Color::White)
}

/// The Polyglot key of `pos` (spec D1103).
///
/// The en passant file only counts when a pawn of the side to move is next to
/// the pawn that advanced two squares; pins are not checked. The key does not
/// look at the castling mode: the caller decides whether the book applies.
///
/// ```
/// use mychess_core::Position;
/// assert_eq!(mychess_book::polyglot_key(&Position::new()), 0x463b_9618_1691_fc9c);
/// ```
pub fn polyglot_key(pos: &Position) -> u64 {
    let mut key = 0u64;
    for (sq, piece) in pos.pieces() {
        let offset = 64 * piece_kind(piece.color, piece.role)
            + 8 * usize::from(sq.rank())
            + usize::from(sq.file());
        key ^= random(offset);
    }
    let rights = [
        (Color::White, CastlingSide::KingSide),
        (Color::White, CastlingSide::QueenSide),
        (Color::Black, CastlingSide::KingSide),
        (Color::Black, CastlingSide::QueenSide),
    ];
    for (i, (color, side)) in rights.into_iter().enumerate() {
        if pos.has_castling_right(color, side) {
            key ^= random(CASTLE_OFFSET + i);
        }
    }
    if let Some(ep) = pos.ep_square_pseudo_legal() {
        key ^= random(EN_PASSANT_OFFSET + usize::from(ep.file()));
    }
    if pos.turn() == Color::White {
        key ^= random(TURN_INDEX);
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(fen: &str) -> u64 {
        polyglot_key(&Position::from_fen(fen).expect("valid FEN"))
    }

    #[test]
    fn the_nine_reference_vectors() {
        let cases = [
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                0x463b_9618_1691_fc9c,
            ),
            (
                "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
                0x823c_9b50_fd11_4196,
            ),
            (
                "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2",
                0x0756_b944_61c5_0fb0,
            ),
            (
                "rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR b KQkq - 0 2",
                0x662f_afb9_65db_29d4,
            ),
            (
                "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
                0x22a4_8b5a_8e47_ff78,
            ),
            (
                "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPPKPPP/RNBQ1BNR b kq - 0 3",
                0x652a_607c_a3f2_42c1,
            ),
            (
                "rnbq1bnr/ppp1pkpp/8/3pPp2/8/8/PPPPKPPP/RNBQ1BNR w - - 0 4",
                0x00fd_d303_c946_bdd9,
            ),
            (
                "rnbqkbnr/p1pppppp/8/8/PpP4P/8/1P1PPPP1/RNBQKBNR b KQkq c3 0 3",
                0x3c81_23ea_7b06_7637,
            ),
            (
                "rnbqkbnr/p1pppppp/8/8/P6P/R1p5/1P1PPPP1/1NBQKBNR b Kkq - 0 4",
                0x5c3f_9b82_9b27_9560,
            ),
        ];
        for (fen, expected) in cases {
            assert_eq!(key(fen), expected, "{fen}");
        }
    }

    #[test]
    fn negative_vector_en_passant_without_adjacent_pawn_is_not_hashed() {
        // Hashing e3 here (the wrong rule) would give 4d0dde8ef7cc031f.
        let k = key("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1");
        assert_ne!(k, 0x4d0d_de8e_f7cc_031f);
    }

    #[test]
    fn pinned_capturer_still_hashes_the_en_passant_file() {
        // The d5 pawn could capture e6 pseudo-legally but is pinned: the
        // Polyglot rule still counts the file, unlike a legality check.
        let with_ep = key("4k3/8/8/K2pP2r/8/8/8/8 w - d6 0 1");
        let without = key("4k3/8/8/K2pP2r/8/8/8/8 w - - 0 1");
        assert_ne!(with_ep, without);
    }
}
