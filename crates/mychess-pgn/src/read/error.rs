//! Fatal reader errors. Problems with the *content* of a PGN are never
//! errors; they are [`Diagnostic`](crate::read::Diagnostic)s.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// A fatal error: the input cannot be read any further.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PgnError {
    /// The file could not be opened.
    #[error("cannot open {path}")]
    Open {
        /// The path that was opened.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// Reading failed.
    #[error("i/o error at offset {offset}")]
    Io {
        /// Offset in the (decompressed) stream where reading stopped.
        offset: u64,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// The gzip stream is corrupt or truncated.
    #[error("corrupt gzip stream at offset {offset}")]
    Gzip {
        /// Offset in the decompressed stream where decoding stopped.
        offset: u64,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// The input starts with a byte order mark of an encoding the reader
    /// does not support (UTF-16). Convert the file to UTF-8 first.
    #[error("unsupported encoding: {encoding} (convert the file to UTF-8)")]
    UnsupportedEncoding {
        /// Name of the encoding the byte order mark announces.
        encoding: &'static str,
    },
}
