//! Chess960 (Fischer random chess): variant tag, the Scharnagl numbering of
//! the 960 starting positions and castling-rook queries.
//!
//! The castling rule itself comes from `shakmaty`; this module only adds the
//! numbering and the variant marker. The Scid vs PC patch
//! (`patches/chess960-new.patch`, `::chess960::numberToFEN` in
//! `tcl/misc/misc.tcl`) uses the same scheme, so the numbers agree.

use shakmaty::{
    Bitboard, Board, CastlingMode, CastlingSide, Color, File, Rank, Role, Setup, Square,
};

use shakmaty::Position as _;

use crate::error::Chess960Error;
use crate::position::{Position, chess_from_setup};

/// The rule set of a game, derived from the castling mode of its start
/// position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Variant {
    /// Standard chess.
    #[default]
    Standard,
    /// Chess960: arbitrary back-rank arrangement, castling by king and rook
    /// origin squares.
    Chess960,
}

impl Variant {
    /// The matching castling mode.
    pub const fn castling_mode(self) -> CastlingMode {
        match self {
            Variant::Standard => CastlingMode::Standard,
            Variant::Chess960 => CastlingMode::Chess960,
        }
    }

    /// The variant that corresponds to a castling mode.
    pub const fn from_castling_mode(mode: CastlingMode) -> Variant {
        match mode {
            CastlingMode::Standard => Variant::Standard,
            CastlingMode::Chess960 => Variant::Chess960,
        }
    }
}

/// Number of Chess960 starting positions.
const COUNT: u16 = 960;

/// Knight placement tables of the Scharnagl scheme, applied to the five
/// squares left after bishops and queen (`R` and `K` fill the rest).
const KRN: [&str; 10] = [
    "NNRKR", "NRNKR", "NRKNR", "NRKRN", "RNNKR", "RNKNR", "RNKRN", "RKNNR", "RKNRN", "RKRNN",
];

/// The back-rank arrangement of Chess960 starting position `n`, files `a` to
/// `h`; `None` from 960 on. Position 518 is `RNBQKBNR`.
pub(crate) fn arrangement(n: u16) -> Option<[Role; 8]> {
    if n >= COUNT {
        return None;
    }
    let mut rest = n;
    let mut slots: [Option<Role>; 8] = [None; 8];
    // Light-squared bishop on b, d, f or h; dark-squared on a, c, e or g.
    slots[usize::from(2 * (rest % 4) + 1)] = Some(Role::Bishop);
    rest /= 4;
    slots[usize::from(2 * (rest % 4))] = Some(Role::Bishop);
    rest /= 4;
    let queen_index = usize::from(rest % 6);
    rest /= 6;
    *slots.iter_mut().filter(|s| s.is_none()).nth(queen_index)? = Some(Role::Queen);
    let table = KRN.get(usize::from(rest))?.bytes();
    let mut free = slots.iter_mut().filter(|s| s.is_none());
    for letter in table {
        *free.next()? = Some(match letter {
            b'N' => Role::Knight,
            b'R' => Role::Rook,
            _ => Role::King,
        });
    }
    let mut out = [Role::King; 8];
    for (slot, role) in out.iter_mut().zip(slots) {
        *slot = role?;
    }
    Some(out)
}

/// The Scharnagl number of a back-rank arrangement, if it is one of the 960.
///
/// Inverse of [`arrangement`]: `b1 + 4*b2 + 16*q + 96*k`, where `b1`/`b2` are
/// the light/dark bishop indices, `q` the queen's index among the six squares
/// left by the bishops and `k` the index of the remaining pattern in `KRN`.
fn arrangement_number(arrangement: [Role; 8]) -> Option<u16> {
    let mut light = None;
    let mut dark = None;
    let mut queen = None;
    let mut free = 0u16;
    let mut pattern = [0u8; 5];
    for (file, role) in arrangement.iter().enumerate() {
        let index = u16::try_from(file).ok()?;
        match role {
            Role::Bishop if file % 2 == 1 => {
                if light.replace(index / 2).is_some() {
                    return None;
                }
            }
            Role::Bishop => {
                if dark.replace(index / 2).is_some() {
                    return None;
                }
            }
            Role::Queen => {
                if queen.replace(free).is_some() {
                    return None;
                }
                free += 1;
            }
            Role::Knight | Role::Rook | Role::King => {
                // Free squares after bishops; the queen is not in `pattern`.
                let slot = pattern.get_mut(usize::from(free - u16::from(queen.is_some())))?;
                *slot = match role {
                    Role::Knight => b'N',
                    Role::Rook => b'R',
                    _ => b'K',
                };
                free += 1;
            }
            Role::Pawn => return None,
        }
    }
    let k = KRN.iter().position(|krn| krn.as_bytes() == pattern)?;
    Some(light? + 4 * dark? + 16 * queen? + 96 * u16::try_from(k).ok()?)
}

impl Position {
    /// Chess960 starting position number `n` (Scharnagl scheme): White to
    /// move, all four castling rights, clocks 0 and 1, castling mode
    /// [`CastlingMode::Chess960`]. Number 518 has the standard arrangement
    /// but still plays in Chess960 mode.
    ///
    /// Values of `n` from 960 on are an error; the Scid patch silently took
    /// them modulo 960. The arrangement is a [`Position`] like any other:
    /// number 518 is `RNBQKBNR`.
    ///
    /// ```
    /// use mychess_core::{CastlingMode, Position};
    ///
    /// let pos = Position::chess960_start(518)?;
    /// assert_eq!(pos.castling_mode(), CastlingMode::Chess960);
    /// assert_eq!(pos.fen(), Position::new().fen());
    /// assert_eq!(pos.chess960_number(), Some(518));
    /// assert_eq!(
    ///     Position::chess960_start(0)?.fen(),
    ///     "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1"
    /// );
    /// assert!(Position::chess960_start(960).is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn chess960_start(n: u16) -> Result<Position, Chess960Error> {
        if n >= COUNT {
            return Err(Chess960Error::NumberOutOfRange(n));
        }
        let arrangement = arrangement(n).ok_or(Chess960Error::NumberOutOfRange(n))?;
        let mut board = Board::empty();
        let mut rights = Bitboard::EMPTY;
        for (file, role) in File::ALL.into_iter().zip(arrangement) {
            for color in Color::ALL {
                let back = Square::from_coords(file, color.backrank());
                board.set_piece_at(back, role.of(color));
                board.set_piece_at(
                    Square::from_coords(file, color.fold_wb(Rank::Second, Rank::Seventh)),
                    Role::Pawn.of(color),
                );
                if role == Role::Rook {
                    rights.add(back);
                }
            }
        }
        let mut setup = Setup::empty();
        setup.board = board;
        setup.castling_rights = rights;
        // Every arrangement of the 960 tables is a valid position; if that
        // ever broke, report it honestly instead of a misleading range error.
        let inner = chess_from_setup(setup, CastlingMode::Chess960).map_err(|_| {
            debug_assert!(false, "Chess960 start {n} rejected by shakmaty");
            Chess960Error::InvalidStart(n)
        })?;
        Ok(Position { inner })
    }

    /// The Scharnagl number (`0..=959`) when this is a Chess960 starting
    /// position: initial pawns and pieces, a mirrored back rank, White to
    /// move and all four castling rights on the rooks. Works in both castling
    /// modes (`Position::new()` is number 518). Clocks are ignored.
    pub fn chess960_number(&self) -> Option<u16> {
        let board = self.inner.board();
        if self.turn() != Color::White {
            return None;
        }
        let mut arrangement = [Role::King; 8];
        for (file, slot) in File::ALL.into_iter().zip(arrangement.iter_mut()) {
            let white = board.piece_at(Square::from_coords(file, Rank::First))?;
            let black = board.piece_at(Square::from_coords(file, Rank::Eighth))?;
            if white.color != Color::White
                || black.color != Color::Black
                || white.role != black.role
            {
                return None;
            }
            let pawn_w = board.piece_at(Square::from_coords(file, Rank::Second))?;
            let pawn_b = board.piece_at(Square::from_coords(file, Rank::Seventh))?;
            if pawn_w != Role::Pawn.of(Color::White) || pawn_b != Role::Pawn.of(Color::Black) {
                return None;
            }
            *slot = white.role;
        }
        if board.occupied().count() != 32 {
            return None;
        }
        let number = arrangement_number(arrangement)?;
        let rooks_ok = Color::ALL.into_iter().all(|color| {
            CastlingSide::ALL.into_iter().all(|side| {
                self.castling_rook(color, side).is_some_and(|rook| {
                    board.piece_at(rook) == Some(Role::Rook.of(color))
                        && rook.rank() == color.backrank()
                })
            })
        });
        rooks_ok.then_some(number)
    }

    /// The rule set of this position (derived from its castling mode).
    pub fn variant(&self) -> Variant {
        Variant::from_castling_mode(self.castling_mode())
    }

    /// The square of the rook that still has the right to castle to `side`
    /// for `color`, if any. In standard chess this is a corner; in Chess960
    /// it can be any square of the first rank.
    ///
    /// ```
    /// use mychess_core::{CastlingSide, Color, Position, Square};
    ///
    /// let pos = Position::new();
    /// assert_eq!(pos.castling_rook(Color::White, CastlingSide::KingSide), Some(Square::H1));
    /// ```
    pub fn castling_rook(&self, color: Color, side: CastlingSide) -> Option<Square> {
        self.inner.castles().rook(color, side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(n: u16) -> String {
        arrangement(n)
            .expect("in range")
            .iter()
            .map(|r| r.upper_char())
            .collect()
    }

    #[test]
    fn scharnagl_vectors() {
        assert_eq!(row(0), "BBQNNRKR");
        assert_eq!(row(1), "BQNBNRKR");
        assert_eq!(row(2), "BQNNRBKR");
        assert_eq!(row(100), "QBBNRNKR");
        assert_eq!(row(518), "RNBQKBNR");
        assert_eq!(row(959), "RKRNNQBB");
    }

    #[test]
    fn all_arrangements_are_distinct_and_valid() {
        let mut seen = std::collections::HashSet::new();
        for n in 0..COUNT {
            let a = arrangement(n).expect("in range");
            assert_eq!(arrangement_number(a), Some(n), "analytic inverse {n}");
            assert!(seen.insert(a), "duplicate arrangement {n}");
            let at = |role| -> Vec<usize> {
                a.iter()
                    .enumerate()
                    .filter(|(_, r)| **r == role)
                    .map(|(i, _)| i)
                    .collect()
            };
            let bishops = at(Role::Bishop);
            assert_eq!(bishops.len(), 2);
            assert_ne!(bishops[0] % 2, bishops[1] % 2, "{n}");
            let rooks = at(Role::Rook);
            let king = at(Role::King);
            assert_eq!((rooks.len(), king.len()), (2, 1));
            assert!(rooks[0] < king[0] && king[0] < rooks[1], "{n}");
            assert_eq!(at(Role::Knight).len(), 2);
            assert_eq!(at(Role::Queen).len(), 1);
        }
        assert_eq!(seen.len(), 960);
    }

    #[test]
    fn arrangement_is_none_from_960_and_invalid_rows_have_no_number() {
        use Role::{Bishop as B, King as K, Knight as N, Queen as Q, Rook as R};
        assert_eq!(arrangement(960), None);
        assert_eq!(arrangement(u16::MAX), None);
        // Two bishops on the same colour.
        assert_eq!(arrangement_number([B, N, B, Q, R, K, R, N]), None);
        // King outside the rooks.
        assert_eq!(arrangement_number([K, R, B, Q, N, B, N, R]), None);
        // Pawn in the row.
        assert_eq!(arrangement_number([R, N, B, Q, K, B, N, Role::Pawn]), None);
    }

    #[test]
    fn start_number_roundtrip_and_range() {
        for n in 0..COUNT {
            let pos = Position::chess960_start(n).unwrap();
            assert_eq!(pos.chess960_number(), Some(n));
            assert_eq!(pos.castling_mode(), CastlingMode::Chess960);
            assert_eq!(
                Position::from_fen(&pos.fen()).unwrap().chess960_number(),
                Some(n)
            );
        }
        assert_eq!(
            Position::chess960_start(960),
            Err(Chess960Error::NumberOutOfRange(960))
        );
        assert_eq!(Position::new().chess960_number(), Some(518));
        assert_eq!(Position::new().variant(), Variant::Standard);
    }

    #[test]
    fn number_is_none_after_a_move_or_for_other_positions() {
        let pos = Position::new();
        let moved = pos.after(pos.parse_uci("e2e4").unwrap()).unwrap();
        assert_eq!(moved.chess960_number(), None);
        let lost =
            Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w Kkq - 0 1").unwrap();
        assert_eq!(lost.chess960_number(), None);
        assert_eq!(
            Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1")
                .unwrap()
                .chess960_number(),
            None
        );
    }
}
