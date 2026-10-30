//! Automatic end-of-game detection.

use shakmaty::Color;

use crate::position::{Position, ZobristHash};

/// Why a game is over by the rules (no resignation or agreement involved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GameEnd {
    /// The side to move is checkmated.
    Checkmate {
        /// The side that delivered mate.
        winner: Color,
    },
    /// The side to move has no legal move and is not in check.
    Stalemate,
    /// Neither side can mate (FIDE rules, see
    /// [`Position::is_insufficient_material`]).
    InsufficientMaterial,
    /// 100 halfmoves without a capture or pawn move.
    FiftyMoves,
    /// The current position occurred at least three times.
    ThreefoldRepetition,
}

impl Position {
    /// The reason the game is over in this position, if any.
    ///
    /// `history` holds the hashes of the **earlier** positions of the game
    /// (not the current one). A repetition needs the same board, side to
    /// move, castling rights and en passant possibility, which is exactly
    /// what [`Position::zobrist`] covers. Checkmate takes precedence over the
    /// draw rules, then stalemate, insufficient material, fifty-move rule
    /// and finally repetition.
    ///
    /// Repetition is detected by comparing 64-bit hashes, so a false positive
    /// needs a hash collision, whose probability is negligible in a real
    /// game.
    ///
    /// ```
    /// use mychess_core::{GameEnd, Position};
    ///
    /// let mut pos = Position::new();
    /// let mut history = Vec::new();
    /// for uci in ["g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8"] {
    ///     history.push(pos.zobrist());
    ///     let m = pos.parse_uci(uci)?;
    ///     pos.play(m)?;
    /// }
    /// // The start position has now occurred three times.
    /// history.pop();
    /// assert_eq!(pos.game_end(&history), Some(GameEnd::ThreefoldRepetition));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn game_end(&self, history: &[ZobristHash]) -> Option<GameEnd> {
        if self.is_checkmate() {
            return Some(GameEnd::Checkmate {
                winner: !self.turn(),
            });
        }
        if self.is_stalemate() {
            return Some(GameEnd::Stalemate);
        }
        if self.is_insufficient_material() {
            return Some(GameEnd::InsufficientMaterial);
        }
        if self.halfmove_clock() >= 100 {
            return Some(GameEnd::FiftyMoves);
        }
        let current = self.zobrist();
        if history.iter().filter(|h| **h == current).count() >= 2 {
            return Some(GameEnd::ThreefoldRepetition);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).unwrap()
    }

    #[test]
    fn mate_and_stalemate() {
        let mate = pos("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
        assert_eq!(
            mate.game_end(&[]),
            Some(GameEnd::Checkmate {
                winner: Color::Black
            })
        );
        let stale = pos("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
        assert_eq!(stale.game_end(&[]), Some(GameEnd::Stalemate));
    }

    #[test]
    fn insufficient_material_cases() {
        for fen in [
            "8/8/4k3/8/8/3K4/8/8 w - - 0 1",
            "8/8/4k3/8/8/3KB3/8/8 w - - 0 1",
            "8/8/4k3/8/8/3KN3/8/8 w - - 0 1",
            "8/8/3bk3/8/8/3KB3/8/8 w - - 0 1",
        ] {
            assert_eq!(
                pos(fen).game_end(&[]),
                Some(GameEnd::InsufficientMaterial),
                "{fen}"
            );
        }
        for fen in [
            "8/8/3nk3/8/8/3KN3/8/8 w - - 0 1",
            "8/8/4k3/8/8/3KP3/8/8 w - - 0 1",
        ] {
            assert_eq!(pos(fen).game_end(&[]), None, "{fen}");
        }
    }

    #[test]
    fn fifty_moves() {
        let p = pos("4k3/8/8/8/8/8/4P3/4K3 w - - 100 80");
        assert_eq!(p.game_end(&[]), Some(GameEnd::FiftyMoves));
        let p = pos("4k3/8/8/8/8/8/4P3/4K3 w - - 99 80");
        assert_eq!(p.game_end(&[]), None);
    }

    #[test]
    fn threefold_needs_same_castling_rights() {
        let p = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        let h = p.zobrist();
        assert_eq!(p.game_end(&[h]), None);
        assert_eq!(p.game_end(&[h, h]), Some(GameEnd::ThreefoldRepetition));
        let other = pos("r3k2r/8/8/8/8/8/8/R3K2R w Qkq - 0 1").zobrist();
        assert_ne!(h, other);
        assert_eq!(p.game_end(&[h, other]), None);
    }
}
