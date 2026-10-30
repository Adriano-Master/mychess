//! Structured file errors and request ids (spec M2-2, D101 and D102).
//!
//! Nothing here carries a ready-made message or a `std::io::Error`: a
//! `Message` must be `Clone` and comparable, and the front-end translates
//! from the variant with Fluent (`message_key`).

use std::io;
use std::path::PathBuf;

use mychess_pgn::read::PgnError;

/// Identifies one asynchronous operation (D102).
///
/// `update` hands out a fresh id for every operation it starts; an answer
/// that carries any other id than the current one of its flow is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct RequestId(pub u64);

/// What went wrong with a file operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileErrorKind {
    /// The file or one of its directories does not exist.
    NotFound,
    /// The operating system refused access.
    PermissionDenied,
    /// The target exists and may not be replaced.
    AlreadyExists,
    /// The target is the file being edited and replacing it would lose
    /// games or text that were not read. Never shown: the controller turns
    /// it into an overwrite question.
    NeedsConfirmation,
    /// The input is larger than the limit (bytes).
    TooLarge {
        /// The limit that was exceeded.
        limit: u64,
    },
    /// The file uses an encoding that is not supported (UTF-16, gzip when
    /// appending). Carries the name of the encoding.
    UnsupportedEncoding(&'static str),
    /// The gzip stream is corrupt or truncated.
    CorruptGzip,
    /// The file has no PGN game.
    NoGamesFound {
        /// Bytes of text that were skipped because they belong to no game.
        skipped_bytes: u64,
    },
    /// Appending would lose characters that the existing (Latin-1) file
    /// cannot hold.
    EncodingLossy,
    /// Any other input/output failure.
    Io(io::ErrorKind),
}

impl FileErrorKind {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            FileErrorKind::NotFound => "file-error-not-found",
            FileErrorKind::PermissionDenied => "file-error-permission-denied",
            FileErrorKind::AlreadyExists => "file-error-already-exists",
            FileErrorKind::NeedsConfirmation => "file-error-needs-confirmation",
            FileErrorKind::TooLarge { .. } => "file-error-too-large",
            FileErrorKind::UnsupportedEncoding(_) => "file-error-unsupported-encoding",
            FileErrorKind::CorruptGzip => "file-error-corrupt-gzip",
            FileErrorKind::NoGamesFound { .. } => "file-error-no-games-found",
            FileErrorKind::EncodingLossy => "file-error-encoding-lossy",
            FileErrorKind::Io(_) => "file-error-io",
        }
    }

    /// Classifies an I/O error (the error itself is dropped: it is not
    /// `Clone`).
    pub fn from_io(error: &io::Error) -> FileErrorKind {
        match error.kind() {
            io::ErrorKind::NotFound => FileErrorKind::NotFound,
            io::ErrorKind::PermissionDenied => FileErrorKind::PermissionDenied,
            io::ErrorKind::AlreadyExists => FileErrorKind::AlreadyExists,
            kind => FileErrorKind::Io(kind),
        }
    }
}

/// A failed file operation: what happened and, when known, to which file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("file error: {kind:?}{}", path_suffix(.path.as_ref()))]
pub struct FileError {
    /// What happened.
    pub kind: FileErrorKind,
    /// The file involved, when there is one.
    pub path: Option<PathBuf>,
}

fn path_suffix(path: Option<&PathBuf>) -> String {
    path.map_or_else(String::new, |p| format!(" ({})", p.display()))
}

impl FileError {
    /// An error of `kind` about `path`.
    pub fn new(kind: FileErrorKind, path: impl Into<Option<PathBuf>>) -> FileError {
        FileError {
            kind,
            path: path.into(),
        }
    }

    /// An I/O error about `path`.
    pub fn from_io(error: &io::Error, path: impl Into<Option<PathBuf>>) -> FileError {
        FileError::new(FileErrorKind::from_io(error), path)
    }

    /// Maps a failure of the PGN reader.
    pub(crate) fn from_pgn(error: &PgnError, path: Option<PathBuf>) -> FileError {
        let kind = match error {
            PgnError::Open { source, .. } | PgnError::Io { source, .. } => {
                FileErrorKind::from_io(source)
            }
            PgnError::Gzip { .. } => FileErrorKind::CorruptGzip,
            PgnError::UnsupportedEncoding { encoding } => {
                FileErrorKind::UnsupportedEncoding(encoding)
            }
            _ => FileErrorKind::Io(io::ErrorKind::Other),
        };
        FileError::new(kind, path)
    }

    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        self.kind.message_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_errors_are_classified() {
        let e = io::Error::from(io::ErrorKind::NotFound);
        assert_eq!(FileErrorKind::from_io(&e), FileErrorKind::NotFound);
        let e = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(FileErrorKind::from_io(&e), FileErrorKind::PermissionDenied);
        let e = io::Error::from(io::ErrorKind::TimedOut);
        assert_eq!(
            FileErrorKind::from_io(&e),
            FileErrorKind::Io(io::ErrorKind::TimedOut)
        );
    }

    #[test]
    fn every_kind_has_a_key() {
        let err = FileError::new(FileErrorKind::CorruptGzip, None);
        assert_eq!(err.message_key(), "file-error-corrupt-gzip");
        assert!(err.to_string().contains("CorruptGzip"));
    }
}
