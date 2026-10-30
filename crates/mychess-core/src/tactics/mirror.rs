//! Mirroring: swap the colours and flip the board vertically (D3259).

use shakmaty::{EnPassantMode, Position as _, Square};

use crate::position::chess_from_setup;
use crate::{Move, Position};

/// The mirror image of a square (rank 1 <-> 8).
pub fn mirror_square(sq: Square) -> Square {
    sq.flip_vertical()
}

/// The mirror image of a move (squares flipped; roles are colour-blind).
pub fn mirror_move(mv: Move) -> Move {
    match mv {
        Move::Null => Move::Null,
        Move::Normal {
            role,
            from,
            to,
            capture,
            promotion,
        } => Move::Normal {
            role,
            from: mirror_square(from),
            to: mirror_square(to),
            capture,
            promotion,
        },
        Move::EnPassant { from, to } => Move::EnPassant {
            from: mirror_square(from),
            to: mirror_square(to),
        },
        Move::Castle {
            king_from,
            rook_from,
        } => Move::Castle {
            king_from: mirror_square(king_from),
            rook_from: mirror_square(rook_from),
        },
    }
}

/// The position with colours swapped and the board flipped vertically: the
/// side to move, castling rights and en passant square follow. Mirroring
/// twice gives back the original position. `None` only if the result is not
/// a valid position (it never is for a valid input).
pub fn mirror(pos: &Position) -> Option<Position> {
    let mode = pos.castling_mode();
    let mut setup = pos.inner.to_setup(EnPassantMode::Always);
    setup.board.mirror();
    setup.turn = setup.turn.other();
    setup.castling_rights = setup.castling_rights.flip_vertical();
    setup.ep_square = setup.ep_square.map(Square::flip_vertical);
    let inner = chess_from_setup(setup, mode).ok()?;
    Some(Position { inner })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_position_mirrors_to_black_to_move_and_back() {
        let start = Position::new();
        let m = mirror(&start).expect("mirror");
        assert_eq!(m.turn(), shakmaty::Color::Black);
        assert_eq!(
            m.fen(),
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1"
        );
        assert_eq!(mirror(&m), Some(start));
    }

    #[test]
    fn mirror_is_an_involution_and_flips_the_turn() {
        let p = Position::from_fen("r3k2r/8/8/3pP3/8/8/8/R3K2R w KQkq d6 0 1").expect("fen");
        let m = mirror(&p).expect("mirror");
        assert_eq!(m.turn(), shakmaty::Color::Black);
        assert_eq!(m.fen(), "r3k2r/8/8/8/3Pp3/8/8/R3K2R b KQkq d3 0 1");
        assert_eq!(mirror(&m), Some(p));
    }

    #[test]
    fn mirrored_moves_are_legal_in_the_mirrored_position() {
        let p = Position::new();
        let m = mirror(&p).expect("mirror");
        for mv in p.legal_moves() {
            assert!(m.is_legal(mirror_move(mv)), "{mv:?}");
        }
    }
}
