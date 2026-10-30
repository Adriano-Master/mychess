//! Typed errors for FEN, SAN, UCI and move handling.
//!
//! Every error carries enough context (the offending text, and the position
//! when relevant) for diagnostics. Offending text is clipped to a short
//! prefix so that hostile input cannot produce huge messages.

use thiserror::Error;

/// Longest text snippet kept inside an error value.
const MAX_SNIPPET: usize = 48;

/// Clips `text` to a short, char-boundary-safe snippet for error contexts.
pub(crate) fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_SNIPPET {
        text.to_owned()
    } else {
        let mut out: String = text.chars().take(MAX_SNIPPET).collect();
        out.push_str("...");
        out
    }
}

/// Error from building a Chess960 starting position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum Chess960Error {
    /// The Scharnagl number is not in `0..=959`.
    #[error("Chess960 starting position number {0} is out of range 0..=959")]
    NumberOutOfRange(u16),
    /// The tables produced a position that the rules engine rejected. This
    /// is an internal inconsistency, not something the caller can cause.
    #[error("Chess960 starting position {0} could not be built")]
    InvalidStart(u16),
}

/// What is wrong with the piece-placement field of a FEN string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum BoardProblem {
    /// A character that is neither a piece letter, a digit `1`-`8` nor `/`.
    #[error("unexpected character {ch:?} at index {index}")]
    UnexpectedChar {
        /// The offending character.
        ch: char,
        /// Character index inside the board field.
        index: usize,
    },
    /// A `/` that does not follow a complete rank (or a repeated `/`).
    #[error("misplaced '/' at index {index}")]
    MisplacedSlash {
        /// Character index inside the board field.
        index: usize,
    },
    /// More than 64 squares were described.
    #[error("more than 64 squares")]
    TooManySquares,
    /// Fewer than 64 squares were described.
    #[error("only {count} squares described, expected 64")]
    TooFewSquares {
        /// Number of squares described.
        count: u32,
    },
}

/// Why a syntactically valid FEN describes an unacceptable position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum IllegalPositionReason {
    /// A side has no king.
    #[error("{0:?} has no king")]
    MissingKing(crate::Color),
    /// A side has more than one king.
    #[error("{0:?} has more than one king")]
    TooManyKings(crate::Color),
    /// A side has more than 16 pieces.
    #[error("{0:?} has more than 16 pieces")]
    TooManyPieces(crate::Color),
    /// The two kings touch each other.
    #[error("kings are adjacent")]
    AdjacentKings,
    /// A pawn stands on the first or eighth rank.
    #[error("pawn on the first or eighth rank")]
    PawnOnBackRank,
    /// The side that is not to move gives check to the side to move's
    /// opponent, i.e. the opposing king could be captured.
    #[error("the side not to move is giving check")]
    OpponentInCheck,
    /// Any other rejection by the rules engine.
    #[error("the position is not valid")]
    Other,
}

/// Error from reading a FEN string.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum FenError {
    /// The piece-placement field is missing or malformed.
    #[error("invalid board field {field:?}: {problem}")]
    InvalidBoard {
        /// The (clipped) board field.
        field: String,
        /// What is wrong with it.
        problem: BoardProblem,
    },
    /// The side-to-move field is missing or is not `w` / `b`.
    #[error("invalid side to move {found:?}, expected \"w\" or \"b\"")]
    InvalidTurn {
        /// The (clipped) field found, `None` when it is missing.
        found: Option<String>,
    },
    /// The castling field contains something other than `-`, `KQkq` or file
    /// letters (`A`-`H`, `a`-`h`).
    #[error("invalid castling field {field:?}")]
    InvalidCastling {
        /// The (clipped) field.
        field: String,
    },
    /// The en passant field is not `-` or a square on the 3rd/6th rank.
    #[error("invalid en passant field {field:?}")]
    InvalidEnPassant {
        /// The (clipped) field.
        field: String,
    },
    /// The halfmove clock is not an integer.
    #[error("invalid halfmove clock {field:?}")]
    InvalidHalfmoveClock {
        /// The (clipped) field.
        field: String,
    },
    /// The move number is not an integer.
    #[error("invalid move number {field:?}")]
    InvalidMoveNumber {
        /// The (clipped) field.
        field: String,
    },
    /// The fields are well formed but the position is not acceptable.
    #[error("illegal position: {reason}")]
    IllegalPosition {
        /// The reason for the rejection.
        reason: IllegalPositionReason,
    },
}

/// Error from applying a move to a position.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum MoveError {
    /// The move is not legal in the position.
    #[error("illegal move {uci} in position {fen}")]
    Illegal {
        /// The move in UCI notation.
        uci: String,
        /// The position, as FEN.
        fen: String,
    },
    /// A null move was requested while the side to move is in check.
    #[error("null move is not allowed while in check ({fen})")]
    NullMoveInCheck {
        /// The position, as FEN.
        fen: String,
    },
    /// The rules engine rejected the position that a null move would create.
    #[error("null move produces an invalid position ({fen})")]
    NullMoveRejected {
        /// The position before the null move, as FEN.
        fen: String,
    },
}

/// Error from reading a SAN move.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SanError {
    /// The text is empty (or only whitespace).
    #[error("empty move text")]
    Empty,
    /// The text is implausibly long for a move.
    #[error("move text too long ({len} bytes)")]
    TooLong {
        /// Length of the text in bytes.
        len: usize,
    },
    /// The text is not move-shaped.
    #[error("cannot parse {text:?} as a move")]
    Syntax {
        /// The (clipped) text.
        text: String,
    },
    /// The promotion piece is missing where required, or not allowed.
    #[error("invalid promotion in {text:?}")]
    InvalidPromotion {
        /// The (clipped) text.
        text: String,
    },
    /// Strict mode only: the move needs a promotion piece but has none.
    #[error("missing promotion piece in {text:?}")]
    MissingPromotion {
        /// The (clipped) text.
        text: String,
    },
    /// Strict mode only: the text is understood but is not canonical SAN.
    #[error("{text:?} is not canonical SAN")]
    NonCanonical {
        /// The (clipped) text.
        text: String,
    },
    /// No legal move matches the text.
    #[error("illegal move {text:?} in position {fen}")]
    Illegal {
        /// The (clipped) text.
        text: String,
        /// The position, as FEN.
        fen: String,
    },
    /// More than one legal move matches the text.
    #[error("ambiguous move {text:?} in position {fen}")]
    Ambiguous {
        /// The (clipped) text.
        text: String,
        /// The position, as FEN.
        fen: String,
    },
    /// Castling is requested but not possible, even tolerantly.
    #[error("illegal castling {text:?} in position {fen}")]
    IllegalCastling {
        /// The (clipped) text.
        text: String,
        /// The position, as FEN.
        fen: String,
    },
    /// A null move was requested while in check.
    #[error("null move {text:?} is not allowed while in check ({fen})")]
    NullMoveInCheck {
        /// The (clipped) text.
        text: String,
        /// The position, as FEN.
        fen: String,
    },
}

/// Error from reading a UCI move.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum UciError {
    /// The text is not a UCI move (`e2e4`, `e7e8q`, `0000`).
    #[error("cannot parse {text:?} as a UCI move")]
    Syntax {
        /// The (clipped) text.
        text: String,
    },
    /// The move is a promotion but no piece letter was given.
    #[error("missing promotion piece in {text:?}")]
    MissingPromotion {
        /// The (clipped) text.
        text: String,
    },
    /// The move is not legal in the position.
    #[error("illegal move {text:?} in position {fen}")]
    Illegal {
        /// The (clipped) text.
        text: String,
        /// The position, as FEN.
        fen: String,
    },
    /// A null move was requested while in check.
    #[error("null move is not allowed while in check ({fen})")]
    NullMoveInCheck {
        /// The position, as FEN.
        fen: String,
    },
}
