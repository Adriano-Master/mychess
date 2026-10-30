//! The error type of the crate.

use std::path::PathBuf;

/// Everything that can go wrong when reading, editing, building or saving a
/// Polyglot book. No external input ever causes a panic: it ends up here.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BookError {
    /// A file system operation failed.
    #[error("I/O error on {}: {source}", path.display())]
    Io {
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The file has fewer bytes than a single 16-byte record.
    #[error("book file is too short ({len} bytes, a record needs 16)")]
    TooShort {
        /// The size found.
        len: u64,
    },
    /// The file is larger than the supported limit (128 MiB).
    #[error("book file is too large ({size} bytes, limit {limit})")]
    TooLarge {
        /// The size found.
        size: u64,
        /// The limit.
        limit: u64,
    },
    /// The book was opened read-only (or its file is not writable).
    #[error("the book is read-only")]
    ReadOnly,
    /// `save` was called on a book that has no file yet; use `save_as`.
    #[error("the book has no file path; use save_as")]
    NoPath,
    /// A move is not legal in its position (or the line broke at `ply`).
    #[error("illegal move at ply {ply}")]
    IllegalMove {
        /// Zero-based index of the offending move in the given line, or in
        /// the given list for `set_position_moves`.
        ply: usize,
    },
    /// A weight of zero was given; omit the move to remove it instead.
    #[error("a weight of zero is not allowed; omit the move to remove it")]
    ZeroWeight,
    /// A weight above `0xfff0` was given.
    #[error("weight {weight} exceeds the maximum of 65520")]
    WeightTooLarge {
        /// The weight given.
        weight: u32,
    },
    /// More than 100 moves were given for one position.
    #[error("too many moves for one position ({count}, the limit is 100)")]
    TooManyMoves {
        /// The number of moves given.
        count: usize,
    },
    /// The same move appears twice in one position update.
    #[error("the same move was given twice")]
    DuplicateMove,
    /// The position is not standard chess (Chess960 and non-standard castling
    /// cannot be expressed in a Polyglot book).
    #[error("Polyglot books only support standard chess positions")]
    UnsupportedPosition,
}

impl BookError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> BookError {
        BookError::Io {
            path: path.into(),
            source,
        }
    }
}
