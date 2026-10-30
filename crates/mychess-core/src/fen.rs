//! FEN reading (tolerant, like the Scid) and writing.

use shakmaty::Position as _;
use shakmaty::fen::Fen;
use shakmaty::{
    Bitboard, Board, CastlingMode, CastlingSide, Color, EnPassantMode, File, Piece, Rank, Role,
    Setup, Square,
};

use crate::error::{BoardProblem, FenError, IllegalPositionReason, clip};
use crate::position::{Position, chess_from_setup, outermost_rook};

/// How the en passant field is written by [`Position::fen_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EnPassantFen {
    /// Only when a pawn of the side to move stands next to the pawn that
    /// just advanced two squares (the Scid's behaviour, the default).
    #[default]
    Adjacent,
    /// Always after a double pawn advance, even if nobody can capture.
    Always,
}

/// How [`Position::from_fen_with`] decides the castling mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FenMode {
    /// Standard chess: `K`/`Q`/`k`/`q` name the corner rooks and rights that
    /// standard chess cannot express are silently discarded.
    Standard,
    /// Chess960: `K`/`Q`/`k`/`q` name the outermost rook on that side of the
    /// king; a missing castling field gives the outermost rooks their rights.
    Chess960,
    /// Standard chess unless the castling rights cannot be expressed in it
    /// (king off e1/e8, or a castling rook off its corner), then Chess960.
    /// The default of [`Position::from_fen`].
    ///
    /// When the castling field uses only `K`/`Q`/`k`/`q`, Chess960 is chosen
    /// only if every letter names a valid outermost rook ("all or nothing"):
    /// a stale `KQkq` on a standard game whose king has moved to g1 is not
    /// reinterpreted, the rights that no longer exist are discarded and the
    /// position stays standard. Letters `A`-`H`/`a`-`h` always decide by the
    /// squares they name.
    #[default]
    Auto,
}

/// How the castling field is written by [`Position::fen_with_style`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FenCastlingStyle {
    /// X-FEN (the default): `KQkq` when the rook is the outermost on its side
    /// of the king, the file letter (`D`, `f`) when it is not.
    #[default]
    XFen,
    /// Shredder-FEN: always the file letter of the rook (`HAha`).
    Shredder,
}

impl Position {
    /// Reads a FEN string, tolerantly, choosing the castling mode
    /// automatically ([`FenMode::Auto`]).
    ///
    /// - The board field may omit the `/` separators if it describes 64 squares.
    /// - The castling, en passant, halfmove clock and move number fields may be
    ///   missing. A missing castling field is deduced from the board.
    /// - Castling rights and en passant squares that do not fit the board are
    ///   silently discarded.
    /// - Move number 0, negative or missing counts as 1; a negative halfmove
    ///   clock counts as 0.
    ///
    /// Positions are rejected when a side does not have exactly one king or
    /// has more than 16 pieces, when the kings touch, when a pawn stands on
    /// the first or eighth rank, or when the side not to move gives check.
    ///
    /// The castling field may be `-`, `KQkq` (outermost rook, X-FEN) or file
    /// letters `A`-`H` / `a`-`h` (Shredder-FEN, X-FEN), in any order.
    ///
    /// With [`FenMode::Auto`], `KQkq` letters only switch to Chess960 when
    /// all of them resolve to a rook and the result cannot be played as
    /// standard chess. A middlegame FEN with stale rights keeps standard
    /// rules instead:
    ///
    /// ```
    /// use mychess_core::{CastlingMode, Position};
    ///
    /// // King already on g1: `K` finds no rook, so nothing is reinterpreted.
    /// let stale = Position::from_fen(
    ///     "r4rk1/pppq1ppp/2np1n2/2b1p3/2B1P3/2NP1N2/PPPQ1PPP/R4RK1 w KQkq - 0 9",
    /// )?;
    /// assert_eq!(stale.castling_mode(), CastlingMode::Standard);
    /// assert!(stale.fen().contains(" w - - "));
    /// // Position 0 of Chess960: every letter resolves, king off e1.
    /// let p0 = Position::from_fen("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1")?;
    /// assert_eq!(p0.chess960_number(), Some(0));
    /// # Ok::<(), mychess_core::FenError>(())
    /// ```
    ///
    /// ```
    /// use mychess_core::{CastlingMode, Position};
    ///
    /// let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")?;
    /// assert_eq!(pos, Position::new());
    /// assert!(Position::from_fen("not a fen").is_err());
    /// // Shredder notation of the standard start stays in standard mode...
    /// let shredder = Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w HAha - 0 1")?;
    /// assert_eq!(shredder.castling_mode(), CastlingMode::Standard);
    /// // ...while a king off the e-file needs Chess960.
    /// let c960 = Position::from_fen("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w HFhf - 0 1")?;
    /// assert_eq!(c960.castling_mode(), CastlingMode::Chess960);
    /// assert_eq!(c960.chess960_number(), Some(0));
    /// # Ok::<(), mychess_core::FenError>(())
    /// ```
    pub fn from_fen(fen: &str) -> Result<Position, FenError> {
        Position::from_fen_with(fen, FenMode::Auto)
    }

    /// Like [`Position::from_fen`], with an explicit [`FenMode`].
    ///
    /// ```
    /// use mychess_core::{CastlingMode, FenMode, Position};
    ///
    /// let pos = Position::from_fen_with(
    ///     "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    ///     FenMode::Chess960,
    /// )?;
    /// assert_eq!(pos.castling_mode(), CastlingMode::Chess960);
    /// assert_eq!(pos.chess960_number(), Some(518));
    /// # Ok::<(), mychess_core::FenError>(())
    /// ```
    pub fn from_fen_with(fen: &str, mode: FenMode) -> Result<Position, FenError> {
        let mut fields = fen.split_ascii_whitespace();

        let board_field = fields.next().unwrap_or("");
        let board = parse_board(board_field)?;

        let turn = match fields.next() {
            Some("w") => Color::White,
            Some("b") => Color::Black,
            other => {
                return Err(FenError::InvalidTurn {
                    found: other.map(clip),
                });
            }
        };

        check_material(&board)?;

        let mut mode = mode;
        let castling_rights = match fields.next() {
            None if mode == FenMode::Chess960 => outermost_rooks(&board),
            None => deduce_castling(&board),
            Some(field) => {
                let parsed = parse_castling(field, &board, mode)?;
                if mode == FenMode::Auto && parsed.letters_only {
                    // All or nothing: Chess960 only when every K/Q/k/q letter
                    // resolved to a rook and standard chess cannot express it.
                    let chess960 = parsed.all_resolved
                        && CastlingMode::detect(&probe_setup(&board, parsed.rights))
                            == CastlingMode::Chess960;
                    if chess960 {
                        parsed.rights
                    } else {
                        mode = FenMode::Standard;
                        parse_castling(field, &board, mode)?.rights
                    }
                } else {
                    parsed.rights
                }
            }
        };

        let ep_square = match fields.next() {
            None | Some("-") => None,
            Some(field) => Some(parse_ep(field)?),
        };

        let halfmoves = match fields.next() {
            None => 0,
            Some(field) => {
                let n = parse_counter(field)
                    .ok_or_else(|| FenError::InvalidHalfmoveClock { field: clip(field) })?;
                u32::try_from(n.max(0)).unwrap_or(u32::MAX)
            }
        };
        let fullmoves = match fields.next() {
            None => 1,
            Some(field) => {
                let n = parse_counter(field)
                    .ok_or_else(|| FenError::InvalidMoveNumber { field: clip(field) })?;
                u32::try_from(n.max(1)).unwrap_or(u32::MAX)
            }
        };

        let mut setup = Setup::empty();
        setup.board = board;
        setup.turn = turn;
        setup.castling_rights = castling_rights;
        setup.ep_square = ep_square;
        setup.halfmoves = halfmoves;
        setup.fullmoves = std::num::NonZeroU32::new(fullmoves).unwrap_or(std::num::NonZeroU32::MIN);

        let castling_mode = match mode {
            FenMode::Standard => CastlingMode::Standard,
            FenMode::Chess960 => CastlingMode::Chess960,
            FenMode::Auto => CastlingMode::detect(&setup),
        };
        let inner = chess_from_setup(setup, castling_mode).map_err(|kinds| {
            let reason = if kinds.contains(shakmaty::PositionErrorKinds::OPPOSITE_CHECK) {
                IllegalPositionReason::OpponentInCheck
            } else {
                IllegalPositionReason::Other
            };
            FenError::IllegalPosition { reason }
        })?;
        Ok(Position { inner })
    }

    /// The position as a six-field FEN string.
    ///
    /// The en passant field follows [`EnPassantFen::Adjacent`] and the
    /// castling field is X-FEN ([`FenCastlingStyle::XFen`]), which is plain
    /// `KQkq` for standard chess.
    pub fn fen(&self) -> String {
        self.fen_with_options(EnPassantFen::Adjacent, FenCastlingStyle::XFen)
    }

    /// The position as a six-field FEN string with the chosen en passant
    /// convention (X-FEN castling field).
    pub fn fen_with(&self, ep: EnPassantFen) -> String {
        self.fen_with_options(ep, FenCastlingStyle::XFen)
    }

    /// The position as a FEN string with the chosen castling notation
    /// (Adjacent en passant convention).
    ///
    /// ```
    /// use mychess_core::{FenCastlingStyle, Position};
    ///
    /// let pos = Position::chess960_start(0)?;
    /// assert!(pos.fen().ends_with("w KQkq - 0 1"));
    /// assert_eq!(
    ///     pos.fen_with_style(FenCastlingStyle::Shredder),
    ///     "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w HFhf - 0 1"
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn fen_with_style(&self, style: FenCastlingStyle) -> String {
        self.fen_with_options(EnPassantFen::Adjacent, style)
    }

    /// The position as a FEN string with both conventions chosen.
    pub fn fen_with_options(&self, ep: EnPassantFen, style: FenCastlingStyle) -> String {
        let mode = match ep {
            EnPassantFen::Adjacent => EnPassantMode::PseudoLegal,
            EnPassantFen::Always => EnPassantMode::Always,
        };
        let xfen = Fen::from_position(&self.inner, mode).to_string();
        match style {
            FenCastlingStyle::XFen => xfen,
            FenCastlingStyle::Shredder => {
                let field = self.shredder_castling_field();
                let mut parts: Vec<&str> = xfen.split(' ').collect();
                if let Some(slot) = parts.get_mut(2) {
                    *slot = &field;
                }
                parts.join(" ")
            }
        }
    }

    /// The castling field in Shredder notation: rook files, highest file
    /// first within each colour (`HFhf`), as X-FEN orders its letters.
    fn shredder_castling_field(&self) -> String {
        let mut out = String::new();
        for color in Color::ALL {
            let rights = self.inner.castles().castling_rights() & color.backrank();
            for rook in rights.into_iter().rev() {
                let file = rook.file().char();
                out.push(color.fold_wb(file.to_ascii_uppercase(), file));
            }
        }
        if out.is_empty() {
            out.push('-');
        }
        out
    }
}

fn parse_board(field: &str) -> Result<Board, FenError> {
    let fail = |problem| FenError::InvalidBoard {
        field: clip(field),
        problem,
    };
    let mut board = Board::empty();
    let mut count: u32 = 0;
    for (index, ch) in field.chars().enumerate() {
        match ch {
            '1'..='8' => {
                let digit = ch.to_digit(10).unwrap_or(0);
                if count + digit > 64 {
                    return Err(fail(BoardProblem::TooManySquares));
                }
                count += digit;
            }
            '/' => {
                // Like the Scid, a slash is fine at any rank boundary,
                // including at the very start and repeated.
                if !count.is_multiple_of(8) {
                    return Err(fail(BoardProblem::MisplacedSlash { index }));
                }
            }
            _ => {
                let piece = Piece::from_char(ch)
                    .ok_or_else(|| fail(BoardProblem::UnexpectedChar { ch, index }))?;
                if count >= 64 {
                    return Err(fail(BoardProblem::TooManySquares));
                }
                board.set_piece_at(square_from_fen_index(count), piece);
                count += 1;
            }
        }
    }
    if count < 64 {
        return Err(fail(BoardProblem::TooFewSquares { count }));
    }
    Ok(board)
}

/// FEN lists squares from a8 to h1; `index` is in `0..64`.
fn square_from_fen_index(index: u32) -> Square {
    Square::from_coords(File::new(index % 8), Rank::new(7 - index / 8))
}

fn check_material(board: &Board) -> Result<(), FenError> {
    let illegal = |reason| FenError::IllegalPosition { reason };
    for color in Color::ALL {
        let kings = (board.by_role(Role::King) & board.by_color(color)).count();
        if kings == 0 {
            return Err(illegal(IllegalPositionReason::MissingKing(color)));
        }
        if kings > 1 {
            return Err(illegal(IllegalPositionReason::TooManyKings(color)));
        }
        if board.by_color(color).count() > 16 {
            return Err(illegal(IllegalPositionReason::TooManyPieces(color)));
        }
    }
    if (board.pawns() & Bitboard::BACKRANKS).any() {
        return Err(illegal(IllegalPositionReason::PawnOnBackRank));
    }
    if let (Some(white), Some(black)) = (board.king_of(Color::White), board.king_of(Color::Black))
        && white.distance(black) <= 1
    {
        return Err(illegal(IllegalPositionReason::AdjacentKings));
    }
    Ok(())
}

/// A setup with only a board and castling rights, to ask shakmaty whether the
/// rights fit standard chess.
fn probe_setup(board: &Board, rights: Bitboard) -> Setup {
    let mut setup = Setup::empty();
    setup.board = board.clone();
    setup.castling_rights = rights;
    setup
}

/// Result of reading a castling field.
struct ParsedCastling {
    /// Rook squares holding a right.
    rights: Bitboard,
    /// Every letter named a valid rook (`-` counts as resolved).
    all_resolved: bool,
    /// The field has `K`/`Q`/`k`/`q` and no file letters.
    letters_only: bool,
}

fn side_of(kingside: bool) -> CastlingSide {
    CastlingSide::from_king_side(kingside)
}

/// Parses the castling field into the set of rook squares holding a right.
///
/// `K`/`Q`/`k`/`q` mean the corner rook in [`FenMode::Standard`] and the
/// outermost rook on that side of the king otherwise. File letters name the
/// rook of that file on the first rank of their colour. Letters that name no
/// rook of the colour are dropped silently, like rights that do not fit the
/// board.
fn parse_castling(field: &str, board: &Board, mode: FenMode) -> Result<ParsedCastling, FenError> {
    if field == "-" {
        return Ok(ParsedCastling {
            rights: Bitboard::EMPTY,
            all_resolved: true,
            letters_only: false,
        });
    }
    let mut rights = Bitboard::EMPTY;
    let mut all_resolved = true;
    let mut has_kq = false;
    let mut has_file = false;
    for ch in field.chars() {
        let (color, file_letter) = match ch {
            'K' | 'Q' => (Color::White, None),
            'k' | 'q' => (Color::Black, None),
            'A'..='H' => (Color::White, File::from_char(ch.to_ascii_lowercase())),
            'a'..='h' => (Color::Black, File::from_char(ch)),
            _ => return Err(FenError::InvalidCastling { field: clip(field) }),
        };
        if file_letter.is_some() {
            has_file = true;
        } else {
            has_kq = true;
        }
        let backrank = color.backrank();
        let rook = if let Some(file) = file_letter {
            Some(Square::from_coords(file, backrank))
        } else {
            let kingside = ch.eq_ignore_ascii_case(&'k');
            if mode == FenMode::Standard {
                let file = if kingside { File::H } else { File::A };
                Some(Square::from_coords(file, backrank))
            } else {
                outermost_rook(board, color, side_of(kingside))
            }
        };
        if let Some(rook) = rook
            && board.piece_at(rook) == Some(Role::Rook.of(color))
            && board
                .king_of(color)
                .is_some_and(|k| k.rank() == backrank && k.file() != rook.file())
        {
            rights.add(rook);
        } else {
            all_resolved = false;
        }
    }
    Ok(ParsedCastling {
        rights,
        all_resolved,
        letters_only: has_kq && !has_file,
    })
}

/// Castling rights for the outermost rooks of both colours (Chess960 FEN
/// without a castling field).
fn outermost_rooks(board: &Board) -> Bitboard {
    let mut rights = Bitboard::EMPTY;
    for color in Color::ALL {
        for side in CastlingSide::ALL {
            if let Some(rook) = outermost_rook(board, color, side) {
                rights.add(rook);
            }
        }
    }
    rights
}

/// Castling rights for a FEN without a castling field: king on its start
/// square and a rook of the same colour in the corner.
fn deduce_castling(board: &Board) -> Bitboard {
    let mut rights = Bitboard::EMPTY;
    for color in Color::ALL {
        let (king, corners) = color.fold_wb(
            (Square::E1, [Square::A1, Square::H1]),
            (Square::E8, [Square::A8, Square::H8]),
        );
        if board.piece_at(king) == Some(Role::King.of(color)) {
            for corner in corners {
                if board.piece_at(corner) == Some(Role::Rook.of(color)) {
                    rights.add(corner);
                }
            }
        }
    }
    rights
}

fn parse_ep(field: &str) -> Result<Square, FenError> {
    let invalid = || FenError::InvalidEnPassant { field: clip(field) };
    let bytes = field.as_bytes();
    if bytes.len() != 2 || !matches!(bytes[1], b'3' | b'6') {
        return Err(invalid());
    }
    Square::from_ascii(bytes).map_err(|_| invalid())
}

/// Parses an optionally negative decimal integer, saturating on overflow.
fn parse_counter(field: &str) -> Option<i64> {
    let (negative, digits) = match field.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, field.strip_prefix('+').unwrap_or(field)),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value: i64 = 0;
    for b in digits.bytes() {
        value = value.saturating_mul(10).saturating_add(i64::from(b - b'0'));
    }
    Some(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn initial_fen_roundtrip() {
        assert_eq!(Position::new().fen(), START);
        assert_eq!(Position::from_fen(START).unwrap(), Position::new());
    }

    #[rstest]
    #[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -")]
    #[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq")]
    #[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w")]
    #[case("rnbqkbnrpppppppp8888PPPPPPPPRNBQKBNR w")]
    #[case("  rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR   w   KQkq   -   0   1  ")]
    fn tolerant_inputs_give_the_initial_position(#[case] fen: &str) {
        assert_eq!(Position::from_fen(fen).unwrap().fen(), START);
    }

    #[test]
    fn missing_castling_is_deduced_from_the_board() {
        let p = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K1N1 w").unwrap();
        assert_eq!(p.fen(), "r3k2r/8/8/8/8/8/8/R3K1N1 w Qkq - 0 1");
    }

    #[test]
    fn incoherent_castling_and_ep_are_discarded() {
        let p = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w KQkq e6 0 1").unwrap();
        assert_eq!(p.fen(), "4k3/8/8/8/8/8/8/4K3 w - - 0 1");
    }

    #[rstest]
    #[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 0", 1, 0)]
    #[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b - - 0 0", 1, 1)]
    #[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b - - -5 -3", 1, 1)]
    #[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 3 10", 10, 18)]
    #[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b - - 3 10", 10, 19)]
    fn move_number_rules(#[case] fen: &str, #[case] number: u32, #[case] ply: u32) {
        let p = Position::from_fen(fen).unwrap();
        assert_eq!(p.fullmove_number(), number);
        assert_eq!(p.ply(), ply);
    }

    #[test]
    fn en_passant_output_conventions() {
        let mut p = Position::new();
        let e4 = p.find_move(Square::E2, Square::E4, None).unwrap();
        p.play(e4).unwrap();
        assert_eq!(
            p.fen(),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
        );
        assert_eq!(
            p.fen_with(EnPassantFen::Always),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"
        );
        let q = Position::from_fen("4k3/8/8/8/3p4/8/4P3/4K3 w - - 0 1").unwrap();
        let q = q
            .after(q.find_move(Square::E2, Square::E4, None).unwrap())
            .unwrap();
        assert_eq!(q.fen(), "4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1");
    }

    #[test]
    fn board_errors() {
        let board_problem = |fen: &str| match Position::from_fen(fen) {
            Err(FenError::InvalidBoard { problem, .. }) => problem,
            other => panic!("unexpected {other:?}"),
        };
        assert!(matches!(
            board_problem("rnbqkbnr/ppppxppp/8/8/8/8/PPPPPPPP/RNBQKBNR w"),
            BoardProblem::UnexpectedChar { ch: 'x', .. }
        ));
        assert!(matches!(
            board_problem("rnbqkbnr/ppp/8/8/8/8/PPPPPPPP/RNBQKBNR w"),
            BoardProblem::MisplacedSlash { .. }
        ));
        assert!(matches!(
            board_problem("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP w"),
            BoardProblem::TooFewSquares { count: 56 }
        ));
        assert_eq!(
            board_problem("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNRR w"),
            BoardProblem::TooManySquares
        );
        assert!(matches!(
            board_problem(""),
            BoardProblem::TooFewSquares { count: 0 }
        ));
    }

    #[test]
    fn field_errors() {
        let b = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR";
        assert!(matches!(
            Position::from_fen(b),
            Err(FenError::InvalidTurn { found: None })
        ));
        assert!(matches!(
            Position::from_fen(&format!("{b} W")),
            Err(FenError::InvalidTurn { found: Some(_) })
        ));
        assert!(matches!(
            Position::from_fen(&format!("{b} w KQxq")),
            Err(FenError::InvalidCastling { .. })
        ));
        assert!(matches!(
            Position::from_fen(&format!("{b} w - e4")),
            Err(FenError::InvalidEnPassant { .. })
        ));
        assert!(matches!(
            Position::from_fen(&format!("{b} w - - x")),
            Err(FenError::InvalidHalfmoveClock { .. })
        ));
        assert!(matches!(
            Position::from_fen(&format!("{b} w - - 0 y")),
            Err(FenError::InvalidMoveNumber { .. })
        ));
    }

    #[rstest]
    #[case(
        "8/8/8/8/8/8/8/4K3 w - - 0 1",
        IllegalPositionReason::MissingKing(Color::Black)
    )]
    #[case(
        "k7/k7/8/8/8/8/8/4K3 w - - 0 1",
        IllegalPositionReason::TooManyKings(Color::Black)
    )]
    #[case(
        "4k3/8/8/8/8/8/8/3KK3 w - - 0 1",
        IllegalPositionReason::TooManyKings(Color::White)
    )]
    #[case("8/8/8/3kK3/8/8/8/8 w - - 0 1", IllegalPositionReason::AdjacentKings)]
    #[case(
        "4k3/8/8/8/8/8/8/P3K3 w - - 0 1",
        IllegalPositionReason::PawnOnBackRank
    )]
    #[case(
        "p3k3/8/8/8/8/8/8/4K3 w - - 0 1",
        IllegalPositionReason::PawnOnBackRank
    )]
    #[case(
        "4k3/8/8/8/8/8/4q3/R3K3 b - - 0 1",
        IllegalPositionReason::OpponentInCheck
    )]
    #[case(
        "rnbqkbnr/pppppppp/pppppppp/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1",
        IllegalPositionReason::TooManyPieces(Color::Black)
    )]
    fn illegal_positions(#[case] fen: &str, #[case] reason: IllegalPositionReason) {
        assert_eq!(
            Position::from_fen(fen),
            Err(FenError::IllegalPosition { reason }),
            "{fen}"
        );
    }

    #[test]
    fn hostile_input_does_not_panic() {
        for s in [
            "",
            " ",
            "/",
            "////",
            "9",
            "w",
            "\u{0}",
            "é/é w",
            &"8".repeat(10_000),
        ] {
            assert!(Position::from_fen(s).is_err());
        }
        let long = format!("{} w", "p".repeat(100_000));
        assert!(Position::from_fen(&long).is_err());
    }
}
