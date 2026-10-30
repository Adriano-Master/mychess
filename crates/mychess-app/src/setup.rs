//! The "set up position" draft (spec M2-2, D105-D113; Scid:
//! `tcl/edit.tcl` `setupBoard`, `makeSetupFen`, `validateSetup`).
//!
//! [`SetupBoard`] is a draft edited by [`SetupMessage`]s. The game is not
//! touched until the controller turns an accepted draft into a replacement.

use std::fmt::Write;

use mychess_core::{
    CastlingSide, Color, FenError, FenMode, File, Piece, Position, Rank, Role, Square,
};

use crate::clipboard::normalize_fen_text;

/// Most FEN texts remembered by the dialog (Scid: 20).
pub const MAX_FEN_HISTORY: usize = 20;
/// Highest half-move clock and move number the dialog accepts.
pub const MAX_COUNTER: u16 = 999;

/// The brush cycle of the mouse wheel (`switchPastePiece`).
const BRUSH_ORDER: [Role; 6] = [
    Role::King,
    Role::Queen,
    Role::Rook,
    Role::Bishop,
    Role::Knight,
    Role::Pawn,
];

/// The four castling rights of the draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CastlingFlags {
    /// White king side (`K`).
    pub white_king: bool,
    /// White queen side (`Q`).
    pub white_queen: bool,
    /// Black king side (`k`).
    pub black_king: bool,
    /// Black queen side (`q`).
    pub black_queen: bool,
}

impl CastlingFlags {
    /// All four rights.
    pub const ALL: CastlingFlags = CastlingFlags {
        white_king: true,
        white_queen: true,
        black_king: true,
        black_queen: true,
    };

    /// The right of `color` on `side`.
    pub fn get(self, color: Color, side: CastlingSide) -> bool {
        match (color, side) {
            (Color::White, CastlingSide::KingSide) => self.white_king,
            (Color::White, CastlingSide::QueenSide) => self.white_queen,
            (Color::Black, CastlingSide::KingSide) => self.black_king,
            (Color::Black, CastlingSide::QueenSide) => self.black_queen,
        }
    }

    fn slot(&mut self, color: Color, side: CastlingSide) -> &mut bool {
        match (color, side) {
            (Color::White, CastlingSide::KingSide) => &mut self.white_king,
            (Color::White, CastlingSide::QueenSide) => &mut self.white_queen,
            (Color::Black, CastlingSide::KingSide) => &mut self.black_king,
            (Color::Black, CastlingSide::QueenSide) => &mut self.black_queen,
        }
    }

    /// The FEN castling field (`KQkq` subset or `-`).
    fn fen_field(self) -> String {
        let mut out = String::new();
        for (on, ch) in [
            (self.white_king, 'K'),
            (self.white_queen, 'Q'),
            (self.black_king, 'k'),
            (self.black_queen, 'q'),
        ] {
            if on {
                out.push(ch);
            }
        }
        if out.is_empty() {
            out.push('-');
        }
        out
    }

    /// Colours swapped.
    fn swapped(self) -> CastlingFlags {
        CastlingFlags {
            white_king: self.black_king,
            white_queen: self.black_queen,
            black_king: self.white_king,
            black_queen: self.white_queen,
        }
    }
}

/// Direction of the brush cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `K` then `Q`, `R`, `B`, `N`, `P`, `k`, ... (mouse wheel down).
    Next,
    /// The reverse order.
    Prev,
}

/// Why a draft cannot become a game (D106). The first problem found wins.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SetupProblem {
    /// A side has no king.
    MissingKing(Color),
    /// A side has more than one king.
    TooManyKings(Color),
    /// A side has more than 16 pieces.
    TooManyPieces(Color),
    /// A side has more than 8 pawns (a rule of the Scid's dialog).
    TooManyPawns(Color),
    /// A pawn stands on the first or eighth rank.
    PawnOnBackRank,
    /// The kings touch.
    AdjacentKings,
    /// The side that is not to move gives check ("enemy king in check").
    OpponentInCheck,
    /// Any other rejection by the rules engine.
    Illegal,
    /// The FEN text was refused ([`SetupMessage::ApplyFenText`]).
    Fen(FenError),
}

impl SetupProblem {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            SetupProblem::MissingKing(_) => "setup-problem-missing-king",
            SetupProblem::TooManyKings(_) => "setup-problem-too-many-kings",
            SetupProblem::TooManyPieces(_) => "setup-problem-too-many-pieces",
            SetupProblem::TooManyPawns(_) => "setup-problem-too-many-pawns",
            SetupProblem::PawnOnBackRank => "setup-problem-pawn-on-back-rank",
            SetupProblem::AdjacentKings => "setup-problem-adjacent-kings",
            SetupProblem::OpponentInCheck => "setup-problem-opponent-in-check",
            SetupProblem::Illegal => "setup-problem-illegal",
            SetupProblem::Fen(_) => "setup-problem-fen",
        }
    }
}

/// A remark about a valid draft: something the core discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SetupWarning {
    /// The castling right was asked for but king or rook is not in place.
    CastlingDiscarded(Color, CastlingSide),
    /// The en passant file was asked for but no capture is possible.
    EnPassantDiscarded,
}

impl SetupWarning {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            SetupWarning::CastlingDiscarded(..) => "setup-warning-castling-discarded",
            SetupWarning::EnPassantDiscarded => "setup-warning-en-passant-discarded",
        }
    }
}

/// The verdict on a draft.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SetupAnalysis {
    /// The draft is a playable position.
    Valid {
        /// The position.
        position: Position,
        /// Its canonical FEN.
        fen: String,
        /// What the core discarded.
        warnings: Vec<SetupWarning>,
    },
    /// The draft is refused.
    Invalid(SetupProblem),
}

impl SetupAnalysis {
    /// The position, when the draft is valid.
    pub fn position(&self) -> Option<&Position> {
        match self {
            SetupAnalysis::Valid { position, .. } => Some(position),
            SetupAnalysis::Invalid(_) => None,
        }
    }

    /// Whether "OK" is enabled.
    pub fn is_valid(&self) -> bool {
        matches!(self, SetupAnalysis::Valid { .. })
    }
}

/// An edit of the draft.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SetupMessage {
    /// Left click: paste the brush, or erase if the square already has it.
    ClickSquare(Square),
    /// Right click: erase; the erased piece becomes the brush.
    RightClickSquare(Square),
    /// Middle click: copy the piece to the brush.
    MiddleClickSquare(Square),
    /// Drag and drop: move the piece; `to == None` is a drop outside.
    Drop {
        /// Origin square.
        from: Square,
        /// Destination square, `None` outside the board.
        to: Option<Square>,
    },
    /// Chooses the brush.
    SelectBrush(Piece),
    /// Mouse wheel over the board.
    CycleBrush(Direction),
    /// Side to move.
    SetTurn(Color),
    /// Toggles one castling right.
    ToggleCastling(Color, CastlingSide),
    /// En passant file (`None` is `-`).
    SetEpFile(Option<File>),
    /// Half-move clock, limited to `0..=999`.
    SetHalfmove(u16),
    /// Move number, limited to `1..=999`.
    SetFullmove(u16),
    /// Only the kings (`K` on g2, `k` on b7).
    Clear,
    /// The initial position (the Chess960 mode, if on, stays on).
    Initial,
    /// Switches the draft between standard chess and Chess960: the castling
    /// field is then read as X-FEN, with the rights on the king and rooks.
    SetChess960(bool),
    /// Turns the board view (does not change the position).
    Flip,
    /// Swaps the colour of every piece and the castling rights, and the
    /// side to move.
    SwapColours,
    /// Reverses the order of the ranks (the Scid's "Invert").
    MirrorRanks,
    /// Reverses the order of the files (the Scid's "Transpose").
    MirrorFiles,
    /// The FEN text field was edited.
    FenEdited(String),
    /// Loads the FEN text into the draft.
    ApplyFenText,
    /// Asks the host for the clipboard (handled by the controller).
    PasteFen,
    /// Empties the FEN text field.
    ClearFenText,
    /// Accepts the draft (handled by the controller).
    Ok,
    /// Discards the draft (handled by the controller).
    Cancel,
}

/// The draft of the "set up position" dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupBoard {
    /// The squares, indexed by `Square as usize` (a1 = 0).
    pub squares: [Option<Piece>; 64],
    /// Side to move.
    pub turn: Color,
    /// Castling rights asked for.
    pub castling: CastlingFlags,
    /// En passant file; the rank follows from the side to move.
    pub ep_file: Option<File>,
    /// Half-move clock, `0..=999`.
    pub halfmove_clock: u16,
    /// Move number, `1..=999`.
    pub fullmove_number: u16,
    /// The brush (initially a white pawn).
    pub brush: Piece,
    /// Castling mode of the starting position.
    pub mode: FenMode,
    /// Whether the view is turned (visual only).
    pub flipped: bool,
    /// The FEN text field: canonical after every board edit.
    pub fen_text: String,
    /// Texts loaded or accepted, newest first, at most [`MAX_FEN_HISTORY`].
    pub fen_history: Vec<String>,
    /// Verdict on the draft, recomputed after every board edit.
    pub analysis: SetupAnalysis,
}

impl SetupBoard {
    /// A draft that starts from `pos`.
    pub fn from_position(pos: &Position) -> SetupBoard {
        let mut board = SetupBoard {
            squares: [None; 64],
            turn: Color::White,
            castling: CastlingFlags::default(),
            ep_file: None,
            halfmove_clock: 0,
            fullmove_number: 1,
            brush: Piece {
                color: Color::White,
                role: Role::Pawn,
            },
            mode: FenMode::Standard,
            flipped: false,
            fen_text: String::new(),
            fen_history: Vec::new(),
            analysis: SetupAnalysis::Invalid(SetupProblem::Illegal),
        };
        board.load_position(pos);
        board.refresh();
        board
    }

    /// Copies a position into the draft (not the brush, view or history).
    fn load_position(&mut self, pos: &Position) {
        self.squares = [None; 64];
        for (sq, piece) in pos.pieces() {
            if let Some(slot) = self.squares.get_mut(sq as usize) {
                *slot = Some(piece);
            }
        }
        self.turn = pos.turn();
        self.castling = CastlingFlags {
            white_king: pos.has_castling_right(Color::White, CastlingSide::KingSide),
            white_queen: pos.has_castling_right(Color::White, CastlingSide::QueenSide),
            black_king: pos.has_castling_right(Color::Black, CastlingSide::KingSide),
            black_queen: pos.has_castling_right(Color::Black, CastlingSide::QueenSide),
        };
        self.ep_file = pos.ep_square().map(Square::file);
        self.halfmove_clock = clamp_counter(pos.halfmove_clock(), 0);
        self.fullmove_number = clamp_counter(pos.fullmove_number(), 1);
        self.mode = if pos.castling_mode() == mychess_core::CastlingMode::Standard {
            FenMode::Standard
        } else {
            FenMode::Chess960
        };
    }

    /// Whether the draft is a Chess960 position (M9-6).
    pub fn is_chess960(&self) -> bool {
        self.mode != FenMode::Standard
    }

    /// The Chess960 start position number (0 to 959) when the draft is
    /// exactly that start position: the indicator of the editor.
    pub fn chess960_number(&self) -> Option<u16> {
        self.analysis
            .position()
            .filter(|_| self.is_chess960())
            .and_then(Position::chess960_number)
    }

    /// The piece on `sq`.
    pub fn piece_at(&self, sq: Square) -> Option<Piece> {
        self.squares.get(sq as usize).copied().flatten()
    }

    fn set(&mut self, sq: Square, piece: Option<Piece>) {
        if let Some(slot) = self.squares.get_mut(sq as usize) {
            *slot = piece;
        }
    }

    /// Recomputes the verdict and the canonical FEN text.
    fn refresh(&mut self) {
        self.analysis = analyze_setup(self);
        self.fen_text = match &self.analysis {
            SetupAnalysis::Valid { fen, .. } => fen.clone(),
            SetupAnalysis::Invalid(_) => board_fen(self),
        };
    }

    fn remember_fen(&mut self, fen: &str) {
        self.fen_history.retain(|f| f != fen);
        self.fen_history.insert(0, fen.to_owned());
        self.fen_history.truncate(MAX_FEN_HISTORY);
    }

    /// Records the canonical FEN of a valid draft in the history (used when
    /// the dialog is accepted).
    pub(crate) fn remember_current(&mut self) {
        if let SetupAnalysis::Valid { fen, .. } = &self.analysis {
            let fen = fen.clone();
            self.remember_fen(&fen);
        }
    }

    fn can_hold(piece: Piece, sq: Square) -> bool {
        piece.role != Role::Pawn || !matches!(sq.rank(), Rank::First | Rank::Eighth)
    }

    fn cycle_brush(&mut self, direction: Direction) {
        let order: Vec<Piece> = [Color::White, Color::Black]
            .into_iter()
            .flat_map(|color| BRUSH_ORDER.map(|role| Piece { color, role }))
            .collect();
        let at = order.iter().position(|p| *p == self.brush).unwrap_or(0);
        let len = order.len();
        let next = match direction {
            Direction::Next => (at + 1) % len,
            Direction::Prev => (at + len - 1) % len,
        };
        if let Some(piece) = order.get(next) {
            self.brush = *piece;
        }
    }

    /// Applies one message to the draft. `PasteFen`, `Ok` and `Cancel` are
    /// for the controller and do nothing here.
    pub fn apply(&mut self, msg: SetupMessage) {
        match msg {
            SetupMessage::ClickSquare(sq) => {
                if !Self::can_hold(self.brush, sq) {
                    return;
                }
                let value = if self.piece_at(sq) == Some(self.brush) {
                    None
                } else {
                    Some(self.brush)
                };
                self.set(sq, value);
            }
            SetupMessage::RightClickSquare(sq) => {
                let Some(piece) = self.piece_at(sq) else {
                    return;
                };
                self.brush = piece;
                self.set(sq, None);
            }
            SetupMessage::MiddleClickSquare(sq) => {
                if let Some(piece) = self.piece_at(sq) {
                    self.brush = piece;
                }
                return;
            }
            SetupMessage::Drop { from, to } => {
                let (Some(to), Some(piece)) = (to, self.piece_at(from)) else {
                    return;
                };
                if !Self::can_hold(piece, to) {
                    return;
                }
                self.set(from, None);
                self.set(to, Some(piece));
                self.brush = piece;
            }
            SetupMessage::SelectBrush(piece) => {
                self.brush = piece;
                return;
            }
            SetupMessage::CycleBrush(direction) => {
                self.cycle_brush(direction);
                return;
            }
            SetupMessage::SetTurn(color) => self.turn = color,
            SetupMessage::ToggleCastling(color, side) => {
                let slot = self.castling.slot(color, side);
                *slot = !*slot;
            }
            SetupMessage::SetEpFile(file) => self.ep_file = file,
            SetupMessage::SetHalfmove(n) => self.halfmove_clock = n.min(MAX_COUNTER),
            SetupMessage::SetFullmove(n) => self.fullmove_number = n.clamp(1, MAX_COUNTER),
            SetupMessage::Clear => self.clear_board(),
            SetupMessage::Initial => {
                let mode = self.mode;
                self.load_position(&Position::new());
                self.mode = mode;
            }
            SetupMessage::SetChess960(on) => {
                self.mode = if on {
                    FenMode::Chess960
                } else {
                    FenMode::Standard
                };
            }
            SetupMessage::Flip => {
                self.flipped = !self.flipped;
                return;
            }
            SetupMessage::SwapColours => {
                for piece in self.squares.iter_mut().flatten() {
                    piece.color = piece.color.other();
                }
                self.turn = self.turn.other();
                self.castling = self.castling.swapped();
            }
            SetupMessage::MirrorRanks => {
                self.remap(Square::flip_vertical);
            }
            SetupMessage::MirrorFiles => {
                self.remap(Square::flip_horizontal);
            }
            SetupMessage::FenEdited(text) => {
                self.fen_text = text;
                return;
            }
            SetupMessage::ClearFenText => {
                self.fen_text.clear();
                return;
            }
            SetupMessage::ApplyFenText => {
                self.apply_fen_text();
                return;
            }
            SetupMessage::PasteFen | SetupMessage::Ok | SetupMessage::Cancel => return,
        }
        self.refresh();
    }

    /// Only the two kings (`K` on g2, `k` on b7), nothing else (Scid:
    /// `edit.tcl:651`).
    fn clear_board(&mut self) {
        self.squares = [None; 64];
        for (sq, color) in [(Square::G2, Color::White), (Square::B7, Color::Black)] {
            self.set(
                sq,
                Some(Piece {
                    color,
                    role: Role::King,
                }),
            );
        }
        self.turn = Color::White;
        self.castling = CastlingFlags::default();
        self.ep_file = None;
        self.halfmove_clock = 0;
        self.fullmove_number = 1;
    }

    /// Moves every piece with `map`; castling and en passant are cleared.
    fn remap(&mut self, map: fn(Square) -> Square) {
        let old = self.squares;
        self.squares = [None; 64];
        for sq in Square::ALL {
            self.set(map(sq), old.get(sq as usize).copied().flatten());
        }
        self.castling = CastlingFlags::default();
        self.ep_file = None;
    }

    fn apply_fen_text(&mut self) {
        let text = normalize_fen_text(&self.fen_text);
        // With the Chess960 box on the text is read as Chess960 whatever its
        // castling field looks like; otherwise the mode is detected (a
        // Chess960 text switches the box on).
        let mode = if self.mode == FenMode::Chess960 {
            FenMode::Chess960
        } else {
            FenMode::Auto
        };
        match Position::from_fen_with(&text, mode) {
            Ok(pos) => {
                self.load_position(&pos);
                let canonical = pos.fen();
                self.remember_fen(&canonical);
                self.refresh();
            }
            Err(error) => {
                self.analysis = SetupAnalysis::Invalid(SetupProblem::Fen(error));
            }
        }
    }
}

fn clamp_counter(value: u32, min: u16) -> u16 {
    u16::try_from(value)
        .unwrap_or(MAX_COUNTER)
        .clamp(min, MAX_COUNTER)
}

/// The FEN text of the draft, built exactly as asked (nothing discarded).
fn board_fen(b: &SetupBoard) -> String {
    let mut fen = String::new();
    for rank in Rank::ALL.into_iter().rev() {
        let mut empty = 0u8;
        for file in File::ALL {
            match b.piece_at(Square::from_coords(file, rank)) {
                Some(piece) => {
                    if empty > 0 {
                        fen.push(char::from(b'0' + empty));
                        empty = 0;
                    }
                    fen.push(piece.char());
                }
                None => empty += 1,
            }
        }
        if empty > 0 {
            fen.push(char::from(b'0' + empty));
        }
        if rank != Rank::First {
            fen.push('/');
        }
    }
    fen.push(' ');
    fen.push(if b.turn == Color::White { 'w' } else { 'b' });
    fen.push(' ');
    fen.push_str(&b.castling.fen_field());
    fen.push(' ');
    match b.ep_file {
        Some(file) => {
            fen.push(file.char());
            fen.push(if b.turn == Color::White { '6' } else { '3' });
        }
        None => fen.push('-'),
    }
    // Writing to a `String` cannot fail.
    let _ = write!(fen, " {} {}", b.halfmove_clock, b.fullmove_number);
    fen
}

/// Validates a draft (D106): the Scid's counts first, then the core's rules.
pub fn analyze_setup(b: &SetupBoard) -> SetupAnalysis {
    if let Some(problem) = count_problem(b) {
        return SetupAnalysis::Invalid(problem);
    }
    let fen = board_fen(b);
    let position = match Position::from_fen_with(&fen, b.mode) {
        Ok(position) => position,
        Err(FenError::IllegalPosition { reason }) => {
            return SetupAnalysis::Invalid(map_reason(reason));
        }
        Err(error) => return SetupAnalysis::Invalid(SetupProblem::Fen(error)),
    };
    let mut warnings = Vec::new();
    for color in [Color::White, Color::Black] {
        for side in [CastlingSide::KingSide, CastlingSide::QueenSide] {
            if b.castling.get(color, side) && !position.has_castling_right(color, side) {
                warnings.push(SetupWarning::CastlingDiscarded(color, side));
            }
        }
    }
    if b.ep_file.is_some() && position.ep_square().is_none() {
        warnings.push(SetupWarning::EnPassantDiscarded);
    }
    SetupAnalysis::Valid {
        fen: position.fen(),
        position,
        warnings,
    }
}

fn map_reason(reason: mychess_core::IllegalPositionReason) -> SetupProblem {
    use mychess_core::IllegalPositionReason as R;
    match reason {
        R::MissingKing(c) => SetupProblem::MissingKing(c),
        R::TooManyKings(c) => SetupProblem::TooManyKings(c),
        R::TooManyPieces(c) => SetupProblem::TooManyPieces(c),
        R::AdjacentKings => SetupProblem::AdjacentKings,
        R::PawnOnBackRank => SetupProblem::PawnOnBackRank,
        R::OpponentInCheck => SetupProblem::OpponentInCheck,
        _ => SetupProblem::Illegal,
    }
}

/// Scid's `validateSetup`: kings, then pieces, then pawns, white first.
fn count_problem(b: &SetupBoard) -> Option<SetupProblem> {
    let count = |color: Color, role: Option<Role>| {
        b.squares
            .iter()
            .flatten()
            .filter(|p| p.color == color && role.is_none_or(|r| p.role == r))
            .count()
    };
    let colors = [Color::White, Color::Black];
    for color in colors {
        match count(color, Some(Role::King)) {
            0 => return Some(SetupProblem::MissingKing(color)),
            1 => {}
            _ => return Some(SetupProblem::TooManyKings(color)),
        }
    }
    for color in colors {
        if count(color, None) > 16 {
            return Some(SetupProblem::TooManyPieces(color));
        }
    }
    for color in colors {
        if count(color, Some(Role::Pawn)) > 8 {
            return Some(SetupProblem::TooManyPawns(color));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    fn start() -> SetupBoard {
        SetupBoard::from_position(&Position::new())
    }

    fn piece(color: Color, role: Role) -> Piece {
        Piece { color, role }
    }

    fn fen_of(b: &SetupBoard) -> Option<String> {
        match &b.analysis {
            SetupAnalysis::Valid { fen, .. } => Some(fen.clone()),
            SetupAnalysis::Invalid(_) => None,
        }
    }

    #[test]
    fn round_trips_the_start_position() {
        let b = start();
        assert_eq!(fen_of(&b).as_deref(), Some(START));
        assert_eq!(b.analysis.position(), Some(&Position::new()));
        assert_eq!(b.fen_text, START);
    }

    #[test]
    fn click_places_erases_and_replaces() {
        let mut b = start();
        b.apply(SetupMessage::Clear);
        b.apply(SetupMessage::SelectBrush(piece(Color::White, Role::Knight)));
        b.apply(SetupMessage::ClickSquare(Square::C3));
        assert_eq!(
            b.piece_at(Square::C3),
            Some(piece(Color::White, Role::Knight))
        );
        b.apply(SetupMessage::ClickSquare(Square::C3));
        assert_eq!(b.piece_at(Square::C3), None);
        b.apply(SetupMessage::ClickSquare(Square::C3));
        b.apply(SetupMessage::SelectBrush(piece(Color::Black, Role::Bishop)));
        b.apply(SetupMessage::ClickSquare(Square::C3));
        assert_eq!(
            b.piece_at(Square::C3),
            Some(piece(Color::Black, Role::Bishop))
        );
    }

    #[test]
    fn pawns_are_refused_on_back_ranks() {
        let mut b = start();
        b.apply(SetupMessage::Clear);
        let before = b.clone();
        b.apply(SetupMessage::ClickSquare(Square::A1));
        b.apply(SetupMessage::ClickSquare(Square::H8));
        assert_eq!(b, before);
        b.apply(SetupMessage::Drop {
            from: Square::G2,
            to: Some(Square::G1),
        });
        // The king may go there.
        assert!(b.piece_at(Square::G1).is_some());
        b.apply(SetupMessage::SelectBrush(piece(Color::White, Role::Pawn)));
        b.apply(SetupMessage::ClickSquare(Square::E4));
        b.apply(SetupMessage::Drop {
            from: Square::E4,
            to: Some(Square::E8),
        });
        assert_eq!(
            b.piece_at(Square::E4),
            Some(piece(Color::White, Role::Pawn))
        );
    }

    #[test]
    fn right_and_middle_click_set_the_brush() {
        let mut b = start();
        b.apply(SetupMessage::MiddleClickSquare(Square::E1));
        assert_eq!(b.brush, piece(Color::White, Role::King));
        assert!(b.piece_at(Square::E1).is_some());
        b.apply(SetupMessage::RightClickSquare(Square::D8));
        assert_eq!(b.brush, piece(Color::Black, Role::Queen));
        assert_eq!(b.piece_at(Square::D8), None);
        let before = b.clone();
        b.apply(SetupMessage::RightClickSquare(Square::D5));
        assert_eq!(b, before);
    }

    #[test]
    fn brush_cycle_has_twelve_steps_in_order() {
        let mut b = start();
        b.brush = piece(Color::White, Role::King);
        b.apply(SetupMessage::CycleBrush(Direction::Next));
        assert_eq!(b.brush, piece(Color::White, Role::Queen));
        b.apply(SetupMessage::CycleBrush(Direction::Prev));
        b.apply(SetupMessage::CycleBrush(Direction::Prev));
        assert_eq!(b.brush, piece(Color::Black, Role::Pawn));
        for _ in 0..12 {
            b.apply(SetupMessage::CycleBrush(Direction::Next));
        }
        assert_eq!(b.brush, piece(Color::Black, Role::Pawn));
    }

    #[test]
    fn drop_moves_and_replaces() {
        let mut b = start();
        b.apply(SetupMessage::Drop {
            from: Square::E2,
            to: Some(Square::E7),
        });
        assert_eq!(b.piece_at(Square::E2), None);
        assert_eq!(
            b.piece_at(Square::E7),
            Some(piece(Color::White, Role::Pawn))
        );
        assert_eq!(b.brush, piece(Color::White, Role::Pawn));
        let before = b.clone();
        b.apply(SetupMessage::Drop {
            from: Square::E2,
            to: Some(Square::E3),
        });
        b.apply(SetupMessage::Drop {
            from: Square::E7,
            to: None,
        });
        assert_eq!(b, before);
    }

    #[test]
    fn clear_and_initial() {
        let mut b = start();
        b.apply(SetupMessage::Clear);
        assert_eq!(fen_of(&b).as_deref(), Some("8/1k6/8/8/8/8/6K1/8 w - - 0 1"));
        b.apply(SetupMessage::Initial);
        assert_eq!(fen_of(&b).as_deref(), Some(START));
    }

    #[test]
    fn swap_and_mirrors_are_involutions() {
        let mut b = start();
        b.apply(SetupMessage::SetTurn(Color::Black));
        let squares = b.squares;
        let turn = b.turn;
        b.apply(SetupMessage::SwapColours);
        assert_eq!(b.turn, Color::White);
        b.apply(SetupMessage::SwapColours);
        assert_eq!((b.squares, b.turn), (squares, turn));
        for msg in [SetupMessage::MirrorRanks, SetupMessage::MirrorFiles] {
            let mut c = start();
            c.apply(msg.clone());
            assert_eq!(c.castling, CastlingFlags::default());
            c.apply(msg);
            assert_eq!(c.squares, start().squares);
        }
    }

    #[test]
    fn validation_follows_scid_order() {
        let mut b = start();
        b.apply(SetupMessage::RightClickSquare(Square::E1));
        assert_eq!(
            b.analysis,
            SetupAnalysis::Invalid(SetupProblem::MissingKing(Color::White))
        );
        b.apply(SetupMessage::SelectBrush(piece(Color::Black, Role::King)));
        b.apply(SetupMessage::ClickSquare(Square::E1));
        b.apply(SetupMessage::ClickSquare(Square::E3));
        assert_eq!(
            b.analysis,
            SetupAnalysis::Invalid(SetupProblem::MissingKing(Color::White))
        );
        b.apply(SetupMessage::SelectBrush(piece(Color::White, Role::King)));
        b.apply(SetupMessage::ClickSquare(Square::E4));
        b.apply(SetupMessage::RightClickSquare(Square::E1));
        b.apply(SetupMessage::RightClickSquare(Square::E3));
        // Kings on e8 (black) and e4 (white): fine.
        assert!(b.analysis.is_valid(), "{:?}", b.analysis);
    }

    #[test]
    fn too_many_pieces_and_pawns() {
        let mut b = start();
        b.apply(SetupMessage::SelectBrush(piece(Color::Black, Role::Pawn)));
        b.apply(SetupMessage::ClickSquare(Square::A3));
        assert_eq!(
            b.analysis,
            SetupAnalysis::Invalid(SetupProblem::TooManyPieces(Color::Black))
        );
        let mut b = start();
        b.apply(SetupMessage::Clear);
        b.apply(SetupMessage::SelectBrush(piece(Color::Black, Role::Pawn)));
        for file in File::ALL {
            b.apply(SetupMessage::ClickSquare(Square::from_coords(
                file,
                Rank::Fourth,
            )));
        }
        b.apply(SetupMessage::ClickSquare(Square::A5));
        assert_eq!(
            b.analysis,
            SetupAnalysis::Invalid(SetupProblem::TooManyPawns(Color::Black))
        );
    }

    #[test]
    fn adjacent_kings_and_opponent_in_check() {
        let mut b = start();
        b.apply(SetupMessage::Clear);
        b.apply(SetupMessage::Drop {
            from: Square::B7,
            to: Some(Square::H3),
        });
        assert_eq!(
            b.analysis,
            SetupAnalysis::Invalid(SetupProblem::AdjacentKings)
        );
        let mut b = start();
        b.apply(SetupMessage::Clear);
        b.apply(SetupMessage::SelectBrush(piece(Color::White, Role::Rook)));
        b.apply(SetupMessage::ClickSquare(Square::B1));
        // White to move and the black king on b7 is attacked by the rook.
        assert_eq!(
            b.analysis,
            SetupAnalysis::Invalid(SetupProblem::OpponentInCheck)
        );
        // Side to move in check is accepted.
        b.apply(SetupMessage::SetTurn(Color::Black));
        assert!(b.analysis.is_valid());
    }

    #[test]
    fn discarded_castling_and_en_passant_warn() {
        let mut b = start();
        b.apply(SetupMessage::RightClickSquare(Square::H1));
        let SetupAnalysis::Valid { warnings, fen, .. } = &b.analysis else {
            panic!("valid expected: {:?}", b.analysis);
        };
        assert_eq!(
            warnings,
            &[SetupWarning::CastlingDiscarded(
                Color::White,
                CastlingSide::KingSide
            )]
        );
        assert!(fen.contains(" Qkq "));
        b.apply(SetupMessage::SetEpFile(Some(File::E)));
        let SetupAnalysis::Valid { warnings, .. } = &b.analysis else {
            panic!("valid expected");
        };
        assert!(warnings.contains(&SetupWarning::EnPassantDiscarded));
    }

    #[test]
    fn counters_are_clamped() {
        let mut b = start();
        b.apply(SetupMessage::SetFullmove(0));
        assert_eq!(b.fullmove_number, 1);
        b.apply(SetupMessage::SetFullmove(5000));
        assert_eq!(b.fullmove_number, 999);
        b.apply(SetupMessage::SetHalfmove(5000));
        assert_eq!(b.halfmove_clock, 999);
    }

    #[test]
    fn fen_text_loads_the_draft_or_reports_the_error() {
        let mut b = start();
        b.apply(SetupMessage::FenEdited(
            "FEN: 8/8/8/8/8/8/8/K6k b".to_owned(),
        ));
        b.apply(SetupMessage::ApplyFenText);
        assert_eq!(fen_of(&b).as_deref(), Some("8/8/8/8/8/8/8/K6k b - - 0 1"));
        assert_eq!(b.fen_history.len(), 1);
        let before = b.squares;
        b.apply(SetupMessage::FenEdited("garbage".to_owned()));
        b.apply(SetupMessage::ApplyFenText);
        assert_eq!(b.squares, before);
        assert!(matches!(
            b.analysis,
            SetupAnalysis::Invalid(SetupProblem::Fen(_))
        ));
        b.apply(SetupMessage::ClearFenText);
        assert!(b.fen_text.is_empty());
    }

    #[test]
    fn fen_history_is_bounded_and_deduplicated() {
        let mut b = start();
        for n in 0..30 {
            let fen = format!("8/8/8/8/8/8/8/K6k w - - {n} 1");
            b.apply(SetupMessage::FenEdited(fen));
            b.apply(SetupMessage::ApplyFenText);
        }
        assert_eq!(b.fen_history.len(), MAX_FEN_HISTORY);
        let first = b.fen_history[0].clone();
        b.remember_fen(&first);
        assert_eq!(b.fen_history.len(), MAX_FEN_HISTORY);
    }

    #[test]
    fn flip_does_not_change_the_position() {
        let mut b = start();
        let before = b.analysis.clone();
        b.apply(SetupMessage::Flip);
        assert!(b.flipped);
        assert_eq!(b.analysis, before);
    }
}
