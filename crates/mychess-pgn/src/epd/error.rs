//! Errors and per-line diagnostics of the EPD reader and writer.

use std::fmt;
use std::path::PathBuf;

use mychess_core::FenError;
use thiserror::Error;

/// A failure that stops reading or writing an EPD file.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EpdError {
    /// The underlying reader, writer or file system failed.
    #[error("i/o error{}", path_suffix(.path.as_ref()))]
    Io {
        /// The file involved, when there is one.
        path: Option<PathBuf>,
        /// The original error.
        #[source]
        source: std::io::Error,
    },
    /// An operation cannot be written as valid EPD text.
    #[error("operation {opcode:?} cannot be written: {reason}")]
    Unwritable {
        /// The opcode of the offending operation.
        opcode: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// A configured limit of [`EpdOptions`](super::EpdOptions) was exceeded.
    #[error("limit exceeded: more than {max} {what}")]
    LimitExceeded {
        /// What was counted (`"records"` or `"decompressed bytes"`).
        what: &'static str,
        /// The configured maximum.
        max: u64,
    },
    /// The operation is not supported for this file, e.g. writing `.gz`.
    #[error("{reason}: {}", .path.display())]
    Unsupported {
        /// The file involved.
        path: PathBuf,
        /// Why it is refused.
        reason: &'static str,
    },
}

fn path_suffix(path: Option<&PathBuf>) -> String {
    path.map(|p| format!(" on {}", p.display()))
        .unwrap_or_default()
}

/// What went wrong (or is worth knowing) on one line of an EPD file.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum EpdDiagnosticKind {
    /// The position fields could not be read; the line was ignored.
    #[error("invalid position: {error}")]
    InvalidPosition {
        /// Why the FEN was rejected.
        error: FenError,
    },
    /// The position already appeared; the later data replaced the earlier
    /// (the record keeps the place of the first). Informative only.
    #[error("position already seen on line {first_line}")]
    DuplicatePosition {
        /// The line where the position first appeared.
        first_line: u64,
    },
    /// A quote was never closed; the operation runs to the end of the line.
    #[error("unterminated string")]
    UnterminatedString,
    /// The opcode is not `[A-Za-z][A-Za-z0-9_]*`; the operation was kept.
    #[error("non-standard opcode {opcode:?}")]
    NonStandardOpcode {
        /// The opcode as found.
        opcode: String,
    },
    /// The line is longer than one MiB; it was ignored.
    #[error("line too long")]
    LineTooLong,
}

/// A problem found on a given line while reading; reading went on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpdDiagnostic {
    /// The 1-based line number.
    pub line: u64,
    /// What happened.
    pub kind: EpdDiagnosticKind,
}

impl fmt::Display for EpdDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.kind)
    }
}
