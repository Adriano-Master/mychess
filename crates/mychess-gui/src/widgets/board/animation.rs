//! Pure move animation: detection by search in the legal moves, and timing.
//!
//! Scid: `::board::animate` (`tcl/board.tcl:2907-3121`) compares board strings;
//! here the move is found among the legal moves, which also covers en
//! passant, promotion and castling without tables.

use std::time::Duration;

use mychess_core::{CastlingSide, File, Move, Piece, Position, Square};
use smallvec::SmallVec;

/// Default animation time (Scid `animateDelay`).
pub const DEFAULT_DURATION: Duration = Duration::from_millis(200);

/// Whether the position moved forward or backward by one move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The new position is the old one after a move.
    Forward,
    /// The new position is the one before the old one's last move.
    Backward,
}

/// A piece sliding between two squares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sprite {
    /// The piece of the new position that ends on `to`.
    pub piece: Piece,
    /// Where the piece was in the previous position.
    pub from: Square,
    /// Where the piece is in the new position.
    pub to: Square,
}

/// An animation between two positions one move apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveAnimation {
    /// The sliding pieces (two for castling).
    pub sprites: SmallVec<[Sprite; 2]>,
    /// Squares the piece layer must not draw while the animation runs.
    pub hidden: SmallVec<[Square; 2]>,
    /// How long it lasts.
    pub duration: Duration,
    /// Direction relative to the game.
    pub direction: Direction,
}

fn same_board(a: &Position, b: &Position) -> bool {
    a.turn() == b.turn()
        && Square::ALL
            .iter()
            .all(|&sq| a.piece_at(sq) == b.piece_at(sq))
}

fn rook_destination(king_from: Square, side: CastlingSide) -> Square {
    let file = match side {
        CastlingSide::KingSide => File::F,
        CastlingSide::QueenSide => File::D,
    };
    Square::from_coords(file, king_from.rank())
}

/// Sprites that carry `piece_pos`'s pieces over `m`: the squares of `m` in
/// game order, with the piece taken from `shown` (the position being shown).
fn sprites_for(m: Move, shown: &Position, direction: Direction) -> SmallVec<[Sprite; 2]> {
    let mut out: SmallVec<[Sprite; 2]> = SmallVec::new();
    let mut push = |game_from: Square, game_to: Square| {
        let (from, to) = match direction {
            Direction::Forward => (game_from, game_to),
            Direction::Backward => (game_to, game_from),
        };
        if let Some(piece) = shown.piece_at(to) {
            out.push(Sprite { piece, from, to });
        }
    };
    match m {
        Move::Null => {}
        Move::Normal { from, to, .. } | Move::EnPassant { from, to } => push(from, to),
        Move::Castle {
            king_from,
            rook_from,
        } => {
            if let (Some(king_to), Some(side)) = (m.to(), m.castling_side()) {
                push(king_from, king_to);
                push(rook_from, rook_destination(king_from, side));
            }
        }
    }
    out
}

fn build(m: Move, shown: &Position, direction: Direction) -> Option<MoveAnimation> {
    let sprites = sprites_for(m, shown, direction);
    if sprites.is_empty() {
        return None;
    }
    Some(MoveAnimation {
        hidden: sprites.iter().map(|s| s.to).collect(),
        sprites,
        duration: DEFAULT_DURATION,
        direction,
    })
}

/// The animation that takes the board from `old` to `new`, if they are one
/// move apart. `None` for equal boards, jumps and the null move.
pub fn animation_between(old: &Position, new: &Position) -> Option<MoveAnimation> {
    if same_board(old, new) {
        return None;
    }
    if let Some(m) = old
        .legal_moves()
        .into_iter()
        .find(|&m| old.after(m).is_ok_and(|after| same_board(&after, new)))
    {
        return build(m, new, Direction::Forward);
    }
    let m = new
        .legal_moves()
        .into_iter()
        .find(|&m| new.after(m).is_ok_and(|after| same_board(&after, old)))?;
    build(m, new, Direction::Backward)
}

/// Fraction of the animation elapsed, in `0.0..=1.0`. A zero duration is
/// already finished. The clock is injected: this never reads the time.
pub fn progress(elapsed: Duration, duration: Duration) -> f32 {
    if duration.is_zero() {
        return 1.0;
    }
    (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

/// Cubic ease-out: `1 - (1 - t)^3`, with `t` limited to `0.0..=1.0`.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = if t.is_nan() { 1.0 } else { t.clamp(0.0, 1.0) };
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("valid test FEN")
    }

    fn sq(name: &str) -> Square {
        name.parse().expect("square")
    }

    fn after_uci(p: &Position, from: &str, to: &str) -> Position {
        let m = p.find_move(sq(from), sq(to), None).expect("legal move");
        p.after(m).expect("playable")
    }

    #[test]
    fn simple_pawn_move_slides_forward() {
        let old = Position::new();
        let new = after_uci(&old, "e2", "e4");
        let a = animation_between(&old, &new).expect("animation");
        assert_eq!(a.direction, Direction::Forward);
        assert_eq!(a.sprites.len(), 1);
        assert_eq!((a.sprites[0].from, a.sprites[0].to), (sq("e2"), sq("e4")));
        assert_eq!(a.hidden.as_slice(), &[sq("e4")]);
    }

    #[test]
    fn going_back_slides_the_piece_in_reverse() {
        let new = Position::new();
        let old = after_uci(&new, "g1", "f3");
        let a = animation_between(&old, &new).expect("animation");
        assert_eq!(a.direction, Direction::Backward);
        assert_eq!((a.sprites[0].from, a.sprites[0].to), (sq("f3"), sq("g1")));
        assert_eq!(a.hidden.as_slice(), &[sq("g1")]);
    }

    #[test]
    fn capture_and_en_passant_animate_the_capturing_piece() {
        let old = pos("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2");
        let new = after_uci(&old, "e5", "d6");
        let a = animation_between(&old, &new).expect("animation");
        assert_eq!((a.sprites[0].from, a.sprites[0].to), (sq("e5"), sq("d6")));
        let old = pos("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1");
        let new = after_uci(&old, "e4", "d5");
        assert!(animation_between(&old, &new).is_some());
    }

    #[test]
    fn promotion_slides_the_promoted_piece() {
        let old = pos("4k3/P7/8/8/8/8/8/4K3 w - - 0 1");
        let m = old
            .find_move(sq("a7"), sq("a8"), Some(mychess_core::Role::Queen))
            .expect("promotion");
        let new = old.after(m).expect("playable");
        let a = animation_between(&old, &new).expect("animation");
        assert_eq!(a.sprites[0].piece.role, mychess_core::Role::Queen);
        assert_eq!((a.sprites[0].from, a.sprites[0].to), (sq("a7"), sq("a8")));
    }

    #[test]
    fn castling_slides_king_and_rook_both_ways() {
        let old = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        for (king_to, rook_from, rook_to) in [("g1", "h1", "f1"), ("c1", "a1", "d1")] {
            let new = after_uci(&old, "e1", king_to);
            let a = animation_between(&old, &new).expect("animation");
            assert_eq!(a.sprites.len(), 2);
            assert!(
                a.sprites
                    .iter()
                    .any(|s| s.from == sq("e1") && s.to == sq(king_to))
            );
            assert!(
                a.sprites
                    .iter()
                    .any(|s| s.from == sq(rook_from) && s.to == sq(rook_to))
            );
            let back = animation_between(&new, &old).expect("animation");
            assert_eq!(back.direction, Direction::Backward);
            assert_eq!(back.sprites.len(), 2);
        }
    }

    #[test]
    fn chess960_castling_slides_both_pieces() {
        let old = pos("4k3/8/8/8/8/8/8/RK5R w HA - 0 1");
        let new = after_uci(&old, "b1", "h1");
        let a = animation_between(&old, &new).expect("animation");
        assert_eq!(a.sprites.len(), 2);
    }

    #[test]
    fn unrelated_and_equal_positions_do_not_animate() {
        let start = Position::new();
        assert_eq!(animation_between(&start, &start), None);
        let far = pos("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
        assert_eq!(animation_between(&start, &far), None);
        let two = after_uci(&after_uci(&start, "e2", "e4"), "e7", "e5");
        assert_eq!(animation_between(&start, &two), None);
    }

    #[test]
    fn progress_and_easing_are_clamped() {
        let d = Duration::from_millis(200);
        assert_eq!(progress(Duration::ZERO, d), 0.0);
        assert_eq!(progress(Duration::from_millis(100), d), 0.5);
        assert_eq!(progress(Duration::from_secs(5), d), 1.0);
        assert_eq!(progress(Duration::from_millis(10), Duration::ZERO), 1.0);
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert!((ease_out_cubic(0.5) - 0.875).abs() < 1e-6);
        assert_eq!(ease_out_cubic(-3.0), 0.0);
        assert_eq!(ease_out_cubic(7.0), 1.0);
    }
}
