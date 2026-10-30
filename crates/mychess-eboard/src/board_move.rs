//! A move made on a physical board, and how it becomes a legal move
//! (spec m8-10, D2709, D2710, D2723).

use mychess_core::{CastlingSide, Move, Position, Role, Square};
use thiserror::Error;

/// The longest move text read; anything longer is not a move.
const MAX_MOVE_TEXT: usize = 16;

/// A move as a board or its driver reports it, not yet checked against a
/// position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoardMove {
    /// From square to square, with the promotion piece when given.
    Coordinate {
        /// Origin square.
        from: Square,
        /// Destination square.
        to: Square,
        /// The piece a pawn becomes, when the board says.
        promotion: Option<Role>,
    },
    /// Castling on one side.
    Castle(CastlingSide),
}

/// Why a board move could not be used.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum BoardMoveError {
    /// The text is not a move.
    #[error("unreadable move: {0:?}")]
    Unreadable(String),
    /// The move is not legal in the position.
    #[error("illegal move: {0}")]
    Illegal(String),
}

impl BoardMove {
    /// Reads `e2e4`, `e2-e4`, `e7e8q`, `e7-e8/Q`, `e5xd6`, `0-0`, `O-O-O`
    /// (D2709). The separators `-`, `/`, `x`, `:` and `=` are ignored.
    ///
    /// # Errors
    ///
    /// [`BoardMoveError::Unreadable`] for anything else.
    pub fn parse(text: &str) -> Result<BoardMove, BoardMoveError> {
        let unreadable = || BoardMoveError::Unreadable(clip(text));
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_MOVE_TEXT || !trimmed.is_ascii() {
            return Err(unreadable());
        }
        let upper = trimmed.to_ascii_uppercase();
        match upper.as_str() {
            "O-O" | "0-0" => return Ok(BoardMove::Castle(CastlingSide::KingSide)),
            "O-O-O" | "0-0-0" => return Ok(BoardMove::Castle(CastlingSide::QueenSide)),
            _ => {}
        }
        let squeezed: Vec<u8> = trimmed
            .bytes()
            .filter(|b| !matches!(b, b'-' | b'/' | b'x' | b'X' | b':' | b'=' | b' '))
            .collect();
        if !matches!(squeezed.len(), 4 | 5) {
            return Err(unreadable());
        }
        let square = |at: usize| {
            let name = std::str::from_utf8(&squeezed[at..at + 2]).ok()?;
            name.to_ascii_lowercase().parse::<Square>().ok()
        };
        let (Some(from), Some(to)) = (square(0), square(2)) else {
            return Err(unreadable());
        };
        let promotion = match squeezed.get(4).map(u8::to_ascii_lowercase) {
            None => None,
            Some(b'q') => Some(Role::Queen),
            Some(b'r') => Some(Role::Rook),
            Some(b'b') => Some(Role::Bishop),
            Some(b'n') => Some(Role::Knight),
            Some(_) => return Err(unreadable()),
        };
        Ok(BoardMove::Coordinate {
            from,
            to,
            promotion,
        })
    }

    /// The legal move this board move stands for in `position` (D2710). A
    /// promotion without a piece is a queen.
    ///
    /// # Errors
    ///
    /// [`BoardMoveError::Illegal`] when no legal move matches.
    pub fn resolve(self, position: &Position) -> Result<Move, BoardMoveError> {
        let illegal = || BoardMoveError::Illegal(self.to_string());
        match self {
            BoardMove::Castle(side) => position
                .legal_moves()
                .into_iter()
                .find(|m| m.castling_side() == Some(side))
                .ok_or_else(illegal),
            BoardMove::Coordinate {
                from,
                to,
                promotion,
            } => {
                if let Some(mv) = position.find_move(from, to, promotion) {
                    return Ok(mv);
                }
                if promotion.is_none() {
                    return position
                        .find_move(from, to, Some(Role::Queen))
                        .ok_or_else(illegal);
                }
                Err(illegal())
            }
        }
    }
}

impl std::fmt::Display for BoardMove {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoardMove::Castle(CastlingSide::KingSide) => f.write_str("O-O"),
            BoardMove::Castle(CastlingSide::QueenSide) => f.write_str("O-O-O"),
            BoardMove::Coordinate {
                from,
                to,
                promotion,
            } => {
                write!(f, "{from}{to}")?;
                if let Some(role) = promotion {
                    write!(f, "{}", role.char())?;
                }
                Ok(())
            }
        }
    }
}

/// Compares the piece placement (first FEN field) of two FEN strings and
/// nothing else: a physical board does not know about en passant or
/// castling rights (D2723).
pub fn placement_matches(a: &str, b: &str) -> bool {
    let first = |fen: &'_ str| fen.split_whitespace().next().map(str::to_owned);
    match (first(a), first(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

fn clip(text: &str) -> String {
    text.chars().take(MAX_MOVE_TEXT).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(name: &str) -> Square {
        name.parse().expect("square")
    }

    fn coordinate(from: &str, to: &str, promotion: Option<Role>) -> BoardMove {
        BoardMove::Coordinate {
            from: sq(from),
            to: sq(to),
            promotion,
        }
    }

    #[test]
    fn reads_the_formats_of_the_boards() {
        assert_eq!(BoardMove::parse("e2e4"), Ok(coordinate("e2", "e4", None)));
        assert_eq!(BoardMove::parse("e2-e4"), Ok(coordinate("e2", "e4", None)));
        assert_eq!(
            BoardMove::parse(" E2-E4 "),
            Ok(coordinate("e2", "e4", None))
        );
        assert_eq!(BoardMove::parse("e5xd6"), Ok(coordinate("e5", "d6", None)));
        assert_eq!(
            BoardMove::parse("e7e8q"),
            Ok(coordinate("e7", "e8", Some(Role::Queen)))
        );
        assert_eq!(
            BoardMove::parse("e7-e8/N"),
            Ok(coordinate("e7", "e8", Some(Role::Knight)))
        );
        assert_eq!(
            BoardMove::parse("0-0"),
            Ok(BoardMove::Castle(CastlingSide::KingSide))
        );
        assert_eq!(
            BoardMove::parse("o-o-o"),
            Ok(BoardMove::Castle(CastlingSide::QueenSide))
        );
    }

    #[test]
    fn rejects_what_is_not_a_move() {
        for text in [
            "",
            "  ",
            "e2",
            "e2e",
            "i2e4",
            "e9e4",
            "e2e4k",
            "e2e4qq",
            "Nf3",
            "e2e4e5e6e7e8e1e2e3",
            "é2e4",
            "0-0-0-0",
        ] {
            assert!(
                matches!(BoardMove::parse(text), Err(BoardMoveError::Unreadable(_))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn resolves_ordinary_moves_and_refuses_illegal_ones() {
        let start = Position::new();
        let mv = coordinate("e2", "e4", None).resolve(&start).expect("legal");
        assert_eq!(start.uci(mv), "e2e4");
        assert!(matches!(
            coordinate("e2", "e5", None).resolve(&start),
            Err(BoardMoveError::Illegal(_))
        ));
    }

    #[test]
    fn resolves_castling_both_ways() {
        let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").expect("fen");
        let by_text = BoardMove::parse("0-0").unwrap().resolve(&pos).expect("o-o");
        let by_squares = coordinate("e1", "g1", None).resolve(&pos).expect("e1g1");
        assert_eq!(by_text, by_squares);
        let long = BoardMove::parse("O-O-O")
            .unwrap()
            .resolve(&pos)
            .expect("o-o-o");
        assert_eq!(pos.uci(long), "e1c1");
        let blocked = Position::from_fen("r3k2r/8/8/8/8/8/8/RN2K2R w KQkq - 0 1").expect("fen");
        assert!(matches!(
            BoardMove::parse("O-O-O").unwrap().resolve(&blocked),
            Err(BoardMoveError::Illegal(_))
        ));
    }

    #[test]
    fn resolves_en_passant_and_promotion() {
        let ep = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").expect("fen");
        let mv = coordinate("e5", "d6", None).resolve(&ep).expect("ep");
        assert!(mv.is_en_passant());

        let promo = Position::from_fen("8/4P2k/8/8/8/8/8/4K3 w - - 0 1").expect("fen");
        let default = coordinate("e7", "e8", None).resolve(&promo).expect("queen");
        assert_eq!(promo.uci(default), "e7e8q");
        let knight = coordinate("e7", "e8", Some(Role::Knight))
            .resolve(&promo)
            .expect("knight");
        assert_eq!(promo.uci(knight), "e7e8n");
    }

    #[test]
    fn placement_ignores_everything_but_the_pieces() {
        let a = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
        let b = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR w - - 5 9";
        assert!(placement_matches(a, b));
        assert!(!placement_matches(a, "8/8/8/8/8/8/8/8 w"));
        assert!(!placement_matches("", ""));
    }

    #[test]
    fn display_round_trips() {
        for text in ["e2e4", "e7e8q", "O-O", "O-O-O"] {
            let parsed = BoardMove::parse(text).expect("parse");
            assert_eq!(parsed.to_string(), text);
        }
    }
}
