// Behaviour derived from Scid vs PC 4.27, src/tkscid.cpp (sc_base_import,
// 1635-1720), src/pgnparse.cpp (LogError, ScidFlags) and src/pgnscid.cpp.
// Copyright (c) 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is
// distributed under the GNU General Public License (see its COPYING); this
// file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Import of PGN into an SI4 database (`sc_base import`, decisions D345 to
//! D350 and D347 of `docs/specs/m3-3-filtro-busca-importacao.md`).
//!
//! [`import_pgn`] reads games from any [`Read`] with the tolerant reader of
//! `mychess-pgn`, prepares them (in parallel, keeping the file order) and
//! stores them with [`Si4Writer::add_prepared_batch`]. Problems with one game
//! never stop the import: they become messages of the [`ImportReport`], whose
//! [`Display`](fmt::Display) is the `(game N, line L) message` text of the
//! Scid, ready for a window or a `.err` file.
//!
//! # Filter
//!
//! Every game added enters the filter of the database with value 1 (D361).
//! The caller does it: after the call, run
//! `filter.append(1)` [`ImportReport::games_added`] times.
//!
//! # Cancelling
//!
//! Cancelling is a normal end (D348): what was read is stored, the report has
//! `interrupted = true` and the database is consistent.

use std::cell::Cell;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use mychess_core::Game;
use mychess_pgn::read::{
    Diagnostic, GameChunker, PgnError, PreGameText, RawGame, ReadOptions, Severity, Termination,
};
use rayon::prelude::*;

use crate::player_stats::PlayerStats;
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::names_write::truncate_name;
use crate::si4::{GameFlags, MAX_WRITE_TAGS, NameKind, PreparedGame, Si4Error, Si4Writer};

/// Database type of a read-only database (a PGN file opened as a database).
pub(crate) const READ_ONLY_BASE_TYPE: u32 = 3;
/// Games read and prepared between two progress reports and cancellation
/// checks (Scid: every 100 games, `tkscid.cpp:1688`).
pub(crate) const CHUNK_GAMES: usize = 100;
/// Bytes of raw games held at once, so that huge games do not pile up.
pub(crate) const CHUNK_BYTES: usize = 32 << 20;
/// Games held in memory before they are written to the database; the `.sn4`
/// is rewritten once per write, so this bounds the work on big files.
const FLUSH_GAMES: usize = 50_000;
/// Bytes of encoded records held in memory before a write.
const FLUSH_BYTES: usize = 64 << 20;
/// Longest tag name the record accepts, in bytes (decision D280).
const MAX_TAG_NAME_LEN: usize = 240;
/// Longest tag value the record accepts, in bytes (decision D280).
const MAX_TAG_VALUE_LEN: usize = 255;

/// What happens to the `PlyCount` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum PlyCountTag {
    /// Keep it as an extra tag (the import of an open database, D349).
    #[default]
    Keep,
    /// Read it and drop it (`pgnscid`, D349).
    Ignore,
}

/// What happens to the `ScidFlags` tag (D350).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ScidFlagsTag {
    /// The letters become the flags of the game; the tag is not stored.
    #[default]
    Apply,
    /// The tag is dropped and the flags stay empty.
    Ignore,
}

/// Options of [`import_pgn`].
///
/// [`ImportOptions::default`] is the import into an open database (text
/// between games ignored, `PlyCount` kept); [`ImportOptions::cli`] is the
/// `pgnscid` profile (decision D349).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImportOptions {
    /// Text between games; overrides `read.pre_game_text`.
    pub pre_game_text: PreGameText,
    /// The `PlyCount` tag.
    pub ply_count: PlyCountTag,
    /// The `ScidFlags` tag.
    pub scid_flags: ScidFlagsTag,
    /// The other options of the reader (limits, move parsing...).
    pub read: ReadOptions,
    /// Size of the input in bytes, for the `total` of the progress reports;
    /// `0` is reported when unknown. [`import_pgn_file`] fills it in.
    pub total_bytes: Option<u64>,
    /// Most diagnostics and notices kept in the report (100 000); further
    /// ones are only counted in [`ImportReport::messages_dropped`].
    pub max_messages: usize,
}

impl Default for ImportOptions {
    fn default() -> ImportOptions {
        ImportOptions {
            pre_game_text: PreGameText::Ignore,
            ply_count: PlyCountTag::Keep,
            scid_flags: ScidFlagsTag::Apply,
            read: ReadOptions::default(),
            total_bytes: None,
            max_messages: 100_000,
        }
    }
}

impl ImportOptions {
    /// The profile of `pgnscid` (`mychess-cli import`): text between games
    /// becomes the initial comment of the next game and `PlyCount` is
    /// ignored.
    pub fn cli() -> ImportOptions {
        ImportOptions {
            pre_game_text: PreGameText::AsComment,
            ply_count: PlyCountTag::Ignore,
            ..ImportOptions::default()
        }
    }

    pub(crate) fn read_options(&self) -> ReadOptions {
        let mut read = self.read.clone();
        read.pre_game_text = self.pre_game_text;
        if self.ply_count == PlyCountTag::Ignore
            && !read.ignored_tags.iter().any(|t| t == "PlyCount")
        {
            read.ignored_tags.push("PlyCount".to_owned());
        }
        read
    }
}

/// Why a game was not stored (decision D347 (e)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkipReason {
    /// The encoded record is longer than the 131 071 bytes of the format.
    RecordTooLarge {
        /// Size of the record.
        size: usize,
        /// The limit.
        limit: usize,
    },
    /// The game is of a variant that is not supported (only its header was
    /// read).
    UnsupportedVariant,
    /// The game is larger than `max_game_bytes` (only its header was read).
    GameTooLarge,
    /// The game cannot be encoded (for example a NUL byte in a name).
    Unencodable(String),
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkipReason::RecordTooLarge { size, limit } => {
                write!(f, "record of {size} bytes exceeds the limit of {limit}")
            }
            SkipReason::UnsupportedVariant => f.write_str("unsupported variant"),
            SkipReason::GameTooLarge => f.write_str("game too large"),
            SkipReason::Unencodable(reason) => write!(f, "cannot be encoded: {reason}"),
        }
    }
}

/// What the import repaired while preparing a game (decision D347). The
/// reader's own remarks are [`Diagnostic`]s; these are the ones of the
/// database layer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImportNoticeKind {
    /// A tag value longer than 255 bytes was shortened.
    TagValueTruncated {
        /// The tag.
        name: String,
    },
    /// Tags beyond the 40 extra tags of a record were dropped.
    TooManyExtraTags {
        /// How many were dropped.
        dropped: usize,
    },
    /// A tag with an empty name was dropped.
    EmptyTagName,
    /// A tag whose name is longer than 240 bytes was dropped.
    TagNameTooLong,
    /// A player, event, site or round name longer than 255 bytes was
    /// shortened.
    NameTruncated,
    /// A character of the `ScidFlags` tag is not a flag letter.
    UnknownFlagChar(char),
    /// The input was not empty but held no game (used when a PGN is opened
    /// as a database; `game` and `line` are 0).
    NoGamesFound,
    /// A tag was dropped because the record refuses it (a reserved name or a
    /// value with a NUL byte).
    TagDropped {
        /// The tag.
        name: String,
    },
}

impl fmt::Display for ImportNoticeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportNoticeKind::TagValueTruncated { name } => {
                write!(f, "value of tag {name} truncated to 255 bytes")
            }
            ImportNoticeKind::TooManyExtraTags { dropped } => {
                write!(f, "more than 40 extra tags, {dropped} dropped")
            }
            ImportNoticeKind::EmptyTagName => f.write_str("tag with an empty name dropped"),
            ImportNoticeKind::TagNameTooLong => {
                f.write_str("tag name longer than 240 bytes, tag dropped")
            }
            ImportNoticeKind::NameTruncated => f.write_str("name truncated to 255 bytes"),
            ImportNoticeKind::UnknownFlagChar(c) => {
                write!(f, "unknown flag letter {c:?} in ScidFlags")
            }
            ImportNoticeKind::NoGamesFound => f.write_str("no games found in the input"),
            ImportNoticeKind::TagDropped { name } => {
                write!(f, "tag {name} dropped, the record cannot hold it")
            }
        }
    }
}

/// A repair made by the import, with the place of the game in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImportNotice {
    /// Number of the game in the file (1-based).
    pub game: u64,
    /// Line of the first header tag of the game.
    pub line: u64,
    /// What happened.
    pub kind: ImportNoticeKind,
}

/// Why the import stopped before the end of the input, keeping the games
/// already stored (decision D347 (f), plus read failures).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImportStop {
    /// The database holds the largest number of games the format allows.
    GameLimit {
        /// The limit.
        limit: u32,
    },
    /// The name base of a kind is full.
    NameLimit {
        /// The kind of name.
        kind: NameKind,
        /// The largest number of names of that kind.
        limit: u32,
    },
    /// The input could not be read any further (I/O or gzip failure).
    Read {
        /// The message of the error.
        message: String,
    },
}

impl fmt::Display for ImportStop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportStop::GameLimit { limit } => {
                write!(f, "limit of {limit} games reached, import stopped")
            }
            ImportStop::NameLimit { kind, limit } => write!(
                f,
                "too many {kind} names, the maximum is {limit}; import stopped"
            ),
            ImportStop::Read { message } => write!(f, "read error, import stopped: {message}"),
        }
    }
}

/// The result of an import (decision D346).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImportReport {
    /// Games found in the input, including the skipped ones.
    pub games_read: u64,
    /// Games stored. The filter of the database must grow by this many
    /// entries of value 1 (decision D361).
    pub games_added: u32,
    /// Numbers (base 1, first included, last excluded) given to the games
    /// stored; empty when nothing was stored.
    pub added: Range<u32>,
    /// Games not stored, by number in the file, with the reason. Only the
    /// first [`ImportOptions::max_messages`] are kept; the others are counted
    /// in `games_skipped_total` and in `messages_dropped`.
    pub games_skipped: Vec<(u32, SkipReason)>,
    /// How many games were not stored, all of them (not only the ones listed
    /// in `games_skipped`).
    pub games_skipped_total: u64,
    /// Remarks of the reader, in file order (informative ones included).
    pub diagnostics: Vec<Diagnostic>,
    /// Repairs made by the database layer, in file order.
    pub notices: Vec<ImportNotice>,
    /// Diagnostics, notices and skipped games left out because of
    /// [`ImportOptions::max_messages`].
    pub messages_dropped: u64,
    /// Bytes of non-blank text outside any game (between games and after the
    /// last one) that the reader skipped. Only [`PgnDatabase::open`]
    /// fills it in.
    ///
    /// [`PgnDatabase::open`]: crate::PgnDatabase::open
    pub text_outside_games: u64,
    /// The import was cancelled; what was read is stored (D348).
    pub interrupted: bool,
    /// Why the import ended before the end of the input, if it did.
    pub stopped: Option<ImportStop>,
    /// Time spent.
    pub elapsed: Duration,
}

impl ImportReport {
    /// Whether there is anything worth showing: a warning or an error of
    /// the reader, a notice, a skipped game or a premature stop.
    /// Informative diagnostics (text between games, encoding fallback) do
    /// not count, as in the Scid, which does not log them.
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity >= Severity::Warning)
            || !self.notices.is_empty()
            || self.games_skipped_total > 0
            || !self.games_skipped.is_empty()
            || self.stopped.is_some()
            || self.messages_dropped > 0
    }
}

/// The text of the Scid: one `(game N, line L) message` line per remark.
///
/// Informative diagnostics are left out (see [`ImportReport::has_warnings`]).
/// Nothing is written when there is nothing to report.
impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The three lists are in game order: merge them, keeping the order
        // inside a game (diagnostics, notices, skipped).
        let mut diagnostics = self
            .diagnostics
            .iter()
            .filter(|d| d.severity >= Severity::Warning)
            .peekable();
        let mut notices = self.notices.iter().peekable();
        let mut skipped = self.games_skipped.iter().peekable();
        loop {
            let d = diagnostics.peek().map(|d| d.game);
            let n = notices.peek().map(|n| n.game);
            let s = skipped.peek().map(|(number, _)| u64::from(*number));
            let Some(next) = [d, n, s].into_iter().flatten().min() else {
                break;
            };
            if d == Some(next) {
                if let Some(d) = diagnostics.next() {
                    write!(f, "(game {}, line {}) {}", d.game, d.line, d.kind)?;
                    if let Some(text) = &d.text {
                        write!(f, ": {text}")?;
                    }
                    writeln!(f)?;
                }
            } else if n == Some(next) {
                if let Some(n) = notices.next() {
                    writeln!(f, "(game {}, line {}) {}", n.game, n.line, n.kind)?;
                }
            } else if let Some((number, reason)) = skipped.next() {
                writeln!(f, "(game {number}) game skipped: {reason}")?;
            }
        }
        if self.messages_dropped > 0 {
            writeln!(f, "{} further messages not shown", self.messages_dropped)?;
        }
        if let Some(stop) = &self.stopped {
            writeln!(f, "{stop}")?;
        }
        Ok(())
    }
}

/// Failure of an import that is not about the content of the PGN.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ImportError {
    /// The database is read-only (type 3: a PGN file opened as a database).
    ///
    /// The check looks only at the type in the header, which in the Scid is
    /// just an icon: a writable SI4 database marked with type 3 is refused
    /// too. The slot manager (M3-5) must decide by the kind of the slot.
    #[error("the database is read-only")]
    ReadOnly,
    /// The PGN file could not be opened.
    #[error("cannot open {path}")]
    Open {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// Writing the database failed. The `games_added` games stored before
    /// the failure stay in the database.
    #[error("cannot write the database ({games_added} games were added before): {source}")]
    Write {
        /// The error of the writer.
        #[source]
        source: Si4Error,
        /// Games stored before the failure.
        games_added: u32,
        /// What the import had done until the failure (added, skipped,
        /// diagnostics, notices; `interrupted` and `elapsed` are filled in).
        report: Box<ImportReport>,
    },
}

/// A reader that counts the bytes taken from the source (compressed bytes
/// for a `.gz`), for the progress, as `BytesUsed()` does in the Scid.
pub(crate) struct CountingReader<R> {
    pub(crate) inner: R,
    pub(crate) count: Rc<Cell<u64>>,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        let read = u64::try_from(n).unwrap_or(0);
        self.count.set(self.count.get().saturating_add(read));
        Ok(n)
    }
}

pub(crate) enum Outcome {
    Ready(Box<PreparedGame>),
    Skipped(SkipReason),
}

/// One game read and prepared, off the writer thread.
pub(crate) struct Processed {
    pub(crate) number: u64,
    pub(crate) outcome: Outcome,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) notices: Vec<ImportNotice>,
}

/// Applies the limits of decision D347 (a) to (c) to the extra tags and
/// removes `ScidFlags`, returning the flags it names.
fn fix_tags(
    game: &mut Game,
    scid_flags: ScidFlagsTag,
    mut notice: impl FnMut(ImportNoticeKind),
) -> GameFlags {
    let mut flags = GameFlags::empty();
    if let Some(letters) = game.tags_mut().remove_extra("ScidFlags")
        && scid_flags == ScidFlagsTag::Apply
    {
        for c in letters.chars().filter(|c| !c.is_whitespace()) {
            match GameFlags::from_letter(c) {
                Some(flag) => flags |= flag,
                None => notice(ImportNoticeKind::UnknownFlagChar(c)),
            }
        }
    }
    let extras = game.tags().extras();
    let needs_fix = extras.len() > MAX_WRITE_TAGS
        || extras.iter().any(|(name, value)| {
            name.is_empty() || name.len() > MAX_TAG_NAME_LEN || value.len() > MAX_TAG_VALUE_LEN
        });
    if !needs_fix {
        return flags;
    }
    let all: Vec<(String, String)> = extras.to_vec();
    for (name, _) in &all {
        game.tags_mut().remove_extra(name);
    }
    let mut kept = 0usize;
    let mut dropped = 0usize;
    for (name, value) in all {
        if name.is_empty() {
            notice(ImportNoticeKind::EmptyTagName);
            continue;
        }
        if name.len() > MAX_TAG_NAME_LEN {
            notice(ImportNoticeKind::TagNameTooLong);
            continue;
        }
        if kept >= MAX_WRITE_TAGS {
            dropped += 1;
            continue;
        }
        let (short, cut) = truncate_name(&value);
        if cut {
            notice(ImportNoticeKind::TagValueTruncated { name: name.clone() });
        }
        // `set_extra` refuses names the record writes itself (the standard
        // tags) and values with a NUL byte; such a tag is dropped, with a
        // notice.
        if game.tags_mut().set_extra(&name, short).is_ok() {
            kept += 1;
        } else {
            notice(ImportNoticeKind::TagDropped { name });
        }
    }
    if dropped > 0 {
        notice(ImportNoticeKind::TooManyExtraTags { dropped });
    }
    flags
}

fn skip_reason(error: &Si4Error) -> SkipReason {
    match error {
        Si4Error::RecordTooLarge { size, limit } => SkipReason::RecordTooLarge {
            size: *size,
            limit: *limit,
        },
        other => SkipReason::Unencodable(other.to_string()),
    }
}

/// Reads one raw game, repairs its tags and encodes it.
pub(crate) fn process(raw: &RawGame, read: &ReadOptions, options: &ImportOptions) -> Processed {
    let parsed = raw.parse(read);
    let number = parsed.number;
    let line = parsed.first_line;
    let diagnostics = parsed.diagnostics;
    let mut notices = Vec::new();
    let mut push = |kind: ImportNoticeKind| {
        notices.push(ImportNotice {
            game: number,
            line,
            kind,
        });
    };
    let outcome = match parsed.termination {
        Termination::UnsupportedVariant => Outcome::Skipped(SkipReason::UnsupportedVariant),
        Termination::Oversize => Outcome::Skipped(SkipReason::GameTooLarge),
        _ => {
            let mut game = parsed.game;
            let flags = fix_tags(&mut game, options.scid_flags, &mut push);
            match PreparedGame::from_game(&game, flags) {
                Ok(prepared) => {
                    if prepared.names_truncated() {
                        push(ImportNoticeKind::NameTruncated);
                    }
                    Outcome::Ready(Box::new(prepared))
                }
                Err(error) => Outcome::Skipped(skip_reason(&error)),
            }
        }
    };
    Processed {
        number,
        outcome,
        diagnostics,
        notices,
    }
}

/// The writer side of the import: the report and the games waiting to be
/// written.
struct Importer<'a> {
    writer: &'a mut Si4Writer,
    stats: Option<&'a mut PlayerStats>,
    max_messages: usize,
    report: ImportReport,
    pending: Vec<PreparedGame>,
    pending_bytes: usize,
}

impl Importer<'_> {
    fn keep_message(&mut self) -> bool {
        let kept = self.report.diagnostics.len() + self.report.notices.len();
        if kept >= self.max_messages {
            self.report.messages_dropped += 1;
            false
        } else {
            true
        }
    }

    fn absorb(&mut self, processed: Processed) {
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
        match processed.outcome {
            Outcome::Ready(game) => {
                self.pending_bytes += game.record().len();
                self.pending.push(*game);
            }
            Outcome::Skipped(reason) => {
                let number = u32::try_from(processed.number).unwrap_or(u32::MAX);
                self.report.games_skipped_total += 1;
                if self.report.games_skipped.len() < self.max_messages {
                    self.report.games_skipped.push((number, reason));
                } else {
                    self.report.messages_dropped += 1;
                }
            }
        }
    }

    fn should_flush(&self) -> bool {
        self.pending.len() >= FLUSH_GAMES || self.pending_bytes >= FLUSH_BYTES
    }

    /// Writes the waiting games. A limit of the format stops the import
    /// (D347 (f)): the games that fit are stored, the rest are dropped.
    fn flush(&mut self) -> Result<(), ImportError> {
        let games = std::mem::take(&mut self.pending);
        self.pending_bytes = 0;
        if games.is_empty() {
            return Ok(());
        }
        if let Some(stop) = self.add_range(&games)? {
            // The first cause wins: a read failure already recorded stays.
            if self.report.stopped.is_none() {
                self.report.stopped = Some(stop);
            }
        }
        Ok(())
    }

    /// Adds `games`; on a limit error splits in halves until the games that
    /// fit are in (a failed batch writes nothing, so every attempt is
    /// clean). Returns the limit that stopped it, if any.
    fn add_range(&mut self, games: &[PreparedGame]) -> Result<Option<ImportStop>, ImportError> {
        let stop = match self.writer.add_prepared_batch(games) {
            Ok(range) => {
                self.record(&range, games);
                return Ok(None);
            }
            Err(Si4Error::NameLimit { kind, limit }) => ImportStop::NameLimit { kind, limit },
            Err(Si4Error::GameLimit { limit }) => ImportStop::GameLimit { limit },
            Err(source) => {
                let games_added = self.report.games_added;
                return Err(ImportError::Write {
                    source,
                    games_added,
                    report: Box::new(std::mem::take(&mut self.report)),
                });
            }
        };
        if games.len() <= 1 {
            return Ok(Some(stop));
        }
        let (left, right) = games.split_at(games.len() / 2);
        if let Some(stop) = self.add_range(left)? {
            return Ok(Some(stop));
        }
        self.add_range(right)
    }

    fn record(&mut self, range: &Range<u32>, games: &[PreparedGame]) {
        if self.report.added.is_empty() {
            self.report.added = range.clone();
        } else {
            self.report.added.end = range.end;
        }
        self.report.games_added = self
            .report
            .games_added
            .saturating_add(range.end - range.start);
        if let Some(stats) = self.stats.as_deref_mut() {
            let names = self.writer.names();
            for game in games {
                let entry = game.entry();
                let [white, black, ..] = game.names();
                if let Some(id) = names.find(NameKind::Player, white) {
                    stats.raise(id, entry.white_elo);
                }
                if let Some(id) = names.find(NameKind::Player, black) {
                    stats.raise(id, entry.black_elo);
                }
            }
        }
    }
}

/// Imports the games of a PGN stream into `writer` (`sc_base import`).
///
/// Games are stored in file order. `stats`, when given, learns the ratings of
/// the games added (the highest Elo of each player, decision D332). The
/// progress is `(bytes taken from reader, options.total_bytes or 0)`, sent
/// after every 100 games; the token is looked at at the same points. Reading
/// and preparing run on the `rayon` pool, 100 games at a time, with the
/// order kept.
///
/// Cancelling is a normal end (`interrupted = true`). A limit of the format
/// (names, games) or a read failure ends the import with `stopped` set and
/// the games read before it stored. A game that cannot be stored is skipped
/// ([`ImportReport::games_skipped`]); the rest goes on.
///
/// The `.sn4`, the header and the `.stc` removal happen once per write of
/// up to 50 000 games or 64 MiB of records, not once per game.
///
/// # Errors
///
/// [`ImportError::ReadOnly`] for a database of type 3 (any database, even a
/// writable SI4 one, whose header carries that icon), and
/// [`ImportError::Write`] when the files cannot be written (the games stored
/// before stay; the error carries the partial report).
pub fn import_pgn<R: Read>(
    reader: R,
    writer: &mut Si4Writer,
    options: &ImportOptions,
    stats: Option<&mut PlayerStats>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<ImportReport, ImportError> {
    if writer.header().base_type() == READ_ONLY_BASE_TYPE {
        return Err(ImportError::ReadOnly);
    }
    let started = Instant::now();
    let read_options = options.read_options();
    let consumed = Rc::new(Cell::new(0u64));
    let mut chunker = GameChunker::new(
        CountingReader {
            inner: reader,
            count: Rc::clone(&consumed),
        },
        read_options.max_game_bytes,
    );
    let total = options.total_bytes.unwrap_or(0);
    let mut importer = Importer {
        writer,
        stats,
        max_messages: options.max_messages,
        report: ImportReport::default(),
        pending: Vec::new(),
        pending_bytes: 0,
    };

    let mut interrupted = cancel.is_cancelled();
    while !interrupted {
        let mut raws: Vec<RawGame> = Vec::with_capacity(CHUNK_GAMES);
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
            .map(|raw| process(raw, &read_options, options))
            .collect();
        for p in processed {
            importer.absorb(p);
        }
        if let Some(error) = read_error {
            importer.report.stopped = Some(ImportStop::Read {
                message: error_chain(&error),
            });
            break;
        }
        if importer.should_flush() {
            importer
                .flush()
                .map_err(|e| stamp(e, started, interrupted))?;
            if importer.report.stopped.is_some() {
                break;
            }
        }
        progress.progress(consumed.get(), total);
        if finished {
            break;
        }
        interrupted = cancel.is_cancelled();
    }
    if importer.report.stopped.is_none() {
        importer
            .flush()
            .map_err(|e| stamp(e, started, interrupted))?;
    } else {
        // The games waiting after a limit cannot be stored either way; a
        // read failure keeps the ones read before it.
        if matches!(importer.report.stopped, Some(ImportStop::Read { .. })) {
            importer
                .flush()
                .map_err(|e| stamp(e, started, interrupted))?;
        }
        importer.pending.clear();
    }
    progress.progress(consumed.get(), total);
    let mut report = importer.report;
    report.interrupted = interrupted;
    report.elapsed = started.elapsed();
    Ok(report)
}

/// Opens the PGN file at `path` (`.pgn.gz` is recognised by its content) and
/// imports it; see [`import_pgn`]. The progress total is the size of the
/// file unless `options.total_bytes` is set.
///
/// # Errors
///
/// [`ImportError::Open`] when the file cannot be opened, plus the errors of
/// [`import_pgn`].
pub fn import_pgn_file(
    path: &Path,
    writer: &mut Si4Writer,
    options: &ImportOptions,
    stats: Option<&mut PlayerStats>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<ImportReport, ImportError> {
    let file = File::open(path).map_err(|source| ImportError::Open {
        path: path.to_owned(),
        source,
    })?;
    let mut options = options.clone();
    if options.total_bytes.is_none() {
        options.total_bytes = file.metadata().ok().map(|m| m.len());
    }
    import_pgn(file, writer, &options, stats, progress, cancel)
}

/// Fills in the end of the partial report of a write failure.
fn stamp(mut error: ImportError, started: Instant, interrupted: bool) -> ImportError {
    if let ImportError::Write { report, .. } = &mut error {
        report.interrupted = interrupted;
        report.elapsed = started.elapsed();
    }
    error
}

/// The message of `error` followed by the messages of its sources, so the
/// underlying I/O error is not lost.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::NoProgress;
    use crate::si4::Si4Database;
    use mychess_core::GameResult;
    use pretty_assertions::assert_eq;
    use std::fmt::Write as _;

    const THREE: &str = "[Event \"One\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n\
        1. e4 e5 2. Nf3 1-0\n\n\
        [Event \"Two\"]\n[White \"C\"]\n[Black \"D\"]\n[Result \"0-1\"]\n\n\
        1. d4 d5 2. c4 0-1\n\n\
        [Event \"Three\"]\n[White \"E\"]\n[Black \"F\"]\n[Result \"1/2-1/2\"]\n\n\
        1. c4 c5 1/2-1/2\n";

    fn new_writer(dir: &tempfile::TempDir) -> Si4Writer {
        Si4Writer::create(&dir.path().join("db"), 0).unwrap()
    }

    fn import(
        pgn: &str,
        options: &ImportOptions,
        dir: &tempfile::TempDir,
    ) -> (ImportReport, Si4Database<File>) {
        let mut writer = new_writer(dir);
        let report = import_pgn(
            pgn.as_bytes(),
            &mut writer,
            options,
            None,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        drop(writer);
        let db = Si4Database::open(&dir.path().join("db")).unwrap();
        (report, db)
    }

    #[test]
    fn imports_three_games_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let (report, mut db) = import(THREE, &ImportOptions::default(), &dir);
        assert_eq!(report.games_read, 3);
        assert_eq!(report.games_added, 3);
        assert_eq!(report.added, 1..4);
        assert!(!report.interrupted && report.stopped.is_none());
        assert!(!report.has_warnings(), "{report}");
        assert_eq!(db.len(), 3);
        assert_eq!(db.game(2).unwrap().tags().event, "Two");
        assert_eq!(db.game(3).unwrap().tags().result, GameResult::Draw);
    }

    #[test]
    fn illegal_move_is_reported_and_the_game_is_kept() {
        let pgn = THREE.replace("1. d4 d5 2. c4 0-1", "1. d4 d5 2. Ke5 0-1");
        let dir = tempfile::tempdir().unwrap();
        let (report, db) = import(&pgn, &ImportOptions::default(), &dir);
        assert_eq!(db.len(), 3);
        let text = report.to_string();
        assert!(text.starts_with("(game 2, line 13) "), "{text}");
    }

    #[test]
    fn cancel_after_the_first_chunk_keeps_what_was_read() {
        let mut pgn = String::new();
        for i in 0..250 {
            write!(pgn, "[Event \"E{i}\"]\n\n1. e4 e5 *\n\n").unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let mut writer = new_writer(&dir);
        let token = CancelToken::new();
        let seen = token.clone();
        let mut sink = move |_: u64, _: u64| seen.cancel();
        let report = import_pgn(
            pgn.as_bytes(),
            &mut writer,
            &ImportOptions::default(),
            None,
            &mut sink,
            &token,
        )
        .unwrap();
        drop(writer);
        assert!(report.interrupted);
        assert_eq!(report.games_read, 100);
        assert_eq!(report.games_added, 100);
        assert_eq!(
            Si4Database::open(&dir.path().join("db")).unwrap().len(),
            100
        );
    }

    #[test]
    fn already_cancelled_token_imports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = new_writer(&dir);
        let token = CancelToken::new();
        token.cancel();
        let report = import_pgn(
            THREE.as_bytes(),
            &mut writer,
            &ImportOptions::default(),
            None,
            &mut NoProgress,
            &token,
        )
        .unwrap();
        assert!(report.interrupted);
        assert_eq!(report.games_added, 0);
    }

    #[test]
    fn scid_flags_become_flags_and_not_a_tag() {
        let pgn = "[Event \"x\"]\n[ScidFlags \"DW\"]\n\n1. e4 *\n";
        let dir = tempfile::tempdir().unwrap();
        let (report, mut db) = import(pgn, &ImportOptions::default(), &dir);
        assert!(!report.has_warnings(), "{report}");
        let entry = db.entry(1).unwrap();
        assert!(
            entry
                .flags
                .contains(GameFlags::DELETE | GameFlags::WHITE_OPENING)
        );
        // Not stored as an extra tag of the record.
        assert_eq!(db.raw_game(1).unwrap().extra_tags.len(), 0);
    }

    #[test]
    fn unknown_flag_letter_is_a_notice() {
        let pgn = "[Event \"x\"]\n[ScidFlags \"Dz\"]\n\n1. e4 *\n";
        let dir = tempfile::tempdir().unwrap();
        let (report, db) = import(pgn, &ImportOptions::default(), &dir);
        assert_eq!(report.notices.len(), 1);
        assert_eq!(
            report.notices[0].kind,
            ImportNoticeKind::UnknownFlagChar('z')
        );
        assert!(db.entry(1).unwrap().flags.contains(GameFlags::DELETE));
    }

    #[test]
    fn long_names_values_and_many_tags_are_repaired() {
        let mut pgn = format!(
            "[Event \"{}\"]\n[White \"{}\"]\n",
            "e".repeat(300),
            "w".repeat(300)
        );
        writeln!(pgn, "[Long \"{}\"]", "v".repeat(300)).unwrap();
        for i in 0..45 {
            writeln!(pgn, "[T{i} \"x\"]").unwrap();
        }
        pgn.push_str("\n1. e4 *\n");
        let dir = tempfile::tempdir().unwrap();
        let (report, mut db) = import(&pgn, &ImportOptions::default(), &dir);
        assert_eq!(db.len(), 1);
        let kinds: Vec<_> = report.notices.iter().map(|n| &n.kind).collect();
        assert!(kinds.contains(&&ImportNoticeKind::NameTruncated));
        assert!(kinds.contains(&&ImportNoticeKind::TagValueTruncated {
            name: "Long".to_owned()
        }));
        // 46 extra tags: Long and T0..T44; the last 6 are dropped.
        assert!(kinds.contains(&&ImportNoticeKind::TooManyExtraTags { dropped: 6 }));
        let game = db.game(1).unwrap();
        assert_eq!(game.tags().extras().len(), 40);
        assert_eq!(game.tags().get_extra("Long").map(str::len), Some(255));
    }

    #[test]
    fn oversize_record_is_skipped_and_the_rest_goes_on() {
        let big = "x".repeat(200_000);
        let pgn = format!("[Event \"a\"]\n\n1. e4 {{{big}}} *\n\n[Event \"b\"]\n\n1. d4 *\n");
        let dir = tempfile::tempdir().unwrap();
        let (report, db) = import(&pgn, &ImportOptions::default(), &dir);
        assert_eq!(report.games_read, 2);
        assert_eq!(report.games_added, 1);
        assert!(matches!(
            report.games_skipped.as_slice(),
            [(1, SkipReason::RecordTooLarge { .. })]
        ));
        assert_eq!(db.len(), 1);
        assert!(report.to_string().contains("(game 1) game skipped"));
    }

    #[test]
    fn pre_game_text_is_ignored_or_kept_as_comment() {
        let pgn = "hello there\n\n[Event \"x\"]\n\n1. e4 *\n";
        let dir = tempfile::tempdir().unwrap();
        let (_, mut db) = import(pgn, &ImportOptions::default(), &dir);
        assert_eq!(db.game(1).unwrap().initial_comment(), None);
        let dir = tempfile::tempdir().unwrap();
        let (_, mut db) = import(pgn, &ImportOptions::cli(), &dir);
        assert_eq!(db.game(1).unwrap().initial_comment(), Some("hello there"));
    }

    #[test]
    fn ply_count_is_kept_or_ignored() {
        let pgn = "[Event \"x\"]\n[PlyCount \"1\"]\n\n1. e4 *\n";
        let dir = tempfile::tempdir().unwrap();
        let (_, mut db) = import(pgn, &ImportOptions::default(), &dir);
        assert_eq!(db.game(1).unwrap().tags().get_extra("PlyCount"), Some("1"));
        let dir = tempfile::tempdir().unwrap();
        let (_, mut db) = import(pgn, &ImportOptions::cli(), &dir);
        assert_eq!(db.game(1).unwrap().tags().get_extra("PlyCount"), None);
    }

    #[test]
    fn read_only_database_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = Si4Writer::create(&dir.path().join("ro"), 3).unwrap();
        let result = import_pgn(
            THREE.as_bytes(),
            &mut writer,
            &ImportOptions::default(),
            None,
            &mut NoProgress,
            &CancelToken::new(),
        );
        assert!(matches!(result, Err(ImportError::ReadOnly)));
    }

    #[test]
    fn player_stats_learn_the_elo_of_added_games() {
        let pgn = "[Event \"x\"]\n[White \"A\"]\n[Black \"B\"]\n[WhiteElo \"2500\"]\n\n1. e4 *\n";
        let dir = tempfile::tempdir().unwrap();
        let mut writer = new_writer(&dir);
        let mut stats = PlayerStats::new();
        import_pgn(
            pgn.as_bytes(),
            &mut writer,
            &ImportOptions::default(),
            Some(&mut stats),
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        let id = writer.names().find(NameKind::Player, b"A").unwrap();
        assert_eq!(stats.max_elo(id), 2500);
    }

    #[test]
    fn file_import_reports_progress_with_the_file_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("in.pgn");
        std::fs::write(&path, THREE).unwrap();
        let mut writer = new_writer(&dir);
        let mut last = (0, 0);
        let mut sink = |done, total| last = (done, total);
        let report = import_pgn_file(
            &path,
            &mut writer,
            &ImportOptions::default(),
            None,
            &mut sink,
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(report.games_added, 3);
        let size = u64::try_from(THREE.len()).unwrap();
        assert_eq!(last, (size, size));
    }

    #[test]
    fn missing_file_is_an_open_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = new_writer(&dir);
        let result = import_pgn_file(
            &dir.path().join("nope.pgn"),
            &mut writer,
            &ImportOptions::default(),
            None,
            &mut NoProgress,
            &CancelToken::new(),
        );
        assert!(matches!(result, Err(ImportError::Open { .. })));
    }

    #[test]
    fn game_limit_stops_the_import_and_stores_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = new_writer(&dir);
        writer.force_num_games(crate::si4::MAX_GAMES);
        let report = import_pgn(
            THREE.as_bytes(),
            &mut writer,
            &ImportOptions::default(),
            None,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(report.games_added, 0);
        assert_eq!(
            report.stopped,
            Some(ImportStop::GameLimit {
                limit: crate::si4::MAX_GAMES
            })
        );
        assert!(report.has_warnings());
    }

    #[test]
    fn write_failure_carries_the_partial_report() {
        use crate::si4::FailPoint;
        let dir = tempfile::tempdir().unwrap();
        let mut writer = new_writer(&dir);
        writer.set_fail_point(FailPoint::at_label("write:entries"));
        let error = import_pgn(
            THREE.as_bytes(),
            &mut writer,
            &ImportOptions::default(),
            None,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap_err();
        let ImportError::Write { report, .. } = &error else {
            panic!("expected a write error, got {error}");
        };
        assert_eq!(report.games_read, 3);
        assert!(error.to_string().contains("cannot write the database"));
    }

    #[test]
    fn skipped_games_are_capped_but_counted() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = new_writer(&dir);
        let mut pgn = String::new();
        let big = "x".repeat(140_000);
        for _ in 0..5 {
            write!(pgn, "[Event \"a\"]\n\n1. e4 {{{big}}} *\n\n").unwrap();
        }
        let options = ImportOptions {
            max_messages: 2,
            ..ImportOptions::default()
        };
        let report = import_pgn(
            pgn.as_bytes(),
            &mut writer,
            &options,
            None,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(report.games_skipped_total, 5);
        assert_eq!(report.games_skipped.len(), 2);
        assert_eq!(report.messages_dropped, 3);
    }
}
