//! Why a database could not be opened or used (spec M3-4, D500).
//!
//! Like [`FileError`], nothing here carries a message or a `std::io::Error`:
//! a [`Message`](crate::Message) must be `Clone` and comparable, and the
//! front-end translates from the variant with Fluent (`message_key`).

use mychess_db::PgnOpenError;
use mychess_db::si4::Si4Error;

use crate::error::{FileError, FileErrorKind};

/// What went wrong with a database operation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BaseError {
    /// A file operation failed (not found, permission, I/O).
    #[error("{0}")]
    File(FileError),
    /// An `.si3` database (Scid 3): it must be upgraded to SI4 first.
    #[error("the database is in the old SI3 format and must be upgraded")]
    Si3NeedsUpgrade,
    /// The `.si4`, `.sn4` or `.sg4` file is damaged or has another version.
    #[error("the database files are corrupt or unsupported")]
    Corrupt,
    /// The database has an unfinished transaction; open it for writing
    /// (compact or repair) first.
    #[error("the database has an unfinished transaction")]
    PendingJournal,
    /// The PGN file is not PGN (an archive or binary data).
    #[error("the file is not a PGN text")]
    UnsupportedFormat,
    /// The PGN file has more games than the limit.
    #[error("the file has more than {limit} games")]
    TooManyGames {
        /// The limit in force.
        limit: u32,
    },
    /// The games take more memory than the limit.
    #[error("the games take more than {limit} bytes in memory")]
    TooLarge {
        /// The limit in force.
        limit: u64,
    },
    /// The memory for the database could not be allocated.
    #[error("out of memory")]
    OutOfMemory,
    /// The file has no game at all.
    #[error("the file has no games")]
    NoGames,
    /// The opening was cancelled by the user.
    #[error("opening cancelled")]
    Cancelled,
    /// A game could not be read from the database.
    #[error("game {number} cannot be read")]
    GameUnreadable {
        /// The game number (from 1).
        number: u32,
    },
    /// The Clipbase holds as many games as its limit allows.
    #[error("the clipbase has a limit of {limit} games")]
    ClipbaseLimit {
        /// The limit in force.
        limit: u32,
    },
    /// The game cannot be stored in the SI4 format.
    #[error("the game cannot be stored in a database")]
    Unencodable,
    /// The game starts from a position the SI4 format cannot hold: Chess960
    /// (except position 518) or a start the `.sg4` record cannot encode.
    /// The PGN format keeps those games (M9-6, D4507).
    #[error("the game start position cannot be stored in a database")]
    UnsupportedStart,
    /// The database cannot take more names of one kind (players, events,
    /// sites or rounds); the games that fit were copied (slice 4d, D579).
    #[error("the database cannot take more than {limit} names of one kind")]
    NameLimit {
        /// The limit in force.
        limit: u32,
    },
    /// Another program (or another part of this one) has the database open
    /// for writing.
    #[error("the database is open for writing elsewhere")]
    AlreadyOpen,
    /// The database is read-only: its flags cannot be changed.
    #[error("the database is read-only")]
    ReadOnly,
    /// A source file of the import is one of the files of the database it
    /// is imported into.
    #[error("the source file is a file of the database")]
    SourceIsTarget,
    /// The file an export would write is one of the files of an open
    /// database (spec M3-9, D2013).
    #[error("the destination is the file of an open database")]
    ExportOverBase,
    /// A database with that name exists already; nothing was created.
    #[error("a database with that name exists already")]
    AlreadyExists,
    /// The file on disk is not the one that was read (it changed behind the
    /// back of the slot): nothing was written and the database was reloaded.
    #[error("the database changed on disk")]
    Stale,
    /// An inconsistency that cannot happen; kept as an error so the program
    /// does not panic.
    #[error("internal error")]
    Internal,
}

impl BaseError {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            BaseError::File(error) => error.message_key(),
            BaseError::Si3NeedsUpgrade => "base-error-si3-needs-upgrade",
            BaseError::Corrupt => "base-error-corrupt",
            BaseError::PendingJournal => "base-error-pending-journal",
            BaseError::UnsupportedFormat => "base-error-unsupported-format",
            BaseError::TooManyGames { .. } => "base-error-too-many-games",
            BaseError::TooLarge { .. } => "base-error-too-large",
            BaseError::OutOfMemory => "base-error-out-of-memory",
            BaseError::NoGames => "base-error-no-games",
            BaseError::Cancelled => "base-error-cancelled",
            BaseError::GameUnreadable { .. } => "base-error-game-unreadable",
            BaseError::ClipbaseLimit { .. } => "base-error-clipbase-limit",
            BaseError::Unencodable => "base-error-unencodable",
            BaseError::UnsupportedStart => "base-error-unsupported-start",
            BaseError::NameLimit { .. } => "base-error-name-limit",
            BaseError::AlreadyOpen => "base-error-already-open",
            BaseError::ReadOnly => "base-error-read-only",
            BaseError::ExportOverBase => "base-error-export-self",
            BaseError::AlreadyExists => "base-error-exists",
            BaseError::SourceIsTarget => "base-error-import-self",
            BaseError::Stale => "base-error-stale",
            BaseError::Internal => "base-error-internal",
        }
    }

    /// Maps a failure of the SI4 reader.
    pub(crate) fn from_si4(error: &Si4Error) -> BaseError {
        match error {
            Si4Error::Io { path: file, source } => {
                BaseError::File(FileError::from_io(source, file.clone()))
            }
            Si4Error::PendingJournal { .. } => BaseError::PendingJournal,
            Si4Error::AlreadyOpen { .. } => BaseError::AlreadyOpen,
            Si4Error::AlreadyExists { .. } => BaseError::AlreadyExists,
            Si4Error::GameLimit { limit } => BaseError::TooManyGames { limit: *limit },
            Si4Error::NameLimit { limit, .. } => BaseError::NameLimit { limit: *limit },
            Si4Error::RecordTooLarge { .. } | Si4Error::InvalidText { .. } => {
                BaseError::Unencodable
            }
            Si4Error::UnsupportedStart { .. } => BaseError::UnsupportedStart,
            Si4Error::OldVersion { .. } => BaseError::Si3NeedsUpgrade,
            Si4Error::InvalidGameNumber { number, .. } => {
                BaseError::GameUnreadable { number: *number }
            }
            _ => BaseError::Corrupt,
        }
    }

    /// Maps a failure to encode a game for a database: a start position the
    /// format cannot hold gets its own message, the rest is "unencodable".
    pub(crate) fn from_encode(error: &Si4Error) -> BaseError {
        match error {
            Si4Error::UnsupportedStart { .. } => BaseError::UnsupportedStart,
            _ => BaseError::Unencodable,
        }
    }

    /// Maps a failure of the PGN loader.
    pub(crate) fn from_pgn(error: &PgnOpenError) -> BaseError {
        match error {
            PgnOpenError::Open { path, source } => {
                BaseError::File(FileError::from_io(source, path.clone()))
            }
            PgnOpenError::UnsupportedFormat { .. } => BaseError::UnsupportedFormat,
            PgnOpenError::Read { path, source } => {
                BaseError::File(FileError::from_pgn(source, Some(path.clone())))
            }
            PgnOpenError::Interrupted => BaseError::Cancelled,
            PgnOpenError::TooManyGames { limit } | PgnOpenError::GameLimit { limit } => {
                BaseError::TooManyGames { limit: *limit }
            }
            PgnOpenError::TooLarge { limit, .. } => BaseError::TooLarge { limit: *limit },
            PgnOpenError::OutOfMemory { .. } => BaseError::OutOfMemory,
            PgnOpenError::GamesFileFull { .. } => BaseError::TooLarge { limit: 1 << 32 },
            _ => BaseError::Corrupt,
        }
    }
}

impl From<FileErrorKind> for BaseError {
    fn from(kind: FileErrorKind) -> BaseError {
        BaseError::File(FileError::new(kind, None))
    }
}

#[cfg(test)]
mod start_tests {
    use super::*;

    #[test]
    fn an_unsupported_start_has_its_own_error_and_message() {
        let error = Si4Error::UnsupportedStart { reason: "x" };
        assert_eq!(BaseError::from_encode(&error), BaseError::UnsupportedStart);
        assert_eq!(BaseError::from_si4(&error), BaseError::UnsupportedStart);
        assert_eq!(
            BaseError::UnsupportedStart.message_key(),
            "base-error-unsupported-start"
        );
        // The Clipbase refuses a Chess960 game (not 518) with that error.
        let mut clip = crate::bases::clipbase::Clipbase::new(10).expect("clipbase");
        let game = mychess_core::Game::from_position(
            mychess_core::Position::chess960_start(100).expect("start"),
        );
        assert_eq!(clip.push_game(&game), Err(BaseError::UnsupportedStart));
        let other = Si4Error::GamesFileFull;
        assert_eq!(BaseError::from_encode(&other), BaseError::Unencodable);
    }
}
