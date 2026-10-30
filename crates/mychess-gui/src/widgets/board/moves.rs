//! Pure move helpers for the board: legal destinations and drop resolution.
//!
//! The GUI never validates moves for the game: it proposes a [`Move`] that is
//! legal in the drawn position and the application re-checks it. Scid:
//! `sc_pos isLegal` (`src/tkscid.cpp`) and `addMove` (`tcl/main.tcl`).

use mychess_core::{Color, Move, Position, Role, Square};

/// A square a selected piece can go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Destination {
    /// The square to show (for castling, the king's final square).
    pub square: Square,
    /// Whether the move captures (draws a ring instead of a dot).
    pub capture: bool,
}

/// What dropping the piece of `from` on `to` means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropResolution {
    /// A legal move that is not a promotion.
    Move(Move),
    /// A promotion: the four candidate moves (queen, rook, bishop, knight).
    NeedsPromotion {
        /// The candidates, one per piece.
        candidates: Vec<Move>,
    },
    /// King dropped on the opposing king: the null move (Scid).
    NullMove,
    /// Nothing can be played.
    Illegal,
}

/// Whether `sq` holds a piece of the side to move.
pub fn is_own_piece(position: &Position, sq: Square) -> bool {
    position
        .piece_at(sq)
        .is_some_and(|piece| piece.color == position.turn())
}

/// The legal destinations of the piece on `from`, without duplicates
/// (a promotion has four moves but one destination).
pub fn legal_destinations(position: &Position, from: Square) -> Vec<Destination> {
    let mut out: Vec<Destination> = Vec::new();
    for m in position.legal_moves_from(from) {
        let Some(square) = m.to() else { continue };
        // Chess960: a king that castles without moving "targets" its own square.
        if square == from {
            continue;
        }
        if !out.iter().any(|d| d.square == square) {
            out.push(Destination {
                square,
                capture: m.is_capture(),
            });
        }
    }
    out
}

/// Resolves a drop (Scid `main.tcl:1318-1330` for the null move).
pub fn resolve_drop(position: &Position, from: Square, to: Square) -> DropResolution {
    if from == to {
        return DropResolution::Illegal;
    }
    let promotions: Vec<Move> = position
        .legal_moves_from(from)
        .into_iter()
        .filter(|m| m.is_promotion() && m.to() == Some(to))
        .collect();
    if !promotions.is_empty() {
        return DropResolution::NeedsPromotion {
            candidates: promotions,
        };
    }
    if let Some(m) = position.find_move(from, to, None) {
        return DropResolution::Move(m);
    }
    if is_king_takes_king(position, from, to) {
        return DropResolution::NullMove;
    }
    DropResolution::Illegal
}

fn is_king_takes_king(position: &Position, from: Square, to: Square) -> bool {
    let side: Color = position.turn();
    let own_king = position
        .piece_at(from)
        .is_some_and(|p| p.role == Role::King && p.color == side);
    let enemy_king = position
        .piece_at(to)
        .is_some_and(|p| p.role == Role::King && p.color != side);
    own_king && enemy_king
}

/// The promotion pieces in the order of the promotion column.
pub const PROMOTION_ROLES: [Role; 4] = [Role::Queen, Role::Rook, Role::Bishop, Role::Knight];

#[cfg(test)]
pub(crate) mod tests {
    use mychess_core::Position;

    use super::*;

    pub(crate) fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("valid test FEN")
    }

    fn sq(name: &str) -> Square {
        name.parse().expect("valid square")
    }

    #[test]
    fn plain_move_resolves_to_the_move() {
        let p = Position::new();
        assert!(matches!(
            resolve_drop(&p, sq("e2"), sq("e4")),
            DropResolution::Move(m) if m.to() == Some(sq("e4"))
        ));
    }

    #[test]
    fn promotion_with_and_without_capture_has_four_candidates() {
        let p = pos("1n2k3/P7/8/8/8/8/8/4K3 w - - 0 1");
        for to in ["a8", "b8"] {
            match resolve_drop(&p, sq("a7"), sq(to)) {
                DropResolution::NeedsPromotion { candidates } => assert_eq!(candidates.len(), 4),
                other => panic!("expected promotion, got {other:?}"),
            }
        }
    }

    #[test]
    fn castling_resolves_by_king_destination_and_by_rook_square() {
        let p = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        for to in ["g1", "h1"] {
            assert!(matches!(
                resolve_drop(&p, sq("e1"), sq(to)),
                DropResolution::Move(m) if m.is_castle()
            ));
        }
        for to in ["c1", "a1"] {
            assert!(matches!(
                resolve_drop(&p, sq("e1"), sq(to)),
                DropResolution::Move(m) if m.is_castle()
            ));
        }
    }

    #[test]
    fn chess960_king_on_own_rook_castles() {
        let p = pos("4k3/8/8/8/8/8/8/RK5R w HA - 0 1");
        for to in ["h1", "a1"] {
            assert!(matches!(
                resolve_drop(&p, sq("b1"), sq(to)),
                DropResolution::Move(m) if m.is_castle()
            ));
        }
    }

    /// M9-6: the king that is already on its final square castles by
    /// "king takes own rook"; the rook square is never a plain move.
    #[test]
    fn chess960_king_already_on_its_final_square_castles_on_the_rook() {
        let p = pos("4k3/8/8/8/8/8/8/6KR w H - 0 1");
        assert!(matches!(
            resolve_drop(&p, sq("g1"), sq("h1")),
            DropResolution::Move(m) if m.is_castle()
        ));
        // The king stays where it is: no destination to draw for it.
        assert!(
            legal_destinations(&p, sq("g1"))
                .iter()
                .all(|d| d.square != sq("g1"))
        );
    }

    /// The rook stands on the king's final square: only the rook gesture
    /// (or the king's destination, which is the same square) castles.
    #[test]
    fn chess960_rook_on_the_king_destination_castles() {
        let p = pos("4k3/8/8/8/8/8/8/4K1R1 w G - 0 1");
        match resolve_drop(&p, sq("e1"), sq("g1")) {
            DropResolution::Move(m) => {
                assert!(m.is_castle());
                assert_eq!(
                    m.castling_side(),
                    Some(mychess_core::CastlingSide::KingSide)
                );
            }
            other => panic!("expected castling, got {other:?}"),
        }
    }

    /// Ambiguity: `Kc1` is an ordinary step and also the destination of the
    /// queen-side castling. The step wins; the rook square castles.
    #[test]
    fn chess960_ambiguous_king_destination_prefers_the_ordinary_move() {
        let p = pos("4k3/8/8/8/8/8/8/RK6 w A - 0 1");
        assert!(matches!(
            resolve_drop(&p, sq("b1"), sq("c1")),
            DropResolution::Move(m) if !m.is_castle()
        ));
        assert!(matches!(
            resolve_drop(&p, sq("b1"), sq("a1")),
            DropResolution::Move(m) if m.is_castle()
        ));
    }

    #[test]
    fn chess960_castling_through_an_attacked_square_is_refused() {
        // The black rook on f8 attacks f1, which the king crosses.
        let p = pos("4kr2/8/8/8/8/8/8/4K1R1 w G - 0 1");
        assert_eq!(
            resolve_drop(&p, sq("e1"), sq("g1")),
            DropResolution::Illegal
        );
        assert_eq!(
            resolve_drop(&p, sq("e1"), sq("f1")),
            DropResolution::Illegal
        );
    }

    #[test]
    fn en_passant_resolves() {
        let p = pos("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2");
        assert!(matches!(
            resolve_drop(&p, sq("e5"), sq("d6")),
            DropResolution::Move(m) if m.is_en_passant()
        ));
    }

    #[test]
    fn king_on_enemy_king_is_the_null_move() {
        let p = pos("8/8/8/3k4/8/8/8/K7 w - - 0 1");
        assert_eq!(
            resolve_drop(&p, sq("a1"), sq("d5")),
            DropResolution::NullMove
        );
    }

    #[test]
    fn illegal_drops_are_rejected() {
        let p = Position::new();
        assert_eq!(
            resolve_drop(&p, sq("e2"), sq("e5")),
            DropResolution::Illegal
        );
        assert_eq!(
            resolve_drop(&p, sq("e7"), sq("e5")),
            DropResolution::Illegal
        );
        assert_eq!(
            resolve_drop(&p, sq("e4"), sq("e5")),
            DropResolution::Illegal
        );
        assert_eq!(
            resolve_drop(&p, sq("e2"), sq("e2")),
            DropResolution::Illegal
        );
    }

    #[test]
    fn pinned_piece_cannot_move_off_the_pin_line() {
        let p = pos("4r1k1/8/8/8/8/8/4B3/4K3 w - - 0 1");
        assert_eq!(
            resolve_drop(&p, sq("e2"), sq("d3")),
            DropResolution::Illegal
        );
    }

    #[test]
    fn destinations_match_legal_moves_and_mark_captures() {
        let p = pos("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1");
        let dests = legal_destinations(&p, sq("e4"));
        assert_eq!(dests.len(), 2);
        assert!(dests.contains(&Destination {
            square: sq("e5"),
            capture: false
        }));
        assert!(dests.contains(&Destination {
            square: sq("d5"),
            capture: true
        }));
    }

    #[test]
    fn promotion_destination_is_listed_once() {
        let p = pos("4k3/P7/8/8/8/8/8/4K3 w - - 0 1");
        assert_eq!(legal_destinations(&p, sq("a7")).len(), 1);
    }

    #[test]
    fn own_piece_follows_the_side_to_move() {
        let p = Position::new();
        assert!(is_own_piece(&p, sq("e2")));
        assert!(!is_own_piece(&p, sq("e7")));
        assert!(!is_own_piece(&p, sq("e4")));
    }
}
