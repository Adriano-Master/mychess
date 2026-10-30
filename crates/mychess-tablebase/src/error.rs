//! The error type of the crate (spec m8-8, D2558).
//!
//! Variants carry only data that is safe to show or log: file and material
//! names, never a raw response body. The GUI translates from the variant.

use std::path::PathBuf;
use std::time::Duration;

/// Result alias of this crate.
pub type Result<T, E = TablebaseError> = std::result::Result<T, E>;

/// Everything that can go wrong asking a tablebase.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TablebaseError {
    /// No table file was found in the configured folders.
    #[error("no tablebase files are available")]
    NoTables,
    /// More folders than [`MAX_DIRECTORIES`](crate::local::MAX_DIRECTORIES).
    #[error("too many tablebase folders (at most {max})")]
    TooManyDirectories {
        /// The limit.
        max: usize,
    },
    /// The position has more pieces than the tablebase covers.
    #[error("{pieces} pieces: the tablebase covers at most {max}")]
    TooManyPieces {
        /// Pieces on the board, kings included.
        pieces: u8,
        /// Most pieces covered.
        max: u8,
    },
    /// The position has fewer than three pieces (two kings).
    #[error("{pieces} pieces: nothing to look up")]
    TooFewPieces {
        /// Pieces on the board, kings included.
        pieces: u8,
    },
    /// The position still has a castling right; tablebases have none.
    #[error("tablebases do not cover positions with castling rights")]
    Castling,
    /// The position cannot be handed to the Syzygy code (for instance more
    /// than one king).
    #[error("the position is not valid for the tablebase")]
    InvalidPosition,
    /// The table for this material is not available.
    #[error("missing table: {material}")]
    MissingTable {
        /// The material, for instance `KRvKP`.
        material: String,
    },
    /// A table file is corrupted.
    #[error("corrupted table: {material}")]
    Corrupt {
        /// The material, for instance `KRvKP`.
        material: String,
    },
    /// The Syzygy code failed in a way that should not happen (it panicked);
    /// the probe was abandoned.
    #[error("internal error while probing")]
    Internal,
    /// A folder could not be read.
    #[error("cannot read {path}: {kind}")]
    Io {
        /// The folder.
        path: PathBuf,
        /// What the operating system said.
        kind: std::io::ErrorKind,
    },
    /// Connection, DNS, TLS or I/O failure talking to the online server.
    #[error("network error: {0}")]
    Network(String),
    /// The online request timed out.
    #[error("the request timed out")]
    Timeout,
    /// The online server asked us to slow down (HTTP 429), or we are still
    /// inside the block that followed one.
    #[error("rate limited; retry in {retry_after:?}")]
    RateLimited {
        /// How long new requests stay blocked.
        retry_after: Duration,
    },
    /// HTTP 5xx.
    #[error("server error (HTTP {0})")]
    Server(u16),
    /// HTTP 404: the server has no answer for this position.
    #[error("the server has no answer for this position")]
    NotFound,
    /// The server answered something this client cannot understand.
    #[error("protocol error: {0}")]
    Protocol(String),
}

impl TablebaseError {
    /// Whether the position simply is outside what this source covers (no
    /// table, too many pieces, castling...), as opposed to a failure. The
    /// app falls back to the other source for these.
    #[must_use]
    pub fn is_not_covered(&self) -> bool {
        matches!(
            self,
            TablebaseError::NoTables
                | TablebaseError::TooManyPieces { .. }
                | TablebaseError::TooFewPieces { .. }
                | TablebaseError::Castling
                | TablebaseError::MissingTable { .. }
                | TablebaseError::NotFound
        )
    }
}

impl From<mychess_online::Error> for TablebaseError {
    fn from(error: mychess_online::Error) -> TablebaseError {
        use mychess_online::Error as E;
        match error {
            E::Timeout => TablebaseError::Timeout,
            E::RateLimited { retry_after } => TablebaseError::RateLimited { retry_after },
            E::NotFound => TablebaseError::NotFound,
            E::Server(status) => TablebaseError::Server(status),
            E::LineTooLong => TablebaseError::Protocol("response too large".to_owned()),
            E::Rejected { status, .. } => {
                TablebaseError::Protocol(format!("request rejected (HTTP {status})"))
            }
            E::Redirect(status) => {
                TablebaseError::Protocol(format!("unexpected redirect (HTTP {status})"))
            }
            E::Protocol(text) => TablebaseError::Protocol(text),
            other => TablebaseError::Network(other.to_string()),
        }
    }
}
