//! Values, attacks, defence and static exchange evaluation (D3260).

use shakmaty::{Bitboard, Board, Color, Position as _, Role, Square, attacks as sattacks};

use crate::Position;

/// The value of the king, used only to order captures. "Material" never
/// counts the king (see [`material_value`]).
pub const KING_VALUE: i32 = 1000;

/// The value of a piece for exchanges: P=1, N=3, B=3, R=5, Q=9 and
/// [`KING_VALUE`] for the king.
pub const fn value(role: Role) -> i32 {
    match role {
        Role::Pawn => 1,
        Role::Knight | Role::Bishop => 3,
        Role::Rook => 5,
        Role::Queen => 9,
        Role::King => KING_VALUE,
    }
}

/// The value of a piece for material counts: like [`value`], but the king is
/// worth 0.
pub const fn material_value(role: Role) -> i32 {
    match role {
        Role::King => 0,
        other => value(other),
    }
}

pub(crate) fn board_of(pos: &Position) -> &Board {
    pos.inner.board()
}

/// Whether the piece on `from` reaches `to` with a capturing move of that
/// piece (pawns: the diagonal in front; sliders stop at the first piece).
/// Legality is not checked: a pinned piece still "attacks". En passant does
/// not count. `false` when `from` is empty.
pub fn attacks(pos: &Position, from: Square, to: Square) -> bool {
    let board = board_of(pos);
    board.attacks_from(from).contains(to)
}

/// The squares a piece of `color` pinned against its own king may still move
/// to (the line between the king and the pinner, the pinner included), or
/// `None` when the piece on `sq` is not pinned. `occupied` is the occupancy
/// used for the geometry; pieces outside it are ignored.
pub(crate) fn pin_ray(
    board: &Board,
    occupied: Bitboard,
    sq: Square,
    color: Color,
) -> Option<Bitboard> {
    let king = (board.by_piece(Role::King.of(color)) & occupied).first()?;
    if king == sq {
        return None;
    }
    let enemy = board.by_color(color.other()) & occupied;
    let straight =
        sattacks::rook_attacks(king, Bitboard::EMPTY) & (board.rooks_and_queens() & enemy);
    let diagonal =
        sattacks::bishop_attacks(king, Bitboard::EMPTY) & (board.bishops_and_queens() & enemy);
    for pinner in straight | diagonal {
        let between = sattacks::between(king, pinner);
        if (between & occupied) == Bitboard::from(sq) {
            return Some(between | Bitboard::from(pinner));
        }
    }
    None
}

/// Whether the piece on `from` could really capture on `to`: it attacks the
/// square and (a) if it is pinned against its own king, `to` is on the line
/// of the pin; (b) if it is the king, `to` is not guarded by the other side
/// (with the king lifted off the board, so it cannot hide behind itself).
/// `false` when `from` is empty. The check status of the position is not
/// considered.
pub fn legal_attack(pos: &Position, from: Square, to: Square) -> bool {
    let board = board_of(pos);
    let Some(piece) = board.piece_at(from) else {
        return false;
    };
    if !board.attacks_from(from).contains(to) {
        return false;
    }
    let occupied = board.occupied();
    if piece.role == Role::King {
        let lifted = occupied & !Bitboard::from(from);
        return attackers_of(board, to, piece.color.other(), lifted).is_empty();
    }
    match pin_ray(board, occupied, from, piece.color) {
        Some(ray) => ray.contains(to),
        None => true,
    }
}

/// Pieces of `color` that attack `sq` geometrically, restricted to
/// `occupied` (so removed pieces neither attack nor block).
pub(crate) fn attackers_of(
    board: &Board,
    sq: Square,
    color: Color,
    occupied: Bitboard,
) -> Bitboard {
    board.attacks_to(sq, color, occupied) & occupied
}

/// The pieces that defend the piece on `sq`: same colour, geometrically
/// attacking `sq` as if an enemy stood there. A defender pinned against its
/// own king counts only if `sq` is on the line of the pin; the king counts
/// only if, whichever enemy piece captured on `sq` first, nobody is left to
/// attack `sq` after that piece is gone. Empty when `sq` is empty.
pub fn defenders(pos: &Position, sq: Square) -> Vec<Square> {
    let board = board_of(pos);
    match board.piece_at(sq) {
        Some(piece) => defender_squares(board, board.occupied(), sq, piece.color),
        None => Vec::new(),
    }
}

/// [`defenders`] for an explicit occupancy.
pub(crate) fn defender_squares(
    board: &Board,
    occupied: Bitboard,
    sq: Square,
    color: Color,
) -> Vec<Square> {
    let mut out = Vec::new();
    for d in attackers_of(board, sq, color, occupied) {
        let is_king = board.king_of(color) == Some(d);
        if is_king {
            let enemies = attackers_of(board, sq, color.other(), occupied);
            let safe_recapture = enemies.into_iter().all(|x| {
                let without = occupied & !Bitboard::from(x);
                attackers_of(board, sq, color.other(), without).is_empty()
            });
            if safe_recapture {
                out.push(d);
            }
        } else if pin_ray(board, occupied, d, color).is_none_or(|ray| ray.contains(sq)) {
            out.push(d);
        }
    }
    out
}

/// Static exchange evaluation of the capture `from` x `to`, from the point
/// of view of the side that captures first: the material gain (positive) or
/// loss of the best sequence of captures on `to`, where each side picks its
/// cheapest attacker, uncovers x-rays as pieces leave, and may stop when
/// going on loses. The king captures only when the other side has no more
/// attackers. Pins after the first capture are ignored. Returns 0 when `from`
/// is empty or `to` does not hold an enemy piece.
pub fn see(pos: &Position, from: Square, to: Square) -> i32 {
    let board = board_of(pos);
    let (Some(attacker), Some(victim)) = (board.piece_at(from), board.piece_at(to)) else {
        return 0;
    };
    if attacker.color == victim.color {
        return 0;
    }
    let mut gain = [0i32; 40];
    gain[0] = value(victim.role);
    let mut occupied = board.occupied() & !Bitboard::from(from);
    let mut on_target = value(attacker.role);
    let mut side = attacker.color.other();
    let mut depth = 0usize;
    while depth + 1 < gain.len() {
        let candidates = attackers_of(board, to, side, occupied);
        let Some((sq, role)) = cheapest(board, candidates) else {
            break;
        };
        let after = occupied & !Bitboard::from(sq);
        if role == Role::King && !attackers_of(board, to, side.other(), after).is_empty() {
            break;
        }
        depth += 1;
        gain[depth] = on_target - gain[depth - 1];
        on_target = value(role);
        occupied = after;
        side = side.other();
    }
    while depth > 0 {
        gain[depth - 1] = -((-gain[depth - 1]).max(gain[depth]));
        depth -= 1;
    }
    gain[0]
}

/// The cheapest piece of `set` (ties: the lowest square).
fn cheapest(board: &Board, set: Bitboard) -> Option<(Square, Role)> {
    set.into_iter()
        .filter_map(|sq| board.role_at(sq).map(|r| (sq, r)))
        .min_by_key(|(sq, r)| (value(*r), u32::from(*sq)))
}

/// Whether attacking the piece on `to` from `from` is worth it: the target
/// is the king (a check), or the exchange [`see`] is positive. `false` when
/// `to` does not hold an enemy piece.
pub fn worth(pos: &Position, from: Square, to: Square) -> bool {
    let board = board_of(pos);
    let (Some(attacker), Some(victim)) = (board.piece_at(from), board.piece_at(to)) else {
        return false;
    };
    if attacker.color == victim.color {
        return false;
    }
    victim.role == Role::King || see(pos, from, to) > 0
}

/// Whether the piece on `sq` is safe: no enemy piece can capture it (legally,
/// see [`legal_attack`]) with an exchange that does not lose material
/// (`see >= 0`). An empty square is trivially safe.
pub fn safe(pos: &Position, sq: Square) -> bool {
    let board = board_of(pos);
    let Some(piece) = board.piece_at(sq) else {
        return true;
    };
    !attackers_of(board, sq, piece.color.other(), board.occupied())
        .into_iter()
        .any(|a| legal_attack(pos, a, sq) && see(pos, a, sq) >= 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("test fen")
    }

    #[test]
    fn values() {
        assert_eq!(value(Role::Queen), 9);
        assert_eq!(value(Role::King), KING_VALUE);
        assert_eq!(material_value(Role::King), 0);
        assert_eq!(material_value(Role::Bishop), 3);
    }

    #[test]
    fn geometric_attack_ignores_pins_but_legal_attack_does_not() {
        // The black knight on e5 is pinned by Re1: it "attacks" c4, cannot take.
        let p = pos("4k3/8/8/4n3/2B5/8/8/4RK2 b - - 0 1");
        assert!(attacks(&p, Square::E5, Square::C4));
        assert!(!legal_attack(&p, Square::E5, Square::C4));
        // A rook pinned along the file may still take along the file.
        let q = pos("4r1k1/8/8/8/8/8/4R3/4K3 w - - 0 1");
        assert!(legal_attack(&q, Square::E2, Square::E8));
        assert!(!legal_attack(&q, Square::E2, Square::A2));
    }

    #[test]
    fn the_king_cannot_attack_a_guarded_square() {
        let p = pos("8/8/8/8/8/2k5/1r6/K7 w - - 0 1");
        assert!(attacks(&p, Square::A1, Square::B2));
        // b2 holds a rook defended by the king: Kxb2 is illegal.
        assert!(!legal_attack(&p, Square::A1, Square::B2));
        // X-ray: the king lifted off the board does not shield f1 from the rook.
        let q = pos("4k3/8/8/8/8/8/8/r3K3 w - - 0 1");
        assert!(!legal_attack(&q, Square::E1, Square::F1));
        assert!(legal_attack(&q, Square::E1, Square::E2));
    }

    #[test]
    fn the_king_defends_only_if_the_recapture_is_safe() {
        // One rook attacks d5: after it captures nobody is left, the king defends.
        let p = pos("8/8/4k3/3p4/8/8/3R4/4K3 w - - 0 1");
        assert!(defenders(&p, Square::D5).contains(&Square::E6));
        // Two rooks on the file: whichever captures first, the other is left.
        let q = pos("8/8/4k3/3p4/8/8/3R4/3RK3 w - - 0 1");
        assert!(!defenders(&q, Square::D5).contains(&Square::E6));
        assert_eq!(defenders(&q, Square::D4), Vec::new());
    }

    #[test]
    fn a_pinned_defender_does_not_defend_off_its_line() {
        // Black bishop d6 defends c5? It is pinned by Re1? No: knight e7 is pinned
        // on the e-file by Re1 and cannot defend c6 / g6.
        let p = pos("4k3/4n3/2p5/8/8/8/8/4RK2 b - - 0 1");
        assert!(!defenders(&p, Square::C6).contains(&Square::E7));
        assert!(attacks(&p, Square::E7, Square::C6));
    }

    #[test]
    fn see_basics() {
        // Undefended pawn: +1.
        let p = pos("4k3/8/8/3p4/8/8/3R4/4K3 w - - 0 1");
        assert_eq!(see(&p, Square::D2, Square::D5), 1);
        // Rook takes a pawn defended by a pawn: 1 - 5 = -4.
        let q = pos("4k3/8/2p5/3p4/8/8/3R4/4K3 w - - 0 1");
        assert_eq!(see(&q, Square::D2, Square::D5), -4);
        assert!(!worth(&q, Square::D2, Square::D5));
        // Empty target: 0.
        assert_eq!(see(&q, Square::D2, Square::D4), 0);
    }

    #[test]
    fn see_uncovers_xrays() {
        // Rd2 and Rd1 vs a pawn defended by one rook on d8: Rxd5 Rxd5 Rxd5 wins a pawn.
        let p = pos("3r2k1/8/8/3p4/8/8/3R4/3RK3 w - - 0 1");
        assert_eq!(see(&p, Square::D2, Square::D5), 1);
    }

    #[test]
    fn worth_counts_the_king_and_safe_requires_no_even_capture() {
        let p = pos("r3k3/2N5/8/8/8/8/8/4K3 b - - 1 1");
        assert!(worth(&p, Square::C7, Square::E8));
        assert!(worth(&p, Square::C7, Square::A8));
        assert!(safe(&p, Square::C7));
        // An equal trade is not safe.
        let q = pos("4k3/8/8/3n4/8/2N5/8/4K3 w - - 0 1");
        assert!(!safe(&q, Square::C3));
        // A pawn defended by a pawn against a knight is safe (1 - 3 < 0).
        let r = pos("4k3/8/8/3n4/8/2P5/1P6/4K3 w - - 0 1");
        assert!(safe(&r, Square::C3));
    }
}
