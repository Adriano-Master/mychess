//! Forks and double attacks (D3263, D3264).

use shakmaty::{Color, Square};

use super::attack::{board_of, legal_attack, safe, worth};
use crate::Position;

/// A fork: one piece attacking two or more enemy pieces worth attacking,
/// without being capturable at a profit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fork {
    /// The forking piece.
    pub forker: Square,
    /// The enemy pieces it attacks (the king counts, as a check), in square
    /// order.
    pub victims: Vec<Square>,
}

/// The enemy pieces the piece on `forker` attacks legally ([`legal_attack`])
/// and profitably ([`worth`]), in square order. Empty when `forker` is empty.
pub fn fork_victims(pos: &Position, forker: Square) -> Vec<Square> {
    let board = board_of(pos);
    let Some(piece) = board.piece_at(forker) else {
        return Vec::new();
    };
    (board.attacks_from(forker) & board.by_color(piece.color.other()))
        .into_iter()
        .filter(|&v| legal_attack(pos, forker, v) && worth(pos, forker, v))
        .collect()
}

/// All forks by the pieces of `by`: two or more [`fork_victims`] and the
/// forker [`safe`]. Who is to move is irrelevant here; callers that need the
/// fork to be "standing" check the turn themselves.
pub fn forks(pos: &Position, by: Color) -> Vec<Fork> {
    let board = board_of(pos);
    let mut out = Vec::new();
    for forker in board.by_color(by) {
        let victims = fork_victims(pos, forker);
        if victims.len() >= 2 && safe(pos, forker) {
            out.push(Fork { forker, victims });
        }
    }
    out
}

/// The enemy pieces that some piece of `by` attacks legally and profitably
/// (king included), in square order, without repetitions.
pub fn threatened_squares(pos: &Position, by: Color) -> Vec<Square> {
    let board = board_of(pos);
    let mut seen = shakmaty::Bitboard::EMPTY;
    for attacker in board.by_color(by) {
        for v in fork_victims(pos, attacker) {
            seen |= shakmaty::Bitboard::from(v);
        }
    }
    seen.into_iter().collect()
}

/// The double attack made by the last move of `by`: if, in `cur`, at least
/// two distinct enemy pieces are threatened ([`threatened_squares`]) and at
/// least one of them was **not** threatened in `prev` (the position before
/// that move), returns all the victims; otherwise an empty list.
pub fn double_attack(prev: &Position, cur: &Position, by: Color) -> Vec<Square> {
    let now = threatened_squares(cur, by);
    if now.len() < 2 {
        return Vec::new();
    }
    let before = threatened_squares(prev, by);
    if now.iter().any(|v| !before.contains(v)) {
        now
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("test fen")
    }

    #[test]
    fn the_knight_fork_of_king_and_rook() {
        let p = pos("r3k3/2N5/8/8/8/8/8/4K3 b - - 1 1");
        let f = forks(&p, Color::White);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].forker, Square::C7);
        assert_eq!(f[0].victims, vec![Square::A8, Square::E8]);
    }

    #[test]
    fn no_fork_before_the_knight_arrives() {
        let p = pos("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1");
        assert_eq!(forks(&p, Color::White), Vec::new());
    }

    #[test]
    fn an_unsafe_forker_is_not_a_fork() {
        // The knight forks king and rook but the black queen takes it for free.
        let p = pos("r3k3/2N5/1q6/8/8/8/8/4K3 b - - 0 1");
        let attackers = fork_victims(&p, Square::C7);
        assert_eq!(attackers.len(), 2);
        assert_eq!(forks(&p, Color::White), Vec::new());
    }

    #[test]
    fn double_attack_needs_a_new_threat() {
        let prev = pos("r3r1k1/8/8/3N4/8/8/8/6K1 w - - 0 1");
        let mv = prev.parse_uci("d5c7").expect("legal");
        let cur = prev.after(mv).expect("play");
        let victims = double_attack(&prev, &cur, Color::White);
        assert_eq!(victims, vec![Square::A8, Square::E8]);
        // Nothing new: the same position after a quiet king move.
        let prev2 = pos("r3r1k1/2N5/8/8/8/8/8/6K1 w - - 0 1");
        let mv2 = prev2.parse_uci("g1h1").expect("legal");
        let cur2 = prev2.after(mv2).expect("play");
        assert_eq!(double_attack(&prev2, &cur2, Color::White), Vec::new());
    }
}
