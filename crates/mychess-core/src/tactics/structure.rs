//! Pawn structure, king safety, game phase and material (D3268, D3269).

use shakmaty::{Color, File, Rank, Role, Square};

use super::attack::{material_value, value};
use crate::Position;

/// `T >= PHASE_OPENING_MIN` is the opening.
pub const PHASE_OPENING_MIN: i32 = 56;
/// `T <= PHASE_ENDGAME_MAX` is the endgame.
pub const PHASE_ENDGAME_MAX: i32 = 24;

/// The phase of the game, see [`phase`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamePhase {
    /// `T >= 56`.
    Opening,
    /// Everything in between.
    Middlegame,
    /// `T <= 24`.
    Endgame,
}

/// `T`: the sum of the values of queens, rooks, bishops and knights of both
/// sides (62 at the start).
pub fn non_pawn_material(pos: &Position) -> i32 {
    pos.pieces()
        .filter(|(_, p)| !matches!(p.role, Role::Pawn | Role::King))
        .map(|(_, p)| value(p.role))
        .sum()
}

/// The phase of the game from [`non_pawn_material`].
pub fn phase(pos: &Position) -> GamePhase {
    let t = non_pawn_material(pos);
    if t >= PHASE_OPENING_MIN {
        GamePhase::Opening
    } else if t <= PHASE_ENDGAME_MAX {
        GamePhase::Endgame
    } else {
        GamePhase::Middlegame
    }
}

/// White's material minus Black's (Q9 R5 B3 N3 P1, kings do not count).
pub fn material_balance(pos: &Position) -> i32 {
    pos.pieces()
        .map(|(_, p)| match p.color {
            Color::White => material_value(p.role),
            Color::Black => -material_value(p.role),
        })
        .sum()
}

fn pawns_of(pos: &Position, color: Color) -> Vec<Square> {
    pos.pieces()
        .filter(|(_, p)| p.role == Role::Pawn && p.color == color)
        .map(|(sq, _)| sq)
        .collect()
}

fn ahead(color: Color, rank: Rank, other: Rank) -> bool {
    match color {
        Color::White => other > rank,
        Color::Black => other < rank,
    }
}

fn adjacent(a: File, b: File) -> bool {
    (i32::from(a) - i32::from(b)).abs() <= 1
}

/// The passed pawns of `color`: no enemy pawn on the same or an adjacent
/// file in front of it.
pub fn passed_pawns(pos: &Position, color: Color) -> Vec<Square> {
    let enemy = pawns_of(pos, color.other());
    pawns_of(pos, color)
        .into_iter()
        .filter(|p| {
            !enemy
                .iter()
                .any(|e| adjacent(p.file(), e.file()) && ahead(color, p.rank(), e.rank()))
        })
        .collect()
}

/// The isolated pawns of `color`: no pawn of the same colour on an adjacent
/// file.
pub fn isolated_pawns(pos: &Position, color: Color) -> Vec<Square> {
    let own = pawns_of(pos, color);
    own.iter()
        .copied()
        .filter(|p| {
            !own.iter()
                .any(|o| o.file() != p.file() && adjacent(p.file(), o.file()))
        })
        .collect()
}

/// The doubled pawns of `color`: every pawn that shares its file with
/// another pawn of the same colour (both count).
pub fn doubled_pawns(pos: &Position, color: Color) -> Vec<Square> {
    let own = pawns_of(pos, color);
    own.iter()
        .copied()
        .filter(|p| own.iter().any(|o| o != p && o.file() == p.file()))
        .collect()
}

/// Whether `color` has two or more bishops, at least one on a light square
/// and one on a dark square.
pub fn bishop_pair(pos: &Position, color: Color) -> bool {
    let mut light = false;
    let mut dark = false;
    for (sq, p) in pos.pieces() {
        if p.role == Role::Bishop && p.color == color {
            if sq.is_light() {
                light = true;
            } else {
                dark = true;
            }
        }
    }
    light && dark
}

/// The pawn shield of the king of `color`: the pawns of `color` on the files
/// `f-1..=f+1` (those that exist) of the two ranks right in front of the
/// king. A king on its last rank has no ranks in front: 0. `0` also when
/// `color` has no king.
pub fn king_shield(pos: &Position, color: Color) -> u8 {
    let Some(king) = pos.king_square(color) else {
        return 0;
    };
    let pawns = pawns_of(pos, color);
    let step = match color {
        Color::White => 1,
        Color::Black => -1,
    };
    let rank = i32::from(king.rank());
    let count = pawns
        .iter()
        .filter(|p| {
            let dr = (i32::from(p.rank()) - rank) * step;
            adjacent(king.file(), p.file()) && (1..=2).contains(&dr)
        })
        .count();
    u8::try_from(count).unwrap_or(u8::MAX)
}

/// The king of `color` is exposed: the game is not in the endgame and its
/// [`king_shield`] is at most 1.
pub fn exposed_king(pos: &Position, color: Color) -> bool {
    pos.king_square(color).is_some()
        && phase(pos) != GamePhase::Endgame
        && king_shield(pos, color) <= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("test fen")
    }

    #[test]
    fn passed_pawns() {
        let p = pos("4k3/8/8/3P4/8/8/8/4K3 w - - 0 1");
        assert_eq!(super::passed_pawns(&p, Color::White), vec![Square::D5]);
        let q = pos("4k3/2p5/8/3P4/8/8/8/4K3 w - - 0 1");
        assert_eq!(super::passed_pawns(&q, Color::White), Vec::new());
        // The black pawn is passed (nothing in front of it on c, d, b).
        assert_eq!(super::passed_pawns(&q, Color::Black).len(), 0);
    }

    #[test]
    fn isolated_and_doubled() {
        let p = pos("4k3/8/8/8/3P4/3P4/P7/4K3 w - - 0 1");
        assert_eq!(isolated_pawns(&p, Color::White).len(), 3);
        assert_eq!(
            doubled_pawns(&p, Color::White),
            vec![Square::D3, Square::D4]
        );
        let q = pos("4k3/8/8/8/3P4/2P5/8/4K3 w - - 0 1");
        assert_eq!(isolated_pawns(&q, Color::White), Vec::new());
        assert_eq!(doubled_pawns(&q, Color::White), Vec::new());
    }

    #[test]
    fn bishop_pair_needs_both_colours() {
        let p = pos("4k3/8/8/8/8/8/8/2B1KB2 w - - 0 1");
        assert!(bishop_pair(&p, Color::White));
        let q = pos("4k3/8/8/8/8/8/8/2B1K1B1 w - - 0 1");
        assert!(!bishop_pair(&q, Color::White));
    }

    #[test]
    fn phase_thresholds() {
        assert_eq!(non_pawn_material(&Position::new()), 62);
        assert_eq!(phase(&Position::new()), GamePhase::Opening);
        assert_eq!(
            phase(&pos("4k3/8/8/8/8/8/8/4K2R w - - 0 1")),
            GamePhase::Endgame
        );
        assert_eq!(material_balance(&Position::new()), 0);
    }

    #[test]
    fn king_exposure() {
        // Middlegame (T = 2*(9+5+5+3+3) = 50): rooks, queens, minors.
        let thin = pos("r1bq1rk1/ppp2ppp/2n5/8/8/8/5P2/RNBQ1RK1 w - - 0 1");
        assert_eq!(king_shield(&thin, Color::White), 1);
        assert!(exposed_king(&thin, Color::White));
        assert!(!exposed_king(&thin, Color::Black));
        let full = pos("r1bq1rk1/ppp2ppp/2n5/8/8/8/5PPP/RNBQ1RK1 w - - 0 1");
        assert_eq!(king_shield(&full, Color::White), 3);
        assert!(!exposed_king(&full, Color::White));
        // Endgame: never exposed.
        let end2 = pos("4k3/8/8/8/8/8/5P2/6KR w - - 0 1");
        assert!(!exposed_king(&end2, Color::White));
    }
}
