//! UCI move strings.

use shakmaty::{Role, Square};

use crate::error::{UciError, clip};
use crate::moves::Move;
use crate::position::Position;

impl Position {
    /// Formats `m` in UCI notation using this position's castling mode.
    ///
    /// See [`Move::to_uci`].
    pub fn uci(&self, m: Move) -> String {
        m.to_uci(self.castling_mode())
    }

    /// Reads a UCI move (`e2e4`, `e7e8q`, `e1g1`, `0000`) in this position.
    ///
    /// Upper- and lower-case letters are accepted. `0000` and `--` give the
    /// null move (refused in check). A promotion needs its piece letter.
    ///
    /// Castling is accepted as "king takes own rook" (always unambiguous),
    /// or as the king moving to its castling destination (`g`/`c` file),
    /// which applies in Chess960 only when no ordinary king move has the same
    /// squares (so engines in standard mode still work).
    ///
    /// ```
    /// use mychess_core::Position;
    ///
    /// let pos = Position::from_fen("4k3/8/8/8/8/8/8/1R1RK3 w D - 0 1")?;
    /// let m = pos.parse_uci("e1d1")?; // king takes own rook
    /// assert!(m.is_castle());
    /// assert_eq!(pos.parse_uci("e1c1")?, m); // king to its destination
    /// assert_eq!(pos.uci(m), "e1d1");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn parse_uci(&self, text: &str) -> Result<Move, UciError> {
        let text = text.trim();
        let syntax = || UciError::Syntax { text: clip(text) };
        if text == "0000" || text == "--" {
            if self.is_check() {
                return Err(UciError::NullMoveInCheck { fen: self.fen() });
            }
            return Ok(Move::Null);
        }
        let bytes = text.as_bytes();
        if !(bytes.len() == 4 || bytes.len() == 5) || !text.is_ascii() {
            return Err(syntax());
        }
        let lower = text.to_ascii_lowercase();
        let lb = lower.as_bytes();
        let from = Square::from_ascii(&lb[0..2]).map_err(|_| syntax())?;
        let to = Square::from_ascii(&lb[2..4]).map_err(|_| syntax())?;
        let promotion = match lb.get(4) {
            None => None,
            Some(b'q') => Some(Role::Queen),
            Some(b'r') => Some(Role::Rook),
            Some(b'b') => Some(Role::Bishop),
            Some(b'n') => Some(Role::Knight),
            Some(_) => return Err(syntax()),
        };
        if let Some(m) = self.find_move(from, to, promotion) {
            return Ok(m);
        }
        let promotes_without_letter = promotion.is_none()
            && self
                .legal_moves()
                .iter()
                .any(|m| m.from() == Some(from) && m.to() == Some(to) && m.is_promotion());
        if promotes_without_letter {
            return Err(UciError::MissingPromotion { text: clip(text) });
        }
        Err(UciError::Illegal {
            text: clip(text),
            fen: self.fen(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::CastlingMode;

    #[test]
    fn writes_and_reads_every_legal_move() {
        let pos =
            Position::from_fen("r3k2r/pPpp1ppp/8/3Pp3/8/8/P3PPPP/R3K2R w KQkq e6 0 1").unwrap();
        let moves = pos.legal_moves();
        assert!(moves.iter().any(|m| m.is_castle()));
        assert!(moves.iter().any(|m| m.is_promotion()));
        assert!(moves.iter().any(|m| m.is_en_passant()));
        for m in moves {
            let text = pos.uci(m);
            assert_eq!(pos.parse_uci(&text), Ok(m), "{text}");
            assert_eq!(pos.parse_uci(&text.to_uppercase()), Ok(m), "{text}");
        }
    }

    #[test]
    fn promotion_is_lowercase_and_required() {
        let pos = Position::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let m = pos.parse_uci("a7a8q").unwrap();
        assert_eq!(m.to_uci(CastlingMode::Standard), "a7a8q");
        assert_eq!(
            pos.parse_uci("A7A8N").unwrap().promotion(),
            Some(Role::Knight)
        );
        assert!(matches!(
            pos.parse_uci("a7a8"),
            Err(UciError::MissingPromotion { .. })
        ));
    }

    #[test]
    fn castling_forms() {
        let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        let short = pos.parse_uci("e1g1").unwrap();
        assert!(short.is_castle());
        assert_eq!(pos.parse_uci("e1h1"), Ok(short));
        assert_eq!(pos.uci(short), "e1g1");
    }

    #[test]
    fn null_move() {
        let pos = Position::new();
        assert_eq!(pos.parse_uci("0000"), Ok(Move::Null));
        assert_eq!(pos.parse_uci("--"), Ok(Move::Null));
        assert_eq!(pos.uci(Move::Null), "0000");
        let check = Position::from_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1").unwrap();
        assert!(matches!(
            check.parse_uci("0000"),
            Err(UciError::NullMoveInCheck { .. })
        ));
    }

    #[test]
    fn errors() {
        let pos = Position::new();
        for bad in ["", "e2", "e2e4e", "e2e9", "é2e4", "e2e4x", "e2e5 extra"] {
            assert!(
                matches!(pos.parse_uci(bad), Err(UciError::Syntax { .. })),
                "{bad}"
            );
        }
        assert!(matches!(
            pos.parse_uci("e2e5"),
            Err(UciError::Illegal { .. })
        ));
    }
}
