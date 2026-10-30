//! Discovered attacks, discovered checks and double checks (D3265).

use shakmaty::{Bitboard, Color, Role, Square, attacks as sattacks};

use super::attack::{board_of, worth};
use crate::{Move, Position};

/// A discovered attack: the move opened the line from `slider` to `target`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiscoveredAttack {
    /// The sliding piece (not the one that moved) whose line was opened.
    pub slider: Square,
    /// The enemy piece now attacked (the king for a discovered check).
    pub target: Square,
    /// The piece that moved.
    pub moved: Role,
}

/// The pieces that give check to the side to move in `pos`.
pub fn checkers(pos: &Position) -> Vec<Square> {
    let board = board_of(pos);
    let turn = pos.turn();
    match board.king_of(turn) {
        Some(king) => board
            .attacks_to(king, turn.other(), board.occupied())
            .into_iter()
            .collect(),
        None => Vec::new(),
    }
}

/// The attacks discovered by `mv`, played from `prev` and giving `cur`: a
/// sliding piece of the mover, **other than the piece that moved**, now sees
/// an enemy piece it did not see before because `mv` left the line (the
/// line was blocked only by the origin square of `mv` in `prev` and is free
/// in `cur`), and the attack is worth it ([`worth`]). The null move and
/// castling by a piece that opens no line give an empty list.
pub fn discovered_attacks(prev: &Position, mv: Move, cur: &Position) -> Vec<DiscoveredAttack> {
    let (Some(from), Some(moved)) = (mv.from(), mv.role()) else {
        return Vec::new();
    };
    let mover: Color = prev.turn();
    let (bp, bc) = (board_of(prev), board_of(cur));
    let (occ_prev, occ_cur) = (bp.occupied(), bc.occupied());
    let mut out = Vec::new();
    for slider in bc.by_color(mover) {
        if slider == from || mv.to() == Some(slider) {
            continue;
        }
        let Some(piece) = bc.piece_at(slider) else {
            continue;
        };
        // The same slider must have been there before the move.
        if bp.piece_at(slider) != Some(piece)
            || !matches!(piece.role, Role::Bishop | Role::Rook | Role::Queen)
        {
            continue;
        }
        for target in sattacks::attacks(slider, piece, occ_cur) & bc.by_color(mover.other()) {
            let between = sattacks::between(slider, target);
            let blocked_only_by_from = (between & occ_prev) == Bitboard::from(from);
            if blocked_only_by_from && (between & occ_cur).is_empty() && worth(cur, slider, target)
            {
                out.push(DiscoveredAttack {
                    slider,
                    target,
                    moved,
                });
            }
        }
    }
    out
}

/// Whether `mv` (played from `prev`, giving `cur`) gives a discovered check:
/// a discovered attack whose target is the enemy king.
pub fn discovered_check(prev: &Position, mv: Move, cur: &Position) -> bool {
    let king = board_of(cur).king_of(prev.turn().other());
    king.is_some_and(|k| {
        discovered_attacks(prev, mv, cur)
            .iter()
            .any(|d| d.target == k)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("test fen")
    }

    #[test]
    fn bishop_move_discovers_a_check_from_the_rook() {
        let prev = pos("4k3/7p/8/8/4B3/8/8/4RK2 w - - 0 1");
        let mv = prev.parse_uci("e4h7").expect("legal");
        let cur = prev.after(mv).expect("play");
        let found = discovered_attacks(&prev, mv, &cur);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].slider, Square::E1);
        assert_eq!(found[0].target, Square::E8);
        assert!(discovered_check(&prev, mv, &cur));
        // Before the move the rook did not attack e8.
        assert!(!super::super::attack::attacks(
            &prev,
            Square::E1,
            Square::E8
        ));
    }

    #[test]
    fn the_piece_that_moves_is_not_a_discoverer() {
        // A direct check by the moving rook is not discovered.
        let prev = pos("4k3/8/8/8/8/8/R7/4K3 w - - 0 1");
        let mv = prev.parse_uci("a2a8").expect("legal");
        let cur = prev.after(mv).expect("play");
        assert_eq!(discovered_attacks(&prev, mv, &cur), Vec::new());
        assert!(!discovered_check(&prev, mv, &cur));
    }

    #[test]
    fn a_double_check_has_two_checkers() {
        // Bb5 checks along b5-e8 (c6, d7 empty); Re1 checks along the e-file.
        let d = pos("4k3/8/8/1B6/8/8/8/4RK2 b - - 0 1");
        assert_eq!(checkers(&d).len(), 2);
        assert_eq!(checkers(&Position::new()).len(), 0);
    }
}
