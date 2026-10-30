//! Errors of the SI4 reader (`docs/specs/m3-1-leitura-si4.md`, D265).
//!
//! Every variant carries diagnostic context (file, offset, game number). The
//! messages are for logs; the interface translates from the variant.

use std::fmt;
use std::io;
use std::path::PathBuf;

use mychess_core::{FenError, GameError};

use super::names::NameKind;

/// One of the three files of an SI4 database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Si4File {
    /// The `.si4` index file.
    Index,
    /// The `.sn4` name file.
    Names,
    /// The `.sg4` game file.
    Games,
}

impl fmt::Display for Si4File {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Si4File::Index => "index (.si4)",
            Si4File::Names => "name (.sn4)",
            Si4File::Games => "game (.sg4)",
        })
    }
}

/// The part of the name file where a problem was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NameSection {
    /// The 36-byte header.
    Header,
    /// The section of the given kind.
    Names(NameKind),
}

impl fmt::Display for NameSection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameSection::Header => f.write_str("header"),
            NameSection::Names(kind) => write!(f, "{kind} names"),
        }
    }
}

/// Error reading an SI4 database.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Si4Error {
    /// An operating system error while opening or reading a file.
    #[error("i/o error on {path}")]
    Io {
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// The magic bytes of a file are wrong.
    #[error("bad magic in the {file} file")]
    BadMagic {
        /// The file with the wrong magic.
        file: Si4File,
    },
    /// The database was written by an older format (SI3 or earlier).
    #[error("database version {found} is older than 400")]
    OldVersion {
        /// The version found in the header.
        found: u16,
    },
    /// The database was written by a newer format than this reader knows.
    #[error("database version {found} is newer than 400")]
    NewerVersion {
        /// The version found in the header.
        found: u16,
    },
    /// A file is shorter than its contents require.
    #[error("truncated {file} file: {need} bytes needed at offset {offset}")]
    Truncated {
        /// The truncated file.
        file: Si4File,
        /// The offset where the data was expected.
        offset: u64,
        /// The number of bytes required there.
        need: u64,
    },
    /// An index entry is invalid.
    #[error("corrupt index entry for game {game}: {reason}")]
    CorruptIndex {
        /// The game number (base 1).
        game: u32,
        /// What is wrong.
        reason: &'static str,
    },
    /// The name file is invalid.
    #[error("corrupt name file ({section}): {reason}")]
    CorruptNames {
        /// The section where the problem was found.
        section: NameSection,
        /// What is wrong.
        reason: &'static str,
    },
    /// A game record is invalid.
    #[error("corrupt record for game {game} at .sg4 offset {offset}: {reason}")]
    CorruptRecord {
        /// The game number (base 1).
        game: u32,
        /// The offset in the `.sg4` file where the problem was found.
        offset: u64,
        /// What is wrong.
        reason: &'static str,
    },
    /// A game number outside `1..=len`.
    #[error("game number {number} is out of range (1..={len})")]
    InvalidGameNumber {
        /// The number requested.
        number: u32,
        /// The number of games in the database.
        len: u32,
    },
    /// A decoded move is not legal in its position.
    #[error("illegal move at ply {ply} of game {game}")]
    IllegalMove {
        /// The game number (base 1).
        game: u32,
        /// The ply (1-based) of the move within its line, counted from the
        /// start of the game.
        ply: u32,
    },
    /// The core refused an operation while building a game (for example a
    /// move that could not be added), other than an illegal move.
    #[error("cannot build game {game}")]
    Game {
        /// The game number (base 1).
        game: u32,
        /// The error of the core.
        #[source]
        source: GameError,
    },
    /// The start position of a game is not accepted by the core.
    #[error("illegal start position in game {game}")]
    IllegalStart {
        /// The game number (base 1).
        game: u32,
        /// Why the core rejected the FEN.
        #[source]
        source: FenError,
    },

    // ---- Errors of the writer (docs/specs/m3-2-escrita-si4.md) ----
    /// A file of the database already exists, so it was not created.
    #[error("database file {path} already exists")]
    AlreadyExists {
        /// The existing file.
        path: PathBuf,
    },
    /// Another writer holds the advisory lock of the `.si4` file (D291).
    #[error("database {path} is already open for writing")]
    AlreadyOpen {
        /// The `.si4` file that is locked.
        path: PathBuf,
    },
    /// The encoded record does not fit the 17-bit length of the index.
    #[error("game record of {size} bytes exceeds the limit of {limit} bytes")]
    RecordTooLarge {
        /// The size of the encoded record.
        size: usize,
        /// The largest size accepted.
        limit: usize,
    },
    /// A tag cannot be stored (decision D280).
    #[error("cannot store tag {name:?}: {reason}")]
    InvalidTag {
        /// The tag name (clipped).
        name: String,
        /// What is wrong.
        reason: &'static str,
    },
    /// A name or comment contains a NUL byte, which would end the C string.
    #[error("text with a NUL byte cannot be stored: {what}")]
    InvalidText {
        /// Which text.
        what: &'static str,
    },
    /// The name base of a kind is full.
    #[error("{kind} name limit of {limit} exceeded")]
    NameLimit {
        /// The kind of name.
        kind: NameKind,
        /// The largest number of names of that kind.
        limit: u32,
    },
    /// The database has the largest number of games the format can hold.
    #[error("the database already holds the maximum of {limit} games")]
    GameLimit {
        /// `MAX_GAMES`.
        limit: u32,
    },
    /// The `.sg4` file would grow past what a 32-bit offset can address.
    #[error("the game file would exceed 4 GiB")]
    GamesFileFull,
    /// The start position cannot be stored in a `.sg4` record (decision D285).
    #[error("start position cannot be stored: {reason}")]
    UnsupportedStart {
        /// What is wrong.
        reason: &'static str,
    },
    /// A move or a structure of the game cannot be encoded.
    #[error("cannot encode the game at ply {ply}: {reason}")]
    UnencodableGame {
        /// The ply (1-based) within its line, counted from the game start.
        ply: u32,
        /// What is wrong.
        reason: &'static str,
    },
    /// A letter that is not one of the 19 editable flags.
    #[error("unknown flag letter {0:?}")]
    UnknownFlag(char),
    /// A flag that cannot be edited (`START`, `PROMO`, `UPROMO` or a set).
    #[error("flag cannot be edited by the user")]
    NotEditableFlag,
    /// A custom flag number outside 1 to 6.
    #[error("custom flag number {0} is not in 1..=6")]
    InvalidCustomFlag(u8),
    /// A transaction journal (`<base>.txn`) that cannot be understood.
    #[error("corrupt transaction journal {path}: {reason}")]
    CorruptJournal {
        /// The journal file.
        path: PathBuf,
        /// What is wrong.
        reason: &'static str,
    },
    /// An unfinished transaction journal exists; open the database for
    /// writing (or call `journal::recover`) to finish it (decision D311).
    #[error("database has an unfinished transaction journal {path}")]
    PendingJournal {
        /// The journal file.
        path: PathBuf,
    },
    /// A failure injected by a test hook (`journal::FailPoint`); the disk is
    /// left as a killed process would leave it.
    #[error("injected failure at stage {index} ({label})")]
    InjectedFailure {
        /// The index of the stage.
        index: usize,
        /// The label of the stage.
        label: String,
    },
    /// The writer's in-memory state could not be put back after a failed
    /// write; drop it and open the database again (decision D313).
    #[error("writer state is unusable after a failed write; reopen the database")]
    WriterUnusable,
    /// An autoload game number above the format limit.
    #[error("autoload game {value} is above the limit of {limit}")]
    InvalidAutoLoad {
        /// The value requested.
        value: u32,
        /// `MAX_GAMES`.
        limit: u32,
    },
}

/// Result alias for this module.
pub type Result<T, E = Si4Error> = std::result::Result<T, E>;

/// A problem found while decoding one game record, before the game number is
/// known. [`Si4Database`](super::Si4Database) turns it into
/// [`Si4Error::CorruptRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{reason} (at byte {at} of the record)")]
pub struct RecordError {
    at: usize,
    reason: &'static str,
}

impl RecordError {
    pub(crate) fn new(at: usize, reason: &'static str) -> RecordError {
        RecordError { at, reason }
    }

    /// Byte offset within the record where the problem was found.
    pub fn at(&self) -> usize {
        self.at
    }

    /// What is wrong.
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

/// An index entry that cannot be decoded or whose name IDs do not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid index entry: {reason}")]
pub struct IndexEntryError {
    reason: &'static str,
}

impl IndexEntryError {
    pub(crate) fn new(reason: &'static str) -> IndexEntryError {
        IndexEntryError { reason }
    }

    /// What is wrong.
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

/// A move byte that cannot be decoded in its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cannot decode move: {reason}")]
pub struct MoveDecodeError {
    reason: &'static str,
}

impl MoveDecodeError {
    pub(crate) fn new(reason: &'static str) -> MoveDecodeError {
        MoveDecodeError { reason }
    }

    /// What is wrong.
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

/// A start position (FEN) that the piece-list codec refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid start position: {reason}")]
pub struct StartPositionError {
    reason: &'static str,
}

impl StartPositionError {
    pub(crate) fn new(reason: &'static str) -> StartPositionError {
        StartPositionError { reason }
    }

    /// What is wrong.
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}
