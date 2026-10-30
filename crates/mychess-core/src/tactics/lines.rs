//! Pins and skewers (D3261, D3262).

use shakmaty::{Bitboard, Color, Role, Square, attacks as sattacks};

use super::attack::{board_of, defender_squares, value};
use crate::Position;

/// A pin: `pinner` attacks `pinned`, and behind it on the same line stands
/// `behind` (the king for an absolute pin).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pin {
    /// The sliding piece that pins.
    pub pinner: Square,
    /// The piece that cannot (or should not) move.
    pub pinned: Square,
    /// The piece behind it: the king, or a more valuable or undefended piece.
    pub behind: Square,
}

/// A skewer: `attacker` attacks `front`, which must move and exposes `back`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Skewer {
    /// The sliding piece.
    pub attacker: Square,
    /// The piece in front (the king, or more valuable than `back`).
    pub front: Square,
    /// The piece behind (never the king) that is worth winning.
    pub back: Square,
}

fn is_slider(role: Role) -> bool {
    matches!(role, Role::Bishop | Role::Rook | Role::Queen)
}

/// For each sliding piece of `by`, the first enemy piece it hits (`front`)
/// and the first piece behind it on the same line (`back`, any colour).
fn lined_up(pos: &Position, by: Color) -> Vec<(Square, Square, Square)> {
    let board = board_of(pos);
    let occupied = board.occupied();
    let mut out = Vec::new();
    for slider in board.by_color(by) {
        let Some(piece) = board.piece_at(slider) else {
            continue;
        };
        if !is_slider(piece.role) {
            continue;
        }
        let hit = sattacks::attacks(slider, piece, occupied) & board.by_color(by.other());
        for front in hit {
            let xray = sattacks::attacks(slider, piece, occupied & !Bitboard::from(front));
            let line = sattacks::ray(slider, front);
            for back in xray & line & occupied {
                if back != front && sattacks::between(slider, back).contains(front) {
                    out.push((slider, front, back));
                }
            }
        }
    }
    out
}

/// All pins by the sliding pieces of `by`: the pinned piece (never the king)
/// and the piece behind it are of the other colour, nothing stands between
/// the three. The pin is **absolute** if `behind` is the king and
/// **relative** if `behind` is worth more than the pinned piece or is
/// undefended.
pub fn pins(pos: &Position, by: Color) -> Vec<Pin> {
    let board = board_of(pos);
    let mut out = Vec::new();
    for (pinner, pinned, behind) in lined_up(pos, by) {
        let (Some(front), Some(back)) = (board.piece_at(pinned), board.piece_at(behind)) else {
            continue;
        };
        if front.role == Role::King || back.color != front.color {
            continue;
        }
        let absolute = back.role == Role::King;
        let relative = value(back.role) > value(front.role)
            || defender_squares(board, board.occupied(), behind, back.color).is_empty();
        if absolute || relative {
            out.push(Pin {
                pinner,
                pinned,
                behind,
            });
        }
    }
    out
}

/// All skewers by the sliding pieces of `by`. `front` is the king or worth
/// more than `back`; `back` (never the king) is worth taking: undefended
/// (with `front` off the board) or worth more than the attacker. A line-up in
/// which `front` is not worth more than `back` is a relative pin, not a
/// skewer.
pub fn skewers(pos: &Position, by: Color) -> Vec<Skewer> {
    let board = board_of(pos);
    let mut out = Vec::new();
    for (attacker, front, back) in lined_up(pos, by) {
        let (Some(a), Some(f), Some(b)) = (
            board.piece_at(attacker),
            board.piece_at(front),
            board.piece_at(back),
        ) else {
            continue;
        };
        if b.role == Role::King || b.color != f.color {
            continue;
        }
        if !(f.role == Role::King || value(f.role) > value(b.role)) {
            continue;
        }
        let without_front = board.occupied() & !Bitboard::from(front);
        let worth = value(b.role) > value(a.role)
            || defender_squares(board, without_front, back, b.color).is_empty();
        if worth {
            out.push(Skewer {
                attacker,
                front,
                back,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("test fen")
    }

    #[test]
    fn absolute_pin() {
        let p = pos("4k3/8/8/4n3/8/8/8/4RK2 w - - 0 1");
        assert_eq!(
            pins(&p, Color::White),
            vec![Pin {
                pinner: Square::E1,
                pinned: Square::E5,
                behind: Square::E8
            }]
        );
        assert_eq!(pins(&p, Color::Black), Vec::new());
    }

    #[test]
    fn relative_pin_needs_a_more_valuable_or_loose_piece_behind() {
        // Bg5 pins the knight f6 to the queen d8 (9 > 3).
        let p = pos("3qk3/8/5n2/6B1/8/8/8/4K3 w - - 0 1");
        let found = pins(&p, Color::White);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].behind, Square::D8);
        // Rook in front of an equal, defended rook: no pin, no skewer.
        let r = pos("rk6/8/8/8/r7/8/8/R3K3 w - - 0 1");
        assert_eq!(pins(&r, Color::White), Vec::new());
        assert_eq!(skewers(&r, Color::White), Vec::new());
    }

    #[test]
    fn skewer_king_then_queen() {
        let p = pos("R3k2q/8/8/8/8/8/8/4K3 b - - 0 1");
        assert_eq!(
            skewers(&p, Color::White),
            vec![Skewer {
                attacker: Square::A8,
                front: Square::E8,
                back: Square::H8
            }]
        );
        // Not a skewer if the back piece is cheap and defended.
        let q = pos("R3k2n/6b1/8/8/8/8/8/4K3 b - - 0 1");
        assert_eq!(skewers(&q, Color::White), Vec::new());
    }

    #[test]
    fn a_valuable_front_piece_in_front_of_a_cheaper_one_is_a_skewer_not_a_pin() {
        // Rook a1 attacks the queen a4, the rook a8 is behind (loose).
        let p = pos("r3k3/8/8/8/q7/8/8/R3K3 w - - 0 1");
        let s = skewers(&p, Color::White);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].front, Square::A4);
        assert_eq!(pins(&p, Color::White), Vec::new());
    }
}
