//! The [`Move`] type: a normal move, en passant, castling or a null move.

use shakmaty::{CastlingMode, CastlingSide, File, Role, Square};

/// A chess move, including the null move.
///
/// Values are plain data. They are only meaningful relative to a position;
/// use [`Position::play`](crate::Position::play) to check legality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Move {
    /// The null move: the side to move passes.
    Null,
    /// An ordinary move, a capture or a promotion (not en passant, not castling).
    Normal {
        /// The piece that moves.
        role: Role,
        /// Origin square.
        from: Square,
        /// Destination square.
        to: Square,
        /// The captured piece, if any.
        capture: Option<Role>,
        /// The promotion piece, if any.
        promotion: Option<Role>,
    },
    /// An en passant capture. `to` is the square the pawn lands on.
    EnPassant {
        /// Origin square.
        from: Square,
        /// Destination square.
        to: Square,
    },
    /// Castling, identified by the king and rook origin squares (this also
    /// covers Chess960). The side follows from the squares, see
    /// [`Move::castling_side`].
    Castle {
        /// The king's origin square.
        king_from: Square,
        /// The rook's origin square.
        rook_from: Square,
    },
}

impl Move {
    /// Returns `true` for the null move.
    pub const fn is_null(self) -> bool {
        matches!(self, Move::Null)
    }

    /// Returns `true` for castling.
    pub const fn is_castle(self) -> bool {
        matches!(self, Move::Castle { .. })
    }

    /// Returns `true` for en passant captures.
    pub const fn is_en_passant(self) -> bool {
        matches!(self, Move::EnPassant { .. })
    }

    /// Returns `true` if the move captures a piece (en passant included).
    pub const fn is_capture(self) -> bool {
        matches!(
            self,
            Move::Normal {
                capture: Some(_),
                ..
            } | Move::EnPassant { .. }
        )
    }

    /// Returns `true` if the move promotes a pawn.
    pub const fn is_promotion(self) -> bool {
        matches!(
            self,
            Move::Normal {
                promotion: Some(_),
                ..
            }
        )
    }

    /// The moving piece, `None` for the null move.
    pub const fn role(self) -> Option<Role> {
        match self {
            Move::Null => None,
            Move::Normal { role, .. } => Some(role),
            Move::EnPassant { .. } => Some(Role::Pawn),
            Move::Castle { .. } => Some(Role::King),
        }
    }

    /// The origin square (the king's for castling), `None` for the null move.
    pub const fn from(self) -> Option<Square> {
        match self {
            Move::Null => None,
            Move::Normal { from, .. } | Move::EnPassant { from, .. } => Some(from),
            Move::Castle { king_from, .. } => Some(king_from),
        }
    }

    /// The destination square (the king's for castling), `None` for the
    /// null move.
    pub fn to(self) -> Option<Square> {
        match self {
            Move::Null => None,
            Move::Normal { to, .. } | Move::EnPassant { to, .. } => Some(to),
            Move::Castle {
                king_from,
                rook_from,
            } => Some(castle_king_to(
                castling_side_of(king_from, rook_from),
                king_from,
            )),
        }
    }

    /// The side the king castles to: king side when the king stands on a
    /// lower file than the rook. `None` for moves that are not castling.
    pub fn castling_side(self) -> Option<CastlingSide> {
        match self {
            Move::Castle {
                king_from,
                rook_from,
            } => Some(castling_side_of(king_from, rook_from)),
            _ => None,
        }
    }

    /// The captured piece, if any (a pawn for en passant).
    pub const fn capture(self) -> Option<Role> {
        match self {
            Move::Normal { capture, .. } => capture,
            Move::EnPassant { .. } => Some(Role::Pawn),
            _ => None,
        }
    }

    /// The promotion piece, if any.
    pub const fn promotion(self) -> Option<Role> {
        match self {
            Move::Normal { promotion, .. } => promotion,
            _ => None,
        }
    }

    /// Formats the move in UCI notation.
    ///
    /// Promotions use a lower-case letter (`e7e8q`), the null move is `0000`.
    /// In [`CastlingMode::Standard`] castling is the two-square king move
    /// (`e1g1`); in [`CastlingMode::Chess960`] it is "king takes own rook".
    pub fn to_uci(self, mode: CastlingMode) -> String {
        let mut out = String::with_capacity(5);
        match self {
            Move::Null => out.push_str("0000"),
            Move::Castle {
                king_from,
                rook_from,
            } => {
                out.push_str(&king_from.to_string());
                match mode {
                    CastlingMode::Standard => {
                        let side = castling_side_of(king_from, rook_from);
                        out.push_str(&castle_king_to(side, king_from).to_string());
                    }
                    CastlingMode::Chess960 => out.push_str(&rook_from.to_string()),
                }
            }
            Move::Normal {
                from,
                to,
                promotion,
                ..
            } => {
                out.push_str(&from.to_string());
                out.push_str(&to.to_string());
                if let Some(p) = promotion {
                    out.push(p.char());
                }
            }
            Move::EnPassant { from, to } => {
                out.push_str(&from.to_string());
                out.push_str(&to.to_string());
            }
        }
        out
    }

    /// Converts to the `shakmaty` representation; `None` for the null move.
    pub(crate) fn to_shakmaty(self) -> Option<shakmaty::Move> {
        match self {
            Move::Null => None,
            Move::Normal {
                role,
                from,
                to,
                capture,
                promotion,
            } => Some(shakmaty::Move::Normal {
                role,
                from,
                capture,
                to,
                promotion,
            }),
            Move::EnPassant { from, to } => Some(shakmaty::Move::EnPassant { from, to }),
            Move::Castle {
                king_from,
                rook_from,
            } => Some(shakmaty::Move::Castle {
                king: king_from,
                rook: rook_from,
            }),
        }
    }

    /// Converts from the `shakmaty` representation; `None` for drops, which
    /// do not exist in standard chess.
    pub(crate) fn from_shakmaty(m: shakmaty::Move) -> Option<Move> {
        match m {
            shakmaty::Move::Normal {
                role,
                from,
                capture,
                to,
                promotion,
            } => Some(Move::Normal {
                role,
                from,
                to,
                capture,
                promotion,
            }),
            shakmaty::Move::EnPassant { from, to } => Some(Move::EnPassant { from, to }),
            shakmaty::Move::Castle { king, rook } => Some(Move::Castle {
                king_from: king,
                rook_from: rook,
            }),
            shakmaty::Move::Put { .. } => None,
        }
    }
}

/// The castling side implied by the king and rook origin squares.
fn castling_side_of(king_from: Square, rook_from: Square) -> CastlingSide {
    CastlingSide::from_king_side(king_from.file() < rook_from.file())
}

/// The king's destination square when castling to `side`.
const fn castle_king_to(side: CastlingSide, king_from: Square) -> Square {
    let file = match side {
        CastlingSide::KingSide => File::G,
        CastlingSide::QueenSide => File::C,
    };
    Square::from_coords(file, king_from.rank())
}
