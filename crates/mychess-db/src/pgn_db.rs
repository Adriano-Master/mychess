// Behaviour derived from Scid vs PC 4.27, tcl/file.tcl (225-260, 636-660),
// src/mfile.cpp (84-127) and src/tkscid.cpp (sc_base_create / sc_base_import
// with a memory-only base).
// Copyright (c) 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is
// distributed under the GNU General Public License (see its COPYING); this
// file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! A PGN file (or `.pgn.gz`) opened as a read-only database held in memory
//! (decisions D351 to D355 and the 3c section of
//! `docs/specs/m3-3-filtro-busca-importacao.md`).
//!
//! [`PgnDatabase::open`] reads the file once, with the same tolerant reader
//! and the same [`crate::si4::PreparedGame`] as the import ([`crate::import_pgn`]), and
//! keeps what an SI4 database keeps: the 47-byte index entries, the name base
//! and the `.sg4` records, all in memory and laid out as the files are (records
//! never cross a 131,072-byte block). The ECO code, the signatures, the order
//! of the names and the limits of the format are therefore the ones of an
//! imported database. Nothing is written to disk.
//!
//! The database is read through [`GameSource`], like a [`Si4Database`]; the
//! searches, the text filter and the export accept both. There is no way to
//! change a `PgnDatabase` (D352): the type implements no write interface, and
//! importing into a database of type 3 is refused with
//! [`ImportError::ReadOnly`](crate::ImportError::ReadOnly).
//!
//! # Failing and cancelling
//!
//! Unlike the import, opening is all or nothing (D354): a cancelled open, a
//! read failure (I/O or gzip), a limit of the format and
//! [`OpenOptions::max_games`] all return an error and no database. Remarks of
//! the reader about the games (illegal moves, bad tags...) do not prevent
//! opening and come back in the [`OpenReport`].
//!
//! # File kinds
//!
//! `.pgn.gz` is recognised by its content (`1F 8B`), not by the extension, and
//! any other extension is tried as PGN (D353). A `.zip` (or another archive or
//! compressed format that is not gzip) and files with a NUL byte near the
//! start (SI4 files, other binaries) are refused with
//! [`PgnOpenError::UnsupportedFormat`].
//!
//! # Already open
//!
//! Telling that a file is already open is the job of the slot manager;
//! [`PgnDatabase::path`] is the canonical path it can compare.

use std::cell::Cell;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use mychess_core::Game;
use mychess_pgn::read::{GameChunker, PgnError, RawGame as PgnRawGame};
use rayon::prelude::*;

use crate::filter::Filter;
use crate::import::{
    CHUNK_BYTES, CHUNK_GAMES, CountingReader, ImportNotice, ImportNoticeKind, ImportOptions,
    ImportReport, Outcome, Processed, READ_ONLY_BASE_TYPE, process,
};
use crate::platform;
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::write::plan_append;
use crate::si4::{
    ENTRY_LEN, HEADER_LEN, Header, HeaderBytes, IndexEntry, MAX_GAMES, NameBase, NameBuilder,
    NameKind, RawGame, Si4Database, Si4Error,
};
use crate::source::GameSource;

/// The description of a PGN opened as a database: `ErrReadOnly` of the Scid
/// (`tkscid.cpp:1091-1097`). The interface should show its own translation,
/// chosen when [`PgnDatabase::is_read_only`] is true.
pub const READ_ONLY_DESCRIPTION: &str = "This database is read-only and can't be altered.";

/// Default of [`OpenOptions::max_games`]: 5 million games, as the Clipbase
/// (decision D355).
pub const DEFAULT_MAX_GAMES: u32 = 5_000_000;

/// Default of [`OpenOptions::max_bytes`]: 2 GiB of records.
pub const DEFAULT_MAX_BYTES: u64 = 2 << 30;

/// Bytes of the file inspected to recognise a format that is not PGN.
const SNIFF_LEN: usize = 4096;

/// The kinds of name of White, Black, Event, Site and Round, in the order of
/// [`PreparedGame::names`](crate::si4::PreparedGame::names).
const NAME_KINDS: [NameKind; 5] = [
    NameKind::Player,
    NameKind::Player,
    NameKind::Event,
    NameKind::Site,
    NameKind::Round,
];

/// What the report of opening a PGN is: the report of the import that built
/// the database. `games_added` is the number of games in the database,
/// `added` is `1..games_added + 1`, `interrupted` is always false and
/// `stopped` is always `None`, because those cases are errors here (D354).
pub type OpenReport = ImportReport;

/// Options of [`PgnDatabase::open`].
///
/// New fields will come with new versions: build it with
/// `OpenOptions { max_games: n, ..OpenOptions::default() }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenOptions {
    /// How the games are read and prepared. The default is the import into an
    /// open database (text between games ignored, `PlyCount` kept, `ScidFlags`
    /// applied); `total_bytes` is filled in with the size of the file.
    pub import: ImportOptions,
    /// The most games the database may hold ([`DEFAULT_MAX_GAMES`]). Games
    /// that are skipped do not count. Beyond it the open fails with
    /// [`PgnOpenError::TooManyGames`] (D355).
    pub max_games: u32,
    /// The most bytes the records may take in memory ([`DEFAULT_MAX_BYTES`],
    /// 2 GiB). Beyond it the open fails with [`PgnOpenError::TooLarge`]. The
    /// peak while loading is about 1.25 times the records (the buffer grows
    /// by a quarter, not by doubling, and a failed allocation is an error,
    /// [`PgnOpenError::OutOfMemory`], not an abort), plus the index
    /// (47 bytes per game) and the names. The format itself reaches 4 GiB.
    pub max_bytes: u64,
}

impl Default for OpenOptions {
    fn default() -> OpenOptions {
        OpenOptions {
            import: ImportOptions::default(),
            max_games: DEFAULT_MAX_GAMES,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

/// A file format that is not supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnsupportedKind {
    /// A `.zip` archive.
    Zip,
    /// A `.7z` archive.
    SevenZip,
    /// A `.rar` archive.
    Rar,
    /// A bzip2 stream (`.bz2`).
    Bzip2,
    /// An xz stream (`.xz`).
    Xz,
    /// A Zstandard stream (`.zst`).
    Zstd,
    /// A `compress` (`.Z`) stream.
    Compress,
    /// Binary data (a NUL byte near the start), such as an SI4 file.
    Binary,
}

impl fmt::Display for UnsupportedKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UnsupportedKind::Zip => "zip archive",
            UnsupportedKind::SevenZip => "7z archive",
            UnsupportedKind::Rar => "rar archive",
            UnsupportedKind::Bzip2 => "bzip2 stream",
            UnsupportedKind::Xz => "xz stream",
            UnsupportedKind::Zstd => "zstd stream",
            UnsupportedKind::Compress => "compress stream",
            UnsupportedKind::Binary => "binary data",
        })
    }
}

/// Why a PGN could not be opened as a database. No database is returned in
/// any of these cases (D354).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PgnOpenError {
    /// The file could not be opened or inspected.
    #[error("cannot open {path}")]
    Open {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// The file is not PGN and not gzip (D353).
    #[error("unsupported format ({kind}): {path}")]
    UnsupportedFormat {
        /// The file.
        path: PathBuf,
        /// What it looks like.
        kind: UnsupportedKind,
    },
    /// The input could not be read (I/O error or corrupt gzip stream).
    #[error("cannot read {path}")]
    Read {
        /// The file.
        path: PathBuf,
        /// The error of the reader.
        #[source]
        source: PgnError,
    },
    /// The opening was cancelled.
    #[error("opening cancelled")]
    Interrupted,
    /// The file has more games than [`OpenOptions::max_games`].
    #[error("the file has more than {limit} games")]
    TooManyGames {
        /// The limit in force.
        limit: u32,
    },
    /// The file has more games than the format can hold.
    #[error("the file has more than the {limit} games a database can hold")]
    GameLimit {
        /// [`MAX_GAMES`].
        limit: u32,
    },
    /// A name base of the format is full.
    #[error("too many {kind} names (game {game}), the maximum is {limit}")]
    NameLimit {
        /// The kind of name.
        kind: NameKind,
        /// The largest number of names of that kind.
        limit: u32,
        /// Number of the game in the file that did not fit.
        game: u64,
    },
    /// The records would not fit in 4 GiB, the reach of the 32-bit offsets of
    /// the format.
    #[error("the games take more than 4 GiB (game {game})")]
    GamesFileFull {
        /// Number of the game in the file that did not fit.
        game: u64,
    },
    /// The records take more than [`OpenOptions::max_bytes`].
    #[error("the games take more than {limit} bytes in memory (game {game})")]
    TooLarge {
        /// The limit in force.
        limit: u64,
        /// Number of the game in the file that did not fit.
        game: u64,
    },
    /// The memory for the database could not be allocated.
    #[error("out of memory loading game {game}")]
    OutOfMemory {
        /// Number of the game in the file that did not fit.
        game: u64,
    },
    /// The database could not be assembled (an internal inconsistency).
    #[error("cannot build the database: {0}")]
    Build(#[source] Si4Error),
}

/// Looks at the first bytes of a file for a format that is not PGN.
fn sniff(head: &[u8]) -> Option<UnsupportedKind> {
    if head.starts_with(&[0x1F, 0x8B]) {
        return None; // gzip: the reader decompresses it
    }
    // bzip2: "BZh" and a block size digit 1-9. The block magic (pi) is not
    // required: the tests of the slice use a bare "BZh9" followed by filler.
    if head.starts_with(b"BZh") && head.get(3).is_some_and(|d| (b'1'..=b'9').contains(d)) {
        return Some(UnsupportedKind::Bzip2);
    }
    let magic: [(&[u8], UnsupportedKind); 7] = [
        (&[0x1F, 0x9D], UnsupportedKind::Compress),
        (b"PK\x03\x04", UnsupportedKind::Zip),
        (b"PK\x05\x06", UnsupportedKind::Zip),
        (b"7z\xBC\xAF\x27\x1C", UnsupportedKind::SevenZip),
        (b"Rar!\x1A\x07", UnsupportedKind::Rar),
        (b"\xFD7zXZ\x00", UnsupportedKind::Xz),
        (b"\x28\xB5\x2F\xFD", UnsupportedKind::Zstd),
    ];
    for (prefix, kind) in magic {
        if head.starts_with(prefix) {
            return Some(kind);
        }
    }
    head.contains(&0).then_some(UnsupportedKind::Binary)
}

/// An SI4 database whose `.sg4` is a buffer in memory.
type MemoryDatabase = Si4Database<Cursor<Vec<u8>>>;

/// A PGN file opened as a read-only database (see the [module](self)).
///
/// ```no_run
/// use mychess_db::{CancelToken, GameSource, NoProgress, OpenOptions, PgnDatabase};
///
/// let (mut db, report) = PgnDatabase::open(
///     "games.pgn.gz".as_ref(),
///     &OpenOptions::default(),
///     &mut NoProgress,
///     &CancelToken::new(),
/// )?;
/// println!("{} games\n{report}", db.len());
/// let first = db.game(1)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct PgnDatabase {
    path: PathBuf,
    db: MemoryDatabase,
}

impl fmt::Debug for PgnDatabase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgnDatabase")
            .field("path", &self.path)
            .field("games", &self.db.len())
            .finish_non_exhaustive()
    }
}

impl PgnDatabase {
    /// Opens the PGN (or `.pgn.gz`) file at `path`.
    ///
    /// The progress is `(bytes taken from the file, size of the file)`,
    /// compressed bytes for a `.gz` (as `BytesUsed()` in the Scid), sent after
    /// every 100 games; the token is looked at at the same points.
    ///
    /// # Errors
    ///
    /// See [`PgnOpenError`]. Cancelling returns
    /// [`PgnOpenError::Interrupted`].
    pub fn open(
        path: &Path,
        options: &OpenOptions,
        progress: &mut dyn ProgressSink,
        cancel: &CancelToken,
    ) -> Result<(PgnDatabase, OpenReport), PgnOpenError> {
        let open_error = |source| PgnOpenError::Open {
            path: path.to_owned(),
            source,
        };
        let mut file = File::open(path).map_err(open_error)?;
        let size = file.metadata().map_err(open_error)?.len();
        let mut head = Vec::with_capacity(SNIFF_LEN);
        (&mut file)
            // SNIFF_LEN is a small constant: the cast cannot truncate.
            .take(SNIFF_LEN as u64)
            .read_to_end(&mut head)
            .map_err(open_error)?;
        if let Some(kind) = sniff(&head) {
            return Err(PgnOpenError::UnsupportedFormat {
                path: path.to_owned(),
                kind,
            });
        }
        file.seek(SeekFrom::Start(0)).map_err(open_error)?;
        // Falls back to the path as given when the system cannot canonicalise
        // it (the file was opened, so this is rare).
        let canonical =
            platform::display_path(fs::canonicalize(path).unwrap_or_else(|_| path.to_owned()));
        let mut options = options.clone();
        options.import.total_bytes.get_or_insert(size);
        PgnDatabase::from_reader(file, &canonical, &options, progress, cancel)
    }

    /// Builds the database from a stream of PGN (or gzip) data. `path` is
    /// stored as given (it is not canonicalised) and labels errors. No format sniffing is done here:
    /// see [`PgnDatabase::open`].
    ///
    /// # Errors
    ///
    /// The ones of [`PgnDatabase::open`] except [`PgnOpenError::Open`] and
    /// [`PgnOpenError::UnsupportedFormat`].
    pub fn from_reader<R: Read>(
        reader: R,
        path: &Path,
        options: &OpenOptions,
        progress: &mut dyn ProgressSink,
        cancel: &CancelToken,
    ) -> Result<(PgnDatabase, OpenReport), PgnOpenError> {
        let started = Instant::now();
        let read_options = options.import.read_options();
        let consumed = Rc::new(Cell::new(0u64));
        let mut chunker = GameChunker::new(
            CountingReader {
                inner: reader,
                count: Rc::clone(&consumed),
            },
            read_options.max_game_bytes,
        );
        let total = options.import.total_bytes.unwrap_or(0);
        let mut loader = Loader::new(options);

        if cancel.is_cancelled() {
            return Err(PgnOpenError::Interrupted);
        }
        loop {
            let mut raws: Vec<PgnRawGame> = Vec::with_capacity(CHUNK_GAMES);
            let mut bytes = 0usize;
            let mut finished = false;
            let mut read_error: Option<PgnError> = None;
            while raws.len() < CHUNK_GAMES && bytes < CHUNK_BYTES {
                match chunker.next() {
                    None => {
                        finished = true;
                        break;
                    }
                    Some(Ok(raw)) => {
                        bytes += raw.bytes().len();
                        raws.push(raw);
                    }
                    Some(Err(error)) => {
                        read_error = Some(error);
                        break;
                    }
                }
            }
            let processed: Vec<Processed> = raws
                .par_iter()
                .map(|raw| process(raw, &read_options, &options.import))
                .collect();
            for p in processed {
                loader.absorb(p)?;
            }
            if let Some(source) = read_error {
                return Err(PgnOpenError::Read {
                    path: path.to_owned(),
                    source,
                });
            }
            let done = if total > 0 {
                consumed.get().min(total)
            } else {
                consumed.get()
            };
            progress.progress(done, total);
            if finished {
                break;
            }
            if cancel.is_cancelled() {
                return Err(PgnOpenError::Interrupted);
            }
        }
        let report_elapsed = started.elapsed();
        let (db, mut report) = loader.finish(path, consumed.get() > 0)?;
        report.elapsed = report_elapsed;
        report.text_outside_games = chunker.skipped_bytes();
        Ok((
            PgnDatabase {
                path: path.to_owned(),
                db,
            },
            report,
        ))
    }

    /// The path of the database. [`PgnDatabase::open`] makes it canonical
    /// (absolute, symbolic links resolved) and, on Windows, writes it in the
    /// usual form (`C:\dir\x.pgn`, `\\server\share\x.pgn`) instead of the
    /// verbatim `\\?\` one; if the system cannot canonicalise it, the path
    /// given is kept. [`PgnDatabase::from_reader`] keeps the path it was given.
    /// The slot manager compares it to tell that a file is already open.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The type of the database: 3, "read-only" (`sc_base type`).
    pub fn base_type(&self) -> u32 {
        self.db.header().base_type()
    }

    /// Always true: a PGN database cannot be changed (D352). Show the
    /// translation of [`READ_ONLY_DESCRIPTION`] as the description.
    #[allow(clippy::unused_self)] // part of the shape of a database handle
    pub const fn is_read_only(&self) -> bool {
        true
    }

    /// A new filter with every game in, value 1 (`Filter::Init`).
    pub fn new_filter(&self) -> Filter {
        Filter::new(self.db.len())
    }

    /// A lower bound of the memory the database takes: the records and the
    /// index. The name base is not counted (D355).
    pub fn memory_estimate(&self) -> u64 {
        // ENTRY_LEN is 47: the cast cannot truncate.
        self.db.games_file_len() + u64::from(self.db.len()) * ENTRY_LEN as u64
    }
}

impl GameSource for PgnDatabase {
    fn header(&self) -> &Header {
        self.db.header()
    }

    fn len(&self) -> u32 {
        self.db.len()
    }

    fn entry(&self, n: u32) -> crate::si4::Result<IndexEntry> {
        self.db.entry(n)
    }

    fn names(&self) -> &NameBase {
        self.db.names()
    }

    fn record_bytes(&mut self, n: u32) -> crate::si4::Result<&[u8]> {
        self.db.record_bytes(n)
    }

    fn raw_game(&mut self, n: u32) -> crate::si4::Result<RawGame> {
        self.db.raw_game(n)
    }

    fn game(&mut self, n: u32) -> crate::si4::Result<Game> {
        self.db.game(n)
    }
}

/// Makes room for `extra` more bytes growing by a quarter (at least 1 MiB)
/// instead of doubling, and reports an allocation failure instead of
/// aborting.
fn grow(buffer: &mut Vec<u8>, extra: usize) -> Result<(), std::collections::TryReserveError> {
    if buffer.capacity() - buffer.len() >= extra {
        return Ok(());
    }
    let step = (buffer.capacity() / 4).max(1 << 20);
    buffer.try_reserve_exact(extra.max(step))
}

/// Builds the in-memory SI4 files game by game.
struct Loader {
    /// `.si4`: room for the header, then the entries.
    index: Vec<u8>,
    /// `.sg4`: the records, with the padding that keeps each in one block.
    records: Vec<u8>,
    names: NameBuilder,
    count: u32,
    max_games: u32,
    max_bytes: u64,
    max_messages: usize,
    report: ImportReport,
}

impl Loader {
    fn new(options: &OpenOptions) -> Loader {
        Loader {
            index: vec![0; HEADER_LEN],
            records: Vec::new(),
            names: NameBuilder::new(),
            count: 0,
            max_games: options.max_games,
            max_bytes: options.max_bytes,
            max_messages: options.import.max_messages,
            report: ImportReport::default(),
        }
    }

    fn keep_message(&mut self) -> bool {
        let kept = self.report.diagnostics.len() + self.report.notices.len();
        if kept >= self.max_messages {
            self.report.messages_dropped += 1;
            false
        } else {
            true
        }
    }

    fn absorb(&mut self, processed: Processed) -> Result<(), PgnOpenError> {
        self.report.games_read += 1;
        for d in processed.diagnostics {
            if self.keep_message() {
                self.report.diagnostics.push(d);
            }
        }
        for n in processed.notices {
            if self.keep_message() {
                self.report.notices.push(n);
            }
        }
        let number = processed.number;
        match processed.outcome {
            Outcome::Skipped(reason) => {
                let skipped = u32::try_from(number).unwrap_or(u32::MAX);
                self.report.games_skipped_total += 1;
                if self.report.games_skipped.len() < self.max_messages {
                    self.report.games_skipped.push((skipped, reason));
                } else {
                    self.report.messages_dropped += 1;
                }
                Ok(())
            }
            Outcome::Ready(game) => {
                // Same order of checks as the writer: the format limit first.
                if self.count >= MAX_GAMES {
                    return Err(PgnOpenError::GameLimit { limit: MAX_GAMES });
                }
                if self.count >= self.max_games {
                    return Err(PgnOpenError::TooManyGames {
                        limit: self.max_games,
                    });
                }
                let record = game.record();
                // usize is at most 64 bits: the cast cannot truncate.
                let (offset, padding) = plan_append(self.records.len() as u64, record.len());
                let offset = u32::try_from(offset)
                    .map_err(|_| PgnOpenError::GamesFileFull { game: number })?;
                let padding = usize::try_from(padding).unwrap_or(0);
                let new_len = self
                    .records
                    .len()
                    .saturating_add(padding)
                    .saturating_add(record.len());
                if u64::try_from(new_len).unwrap_or(u64::MAX) > self.max_bytes {
                    return Err(PgnOpenError::TooLarge {
                        limit: self.max_bytes,
                        game: number,
                    });
                }
                let extra = new_len - self.records.len();
                grow(&mut self.records, extra)
                    .map_err(|_| PgnOpenError::OutOfMemory { game: number })?;
                grow(&mut self.index, ENTRY_LEN)
                    .map_err(|_| PgnOpenError::OutOfMemory { game: number })?;
                let mut ids = [0u32; 5];
                for ((slot, kind), name) in ids.iter_mut().zip(NAME_KINDS).zip(game.names()) {
                    let id = self.names.add_name(kind, name).map_err(|e| match e {
                        Si4Error::NameLimit { kind, limit } => PgnOpenError::NameLimit {
                            kind,
                            limit,
                            game: number,
                        },
                        other => PgnOpenError::Build(other),
                    })?;
                    self.names.inc_frequency(kind, id, 1);
                    *slot = id;
                }
                self.records
                    .resize(self.records.len().saturating_add(padding), 0);
                self.records.extend_from_slice(record);
                let mut entry = game.entry().clone();
                entry.offset = offset;
                entry.length = u32::try_from(record.len()).unwrap_or(u32::MAX);
                [
                    entry.white_id,
                    entry.black_id,
                    entry.event_id,
                    entry.site_id,
                    entry.round_id,
                ] = ids;
                self.index.extend_from_slice(&entry.to_bytes());
                self.count += 1;
                Ok(())
            }
        }
    }

    fn finish(
        mut self,
        path: &Path,
        had_input: bool,
    ) -> Result<(MemoryDatabase, ImportReport), PgnOpenError> {
        let mut header = HeaderBytes::new(READ_ONLY_BASE_TYPE);
        header.set_description(READ_ONLY_DESCRIPTION);
        header.set_num_games(self.count);
        if let Some(slot) = self.index.get_mut(..HEADER_LEN) {
            slot.copy_from_slice(header.as_bytes());
        }
        self.records.shrink_to_fit();
        let names = self.names.to_bytes();
        let db = Si4Database::from_parts(
            self.index,
            &names,
            Cursor::new(self.records),
            path.to_owned(),
        )
        .map_err(PgnOpenError::Build)?;
        self.report.games_added = self.count;
        self.report.added = 1..self.count.saturating_add(1);
        if had_input && self.report.games_read == 0 {
            self.report.notices.push(ImportNotice {
                game: 0,
                line: 0,
                kind: ImportNoticeKind::NoGamesFound,
            });
        }
        Ok((db, self.report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::NoProgress;
    use crate::si4::{Si4Writer, bytes_to_string};
    use crate::{HeaderSearch, PlayerStats, search_header};
    use pretty_assertions::assert_eq;

    const THREE: &str = "[Event \"One\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n\
        1. e4 e5 2. Nf3 Nc6 3. Bb5 1-0\n\n\
        [Event \"Two\"]\n[White \"C\"]\n[Black \"B\"]\n[Result \"0-1\"]\n\n\
        1. d4 d5 2. c4 e6 0-1\n\n\
        [Event \"Three\"]\n[White \"E\"]\n[Black \"F\"]\n[Result \"1/2-1/2\"]\n\n\
        1. c4 c5 1/2-1/2\n";

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// One gzip member in a single stored block (inputs under 64 KiB).
    fn gzip_stored(data: &[u8]) -> Vec<u8> {
        let len = u16::try_from(data.len()).unwrap();
        let mut out = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 0xFF, 1];
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(data).to_le_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
        out
    }

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn open(path: &Path) -> Result<(PgnDatabase, OpenReport), PgnOpenError> {
        PgnDatabase::open(
            path,
            &OpenOptions::default(),
            &mut NoProgress,
            &CancelToken::new(),
        )
    }

    #[test]
    fn opens_a_pgn_as_a_read_only_type_3_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "x.pgn", THREE.as_bytes());
        let (mut db, report) = open(&path).unwrap();
        assert_eq!(db.len(), 3);
        assert_eq!(db.header().base_type(), 3);
        assert_eq!(db.base_type(), 3);
        assert!(db.is_read_only());
        assert_eq!(db.header().description(), READ_ONLY_DESCRIPTION.as_bytes());
        assert_eq!(report.games_added, 3);
        assert_eq!(report.added, 1..4);
        assert!(!report.has_warnings());
        let filter = db.new_filter();
        assert_eq!((filter.len(), filter.count()), (3, 3));
        let game = db.game(2).unwrap();
        assert_eq!(game.tags().event, "Two");
        assert_eq!(db.path(), fs::canonicalize(&path).unwrap());
        // Only the PGN is in the directory.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn matches_an_imported_database_entry_by_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "x.pgn", THREE.as_bytes());
        let (pgn, _) = open(&path).unwrap();
        let base = dir.path().join("db");
        let mut writer = Si4Writer::create(&base, 0).unwrap();
        crate::import_pgn(
            THREE.as_bytes(),
            &mut writer,
            &ImportOptions::default(),
            None,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        drop(writer);
        let si4 = Si4Database::open(&base).unwrap();
        assert_eq!(pgn.len(), si4.len());
        for n in 1..=si4.len() {
            let a = GameSource::entry(&pgn, n).unwrap();
            let b = si4.entry(n).unwrap();
            assert_eq!(a, b, "entry {n}");
        }
        for kind in NameKind::ALL {
            let count = si4.names().num_names(kind);
            assert_eq!(pgn.names().num_names(kind), count);
            for id in 0..count {
                assert_eq!(pgn.names().get(kind, id), si4.names().get(kind, id));
                assert_eq!(
                    pgn.names().frequency(kind, id),
                    si4.names().frequency(kind, id)
                );
            }
        }
    }

    #[test]
    fn a_gz_file_gives_the_same_database_whatever_its_extension() {
        let dir = tempfile::tempdir().unwrap();
        let plain = write(&dir, "x.pgn", THREE.as_bytes());
        let gz = write(&dir, "x.pgn.gz", &gzip_stored(THREE.as_bytes()));
        let odd = write(&dir, "x.data", &gzip_stored(THREE.as_bytes()));
        let (a, _) = open(&plain).unwrap();
        for path in [gz, odd] {
            let (b, _) = open(&path).unwrap();
            assert_eq!(a.len(), b.len());
            for n in 1..=a.len() {
                assert_eq!(a.entry(n).unwrap(), b.entry(n).unwrap());
            }
        }
        // Unknown extension, plain content: tried as PGN.
        let other = write(&dir, "x.txt", THREE.as_bytes());
        assert_eq!(open(&other).unwrap().0.len(), 3);
    }

    #[test]
    fn warnings_do_not_prevent_opening() {
        let pgn = "[Event \"E\"]\n[Result \"*\"]\n\n1. e4 e5 2. Ke5 *\n\n\
                   [Event \"F\"]\n[Result \"*\"]\n\n1. d4 *\n";
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "w.pgn", pgn.as_bytes());
        let (db, report) = open(&path).unwrap();
        assert_eq!(db.len(), 2);
        assert!(report.has_warnings());
        assert!(report.to_string().contains("(game 1, line"), "{report}");
    }

    #[test]
    fn refuses_zip_and_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        let zip = write(&dir, "x.zip", b"PK\x03\x04rest of the archive");
        let si4 = write(&dir, "x.si4", b"Scid.si\0\x01\x90");
        let kind = |p: &Path| match open(p) {
            Err(PgnOpenError::UnsupportedFormat { kind, .. }) => kind,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(kind(&zip), UnsupportedKind::Zip);
        assert_eq!(kind(&si4), UnsupportedKind::Binary);
        assert!(matches!(
            open(&dir.path().join("missing.pgn")),
            Err(PgnOpenError::Open { .. })
        ));
    }

    #[test]
    fn an_empty_file_is_an_empty_database() {
        let dir = tempfile::tempdir().unwrap();
        let (db, report) = open(&write(&dir, "e.pgn", b"")).unwrap();
        assert!(GameSource::is_empty(&db));
        assert_eq!(report.added, 1..1);
        assert_eq!(db.new_filter().count(), 0);
    }

    #[test]
    fn cancelling_returns_no_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "x.pgn", THREE.as_bytes());
        let cancel = CancelToken::new();
        cancel.cancel();
        let result = PgnDatabase::open(&path, &OpenOptions::default(), &mut NoProgress, &cancel);
        assert!(matches!(result, Err(PgnOpenError::Interrupted)));
    }

    #[test]
    fn cancelling_in_the_middle_returns_no_database() {
        let mut text = String::new();
        for _ in 0..250 {
            text.push_str(THREE);
            text.push('\n');
        }
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "big.pgn", text.as_bytes());
        let cancel = CancelToken::new();
        let mut reports = 0;
        let mut sink = |_: u64, _: u64| {
            reports += 1;
            cancel.cancel();
        };
        let result = PgnDatabase::open(&path, &OpenOptions::default(), &mut sink, &cancel);
        assert!(matches!(result, Err(PgnOpenError::Interrupted)));
        assert_eq!(reports, 1);
    }

    #[test]
    fn too_many_games_fails_the_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "x.pgn", THREE.as_bytes());
        let options = OpenOptions {
            max_games: 2,
            ..OpenOptions::default()
        };
        let result = PgnDatabase::open(&path, &options, &mut NoProgress, &CancelToken::new());
        assert!(matches!(
            result,
            Err(PgnOpenError::TooManyGames { limit: 2 })
        ));
        let exact = OpenOptions {
            max_games: 3,
            ..OpenOptions::default()
        };
        assert!(PgnDatabase::open(&path, &exact, &mut NoProgress, &CancelToken::new()).is_ok());
    }

    #[test]
    fn a_corrupt_gzip_stream_fails_the_open() {
        let mut bytes = gzip_stored(THREE.as_bytes());
        let len = bytes.len();
        bytes[len - 6] ^= 0xFF; // the CRC
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "bad.pgn.gz", &bytes);
        assert!(matches!(open(&path), Err(PgnOpenError::Read { .. })));
    }

    #[test]
    fn progress_ends_at_the_size_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "x.pgn", THREE.as_bytes());
        let mut last = (0, 0);
        let mut sink = |done: u64, total: u64| last = (done, total);
        PgnDatabase::open(
            &path,
            &OpenOptions::default(),
            &mut sink,
            &CancelToken::new(),
        )
        .unwrap();
        let size = THREE.len() as u64;
        assert_eq!(last, (size, size));
    }

    #[test]
    fn the_header_search_and_the_text_filter_work_on_a_pgn() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "x.pgn", THREE.as_bytes());
        let (mut db, _) = open(&path).unwrap();
        let mut filter = db.new_filter();
        let stats = PlayerStats::from_database(&db);
        let search = HeaderSearch {
            black: "B".into(),
            ..HeaderSearch::default()
        };
        let report = search_header(
            &mut db,
            &search,
            &mut filter,
            &stats,
            None,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(report.matched, 2);
        assert_eq!(filter.count(), 2);
        let mut all = db.new_filter();
        assert_eq!(all.textfilter(&db, "three", true), 2);
        assert_eq!(all.count(), 1);
        let name = db.entry(3).unwrap().event_id;
        assert_eq!(
            bytes_to_string(db.names().get(NameKind::Event, name).unwrap()),
            "Three"
        );
    }
}
