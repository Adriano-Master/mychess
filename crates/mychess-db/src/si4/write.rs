// Derived from Scid vs PC 4.27, src/tkscid.cpp (sc_createbase, sc_savegame,
// sc_game_flag, sc_base description/type/autoload), src/gfile.cpp (AddGame),
// src/index.cpp (WriteEntries, WriteHeader) and src/namebase.cpp
// (WriteNameFile).
// Copyright (c) 1999-2003 Shane Hudson. All rights reserved (notices of the
// Scid sources); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! [`Si4Writer`]: a database opened for writing (`.si4` + `.sn4` + `.sg4`).
//!
//! Every operation follows the safe protocol of
//! `docs/specs/m3-2-escrita-si4.md` ("Protocolo de gravação segura"): the
//! data a new index entry points to is on disk before the entry, and the
//! entry before the header that counts it.
//!
//! 1. the `.stc` cache is removed;
//! 2. the record is appended to the `.sg4` (`fsync`);
//! 3. the whole `.sn4` is written to `<base>.sn4.tmp` and renamed over the
//!    old one (`fsync`, `rename`);
//! 4. the entry is written in place (`fsync`);
//! 5. when a game was added, the header with the new count is written
//!    (`fsync`): the commit point.
//!
//! A process that dies at any step leaves a database that opens, with
//! either the old or the new content. The advisory lock of the `.si4` file
//! (decision D291) keeps two writers apart; see [`Si4Writer::open`].

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use mychess_core::Game;

mod eco;
mod elo;
mod flags;
mod name_edit;
mod names;
mod rewrite;
mod share;

pub use elo::SideRating;
pub use name_edit::{NameEdit, NameEditError, NameEditReport, NameEditTarget, NameScope};
pub use names::{NameChange, NameChangeReport, life_span_allows};
pub use rewrite::{MAX_TAG_VALUE, RewriteReport, StripReport, StripWhat, TagOp, TagReport};
pub use share::{SharedField, SharedTag, plan_share_tags};

use super::database::{check_index_size, find_file, strip_known_extension};
use super::entry::{GameFlags, IndexEntry};
use super::error::{IndexEntryError, Result, Si4Error, Si4File};
use super::gfile::BLOCK_SIZE;
use super::header::{ENTRY_LEN, HEADER_LEN, Header, MAX_GAMES};
use super::header_write::HeaderBytes;
use super::journal::{self, FailPoint};
use super::names::{NameBase, NameKind};
use super::names_write::{MAX_NAME_LEN, NAME_MAX_ID, NameBuilder};
use super::prepare::{NAME_FIELDS, PreparedGame};
use super::record_write::MAX_RECORD_LEN;

/// The kind of each of the five names of a game, in the order they are added.
const NAME_KINDS: [NameKind; NAME_FIELDS] = [
    NameKind::Player,
    NameKind::Player,
    NameKind::Event,
    NameKind::Site,
    NameKind::Round,
];

/// What to do with a flag (`sc_game flag <flag> <n> <0|1|invert>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagOp {
    /// Turn the flag on.
    Set,
    /// Turn the flag off.
    Clear,
    /// Turn it on if it is off and off if it is on.
    Toggle,
}

/// Where the next record goes in the `.sg4` file, given its current size
/// (`GFile::AddGame`): `(offset, padding)`, where `padding` is the number of
/// zero bytes to write before the record.
///
/// A record never crosses a block of 131,072 bytes. When it does not fit in
/// the rest of the last block, it starts the next block and the gap is zeros.
/// The last block is never filled: the file ends with the last record. A size
/// that is an exact multiple of the block size counts as a full last block.
pub fn plan_append(file_len: u64, record_len: usize) -> (u64, u64) {
    if file_len == 0 {
        return (0, 0);
    }
    let last_block_size = match file_len % BLOCK_SIZE {
        0 => BLOCK_SIZE,
        rest => rest,
    };
    let record_len = u64::try_from(record_len).unwrap_or(u64::MAX);
    if last_block_size.saturating_add(record_len) > BLOCK_SIZE {
        let offset = file_len.div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
        (offset, offset - file_len)
    } else {
        (file_len, 0)
    }
}

/// A database opened for writing.
///
/// The `.si4` file is locked with [`File::try_lock`] for as long as the
/// writer lives. The lock is advisory on Unix; on Windows it also stops other
/// processes (and other handles of this process) from reading the `.si4`
/// file, so drop the writer before opening a [`Si4Database`](super::Si4Database)
/// on the same files. Scid does not know about the lock.
///
/// ```no_run
/// use mychess_core::Game;
/// use mychess_db::si4::{GameFlags, Si4Writer};
///
/// let mut db = Si4Writer::create("games/mine".as_ref(), 1)?;
/// let number = db.add_game(&Game::new(), GameFlags::empty())?;
/// assert_eq!(number, 1);
/// # Ok::<(), mychess_db::si4::Si4Error>(())
/// ```
#[derive(Debug)]
pub struct Si4Writer {
    pub(super) index_path: PathBuf,
    pub(super) names_path: PathBuf,
    pub(super) games_path: PathBuf,
    pub(super) stem: PathBuf,
    pub(super) index_file: File,
    pub(super) games_file: File,
    pub(super) header_bytes: HeaderBytes,
    pub(super) header: Header,
    pub(super) names: NameBuilder,
    /// Set when a failed write could not be undone in memory and on disk
    /// (decision D313): every later write fails with `WriterUnusable`.
    pub(super) poisoned: bool,
    /// Test hook: stages `write:entries` and `write:header` of the add and
    /// replace operations call it (see [`Si4Writer::set_fail_point`]).
    fail_point: Option<FailPoint>,
}

pub(super) fn io_error(path: &Path) -> impl FnOnce(io::Error) -> Si4Error + '_ {
    move |source| Si4Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

pub(super) fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Makes a directory entry durable (Unix only; elsewhere `rename` is the best
/// we can do). Errors are ignored: the data is already safe.
pub(super) fn sync_parent(path: &Path) {
    #[cfg(unix)]
    {
        let parent = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        if let Ok(dir) = File::open(parent) {
            dir.sync_all().ok();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Writes `bytes` to `<path>.tmp` in the same directory, syncs it and renames
/// it over `path`.
fn replace_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = with_suffix(path, ".tmp");
    let result = (|| {
        let mut file = File::create(&tmp).map_err(io_error(&tmp))?;
        file.write_all(bytes).map_err(io_error(&tmp))?;
        file.sync_all().map_err(io_error(&tmp))?;
        drop(file);
        fs::rename(&tmp, path).map_err(io_error(path))
    })();
    if result.is_err() {
        fs::remove_file(&tmp).ok();
    } else {
        sync_parent(path);
    }
    result
}

fn corrupt_entry(game: u32, error: IndexEntryError) -> Si4Error {
    Si4Error::CorruptIndex {
        game,
        reason: error.reason(),
    }
}

impl Si4Writer {
    /// Creates a new empty database (`sc_base create`): a `.si4` of 182
    /// bytes, a `.sn4` of 36 bytes and an empty `.sg4`, with `base_type` in
    /// the header (0 is what the core writes; the interface uses 1).
    ///
    /// `base` may be the base name or the path of any of the three files.
    /// Unlike Scid it never overwrites: if any of the three files exists it
    /// fails with [`Si4Error::AlreadyExists`]. The `.si4` is created last, so
    /// a failure halfway does not leave something that looks like a database.
    pub fn create(base: &Path, base_type: u32) -> Result<Si4Writer> {
        let stem = strip_known_extension(base);
        let index_path = find_file(&stem, "si4");
        let names_path = find_file(&stem, "sn4");
        let games_path = find_file(&stem, "sg4");
        for path in [&index_path, &names_path, &games_path] {
            if path.exists() {
                return Err(Si4Error::AlreadyExists { path: path.clone() });
            }
        }
        // Only what this call created is cleaned up on failure.
        let mut created: Vec<PathBuf> = Vec::new();
        let result = (|| {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&games_path)
            {
                Ok(file) => {
                    created.push(games_path.clone());
                    file.sync_all().map_err(io_error(&games_path))?;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    return Err(Si4Error::AlreadyExists {
                        path: games_path.clone(),
                    });
                }
                Err(source) => {
                    return Err(Si4Error::Io {
                        path: games_path.clone(),
                        source,
                    });
                }
            }
            replace_file(&names_path, &NameBuilder::new().to_bytes())?;
            created.push(names_path.clone());
            replace_file(&index_path, HeaderBytes::new(base_type).as_bytes())
        })();
        if let Err(error) = result {
            // Do not leave half a database behind.
            for path in &created {
                fs::remove_file(path).ok();
            }
            return Err(error);
        }
        remove_cache(&stem);
        // The files were just created here: if the database cannot be opened
        // (a damaged journal left by an earlier database of that name, a
        // lock...), they are not left behind as a half-made database.
        Si4Writer::open(&stem).inspect_err(|_| {
            // The three names were free when `create` began.
            for path in [&index_path, &names_path, &games_path] {
                fs::remove_file(path).ok();
            }
        })
    }

    /// Opens a database for writing.
    ///
    /// The `.si4` file is locked (decision D291): if another writer holds the
    /// lock the result is [`Si4Error::AlreadyOpen`]. The checks are the ones of
    /// [`Si4Database::open`](super::Si4Database::open) (header, size of the
    /// index, name file, every name ID). Frequencies of names are counted from
    /// the entries, as Scid does when it opens a database; the `maxFrequency`
    /// markers of the file are the floor (decision D279).
    pub fn open(base: &Path) -> Result<Si4Writer> {
        let stem = strip_known_extension(base);
        // Finish an interrupted transaction and drop orphan temporaries first
        // (decision D290; this also covers the `.tmp` of `replace_file`).
        journal::recover(&stem)?;
        let index_path = find_file(&stem, "si4");
        let names_path = find_file(&stem, "sn4");
        let games_path = find_file(&stem, "sg4");
        let index_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&index_path)
            .map_err(io_error(&index_path))?;
        match index_file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(Si4Error::AlreadyOpen { path: index_path });
            }
            Err(TryLockError::Error(source)) => {
                return Err(Si4Error::Io {
                    path: index_path,
                    source,
                });
            }
        }
        let mut head = Vec::new();
        (&index_file)
            .take(HEADER_LEN as u64)
            .read_to_end(&mut head)
            .map_err(io_error(&index_path))?;
        let header_bytes = HeaderBytes::from_bytes(&head)?;
        let header = header_bytes.parse()?;
        let file_len = index_file.metadata().map_err(io_error(&index_path))?.len();
        check_index_size(&header, usize::try_from(file_len).unwrap_or(usize::MAX))?;
        let name_data = fs::read(&names_path).map_err(io_error(&names_path))?;
        let name_base = NameBase::parse(&name_data)?;
        let mut names = NameBuilder::from_name_base(&name_base);
        // Stream the entries (the size was checked above) and count the
        // frequencies without holding the whole index in memory.
        let mut file_ref = &index_file;
        file_ref
            .seek(SeekFrom::Start(HEADER_LEN as u64))
            .map_err(io_error(&index_path))?;
        let body = u64::from(header.num_games()) * ENTRY_LEN as u64;
        let mut reader = BufReader::with_capacity(1 << 16, file_ref.take(body));
        let mut buffer = [0u8; ENTRY_LEN];
        for n in 1..=header.num_games() {
            reader.read_exact(&mut buffer).map_err(|source| {
                if source.kind() == io::ErrorKind::UnexpectedEof {
                    Si4Error::Truncated {
                        file: Si4File::Index,
                        offset: entry_position(n),
                        need: ENTRY_LEN as u64,
                    }
                } else {
                    Si4Error::Io {
                        path: index_path.clone(),
                        source,
                    }
                }
            })?;
            let (entry, _) = IndexEntry::parse_lenient(&buffer).map_err(|e| corrupt_entry(n, e))?;
            entry
                .verify_names(&name_base)
                .map_err(|e| corrupt_entry(n, e))?;
            for (kind, id) in entry_name_ids(&entry) {
                names.inc_frequency(kind, id, 1);
            }
        }
        drop(reader);
        let games_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&games_path)
            .map_err(io_error(&games_path))?;
        Ok(Si4Writer {
            index_path,
            names_path,
            games_path,
            stem,
            index_file,
            games_file,
            header_bytes,
            header,
            names,
            poisoned: false,
            fail_point: None,
        })
    }

    /// Installs a test hook (see [`FailPoint`]): the stages `write:entries`
    /// and `write:header` of the add and replace operations call it, and an
    /// injected failure there goes through the normal error path (the
    /// in-memory names are rolled back and the `.sn4` is rewritten), unlike
    /// the crash simulation of the compaction functions. Intended for tests.
    pub fn set_fail_point(&mut self, hook: FailPoint) {
        self.fail_point = Some(hook);
    }

    /// The test hook, to read the stages it saw.
    pub fn fail_point(&self) -> Option<&FailPoint> {
        self.fail_point.as_ref()
    }

    /// Test hook: makes the header claim `count` games without writing
    /// their entries (to reach `MAX_GAMES` without 16 million games).
    #[cfg(test)]
    pub(crate) fn force_num_games(&mut self, count: u32) {
        self.header_bytes.set_num_games(count);
        if let Ok(header) = self.header_bytes.parse() {
            self.header = header;
        }
    }

    /// The number of games.
    pub fn len(&self) -> u32 {
        self.header.num_games()
    }

    /// Whether the database has no games.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The header as it is on disk.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The name base in memory: names, live frequencies and `maxFrequency`.
    pub fn names(&self) -> &NameBuilder {
        &self.names
    }

    /// The path of the `.si4` file.
    pub fn index_path(&self) -> &Path {
        &self.index_path
    }

    /// The path of the `.sn4` file.
    pub fn names_path(&self) -> &Path {
        &self.names_path
    }

    /// The path of the `.sg4` file.
    pub fn games_path(&self) -> &Path {
        &self.games_path
    }

    /// Encodes and adds a game at the end (`sc_game save 0`). `flags` are
    /// the user flags of the new game. Returns its number (base 1).
    pub fn add_game(&mut self, game: &Game, flags: GameFlags) -> Result<u32> {
        let prepared = PreparedGame::from_game(game, flags)?;
        self.add_prepared(&prepared)
    }

    /// Adds an already encoded game; see [`Si4Writer::add_prepared_batch`].
    pub fn add_prepared(&mut self, game: &PreparedGame) -> Result<u32> {
        let numbers = self.add_prepared_batch(std::slice::from_ref(game))?;
        Ok(numbers.start)
    }

    /// Adds several games with one `.sn4` rewrite and one header write, as
    /// `sc_base import` does (the cost of rewriting the names is paid once).
    /// Returns the numbers given to the games, first included, last
    /// excluded. Nothing is written if any game is refused: the record
    /// size, the name limits and `MAX_GAMES` are checked first.
    pub fn add_prepared_batch(&mut self, games: &[PreparedGame]) -> Result<Range<u32>> {
        self.check_alive()?;
        let first = self.len() + 1;
        if games.is_empty() {
            return Ok(first..first);
        }
        let count = u32::try_from(games.len()).unwrap_or(u32::MAX);
        if self.len().saturating_add(count) > MAX_GAMES {
            return Err(Si4Error::GameLimit { limit: MAX_GAMES });
        }
        check_records(games)?;
        self.check_name_capacity(games)?;

        remove_cache(&self.stem);
        let offsets = self.append_records(games)?;

        self.names.begin();
        let entries = match self.name_entries(games, &offsets, None) {
            Ok(entries) => entries,
            Err(error) => {
                self.names.rollback();
                return Err(error);
            }
        };
        if let Err(error) = replace_file(&self.names_path, &self.names.to_bytes()) {
            self.names.rollback();
            return Err(error);
        }
        // The new names are on disk. The transaction of the name base stays
        // open until the entries and the header are written (decision D313).
        let position = entry_position(first);
        let mut buffer = Vec::with_capacity(entries.len() * ENTRY_LEN);
        for entry in &entries {
            buffer.extend_from_slice(&entry.to_bytes());
        }
        let written = self.stage("write:entries").and_then(|()| {
            self.write_at(position, &buffer)?;
            self.stage("write:header")?;
            self.update_header(|h| {
                h.set_num_games(first - 1 + count);
                Ok(())
            })
        });
        if let Err(error) = written {
            self.fail_after_names(&error);
            return Err(error);
        }
        self.names.commit();
        Ok(first..first + count)
    }

    /// Encodes a game and replaces game `number` with it (`sc_game save n`).
    /// See [`Si4Writer::replace_prepared`].
    pub fn replace_game(&mut self, number: u32, game: &Game) -> Result<()> {
        let prepared = PreparedGame::from_game(game, GameFlags::empty())?;
        self.replace_prepared(number, &prepared)
    }

    /// Replaces game `number` (`1..=len()`) with an encoded game.
    ///
    /// The new record goes to the end of the `.sg4` and the old one stays as
    /// dead space until the file is compacted. The entry is overwritten in
    /// place. The 19 editable flags (including `D`) of the old entry are kept
    /// and the ones of `game` are ignored; the derived flags come from the
    /// new game. The names of the new game get `+1` and then the old ones
    /// `-1`: an old name that is no longer used stays in the `.sn4` with
    /// frequency 0 (rule 14).
    pub fn replace_prepared(&mut self, number: u32, game: &PreparedGame) -> Result<()> {
        self.check_alive()?;
        let old_bytes = self.read_entry(number)?;
        let (old, _) =
            IndexEntry::parse_lenient(&old_bytes).map_err(|e| corrupt_entry(number, e))?;
        check_records(std::slice::from_ref(game))?;
        self.check_name_capacity(std::slice::from_ref(game))?;

        remove_cache(&self.stem);
        let offsets = self.append_records(std::slice::from_ref(game))?;

        self.names.begin();
        let entries = match self.name_entries(std::slice::from_ref(game), &offsets, Some(&old)) {
            Ok(entries) => entries,
            Err(error) => {
                self.names.rollback();
                return Err(error);
            }
        };
        if let Err(error) = replace_file(&self.names_path, &self.names.to_bytes()) {
            self.names.rollback();
            return Err(error);
        }
        let Some(mut entry) = entries.into_iter().next() else {
            self.undo_names();
            return Err(Si4Error::CorruptIndex {
                game: number,
                reason: "internal: no entry built",
            });
        };
        entry.flags = (old.flags & GameFlags::EDITABLE) | (entry.flags & GameFlags::DERIVED);
        let written = self
            .stage("write:entries")
            .and_then(|()| self.write_at(entry_position(number), &entry.to_bytes()));
        match written {
            Ok(()) => {
                self.names.commit();
                Ok(())
            }
            Err(error) => {
                self.fail_after_names(&error);
                Err(error)
            }
        }
    }

    /// Turns a flag of game `number` on or off, or flips it
    /// (`sc_game flag`). `flag` must be one of the 19 editable flags
    /// ([`GameFlags::EDITABLE`]); `START`, `PROMO` and `UPROMO` are derived
    /// from the game and cannot be edited. Only the 47 bytes of the entry are
    /// written, and only if the value changes; every other byte of the entry
    /// (even the spare bit) stays as it is. Returns whether it changed.
    pub fn set_flag(&mut self, number: u32, flag: GameFlags, op: FlagOp) -> Result<bool> {
        if flag.is_empty() || !GameFlags::EDITABLE.contains(flag) {
            return Err(Si4Error::NotEditableFlag);
        }
        self.check_alive()?;
        let mut bytes = self.read_entry(number)?;
        let old = flags_of(&bytes);
        let new = match op {
            FlagOp::Set => old | flag,
            FlagOp::Clear => old & !flag,
            FlagOp::Toggle => old ^ flag,
        };
        if new == old {
            return Ok(false);
        }
        // Flags 0-15 are in bytes 7-8; custom flags are in bits 0-5 of byte 6.
        let low = u16::try_from(new.bits() & 0xFFFF).unwrap_or(0);
        let custom = u8::try_from((new.bits() >> 16) & 0x3F).unwrap_or(0);
        if let Some(dst) = bytes.get_mut(7..9) {
            dst.copy_from_slice(&low.to_be_bytes());
        }
        if let Some(byte) = bytes.get_mut(6) {
            *byte = (*byte & 0xC0) | custom;
        }
        self.write_at(entry_position(number), &bytes)?;
        Ok(true)
    }

    /// Sets the database type (`sc_base type`) and writes the header.
    pub fn set_base_type(&mut self, base_type: u32) -> Result<()> {
        self.update_header(|h| {
            h.set_base_type(base_type);
            Ok(())
        })
    }

    /// Sets the description (`sc_base description`): at most 107 bytes, up to
    /// the first newline.
    pub fn set_description(&mut self, text: &str) -> Result<()> {
        self.update_header(|h| {
            h.set_description(text);
            Ok(())
        })
    }

    /// Sets the game loaded when the database is opened (`sc_base autoload`):
    /// `0` none, `1` the first, up to `MAX_GAMES`. Not checked against the
    /// number of games (decision D283).
    pub fn set_auto_load(&mut self, game: u32) -> Result<()> {
        self.update_header(|h| h.set_auto_load(game))
    }

    /// Sets the description of custom flag `number` (1 to 6): at most 8 bytes.
    pub fn set_custom_flag_description(&mut self, number: u8, text: &str) -> Result<()> {
        self.update_header(|h| h.set_custom_flag_description(number, text))
    }

    // ---- internals ----

    pub(super) fn check_alive(&self) -> Result<()> {
        if self.poisoned {
            Err(Si4Error::WriterUnusable)
        } else {
            Ok(())
        }
    }

    /// Calls the test hook, if any.
    fn stage(&mut self, label: &str) -> Result<()> {
        match &mut self.fail_point {
            Some(hook) => hook.check(label),
            None => Ok(()),
        }
    }

    /// Handles a failure that came after the `.sn4` was replaced. An I/O
    /// error from the write or its `fsync` leaves the entry or header in an
    /// unknown state (maybe torn), so the writer becomes unusable (decision
    /// D322); any other failure happened before a byte was written and the
    /// names are undone.
    fn fail_after_names(&mut self, error: &Si4Error) {
        if matches!(error, Si4Error::Io { .. }) {
            self.names.commit();
            self.poisoned = true;
        } else {
            self.undo_names();
        }
    }

    /// Undoes a name-base transaction after a failure that came after the
    /// `.sn4` was replaced: memory goes back and the old `.sn4` is written
    /// again. If that rewrite fails too, the file and the memory may differ
    /// and the writer is marked unusable (decision D313).
    fn undo_names(&mut self) {
        self.names.rollback();
        if replace_file(&self.names_path, &self.names.to_bytes()).is_err() {
            self.poisoned = true;
        }
    }

    fn read_entry(&mut self, number: u32) -> Result<[u8; ENTRY_LEN]> {
        if number == 0 || number > self.len() {
            return Err(Si4Error::InvalidGameNumber {
                number,
                len: self.len(),
            });
        }
        let position = entry_position(number);
        let path = &self.index_path;
        let mut bytes = [0u8; ENTRY_LEN];
        self.index_file
            .seek(SeekFrom::Start(position))
            .map_err(io_error(path))?;
        self.index_file.read_exact(&mut bytes).map_err(|source| {
            if source.kind() == io::ErrorKind::UnexpectedEof {
                Si4Error::Truncated {
                    file: Si4File::Index,
                    offset: position,
                    need: ENTRY_LEN as u64,
                }
            } else {
                Si4Error::Io {
                    path: path.clone(),
                    source,
                }
            }
        })?;
        Ok(bytes)
    }

    /// Writes bytes of the `.si4` at `position` and syncs.
    fn write_at(&mut self, position: u64, data: &[u8]) -> Result<()> {
        let path = &self.index_path;
        self.index_file
            .seek(SeekFrom::Start(position))
            .map_err(io_error(path))?;
        self.index_file.write_all(data).map_err(io_error(path))?;
        self.index_file.sync_all().map_err(io_error(path))
    }

    /// Applies `edit` to a copy of the header, writes it (182 bytes, synced)
    /// and only then adopts it, so a failure leaves the state as it was.
    fn update_header(&mut self, edit: impl FnOnce(&mut HeaderBytes) -> Result<()>) -> Result<()> {
        self.check_alive()?;
        let mut next = self.header_bytes.clone();
        edit(&mut next)?;
        let parsed = next.parse()?;
        self.write_at(0, next.as_bytes())?;
        self.header_bytes = next;
        self.header = parsed;
        Ok(())
    }

    /// Appends the records to the `.sg4` and syncs; returns their offsets.
    fn append_records(&mut self, games: &[PreparedGame]) -> Result<Vec<u32>> {
        let path = &self.games_path;
        // Dead space (records replaced by `replace_prepared`) stays in the
        // file and is reclaimed by `compact::compact_games`.
        let start = self.games_file.metadata().map_err(io_error(path))?.len();
        let mut end = start;
        // Plan everything first, so a file that would be too big is not touched.
        let mut plan = Vec::with_capacity(games.len());
        for game in games {
            let (offset, padding) = plan_append(end, game.record().len());
            let offset32 = u32::try_from(offset).map_err(|_| Si4Error::GamesFileFull)?;
            plan.push((offset32, padding));
            end = offset + u64::try_from(game.record().len()).unwrap_or(u64::MAX);
        }
        self.games_file
            .seek(SeekFrom::Start(start))
            .map_err(io_error(path))?;
        let mut writer = BufWriter::with_capacity(1 << 16, &self.games_file);
        let zeros = [0u8; 4096];
        for (game, &(_, padding)) in games.iter().zip(&plan) {
            let mut left = padding;
            while left > 0 {
                let n = usize::try_from(left)
                    .unwrap_or(zeros.len())
                    .min(zeros.len());
                writer.write_all(&zeros[..n]).map_err(io_error(path))?;
                left -= u64::try_from(n).unwrap_or(left);
            }
            writer.write_all(game.record()).map_err(io_error(path))?;
        }
        writer.flush().map_err(io_error(path))?;
        drop(writer);
        self.games_file.sync_all().map_err(io_error(path))?;
        Ok(plan.into_iter().map(|(offset, _)| offset).collect())
    }

    /// Adds the names of each game (and, replacing, subtracts the old ones),
    /// and returns the entries with their IDs, offsets and lengths. The
    /// caller has opened a transaction of the name base.
    fn name_entries(
        &mut self,
        games: &[PreparedGame],
        offsets: &[u32],
        replaced: Option<&IndexEntry>,
    ) -> Result<Vec<IndexEntry>> {
        let mut entries = Vec::with_capacity(games.len());
        for (game, &offset) in games.iter().zip(offsets) {
            let mut ids = [0u32; NAME_FIELDS];
            for ((slot, kind), name) in ids.iter_mut().zip(NAME_KINDS).zip(game.names()) {
                *slot = self.names.add_name(kind, name)?;
                self.names.inc_frequency(kind, *slot, 1);
            }
            let mut entry = game.entry().clone();
            entry.offset = offset;
            entry.length = u32::try_from(game.record().len()).unwrap_or(u32::MAX);
            entry.white_id = ids[0];
            entry.black_id = ids[1];
            entry.event_id = ids[2];
            entry.site_id = ids[3];
            entry.round_id = ids[4];
            entries.push(entry);
        }
        if let Some(old) = replaced {
            for (kind, id) in entry_name_ids(old) {
                self.names.inc_frequency(kind, id, -1);
            }
        }
        Ok(entries)
    }

    /// Checks, before anything is written, that every name can be added
    /// (rule 8: the limit is tested before the lookup).
    fn check_name_capacity(&self, games: &[PreparedGame]) -> Result<()> {
        let mut counts = NameKind::ALL.map(|kind| self.names.num_names(kind));
        let mut pending: [HashSet<&[u8]>; 4] = Default::default();
        for game in games {
            for (kind, name) in NAME_KINDS.into_iter().zip(game.names()) {
                let k = kind.index();
                let limit = NAME_MAX_ID[k];
                if counts[k] >= limit {
                    return Err(Si4Error::NameLimit { kind, limit });
                }
                let name = name.get(..MAX_NAME_LEN).unwrap_or(name);
                if self.names.find(kind, name).is_none() && pending[k].insert(name) {
                    counts[k] += 1;
                }
            }
        }
        Ok(())
    }
}

/// The offset of entry `number` (base 1) in the `.si4` file.
pub(super) fn entry_position(number: u32) -> u64 {
    HEADER_LEN as u64 + ENTRY_LEN as u64 * u64::from(number.saturating_sub(1))
}

fn entry_name_ids(entry: &IndexEntry) -> [(NameKind, u32); NAME_FIELDS] {
    [
        (NameKind::Player, entry.white_id),
        (NameKind::Player, entry.black_id),
        (NameKind::Event, entry.event_id),
        (NameKind::Site, entry.site_id),
        (NameKind::Round, entry.round_id),
    ]
}

fn flags_of(entry: &[u8; ENTRY_LEN]) -> GameFlags {
    let low = u32::from(entry.get(7).copied().unwrap_or(0)) << 8
        | u32::from(entry.get(8).copied().unwrap_or(0));
    let custom = u32::from(entry.get(6).copied().unwrap_or(0) & 0x3F);
    GameFlags::from_bits_retain(low | (custom << 16))
}

fn check_records(games: &[PreparedGame]) -> Result<()> {
    for game in games {
        let size = game.record().len();
        if size > MAX_RECORD_LEN {
            return Err(Si4Error::RecordTooLarge {
                size,
                limit: MAX_RECORD_LEN,
            });
        }
    }
    Ok(())
}

/// Removes the `.stc` tree cache, if there is one.
pub(super) fn remove_cache(stem: &Path) {
    let cache = find_file(stem, "stc");
    fs::remove_file(cache).ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::si4::Si4Database;
    use mychess_core::{GameResult, NodeId, Position};
    use std::path::Path;

    fn play(game: &mut Game, parent: NodeId, uci: &str) -> NodeId {
        let mv = game.position_at(parent).unwrap().parse_uci(uci).unwrap();
        game.add_move(parent, mv).unwrap()
    }

    fn sample(white: &str, black: &str) -> Game {
        let mut game = Game::new();
        let root = game.root();
        let e4 = play(&mut game, root, "e2e4");
        let e5 = play(&mut game, e4, "e7e5");
        play(&mut game, e5, "g1f3");
        game.set_comment(e4, "best by test").unwrap();
        let tags = game.tags_mut();
        tags.white = white.into();
        tags.black = black.into();
        tags.event = "Test".into();
        tags.result = GameResult::White;
        game
    }

    fn base(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("games")
    }

    /// Re-encodes every record and recomputes every derived field of the six
    /// databases bundled with Scid (rules 33 to 38). Run with
    /// `cargo test -p mychess-db --release -- --ignored compat_roundtrip`.
    #[test]
    #[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
    fn compat_roundtrip_of_bundled_bases() {
        use crate::si4::RawGame;
        use crate::si4::derive::{decode_count, derive_mainline, encode_count};
        use crate::si4::record_write::{encode_record, start_list_position};
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27/bases");
        if !dir.is_dir() {
            return;
        }
        let mut stored_mismatches = 0u32;
        let mut total = 0u32;
        for name in [
            "endings",
            "matein1",
            "matein2",
            "matein3",
            "matein4andmore",
            "tactics",
        ] {
            let base = dir.join(name);
            let mut db = Si4Database::open(&base).unwrap();
            let index = fs::read(dir.join(format!("{name}.si4"))).unwrap();
            for n in 1..=db.len() {
                total += 1;
                let record = db.record_bytes(n).unwrap().to_vec();
                let raw = RawGame::parse(&record).unwrap();
                let encoded = encode_record(&raw).unwrap();
                assert_eq!(encoded.bytes, record, "{name} game {n}: record bytes");

                let entry = db.entry(n).unwrap();
                let at = HEADER_LEN + ENTRY_LEN * usize::try_from(n - 1).unwrap();
                assert_eq!(
                    entry.to_bytes().as_slice(),
                    &index[at..at + ENTRY_LEN],
                    "{name} game {n}: entry bytes"
                );
                assert_eq!(
                    entry.variation_count,
                    decode_count(encode_count(encoded.variation_count))
                );
                assert_eq!(
                    entry.comment_count,
                    decode_count(encode_count(encoded.comment_count))
                );
                assert_eq!(
                    entry.nag_count,
                    decode_count(encode_count(encoded.nag_count))
                );

                let standard = raw.start_fen.is_none();
                let start = start_list_position(raw.start_fen.as_deref()).unwrap();
                let moves: Vec<_> = raw.mainline().iter().map(|m| m.mv).collect();
                let derived = derive_mainline(&start, standard, &moves).unwrap();
                assert_eq!(
                    entry.flags.contains(GameFlags::START),
                    !standard,
                    "{name} game {n}: START"
                );
                if standard {
                    assert_eq!(
                        entry.final_mat_sig, derived.final_mat_sig,
                        "{name} {n}: material"
                    );
                    assert_eq!(
                        u32::from(entry.num_half_moves),
                        derived.num_half_moves % 1024,
                        "{name} {n}: half moves"
                    );
                    assert_eq!(
                        entry.home_pawn_data, derived.home_pawn_data,
                        "{name} {n}: pawns"
                    );
                    assert_eq!(
                        entry.flags.contains(GameFlags::PROMOTION),
                        derived.promotions,
                        "{name} {n}: promotions"
                    );
                    assert_eq!(
                        entry.flags.contains(GameFlags::UNDER_PROMOTION),
                        derived.under_promotions,
                        "{name} {n}: under-promotions"
                    );
                    if entry.stored_line_code != derived.stored_line_code {
                        stored_mismatches += 1;
                    }
                }
            }
        }
        assert_eq!(total, 29_109, "every game of the six databases is checked");
        assert_eq!(
            stored_mismatches, 0,
            "stored line codes differ from the table"
        );
    }

    /// Decodes each game of the bundled databases to a core `Game`, encodes
    /// it again and compares record and entry with the originals.
    #[test]
    #[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
    #[allow(clippy::print_stderr)]
    fn compat_game_path_of_bundled_bases() {
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27/bases");
        if !dir.is_dir() {
            return;
        }
        for name in [
            "endings",
            "matein1",
            "matein2",
            "matein3",
            "matein4andmore",
            "tactics",
        ] {
            let mut db = Si4Database::open(&dir.join(name)).unwrap();
            let (mut record_diff, mut entry_diff, mut merged) = (0u32, 0u32, 0u32);
            for n in 1..=db.len() {
                let record = db.record_bytes(n).unwrap().to_vec();
                let entry = db.entry(n).unwrap();
                let (game, report) = db.game_with_report(n).unwrap();
                if report.merged_variations + report.discarded_variations > 0 {
                    merged += 1;
                    continue;
                }
                // Games with their own start position carry stale derived
                // fields in these databases (docs/formatos/si4.md, section 3),
                // so only the other games are compared.
                if entry.flags.contains(GameFlags::START) {
                    continue;
                }
                let prepared = PreparedGame::from_game(&game, GameFlags::empty()).unwrap();
                if prepared.record() != record.as_slice() {
                    record_diff += 1;
                    if record_diff < 3 {
                        eprintln!("{name} {n}: record {:?} vs {:?}", prepared.record(), record);
                    }
                }
                let mut mine = prepared.entry().clone();
                // The parts that depend on the database.
                mine.offset = entry.offset;
                mine.white_id = entry.white_id;
                mine.black_id = entry.black_id;
                mine.event_id = entry.event_id;
                mine.site_id = entry.site_id;
                mine.round_id = entry.round_id;
                mine.flags =
                    (mine.flags & GameFlags::DERIVED) | (entry.flags & GameFlags::EDITABLE);
                mine.length_spare_bit = entry.length_spare_bit;
                if mine.to_bytes() != entry.to_bytes() {
                    entry_diff += 1;
                    if entry_diff < 3 {
                        eprintln!("{name} {n}: entry {mine:?}\n  vs {entry:?}");
                    }
                }
            }
            eprintln!(
                "{name}: record diffs {record_diff}, entry diffs {entry_diff}, merged {merged}"
            );
            assert_eq!((record_diff, entry_diff), (0, 0), "{name}");
        }
    }

    /// Writes the name file of each bundled database again, from the names and
    /// the frequencies counted in the entries, and compares the bytes.
    #[test]
    #[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
    #[allow(clippy::print_stderr)]
    fn compat_name_file_of_bundled_bases() {
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27/bases");
        if !dir.is_dir() {
            return;
        }
        for name in [
            "endings",
            "matein1",
            "matein2",
            "matein3",
            "matein4andmore",
            "tactics",
        ] {
            let original = fs::read(dir.join(format!("{name}.sn4"))).unwrap();
            let db = Si4Database::open(&dir.join(name)).unwrap();
            let mut builder = NameBuilder::from_name_base(db.names());
            for entry in db.entries() {
                for (kind, id) in entry_name_ids(&entry.unwrap()) {
                    builder.inc_frequency(kind, id, 1);
                }
            }
            let rewritten = builder.to_bytes();
            let first_diff = original.iter().zip(&rewritten).position(|(a, b)| a != b);
            eprintln!(
                "{name}: {} vs {} bytes, first difference {first_diff:?}",
                original.len(),
                rewritten.len()
            );
            assert_eq!(original.len(), rewritten.len(), "{name}");
            assert!(original == rewritten, "{name}");
        }
    }

    #[test]
    fn plan_append_follows_the_block_rules() {
        assert_eq!(plan_append(0, 10), (0, 0));
        assert_eq!(plan_append(100, 10), (100, 0));
        // Fits exactly at the end of the first block.
        assert_eq!(plan_append(131_062, 10), (131_062, 0));
        // Does not fit: the next block starts at 131,072 and the gap is zeros.
        assert_eq!(plan_append(131_063, 10), (131_072, 9));
        // A full last block: the next record opens a new one.
        assert_eq!(plan_append(131_072, 1), (131_072, 0));
        assert_eq!(plan_append(262_144, 5), (262_144, 0));
        // Offset 0 of the second block is valid.
        assert_eq!(
            plan_append(131_072 + 100, 131_071),
            (262_144, 131_072 - 100)
        );
    }

    #[test]
    fn creates_an_empty_database_with_the_scid_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let writer = Si4Writer::create(&base(&dir), 1).unwrap();
        assert!(writer.is_empty());
        drop(writer);
        let si4 = fs::read(dir.path().join("games.si4")).unwrap();
        let sn4 = fs::read(dir.path().join("games.sn4")).unwrap();
        assert_eq!(si4.len(), 182);
        assert_eq!(sn4.len(), 36);
        assert_eq!(fs::metadata(dir.path().join("games.sg4")).unwrap().len(), 0);
        assert_eq!(&si4[..8], b"Scid.si\0");
        assert_eq!(&si4[8..20], &[1, 0x90, 0, 0, 0, 1, 0, 0, 0, 0, 0, 2]);
        assert!(si4[20..].iter().all(|&b| b == 0));
        assert_eq!(&sn4[..8], b"Scid.sn\0");
        assert!(sn4[8..].iter().all(|&b| b == 0));
        // No temporaries are left.
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), 3, "{names:?}");
        // An empty database opens for reading.
        assert_eq!(Si4Database::open(&base(&dir)).unwrap().len(), 0);
    }

    #[test]
    fn create_refuses_to_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        drop(Si4Writer::create(&base(&dir), 0).unwrap());
        assert!(matches!(
            Si4Writer::create(&base(&dir), 0),
            Err(Si4Error::AlreadyExists { .. })
        ));
        assert!(matches!(
            Si4Writer::create(&dir.path().join("GAMES.SN4"), 0),
            Err(Si4Error::AlreadyExists { .. })
        ));
    }

    #[test]
    fn a_second_writer_is_refused_while_the_first_is_open() {
        let dir = tempfile::tempdir().unwrap();
        let first = Si4Writer::create(&base(&dir), 0).unwrap();
        assert!(matches!(
            Si4Writer::open(&base(&dir)),
            Err(Si4Error::AlreadyOpen { .. })
        ));
        drop(first);
        assert!(Si4Writer::open(&base(&dir)).is_ok());
    }

    #[test]
    fn added_games_are_read_back_and_counts_are_kept_after_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&base(&dir), 1).unwrap();
        assert_eq!(
            writer
                .add_game(&sample("Anand", "Carlsen"), GameFlags::empty())
                .unwrap(),
            1
        );
        assert_eq!(
            writer
                .add_game(&sample("Carlsen", "Anand"), GameFlags::USER)
                .unwrap(),
            2
        );
        assert_eq!(writer.len(), 2);
        assert_eq!(writer.names().frequency(NameKind::Player, 0), Some(2));
        drop(writer);

        let mut db = Si4Database::open(&base(&dir)).unwrap();
        assert_eq!(db.len(), 2);
        let game = db.game(2).unwrap();
        assert_eq!(game.tags().white, "Carlsen");
        assert_eq!(game.mainline().count(), 3);
        assert_eq!(db.entry(2).unwrap().flags, GameFlags::USER);
        let first = db.entry(1).unwrap();
        assert_eq!(first.offset, 0);
        assert_eq!(db.entry(2).unwrap().offset, first.length);
        drop(db);

        let reopened = Si4Writer::open(&base(&dir)).unwrap();
        assert_eq!(reopened.len(), 2);
        assert_eq!(reopened.names().frequency(NameKind::Player, 0), Some(2));
        assert_eq!(reopened.names().frequency(NameKind::Event, 0), Some(2));
        // maxFrequency is the historic marker.
        assert_eq!(reopened.names().max_frequency(NameKind::Player), 2);
    }

    #[test]
    fn replacing_keeps_flags_appends_the_record_and_updates_frequencies() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&base(&dir), 1).unwrap();
        writer
            .add_game(
                &sample("Old", "Opp"),
                GameFlags::DELETE | GameFlags::CUSTOM2,
            )
            .unwrap();
        let before = fs::metadata(writer.games_path()).unwrap().len();
        writer.replace_game(1, &sample("New", "Opp")).unwrap();
        assert!(fs::metadata(writer.games_path()).unwrap().len() > before);
        assert_eq!(writer.len(), 1);
        // "Old" stays in the name file with frequency 0; "Opp" is used once.
        let old = writer.names().find(NameKind::Player, b"Old").unwrap();
        assert_eq!(writer.names().frequency(NameKind::Player, old), Some(0));
        let opp = writer.names().find(NameKind::Player, b"Opp").unwrap();
        assert_eq!(writer.names().frequency(NameKind::Player, opp), Some(1));
        // The frequency of "Opp" went to 2 and back to 1: the marker stays.
        assert_eq!(writer.names().max_frequency(NameKind::Player), 2);
        assert!(matches!(
            writer.replace_game(2, &sample("A", "B")),
            Err(Si4Error::InvalidGameNumber { number: 2, len: 1 })
        ));
        assert!(matches!(
            writer.replace_game(0, &sample("A", "B")),
            Err(Si4Error::InvalidGameNumber { number: 0, .. })
        ));
        drop(writer);
        let mut db = Si4Database::open(&base(&dir)).unwrap();
        let entry = db.entry(1).unwrap();
        assert_eq!(entry.flags, GameFlags::DELETE | GameFlags::CUSTOM2);
        assert!(entry.offset > 0);
        assert_eq!(db.game(1).unwrap().tags().white, "New");
    }

    #[test]
    fn flags_are_edited_in_the_entry_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&base(&dir), 1).unwrap();
        writer
            .add_game(&sample("A", "B"), GameFlags::empty())
            .unwrap();
        let index_before = fs::read(writer.index_path()).unwrap();
        assert!(writer.set_flag(1, GameFlags::TACTICS, FlagOp::Set).unwrap());
        assert!(!writer.set_flag(1, GameFlags::TACTICS, FlagOp::Set).unwrap());
        assert!(
            writer
                .set_flag(1, GameFlags::CUSTOM6, FlagOp::Toggle)
                .unwrap()
        );
        assert!(
            writer
                .set_flag(1, GameFlags::CUSTOM6, FlagOp::Clear)
                .unwrap()
        );
        let after = fs::read(writer.index_path()).unwrap();
        // Only bytes 7-8 of the entry changed (TACTICS = bit 10).
        let diff: Vec<usize> = (0..after.len())
            .filter(|&i| after[i] != index_before[i])
            .collect();
        assert_eq!(diff, vec![182 + 7]);
        assert_eq!(after[182 + 7], 0x04);
        assert!(matches!(
            writer.set_flag(1, GameFlags::START, FlagOp::Set),
            Err(Si4Error::NotEditableFlag)
        ));
        assert!(matches!(
            writer.set_flag(1, GameFlags::empty(), FlagOp::Set),
            Err(Si4Error::NotEditableFlag)
        ));
        assert!(matches!(
            writer.set_flag(2, GameFlags::USER, FlagOp::Set),
            Err(Si4Error::InvalidGameNumber { .. })
        ));
    }

    #[test]
    fn header_edits_rewrite_only_the_header() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&base(&dir), 0).unwrap();
        writer
            .add_game(&sample("A", "B"), GameFlags::empty())
            .unwrap();
        let before = fs::read(writer.index_path()).unwrap();
        writer.set_base_type(4).unwrap();
        writer.set_description("My games\nignored").unwrap();
        writer.set_auto_load(1).unwrap();
        writer.set_custom_flag_description(3, "Sicilian").unwrap();
        assert_eq!(writer.header().base_type(), 4);
        assert_eq!(writer.header().description(), b"My games");
        let after = fs::read(writer.index_path()).unwrap();
        assert_eq!(&after[182..], &before[182..], "entries are untouched");
        let header = Header::parse(&after).unwrap();
        assert_eq!(header.num_games(), 1);
        assert_eq!(header.auto_load_raw(), 2);
        assert_eq!(header.custom_flag_descriptions()[2], b"Sicilian");
        assert!(matches!(
            writer.set_auto_load(MAX_GAMES + 1),
            Err(Si4Error::InvalidAutoLoad { .. })
        ));
    }

    #[test]
    fn a_batch_writes_names_once_and_numbers_the_games() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&base(&dir), 0).unwrap();
        let games: Vec<PreparedGame> = (0..5)
            .map(|i| {
                PreparedGame::from_game(&sample(&format!("P{i}"), "Q"), GameFlags::empty()).unwrap()
            })
            .collect();
        assert_eq!(writer.add_prepared_batch(&games).unwrap(), 1..6);
        assert_eq!(writer.add_prepared_batch(&[]).unwrap(), 6..6);
        assert_eq!(writer.names().frequency(NameKind::Player, 1), Some(5));
        drop(writer);
        let mut db = Si4Database::open(&base(&dir)).unwrap();
        for n in 1..=5 {
            assert_eq!(db.game(n).unwrap().tags().white, format!("P{}", n - 1));
        }
    }

    #[test]
    fn a_record_that_does_not_fit_the_block_opens_the_next_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&base(&dir), 0).unwrap();
        // Games with a long comment: each record is about 60 KB.
        let big = |white: &str| {
            let mut game = sample(white, "B");
            let first = game.mainline().next().unwrap();
            game.set_comment(first, &"x".repeat(60_000)).unwrap();
            game
        };
        for i in 0..3 {
            writer
                .add_game(&big(&format!("W{i}")), GameFlags::empty())
                .unwrap();
        }
        let len = fs::metadata(writer.games_path()).unwrap().len();
        drop(writer);
        let mut db = Si4Database::open(&base(&dir)).unwrap();
        let e1 = db.entry(1).unwrap();
        let e2 = db.entry(2).unwrap();
        let e3 = db.entry(3).unwrap();
        assert_eq!((e1.offset, e2.offset), (0, e1.length));
        // Two records fit in the first block, the third starts the second.
        assert_eq!(u64::from(e3.offset), BLOCK_SIZE);
        assert_eq!(
            len,
            BLOCK_SIZE + u64::from(e3.length),
            "the file ends with the last record"
        );
        let bytes = fs::read(dir.path().join("games.sg4")).unwrap();
        let gap_start = usize::try_from(e2.offset + e2.length).unwrap();
        assert!(
            bytes[gap_start..usize::try_from(BLOCK_SIZE).unwrap()]
                .iter()
                .all(|&b| b == 0)
        );
        for n in 1..=3 {
            assert!(db.game(n).is_ok(), "game {n}");
        }
    }

    #[test]
    fn the_cache_file_is_removed_when_a_game_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&base(&dir), 0).unwrap();
        let cache = dir.path().join("games.stc");
        fs::write(&cache, b"cache").unwrap();
        writer
            .add_game(&Game::from_position(Position::new()), GameFlags::empty())
            .unwrap();
        assert!(!cache.exists());
    }

    #[test]
    fn invalid_games_leave_the_database_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&base(&dir), 0).unwrap();
        writer
            .add_game(&sample("A", "B"), GameFlags::empty())
            .unwrap();
        let snapshot = |w: &Si4Writer| {
            (
                fs::read(w.index_path()).unwrap(),
                fs::read(w.names_path()).unwrap(),
                fs::read(w.games_path()).unwrap(),
            )
        };
        let before = snapshot(&writer);
        let mut bad = sample("C", "D");
        bad.tags_mut().set_extra("Long", &"x".repeat(300)).unwrap();
        assert!(writer.add_game(&bad, GameFlags::empty()).is_err());
        assert_eq!(snapshot(&writer), before);
        assert_eq!(writer.len(), 1);
    }

    #[test]
    fn failure_after_the_name_file_rolls_the_session_back() {
        for label in ["write:entries", "write:header"] {
            let dir = tempfile::tempdir().unwrap();
            let mut writer = Si4Writer::create(&base(&dir), 0).unwrap();
            writer
                .add_game(&sample("A", "B"), GameFlags::empty())
                .unwrap();
            let names_before = fs::read(writer.names_path()).unwrap();
            let freq_before = writer.names().max_frequency(NameKind::Player);
            writer.set_fail_point(FailPoint::at_label(label));
            let error = writer.add_game(&sample("A", "C"), GameFlags::empty());
            assert!(
                matches!(error, Err(Si4Error::InjectedFailure { .. })),
                "{label}"
            );
            // Memory and the .sn4 are back to the state before the call.
            assert_eq!(writer.len(), 1);
            assert_eq!(writer.names().num_names(NameKind::Player), 2);
            assert_eq!(writer.names().frequency(NameKind::Player, 0), Some(1));
            assert_eq!(writer.names().max_frequency(NameKind::Player), freq_before);
            assert_eq!(fs::read(writer.names_path()).unwrap(), names_before);
            // The writer still works.
            assert_eq!(
                writer
                    .add_game(&sample("A", "C"), GameFlags::empty())
                    .unwrap(),
                2
            );
            drop(writer);
            let mut db = Si4Database::open(&base(&dir)).unwrap();
            assert_eq!(db.len(), 2, "{label}");
            assert_eq!(db.game(2).unwrap().tags().black, "C");
        }
    }

    #[test]
    fn failed_replace_restores_frequencies() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&base(&dir), 0).unwrap();
        writer
            .add_game(&sample("A", "B"), GameFlags::empty())
            .unwrap();
        let names_before = fs::read(writer.names_path()).unwrap();
        writer.set_fail_point(FailPoint::at_label("write:entries"));
        assert!(writer.replace_game(1, &sample("X", "Y")).is_err());
        assert_eq!(writer.names().num_names(NameKind::Player), 2);
        assert_eq!(writer.names().frequency(NameKind::Player, 0), Some(1));
        assert_eq!(fs::read(writer.names_path()).unwrap(), names_before);
        writer.replace_game(1, &sample("X", "Y")).unwrap();
    }

    #[test]
    fn a_poisoned_writer_refuses_writes() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&base(&dir), 0).unwrap();
        writer.poisoned = true;
        assert!(matches!(
            writer.add_game(&sample("A", "B"), GameFlags::empty()),
            Err(Si4Error::WriterUnusable)
        ));
        assert!(matches!(
            writer.set_description("x"),
            Err(Si4Error::WriterUnusable)
        ));
    }

    #[test]
    fn create_cleans_only_what_it_made() {
        let dir = tempfile::tempdir().unwrap();
        // An existing .sg4 makes create refuse and stay untouched.
        fs::write(dir.path().join("games.sg4"), b"keep").unwrap();
        assert!(matches!(
            Si4Writer::create(&base(&dir), 0),
            Err(Si4Error::AlreadyExists { .. })
        ));
        assert_eq!(fs::read(dir.path().join("games.sg4")).unwrap(), b"keep");
        assert!(!dir.path().join("games.sn4").exists());
        assert!(!dir.path().join("games.si4").exists());
    }

    #[test]
    fn open_removes_orphan_temporaries_and_the_reader_ignores_them() {
        let dir = tempfile::tempdir().unwrap();
        drop(Si4Writer::create(&base(&dir), 0).unwrap());
        let orphan = dir.path().join("games.sn4.tmp");
        fs::write(&orphan, b"half").unwrap();
        Si4Database::open(&base(&dir)).unwrap();
        assert!(orphan.exists());
        drop(Si4Writer::open(&base(&dir)).unwrap());
        assert!(!orphan.exists());
    }

    #[test]
    fn open_finishes_a_committed_journal() {
        let dir = tempfile::tempdir().unwrap();
        drop(Si4Writer::create(&base(&dir), 0).unwrap());
        fs::write(dir.path().join("games.sg4.tmp"), b"").unwrap();
        fs::write(
            dir.path().join("games.txn"),
            "mychess-txn 1\nop test\nrename\tgames.sg4.tmp\tgames.sg4\nend\n",
        )
        .unwrap();
        assert!(matches!(
            Si4Database::open(&base(&dir)),
            Err(Si4Error::PendingJournal { .. })
        ));
        drop(Si4Writer::open(&base(&dir)).unwrap());
        assert!(!dir.path().join("games.txn").exists());
        assert!(!dir.path().join("games.sg4.tmp").exists());
        Si4Database::open(&base(&dir)).unwrap();
    }

    #[test]
    fn a_corrupt_journal_is_kept_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        drop(Si4Writer::create(&base(&dir), 0).unwrap());
        fs::write(dir.path().join("games.txn"), "garbage").unwrap();
        assert!(matches!(
            Si4Writer::open(&base(&dir)),
            Err(Si4Error::CorruptJournal { .. })
        ));
        assert!(dir.path().join("games.txn").exists());
    }

    /// On Windows the lock of a live writer also blocks reads of the `.si4`
    /// (decision D303); elsewhere it is advisory and readers pass.
    #[test]
    #[cfg(windows)]
    fn reader_fails_while_a_writer_is_alive_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let writer = Si4Writer::create(&base(&dir), 0).unwrap();
        assert!(Si4Database::open(&base(&dir)).is_err());
        drop(writer);
        assert!(Si4Database::open(&base(&dir)).is_ok());
    }

    #[test]
    fn a_planted_journal_cannot_touch_foreign_files_on_open() {
        let dir = tempfile::tempdir().unwrap();
        drop(Si4Writer::create(&base(&dir), 0).unwrap());
        fs::write(dir.path().join("other.pgn"), b"keep").unwrap();
        fs::write(
            dir.path().join("games.txn"),
            "mychess-txn 1\nop x\nremove\tother.pgn\nend\n",
        )
        .unwrap();
        assert!(matches!(
            Si4Writer::open(&base(&dir)),
            Err(Si4Error::CorruptJournal { .. })
        ));
        assert!(dir.path().join("other.pgn").exists());
    }

    #[test]
    fn an_io_failure_after_the_name_file_poisons_the_writer() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&base(&dir), 0).unwrap();
        let io = Si4Error::Io {
            path: PathBuf::from("x"),
            source: io::Error::other("sync failed"),
        };
        writer.names.begin();
        writer.fail_after_names(&io);
        assert!(matches!(
            writer.set_description("x"),
            Err(Si4Error::WriterUnusable)
        ));
    }
}
