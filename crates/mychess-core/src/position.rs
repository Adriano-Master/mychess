//! The [`Position`] type: board, side to move, rights and move generation.

use std::fmt;

use shakmaty::zobrist::Zobrist64;
use shakmaty::{
    Board, CastlingMode, CastlingSide, Chess, Color, EnPassantMode, FromSetup, Piece,
    Position as _, PositionError, PositionErrorKinds, Role, Setup, Square,
};

use crate::error::MoveError;
use crate::moves::Move;

/// A 64-bit Zobrist hash of a position.
///
/// The hash covers the piece placement, side to move, castling rights and the
/// en passant square when an en passant capture is actually possible. It does
/// not cover the halfmove clock or the move number. Values are stable across
/// runs and platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ZobristHash(u64);

impl ZobristHash {
    /// The raw 64-bit value.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<ZobristHash> for u64 {
    fn from(hash: ZobristHash) -> u64 {
        hash.0
    }
}

impl fmt::Display for ZobristHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// A chess position with its legal-move machinery.
///
/// Built on `shakmaty`, but the public API only uses `mychess-core` types.
/// The castling mode (standard or Chess960) is stored in the position.
///
/// `==` and [`Hash`] compare the board, side to move, castling rights and en
/// passant square, but **ignore the clocks** (halfmove clock and move
/// number). Two positions reached by different move orders or at different
/// move numbers are therefore equal, which is what transposition lookups
/// want. The castling mode is **not** compared either: the standard start and
/// Chess960 number 518 are equal (and have the same [`Position::zobrist`]). Compare [`Position::halfmove_clock`] and
/// [`Position::fullmove_number`] explicitly when the clocks matter.
#[derive(Debug, Clone)]
pub struct Position {
    pub(crate) inner: Chess,
}

// Manual `PartialEq`/`Hash`: shakmaty's derived versions compare the
// pseudolegal en passant square, so `... w - d6` (capture illegal, e.g. pinned
// pawn) would differ from `... w - -`. Only a legal en passant capture counts
// (spec D1/D40), consistent with `ep_square()` and `zobrist()`.
impl PartialEq for Position {
    fn eq(&self, other: &Position) -> bool {
        self.inner.turn() == other.inner.turn()
            && self.inner.board() == other.inner.board()
            && self.inner.castles().castling_rights() == other.inner.castles().castling_rights()
            && self.inner.ep_square(EnPassantMode::Legal)
                == other.inner.ep_square(EnPassantMode::Legal)
    }
}

impl Eq for Position {}

impl std::hash::Hash for Position {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.inner.turn().hash(state);
        self.inner.board().hash(state);
        self.inner.castles().castling_rights().hash(state);
        self.inner.ep_square(EnPassantMode::Legal).hash(state);
    }
}

impl Default for Position {
    fn default() -> Self {
        Position::new()
    }
}

/// Builds a [`Chess`] from a setup, tolerating what the Scid tolerates:
/// inconsistent castling rights and en passant squares are discarded, and
/// excess material or implausible checks are accepted.
pub(crate) fn chess_from_setup(
    setup: Setup,
    mode: CastlingMode,
) -> Result<Chess, PositionErrorKinds> {
    Chess::from_setup(setup, mode)
        .or_else(PositionError::ignore_invalid_castling_rights)
        .or_else(PositionError::ignore_invalid_ep_square)
        .or_else(PositionError::ignore_too_much_material)
        .or_else(PositionError::ignore_impossible_check)
        .map_err(|e| e.kinds())
}

/// The outermost own rook on the first rank on `side` of the king, if the
/// king stands on its first rank.
pub(crate) fn outermost_rook(board: &Board, color: Color, side: CastlingSide) -> Option<Square> {
    let king = board
        .king_of(color)
        .filter(|k| k.rank() == color.backrank())?;
    let rooks = board.by_piece(Role::Rook.of(color)) & color.backrank();
    match side {
        CastlingSide::KingSide => rooks.into_iter().rev().find(|r| r.file() > king.file()),
        CastlingSide::QueenSide => rooks.into_iter().find(|r| r.file() < king.file()),
    }
}

impl Position {
    /// The standard starting position.
    pub fn new() -> Position {
        Position {
            inner: Chess::new(),
        }
    }

    /// The side to move.
    pub fn turn(&self) -> Color {
        self.inner.turn()
    }

    /// Halfmoves since the last capture or pawn move.
    pub fn halfmove_clock(&self) -> u32 {
        self.inner.halfmoves()
    }

    /// The move number, starting at 1 and incremented after Black moves.
    pub fn fullmove_number(&self) -> u32 {
        self.inner.fullmoves().get()
    }

    /// The number of halfmoves played since move number 1 with White to move.
    pub fn ply(&self) -> u32 {
        (self.fullmove_number() - 1)
            .saturating_mul(2)
            .saturating_add(u32::from(self.turn() == Color::Black))
    }

    /// The castling mode this position was created with.
    pub fn castling_mode(&self) -> CastlingMode {
        self.inner.castles().mode()
    }

    /// The piece on `sq`, if any.
    pub fn piece_at(&self, sq: Square) -> Option<Piece> {
        self.inner.board().piece_at(sq)
    }

    /// All pieces on the board with their squares.
    pub fn pieces(&self) -> impl Iterator<Item = (Square, Piece)> + '_ {
        self.inner.board().iter()
    }

    /// The square of `color`'s king.
    pub fn king_square(&self, color: Color) -> Option<Square> {
        self.inner.board().king_of(color)
    }

    /// Whether `color` still has the right to castle to `side`.
    pub fn has_castling_right(&self, color: Color, side: CastlingSide) -> bool {
        self.inner.castles().has(color, side)
    }

    /// The en passant target square, only when a legal en passant capture
    /// exists.
    pub fn ep_square(&self) -> Option<Square> {
        self.inner.ep_square(EnPassantMode::Legal)
    }

    /// The en passant target square when a pawn of the side to move stands
    /// next to the pawn that just advanced two squares, **without** checking
    /// that the capture is legal (pinned capturers count). This is the rule
    /// used by the Polyglot key and by [`EnPassantFen::Adjacent`](crate::EnPassantFen).
    pub fn ep_square_pseudo_legal(&self) -> Option<Square> {
        self.inner.ep_square(EnPassantMode::PseudoLegal)
    }

    /// Whether the side to move is in check.
    pub fn is_check(&self) -> bool {
        self.inner.is_check()
    }

    /// Whether the side to move is checkmated.
    pub fn is_checkmate(&self) -> bool {
        self.inner.is_checkmate()
    }

    /// Whether the side to move is stalemated.
    pub fn is_stalemate(&self) -> bool {
        self.inner.is_stalemate()
    }

    /// Whether neither side can ever deliver checkmate (FIDE rules: K v K,
    /// K+B v K, K+N v K, and bishops all on the same colour). K+N v K+N is
    /// not counted as insufficient.
    pub fn is_insufficient_material(&self) -> bool {
        self.inner.is_insufficient_material()
    }

    /// Whether `color` can never deliver checkmate even with the help of the
    /// opponent (shakmaty's rule: K+B+B on one colour is insufficient, K+N+N
    /// is not).
    pub fn has_insufficient_material(&self, color: Color) -> bool {
        self.inner.has_insufficient_material(color)
    }

    /// All legal moves. The null move is not included.
    pub fn legal_moves(&self) -> Vec<Move> {
        self.inner
            .legal_moves()
            .into_iter()
            .filter_map(Move::from_shakmaty)
            .collect()
    }

    /// The legal moves whose origin square is `from`.
    pub fn legal_moves_from(&self, from: Square) -> Vec<Move> {
        let mut moves = self.legal_moves();
        moves.retain(|m| m.from() == Some(from));
        moves
    }

    /// Finds the legal move from `from` to `to` (with `promotion` for pawn
    /// promotions). Castling is matched by the rook's square ("king takes
    /// own rook"); failing that, an ordinary move wins; failing that,
    /// castling is matched by the king's destination square. The order
    /// matters in Chess960, where `Kg1` can be an ordinary move or castling.
    pub fn find_move(&self, from: Square, to: Square, promotion: Option<Role>) -> Option<Move> {
        let moves = self.legal_moves();
        let candidates = || {
            moves
                .iter()
                .copied()
                .filter(|m| m.from() == Some(from) && m.promotion() == promotion)
        };
        candidates()
            .find(|m| matches!(m, Move::Castle { rook_from, .. } if *rook_from == to))
            .or_else(|| candidates().find(|m| !m.is_castle() && m.to() == Some(to)))
            .or_else(|| candidates().find(|m| m.is_castle() && m.to() == Some(to)))
    }

    /// Whether `m` is legal. The null move is legal when not in check.
    pub fn is_legal(&self, m: Move) -> bool {
        match m.to_shakmaty() {
            None => !self.is_check(),
            Some(sm) => self.inner.is_legal(sm),
        }
    }

    /// Plays `m` on this position.
    ///
    /// On error the position is left unchanged.
    ///
    /// The null move swaps the side to move, clears the en passant square,
    /// advances the halfmove clock (and the move number after Black) and
    /// keeps the castling rights. It is refused while in check.
    ///
    /// ```
    /// use mychess_core::Position;
    ///
    /// let mut pos = Position::new();
    /// let e4 = pos.parse_uci("e2e4")?;
    /// pos.play(e4)?;
    /// assert_eq!(pos.fen(), "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1");
    /// assert!(pos.play(e4).is_err()); // no longer legal
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn play(&mut self, m: Move) -> Result<(), MoveError> {
        match m.to_shakmaty() {
            None => self.play_null(),
            Some(sm) => {
                if !self.inner.is_legal(sm) {
                    return Err(MoveError::Illegal {
                        uci: m.to_uci(self.castling_mode()),
                        fen: self.fen(),
                    });
                }
                self.inner.play_unchecked(sm);
                Ok(())
            }
        }
    }

    /// Like [`Position::play`], but also accepts a castling move whose right
    /// was lost, provided king and rook stand on their original squares and
    /// castling is otherwise possible (path free, no check on the way). This
    /// is what [`SanMode::Tolerant`](crate::SanMode::Tolerant) reports with
    /// [`SanWarning::IllegalCastling`](crate::SanWarning::IllegalCastling).
    pub fn play_lenient(&mut self, m: Move) -> Result<(), MoveError> {
        if let Some(side) = m.castling_side()
            && !self.is_legal(m)
            && let Some(mut granted) = self.with_castling_right(side)
        {
            granted.play(m)?;
            *self = granted;
            return Ok(());
        }
        self.play(m)
    }

    /// Returns a copy of this position after playing `m`.
    pub fn after(&self, m: Move) -> Result<Position, MoveError> {
        let mut next = self.clone();
        next.play(m)?;
        Ok(next)
    }

    /// A copy of the position where the side to move has the castling right
    /// for `side` restored (when king and rook are on their start squares).
    pub(crate) fn with_castling_right(&self, side: CastlingSide) -> Option<Position> {
        let color = self.turn();
        let mode = self.castling_mode();
        let rook = match mode {
            CastlingMode::Standard => {
                let rank_sq = |kingside: Square, queenside: Square| match side {
                    CastlingSide::KingSide => kingside,
                    CastlingSide::QueenSide => queenside,
                };
                color.fold_wb(
                    rank_sq(Square::H1, Square::A1),
                    rank_sq(Square::H8, Square::A8),
                )
            }
            CastlingMode::Chess960 => {
                // The right must be missing, the king on its first rank and
                // the rook is the outermost one on that side.
                if self.castling_rook(color, side).is_some() {
                    return None;
                }
                outermost_rook(self.inner.board(), color, side)?
            }
        };
        let mut setup = self.inner.to_setup(EnPassantMode::Always);
        setup.castling_rights.add(rook);
        let inner = chess_from_setup(setup, mode).ok()?;
        Some(Position { inner })
    }

    fn play_null(&mut self) -> Result<(), MoveError> {
        if self.is_check() {
            return Err(MoveError::NullMoveInCheck { fen: self.fen() });
        }
        let mode = self.castling_mode();
        let mover = self.turn();
        let mut setup = self.inner.to_setup(EnPassantMode::Always);
        setup.swap_turn();
        setup.halfmoves = setup.halfmoves.saturating_add(1);
        if mover == Color::Black {
            setup.fullmoves = setup.fullmoves.saturating_add(1);
        }
        match chess_from_setup(setup, mode) {
            Ok(inner) => {
                self.inner = inner;
                Ok(())
            }
            Err(_) => Err(MoveError::NullMoveRejected { fen: self.fen() }),
        }
    }

    /// The Zobrist hash of the position (see [`ZobristHash`]).
    pub fn zobrist(&self) -> ZobristHash {
        let Zobrist64(value) = self.inner.zobrist_hash::<Zobrist64>(EnPassantMode::Legal);
        ZobristHash(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_of(p: &Position) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        p.hash(&mut h);
        h.finish()
    }

    #[test]
    fn illegal_en_passant_equals_no_en_passant() {
        // The e5 pawn is pinned along the rank, so exd6 is illegal.
        let a = Position::from_fen("8/8/8/K2pP2r/8/8/8/7k w - d6").unwrap();
        let b = Position::from_fen("8/8/8/K2pP2r/8/8/8/7k w - -").unwrap();
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_eq!(a.zobrist(), b.zobrist());
        assert_eq!(a.ep_square(), None);
    }

    #[test]
    fn legal_en_passant_still_distinguishes() {
        let a = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6").unwrap();
        let b = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - -").unwrap();
        assert_ne!(a, b);
        assert_ne!(a.zobrist(), b.zobrist());
        assert!(a.ep_square().is_some());
    }

    #[test]
    fn initial_position_basics() {
        let pos = Position::new();
        assert_eq!(pos.turn(), Color::White);
        assert_eq!(pos.legal_moves().len(), 20);
        assert_eq!(pos.fullmove_number(), 1);
        assert_eq!(pos.ply(), 0);
        assert_eq!(pos.castling_mode(), CastlingMode::Standard);
        assert_eq!(pos.pieces().count(), 32);
        assert_eq!(pos.king_square(Color::Black), Some(Square::E8));
        assert!(pos.has_castling_right(Color::White, CastlingSide::KingSide));
        assert_eq!(pos.zobrist().value(), 0x463b_9618_1691_fc9c);
    }

    #[test]
    fn play_updates_state_and_rejects_illegal() {
        let mut pos = Position::new();
        let e4 = pos.find_move(Square::E2, Square::E4, None).unwrap();
        pos.play(e4).unwrap();
        assert_eq!(pos.turn(), Color::Black);
        assert_eq!(pos.ply(), 1);
        let before = pos.clone();
        let bad = Move::Normal {
            role: Role::Pawn,
            from: Square::E2,
            to: Square::E4,
            capture: None,
            promotion: None,
        };
        assert!(matches!(pos.play(bad), Err(MoveError::Illegal { .. })));
        assert_eq!(pos, before);
    }

    #[test]
    fn null_move_behaviour() {
        let mut pos = Position::new();
        pos.play(Move::Null).unwrap();
        assert_eq!(pos.turn(), Color::Black);
        assert_eq!(pos.halfmove_clock(), 1);
        assert_eq!(pos.fullmove_number(), 1);
        pos.play(Move::Null).unwrap();
        assert_eq!(pos.turn(), Color::White);
        assert_eq!(pos.fullmove_number(), 2);
        assert!(pos.has_castling_right(Color::Black, CastlingSide::QueenSide));
    }

    #[test]
    fn find_move_matches_castling_by_king_target() {
        let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        let m = pos.find_move(Square::E1, Square::G1, None).unwrap();
        assert!(m.is_castle());
        assert_eq!(m.to(), Some(Square::G1));
    }

    fn perft(pos: &Position, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        pos.legal_moves()
            .into_iter()
            .map(|m| perft(&pos.after(m).unwrap(), depth - 1))
            .sum()
    }

    #[test]
    fn perft_initial_and_kiwipete() {
        assert_eq!(perft(&Position::new(), 4), 197_281);
        let kiwi = Position::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        assert_eq!(perft(&kiwi, 3), 97_862);
    }

    /// Every legal move of every position up to depth 2 round-trips through
    /// SAN, UCI and FEN.
    fn check_roundtrips(pos: &Position, depth: u32) {
        assert_eq!(Position::from_fen(&pos.fen()).unwrap().fen(), pos.fen());
        if depth == 0 {
            return;
        }
        for m in pos.legal_moves() {
            let san = pos.san(m).unwrap();
            assert_eq!(
                pos.parse_san(&san, crate::SanMode::Strict).unwrap().mv,
                m,
                "{san}"
            );
            assert_eq!(pos.parse_uci(&pos.uci(m)), Ok(m));
            check_roundtrips(&pos.after(m).unwrap(), depth - 1);
        }
    }

    #[test]
    fn san_uci_fen_roundtrips() {
        check_roundtrips(&Position::new(), 3);
        let kiwi = Position::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        check_roundtrips(&kiwi, 2);
        let promo = Position::from_fen("n1n5/PPPk4/8/8/8/8/4Kppp/5N1N b - - 0 1").unwrap();
        check_roundtrips(&promo, 2);
    }
}
