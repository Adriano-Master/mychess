// Derived from Scid vs PC 4.27, src/position.cpp (piece list, DoSimpleMove/UndoSimpleMove, ReadFromFEN).
// Copyright (c) 1999-2003 Shane Hudson. All rights reserved (notice of
// position.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! A minimal position with Scid's piece list, used only to decode moves
//! (`docs/formatos/si4.md`, section 9.4; decision D264).
//!
//! A move byte names the moving piece by its index in the piece list of the
//! side to move, so the decoder must reproduce the order of that list exactly
//! (`Position::List` in Scid): it depends on the start position, on captures
//! (swap with the last piece) and not on any move generator. The type tracks
//! only what the codec needs: board, lists, side to move.

use mychess_core::{Color, Piece, Role, Square};

use super::raw::RawMove;

use super::error::StartPositionError;

/// Information needed to take back one move (`UndoSimpleMove`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Undo {
    from: Square,
    to: Square,
    captured: Option<Captured>,
    promoted: bool,
    castle: bool,
    null: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Captured {
    piece: Piece,
    square: Square,
    /// Index of the piece in its list before the capture.
    index: u8,
}

/// The board plus Scid's piece lists.
///
/// Equality compares the board, the side to move and the lists up to their
/// counts (stale entries of `list_pos` for empty squares are ignored).
#[derive(Debug, Clone)]
pub struct ListPosition {
    board: [Option<Piece>; 64],
    list: [[Square; 16]; 2],
    count: [u8; 2],
    list_pos: [u8; 64],
    turn: Color,
}

impl PartialEq for ListPosition {
    fn eq(&self, other: &ListPosition) -> bool {
        self.turn == other.turn
            && self.board == other.board
            && [Color::White, Color::Black]
                .iter()
                .all(|&c| self.piece_squares(c) == other.piece_squares(c))
    }
}

impl Eq for ListPosition {}

fn side(color: Color) -> usize {
    usize::from(color == Color::Black)
}

fn at(sq: Square) -> usize {
    // A `Square` is always in 0..64, so the cast cannot truncate.
    sq.to_u32() as usize
}

impl ListPosition {
    fn empty() -> ListPosition {
        ListPosition {
            board: [None; 64],
            list: [[Square::A1; 16]; 2],
            count: [0; 2],
            list_pos: [0; 64],
            turn: Color::White,
        }
    }

    /// The standard start position with Scid's list order: king, rook a,
    /// knight b, bishop c, queen, bishop f, knight g, rook h, then the pawns
    /// a to h (`position.cpp`, `StdStart`).
    pub fn standard() -> ListPosition {
        const BACK: [Role; 8] = [
            Role::King,
            Role::Rook,
            Role::Knight,
            Role::Bishop,
            Role::Queen,
            Role::Bishop,
            Role::Knight,
            Role::Rook,
        ];
        const BACK_FILES: [usize; 8] = [4, 0, 1, 2, 3, 5, 6, 7];
        let mut pos = ListPosition::empty();
        for (color, back_rank, pawn_rank) in
            [(Color::White, 0usize, 8usize), (Color::Black, 56, 48)]
        {
            for (i, (&role, &file)) in BACK.iter().zip(BACK_FILES.iter()).enumerate() {
                pos.place(Piece { color, role }, back_rank + file, i);
            }
            for file in 0..8 {
                pos.place(
                    Piece {
                        color,
                        role: Role::Pawn,
                    },
                    pawn_rank + file,
                    8 + file,
                );
            }
            if let Some(c) = pos.count.get_mut(side(color)) {
                *c = 16;
            }
        }
        pos
    }

    /// Places a piece during construction.
    fn place(&mut self, piece: Piece, square_index: usize, list_index: usize) {
        let Ok(sq) = Square::try_from(square_index) else {
            return;
        };
        if let Some(cell) = self.board.get_mut(at(sq)) {
            *cell = Some(piece);
        }
        if let Some(slot) = self
            .list
            .get_mut(side(piece.color))
            .and_then(|l| l.get_mut(list_index))
        {
            *slot = sq;
        }
        if let (Some(pos), Ok(index)) = (self.list_pos.get_mut(at(sq)), u8::try_from(list_index)) {
            *pos = index;
        }
    }

    /// Builds the position of a FEN (placement and side to move only).
    ///
    /// Pieces enter the lists in reading order (a8 to h1) except the king,
    /// which goes to index 0 and pushes the piece there to the end
    /// (`Position::AddPiece`). Validation (decision D259): 8 ranks of 8
    /// squares, valid letters, one king per side, at most 16 pieces per side,
    /// no pawn on the first or last rank, a side to move of `w` or `b`. The
    /// legality of the position is **not** checked here.
    pub fn from_fen(fen: &str) -> Result<ListPosition, StartPositionError> {
        let mut pos = ListPosition::empty();
        let mut chars = fen.chars();
        let mut count = 0usize;
        let mut kings = [false; 2];
        // Square count at which the last '/' was read: one per rank boundary.
        let mut last_slash = None;
        while count < 64 {
            let Some(ch) = chars.next() else {
                return Err(StartPositionError::new("placement ends before 64 squares"));
            };
            match ch {
                '/' => {
                    if count == 0 {
                        return Err(StartPositionError::new("leading '/' in the placement"));
                    }
                    if !count.is_multiple_of(8) {
                        return Err(StartPositionError::new("misplaced '/' in the placement"));
                    }
                    if last_slash == Some(count) {
                        return Err(StartPositionError::new("repeated '/' in the placement"));
                    }
                    last_slash = Some(count);
                }
                '1'..='8' => {
                    let skip = usize::from(ch as u8 - b'0');
                    if count % 8 + skip > 8 {
                        return Err(StartPositionError::new(
                            "empty squares cross the end of a rank",
                        ));
                    }
                    count += skip;
                }
                _ => {
                    let piece = Piece::from_char(ch)
                        .ok_or(StartPositionError::new("invalid piece letter"))?;
                    let rank = 7 - count / 8;
                    let sq = rank * 8 + count % 8;
                    if piece.role == Role::Pawn && (rank == 0 || rank == 7) {
                        return Err(StartPositionError::new("pawn on the first or last rank"));
                    }
                    pos.add_piece(piece, sq, &mut kings)?;
                    count += 1;
                }
            }
        }
        if kings != [true, true] {
            return Err(StartPositionError::new("each side needs exactly one king"));
        }
        let rest = chars.as_str();
        let turn = rest.trim_start().chars().next();
        pos.turn = match turn {
            Some('w') => Color::White,
            Some('b') => Color::Black,
            _ => return Err(StartPositionError::new("side to move is not w or b")),
        };
        Ok(pos)
    }

    fn add_piece(
        &mut self,
        piece: Piece,
        square_index: usize,
        kings: &mut [bool; 2],
    ) -> Result<(), StartPositionError> {
        let s = side(piece.color);
        let n = usize::from(self.count.get(s).copied().unwrap_or(16));
        if n >= 16 {
            return Err(StartPositionError::new("more than 16 pieces for a side"));
        }
        Square::try_from(square_index)
            .map_err(|_| StartPositionError::new("square out of range"))?;
        if piece.role == Role::King {
            let flag = kings
                .get_mut(s)
                .ok_or(StartPositionError::new("square out of range"))?;
            if *flag {
                return Err(StartPositionError::new("more than one king for a side"));
            }
            *flag = true;
            // The king goes to index 0; the piece there moves to the end.
            if n > 0 {
                let old = self.list.get(s).and_then(|l| l.first()).copied();
                if let (Some(old), Some(slot)) =
                    (old, self.list.get_mut(s).and_then(|l| l.get_mut(n)))
                {
                    *slot = old;
                    if let (Some(lp), Ok(idx)) = (self.list_pos.get_mut(at(old)), u8::try_from(n)) {
                        *lp = idx;
                    }
                }
            }
            self.place(piece, square_index, 0);
        } else {
            self.place(piece, square_index, n);
        }
        if let Some(c) = self.count.get_mut(s) {
            *c += 1;
        }
        Ok(())
    }

    /// The side to move.
    pub fn turn(&self) -> Color {
        self.turn
    }

    /// The piece on a square.
    pub fn piece_at(&self, sq: Square) -> Option<Piece> {
        self.board.get(at(sq)).copied().flatten()
    }

    /// The squares of the pieces of `color`, in piece-list order.
    pub fn piece_squares(&self, color: Color) -> &[Square] {
        let n = usize::from(self.count.get(side(color)).copied().unwrap_or(0));
        self.list
            .get(side(color))
            .and_then(|l| l.get(..n))
            .unwrap_or(&[])
    }

    /// The square of piece number `index` of the side to move, if any.
    pub(crate) fn mover_square(&self, index: u8) -> Option<Square> {
        self.piece_squares(self.turn)
            .get(usize::from(index))
            .copied()
    }

    /// The index in the piece list of the side to move of the piece on `sq`,
    /// or `None` if that square does not hold one of its pieces. This is the
    /// piece number of a move byte (`docs/formatos/si4.md`, section 9.4).
    pub(crate) fn mover_index(&self, sq: Square) -> Option<u8> {
        let index = self.list_pos.get(at(sq)).copied()?;
        (self.mover_square(index) == Some(sq)).then_some(index)
    }

    fn index_of(&self, sq: Square) -> u8 {
        self.list_pos.get(at(sq)).copied().unwrap_or(0)
    }

    fn set_board(&mut self, sq: Square, piece: Option<Piece>) {
        if let Some(cell) = self.board.get_mut(at(sq)) {
            *cell = piece;
        }
    }

    fn set_list(&mut self, color: Color, index: u8, sq: Square) {
        if let Some(slot) = self
            .list
            .get_mut(side(color))
            .and_then(|l| l.get_mut(usize::from(index)))
        {
            *slot = sq;
        }
        if let Some(pos) = self.list_pos.get_mut(at(sq)) {
            *pos = index;
        }
    }

    /// The squares a castling king and its rook use, from the king's
    /// destination: `(rook_from, rook_to)`. Scid recognises castling only by
    /// the king leaving the e-file for the c- or g-file.
    fn castle_rook_squares(from: Square, to: Square, role: Role) -> Option<(Square, Square)> {
        use mychess_core::File;
        if role != Role::King || from.file() != File::E {
            return None;
        }
        match to.file() {
            File::C => Some((to.offset(-2)?, to.offset(1)?)),
            File::G => Some((to.offset(1)?, to.offset(-1)?)),
            _ => None,
        }
    }

    /// Plays a move for the side to move (`Position::DoSimpleMove`).
    ///
    /// Legality is not checked. An inconsistency that would corrupt the
    /// lists (no piece on `from`, capturing an own piece, castling without a
    /// rook, a promotion of a non-pawn) is an error and leaves the position
    /// unchanged.
    pub(crate) fn make(&mut self, mv: RawMove) -> Result<Undo, &'static str> {
        let us = self.turn;
        let them = !us;
        let (from, to) = (mv.from, mv.to);
        let piece = self
            .piece_at(from)
            .filter(|p| p.color == us)
            .ok_or("no piece of the side to move on the origin square")?;
        let mut undo = Undo {
            from,
            to,
            captured: None,
            promoted: false,
            castle: false,
            null: false,
        };
        if from == to {
            if piece.role != Role::King {
                return Err("non-king piece moves to its own square");
            }
            undo.null = true;
            self.turn = them;
            return Ok(undo);
        }

        // Find the captured piece (en passant: the pawn behind `to`).
        let mut captured = None;
        match self.piece_at(to) {
            Some(target) if target.color == us => return Err("capture of an own piece"),
            Some(target) => captured = Some((target, to)),
            None if piece.role == Role::Pawn && from.file() != to.file() => {
                let behind = to
                    .offset(if us == Color::White { -8 } else { 8 })
                    .ok_or("en passant square out of range")?;
                let pawn = Piece {
                    color: them,
                    role: Role::Pawn,
                };
                if self.piece_at(behind) != Some(pawn) {
                    return Err("en passant without a pawn to capture");
                }
                captured = Some((pawn, behind));
            }
            None => {}
        }
        let castle_rooks = Self::castle_rook_squares(from, to, piece.role);
        let rook = Piece {
            color: us,
            role: Role::Rook,
        };
        if let Some((rook_from, _)) = castle_rooks
            && self.piece_at(rook_from) != Some(rook)
        {
            return Err("castling without a rook");
        }
        if mv.promotion.is_some() && piece.role != Role::Pawn {
            return Err("promotion of a piece that is not a pawn");
        }

        // All checks passed: mutate.
        if let Some((cap_piece, cap_sq)) = captured {
            let index = self.index_of(cap_sq);
            let s = side(them);
            let n = self.count.get(s).copied().unwrap_or(0);
            let last_index = n.checked_sub(1).ok_or("capture from an empty piece list")?;
            let last_sq = self
                .list
                .get(s)
                .and_then(|l| l.get(usize::from(last_index)))
                .copied()
                .ok_or("piece list out of range")?;
            if let Some(c) = self.count.get_mut(s) {
                *c = last_index;
            }
            self.set_list(them, index, last_sq);
            self.set_board(cap_sq, None);
            undo.captured = Some(Captured {
                piece: cap_piece,
                square: cap_sq,
                index,
            });
        }
        let index = self.index_of(from);
        let mut moved = piece;
        if let Some(role) = mv.promotion {
            moved = Piece { color: us, role };
            undo.promoted = true;
        }
        self.set_list(us, index, to);
        self.set_board(from, None);
        self.set_board(to, Some(moved));
        if let Some((rook_from, rook_to)) = castle_rooks {
            let rook_index = self.index_of(rook_from);
            self.set_list(us, rook_index, rook_to);
            self.set_board(rook_from, None);
            self.set_board(rook_to, Some(rook));
            undo.castle = true;
        }
        self.turn = them;
        Ok(undo)
    }

    /// Takes back the last move made with [`ListPosition::make`]
    /// (`Position::UndoSimpleMove`), restoring the lists exactly.
    pub(crate) fn unmake(&mut self, undo: Undo) -> Result<(), &'static str> {
        self.turn = !self.turn;
        if undo.null {
            return Ok(());
        }
        let us = self.turn;
        let them = !us;
        let (from, to) = (undo.from, undo.to);
        let moved = self.piece_at(to).ok_or("no piece to take back")?;
        // Read before the captured piece is put back: both may share `to`.
        let index = self.index_of(to);

        if let Some(cap) = undo.captured {
            let s = side(them);
            let n = self.count.get(s).copied().unwrap_or(0);
            if n >= 16 {
                return Err("piece list overflow while taking back");
            }
            // The piece now at `cap.index` goes back to the end of the list.
            let displaced = self
                .list
                .get(s)
                .and_then(|l| l.get(usize::from(cap.index)))
                .copied()
                .ok_or("piece list out of range")?;
            self.set_list(them, n, displaced);
            self.set_list(them, cap.index, cap.square);
            if let Some(c) = self.count.get_mut(s) {
                *c = n + 1;
            }
        }
        let original = if undo.promoted {
            Piece {
                color: us,
                role: Role::Pawn,
            }
        } else {
            moved
        };
        self.set_list(us, index, from);
        self.set_board(to, None);
        self.set_board(from, Some(original));
        if let Some(cap) = undo.captured {
            self.set_board(cap.square, Some(cap.piece));
        }
        if undo.castle
            && let Some((rook_from, rook_to)) = Self::castle_rook_squares(from, to, Role::King)
        {
            let rook_index = self.index_of(rook_to);
            self.set_list(us, rook_index, rook_from);
            self.set_board(rook_to, None);
            self.set_board(
                rook_from,
                Some(Piece {
                    color: us,
                    role: Role::Rook,
                }),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(from: Square, to: Square) -> RawMove {
        RawMove {
            from,
            to,
            promotion: None,
        }
    }

    #[test]
    fn standard_list_order_matches_scid() {
        let pos = ListPosition::standard();
        let white = pos.piece_squares(Color::White);
        assert_eq!(white.len(), 16);
        assert_eq!(
            &white[..8],
            &[
                Square::E1,
                Square::A1,
                Square::B1,
                Square::C1,
                Square::D1,
                Square::F1,
                Square::G1,
                Square::H1
            ]
        );
        assert_eq!(white[12], Square::E2);
        let black = pos.piece_squares(Color::Black);
        assert_eq!(
            (black[0], black[4], black[15]),
            (Square::E8, Square::D8, Square::H7)
        );
        assert_eq!(pos.turn(), Color::White);
    }

    #[test]
    fn fen_puts_the_king_first_and_pushes_the_piece_at_zero() {
        // White: rook a1 is read after the king on e1? Reading order is a8..h1,
        // so for rank 1 the rook (a1) comes first, then the king (e1).
        let pos = ListPosition::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
        // Rook added first at index 0; the king takes index 0 and the rook
        // moves to the end (index 1).
        assert_eq!(pos.piece_squares(Color::White), &[Square::E1, Square::A1]);
        // King read first: stays at index 0.
        let pos = ListPosition::from_fen("k7/8/8/8/8/8/8/K1R5 b - - 0 1").unwrap();
        assert_eq!(pos.piece_squares(Color::White), &[Square::A1, Square::C1]);
        assert_eq!(pos.turn(), Color::Black);
    }

    #[test]
    fn fen_with_the_king_after_several_pieces() {
        // a8..h1 reading order: black king on h8 is after the rook a8 and
        // knight b8.
        let pos = ListPosition::from_fen("rn5k/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        // rook -> [a8]; knight -> [a8, b8]; king -> [h8, b8, a8]
        assert_eq!(
            pos.piece_squares(Color::Black),
            &[Square::H8, Square::B8, Square::A8]
        );
    }

    #[test]
    fn fen_validation() {
        let bad = [
            "4k3/8/8/8/8/8/8/4K3",             // no side to move
            "4k3/8/8/8/8/8/8/4K3 x - - 0 1",   // bad side
            "4k3/8/8/8/8/8/8/8 w - - 0 1",     // one king missing
            "4k3/8/8/8/8/8/8/4KK2 w - - 0 1",  // two white kings
            "4k3/8/8/8/8/8/8/P3K3 w - - 0 1",  // pawn on rank 1
            "4k2P/8/8/8/8/8/8/4K3 w - - 0 1",  // pawn on rank 8
            "4k3/8/8/8/8/8/8/4K2X w - - 0 1",  // bad letter
            "4k3/8/8/8/8/8/8/4K w - - 0 1",    // short rank
            "4k3/8/8/8/8/8/8/4K4 w - - 0 1",   // rank too long
            "4k3/8/8/8/8/8/8 w - - 0 1",       // seven ranks
            "4k3/8/8/8/8/8/8/9K3 w - - 0 1",   // 9 is not a digit of the format
            "4k3//8/8/8/8/8/8/4K3 w - - 0 1",  // two '/' at one rank boundary
            "//4k3/8/8/8/8/8/8/4K3 w - - 0 1", // two '/' at the start
            "/4k3/8/8/8/8/8/8/4K3 w - - 0 1",  // leading '/'
        ];
        for fen in bad {
            assert!(ListPosition::from_fen(fen).is_err(), "{fen}");
        }
        // 17 white pieces: 8 pawns, 8 other pieces plus the king.
        assert!(
            ListPosition::from_fen("4k3/8/8/8/8/PPPPPPPP/QQQQQQQQ/4K3 w - - 0 1").is_err(),
            "17 pieces"
        );
        assert!(ListPosition::from_fen("4k3/8/8/8/8/PPPPPPPP/QQQQQQQ1/4K3 w - - 0 1").is_ok());
    }

    #[test]
    fn capture_swaps_with_the_last_piece_and_undo_restores_the_order() {
        let mut pos = ListPosition::standard();
        let before = pos.clone();
        // 1.e4 d5 2.exd5: black's d7 pawn (index 11) is captured.
        let mut undos = Vec::new();
        undos.push(pos.make(mv(Square::E2, Square::E4)).unwrap());
        undos.push(pos.make(mv(Square::D7, Square::D5)).unwrap());
        let mid = pos.clone();
        let cap = pos.make(mv(Square::E4, Square::D5)).unwrap();
        let black = pos.piece_squares(Color::Black);
        assert_eq!(black.len(), 15);
        // Index 11 (d-pawn) now holds the former last piece (h7 pawn).
        assert_eq!(black[11], Square::H7);
        assert_eq!(
            pos.piece_at(Square::D5).map(|p| p.color),
            Some(Color::White)
        );
        pos.unmake(cap).unwrap();
        assert_eq!(pos, mid);
        while let Some(u) = undos.pop() {
            pos.unmake(u).unwrap();
        }
        assert_eq!(pos, before);
    }

    #[test]
    fn castling_en_passant_promotion_and_null_round_trip() {
        let mut pos = ListPosition::from_fen("r3k2r/1P6/8/3pP3/8/8/8/R3K2R w KQkq d6 0 1").unwrap();
        let start = pos.clone();
        // Castle kingside: the rook changes square but not its list index.
        let rook_index = pos
            .piece_squares(Color::White)
            .iter()
            .position(|&s| s == Square::H1);
        let u = pos.make(mv(Square::E1, Square::G1)).unwrap();
        assert_eq!(pos.piece_at(Square::F1).map(|p| p.role), Some(Role::Rook));
        assert_eq!(
            pos.piece_squares(Color::White)
                .iter()
                .position(|&s| s == Square::F1),
            rook_index
        );
        pos.unmake(u).unwrap();
        assert_eq!(pos, start);
        // Queenside castling.
        let u = pos.make(mv(Square::E1, Square::C1)).unwrap();
        assert_eq!(pos.piece_at(Square::D1).map(|p| p.role), Some(Role::Rook));
        pos.unmake(u).unwrap();
        assert_eq!(pos, start);
        // En passant: exd6 removes the pawn on d5.
        let u = pos.make(mv(Square::E5, Square::D6)).unwrap();
        assert_eq!(pos.piece_at(Square::D5), None);
        assert_eq!(pos.piece_squares(Color::Black).len(), 3);
        pos.unmake(u).unwrap();
        assert_eq!(pos, start);
        // Promotion with capture: bxa8=N.
        let promo = RawMove {
            from: Square::B7,
            to: Square::A8,
            promotion: Some(Role::Knight),
        };
        let u = pos.make(promo).unwrap();
        assert_eq!(pos.piece_at(Square::A8).map(|p| p.role), Some(Role::Knight));
        pos.unmake(u).unwrap();
        assert_eq!(pos, start);
        // Null move.
        let u = pos.make(mv(Square::E1, Square::E1)).unwrap();
        assert_eq!(pos.turn(), Color::Black);
        pos.unmake(u).unwrap();
        assert_eq!(pos, start);
    }

    #[test]
    fn inconsistent_moves_are_errors_and_leave_the_position_unchanged() {
        let mut pos = ListPosition::standard();
        let before = pos.clone();
        assert!(pos.make(mv(Square::E4, Square::E5)).is_err()); // empty square
        assert!(pos.make(mv(Square::E7, Square::E6)).is_err()); // enemy piece
        assert!(pos.make(mv(Square::D1, Square::D2)).is_err()); // own piece
        assert!(pos.make(mv(Square::D1, Square::D1)).is_err()); // queen "null"
        assert!(pos.make(mv(Square::E1, Square::G1)).is_err()); // g1 holds an own knight
        assert_eq!(pos, before);
    }
}
