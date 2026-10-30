// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_game_strip` 9410-9560,
// `sc_base_tag` 2677-2829, `sc_savegame`) and src/gfile.cpp (`AddGame`).
// Copyright notices of src/tkscid.cpp: Copyright (c) 1999-2004 Shane Hudson;
// Copyright (c) 2006-2007 Pascal Georges; Copyright (c) 2009-2013 Stevenaaus.
// Scid is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Bulk rewriting of game records: removing comments and variations
//! ([`Si4Writer::strip_games`]) and adding or removing extra tags
//! ([`Si4Writer::edit_tags`]); decisions D3520, D3521, D3531 and rules 34 to 39
//! of `docs/specs/m7-1-manutencao.md`.
//!
//! Both work on the raw tree of the record ([`RawGame`]), not on a
//! `mychess_core::Game`, so nothing but the asked change is touched: the
//! header names, the flags and every derived field of the index entry stay as
//! they are. A game that needs no change is not rewritten (its record and its
//! entry keep their bytes).
//!
//! The safe order is: (1) every new record is appended to the `.sg4` and
//! synced; (2) only then the entries (offset, length and the three counts)
//! are patched in place and synced. A cancellation or an error before step 2
//! cuts the `.sg4` back to its old length, so the database is exactly as it
//! was ("nothing partial"); a crash leaves the old records referenced, plus
//! dead space that compaction reclaims. The old records are never released
//! here (rule 38). Names are not touched, so the `.sn4` is not rewritten.

use std::io::{Read, Seek, SeekFrom, Write};

use super::{Si4Writer, entry_position, io_error, plan_append, remove_cache};
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, Ticker};
use crate::si4::derive::encode_count;
use crate::si4::entry::IndexEntry;
use crate::si4::error::{Result, Si4Error};
use crate::si4::header::ENTRY_LEN;
use crate::si4::raw::RawGame;
use crate::si4::record_write::{EncodedRecord, MAX_WRITE_TAGS, encode_record};

use super::names::CHUNK_ENTRIES;
use crate::si4::journal::FailPoint;

/// Progress is reported (and the token looked at) every this many games.
const REPORT_EVERY: u32 = 256;
/// Appended bytes held in memory before they go to the file.
const FLUSH_BYTES: usize = 4 << 20;
/// The longest value the tag editor accepts (rule 34).
pub const MAX_TAG_VALUE: usize = 240;
/// The longest tag name (`MAX_TAG_NAME_LEN` of the record writer).
const MAX_TAG_NAME: usize = 240;

/// Tags that cannot be added: the ones of the Scid list (`doAddTag`,
/// `maint.tcl` 2178-2188) plus the ones with a dedicated field or stored apart
/// (`Event`, `Site`, `White`, `Black`, `EventDate`). Compared without case.
const RESERVED_TAGS: [&str; 18] = [
    "ECO",
    "Round",
    "Date",
    "Result",
    "SetUp",
    "BlackElo",
    "WhiteElo",
    "FEN",
    "WhiteRatingType",
    "BlackRatingType",
    "WhiteEstimateElo",
    "BlackEstimateElo",
    "EcoCode",
    "Event",
    "Site",
    "White",
    "Black",
    "EventDate",
];

/// What [`Si4Writer::strip_games`] removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StripWhat {
    /// Every comment (the game's, the moves' and the variations'), with the
    /// embedded commands (`[%eval]`, `[%clk]`...) that live in them.
    Comments,
    /// Every variation, at any depth, with what is inside them.
    Variations,
    /// Both.
    Both,
}

impl StripWhat {
    fn comments(self) -> bool {
        matches!(self, StripWhat::Comments | StripWhat::Both)
    }

    fn variations(self) -> bool {
        matches!(self, StripWhat::Variations | StripWhat::Both)
    }
}

/// One edit of the extra tags, for [`Si4Writer::edit_tags`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagOp {
    /// Sets the tag `name` to `value`: a tag the game already has gets the
    /// new value (in its place); otherwise it is added at the end.
    Add {
        /// The tag name: ASCII letters, digits and `_`, not a reserved one.
        name: String,
        /// The value: at most 240 bytes, without line breaks.
        value: String,
    },
    /// Removes every occurrence of the tag `name` (exact, case-sensitive).
    Strip {
        /// The tag name.
        name: String,
    },
}

/// What a bulk rewrite did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RewriteReport {
    /// Games of the list that were looked at.
    pub examined: u32,
    /// Games rewritten. `0` when the run was interrupted: nothing is written.
    pub changed: u32,
    /// Games that needed no change.
    pub unchanged: u32,
    /// Games whose entry or record could not be read or decoded.
    pub unreadable: u32,
    /// Games left alone because the change does not fit (40 extra tags
    /// already, a record that would pass 131,071 bytes, or one that
    /// `encode_record` refused).
    pub skipped: u32,
    /// Bytes added to the `.sg4` (records and block padding); the space of
    /// the old records is not freed until the file is compacted.
    pub bytes_appended: u64,
    /// The run was cancelled; the database is as it was before the call.
    pub interrupted: bool,
}

/// The result of [`Si4Writer::strip_games`].
pub type StripReport = RewriteReport;
/// The result of [`Si4Writer::edit_tags`].
pub type TagReport = RewriteReport;

/// How a game came out of an edit.
enum Edit {
    Unchanged,
    Changed,
    Skipped,
}

/// The records appended so far, waiting to go to the file.
struct Append {
    /// Length of the `.sg4` before the first record.
    start: u64,
    /// Where the next record is planned (file length once all is flushed).
    end: u64,
    /// Bytes already written after `start`.
    flushed: u64,
    buffer: Vec<u8>,
}

impl Append {
    fn push(&mut self, padding: u64, record: &[u8]) {
        let padding_len = usize::try_from(padding).unwrap_or(0);
        self.buffer.resize(self.buffer.len() + padding_len, 0);
        self.buffer.extend_from_slice(record);
        self.end += padding + u64::try_from(record.len()).unwrap_or(0);
    }
}

/// Checks a tag name for `Add`/`Strip` (rule 34).
fn check_tag_name(name: &str, adding: bool) -> Result<()> {
    let invalid = |reason: &'static str| Si4Error::InvalidTag {
        name: name.chars().take(40).collect(),
        reason,
    };
    if name.is_empty() {
        return Err(invalid("empty tag name"));
    }
    if !adding {
        return Ok(());
    }
    if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(invalid("tag name must be ASCII letters, digits and '_'"));
    }
    if name.len() > MAX_TAG_NAME {
        return Err(invalid("tag name longer than 240 bytes"));
    }
    if RESERVED_TAGS.iter().any(|r| r.eq_ignore_ascii_case(name)) {
        return Err(invalid("reserved tag name"));
    }
    Ok(())
}

fn check_tag_value(name: &str, value: &str) -> Result<()> {
    let invalid = |reason: &'static str| Si4Error::InvalidTag {
        name: name.chars().take(40).collect(),
        reason,
    };
    if value.len() > MAX_TAG_VALUE {
        return Err(invalid("tag value longer than 240 bytes"));
    }
    if value.contains(['\r', '\n', '\0']) {
        return Err(invalid("tag value with a line break or NUL"));
    }
    Ok(())
}

/// Removes what `what` asks from the tree; says whether anything went away.
fn strip_tree(raw: &mut RawGame, what: StripWhat) -> Edit {
    let mut changed = false;
    for line in &mut raw.lines {
        if what.comments() {
            changed |= line.start_comment.take().is_some();
        }
        for node in &mut line.moves {
            if what.comments() {
                changed |= node.comment.take().is_some();
            }
            if what.variations() && !node.variations.is_empty() {
                // The lines stay in the arena, unreachable: the encoder only
                // follows the variations of the nodes.
                node.variations.clear();
                changed = true;
            }
        }
    }
    if changed {
        Edit::Changed
    } else {
        Edit::Unchanged
    }
}

fn edit_tag_list(raw: &mut RawGame, op: &TagOp) -> Edit {
    match op {
        TagOp::Strip { name } => {
            let before = raw.extra_tags.len();
            raw.extra_tags
                .retain(|(n, _)| n.as_slice() != name.as_bytes());
            if raw.extra_tags.len() == before {
                Edit::Unchanged
            } else {
                Edit::Changed
            }
        }
        TagOp::Add { name, value } => {
            let at = raw
                .extra_tags
                .iter()
                .position(|(n, _)| n.as_slice() == name.as_bytes());
            if let Some(slot) = at.and_then(|i| raw.extra_tags.get_mut(i)) {
                if slot.1.as_slice() == value.as_bytes() {
                    Edit::Unchanged
                } else {
                    value.as_bytes().clone_into(&mut slot.1);
                    Edit::Changed
                }
            } else if raw.extra_tags.len() >= MAX_WRITE_TAGS {
                Edit::Skipped
            } else {
                raw.extra_tags
                    .push((name.as_bytes().to_vec(), value.as_bytes().to_vec()));
                Edit::Changed
            }
        }
    }
}

/// What an entry needs to point to its new record: 16 bytes per changed game
/// (the rest of the entry is read again when the entries are patched).
#[derive(Debug, Clone, Copy)]
struct Patch {
    number: u32,
    offset: u32,
    length: u32,
    /// The three 4-bit count codes (variations, comments, NAGs).
    counts: u16,
}

impl Patch {
    fn new(number: u32, offset: u32, bytes: &EncodedRecord) -> Patch {
        let counts = u16::from(encode_count(bytes.variation_count))
            | (u16::from(encode_count(bytes.comment_count)) << 4)
            | (u16::from(encode_count(bytes.nag_count)) << 8);
        Patch {
            number,
            offset,
            length: u32::try_from(bytes.bytes.len()).unwrap_or(0),
            counts,
        }
    }
}

/// Patches offset, length and the three counts of an entry; every other byte
/// stays as it was.
fn patch_entry(entry: &mut [u8; ENTRY_LEN], patch: &Patch) {
    entry[0..4].copy_from_slice(&patch.offset.to_be_bytes());
    entry[4..6].copy_from_slice(
        &u16::try_from(patch.length & 0xFFFF)
            .unwrap_or(0)
            .to_be_bytes(),
    );
    entry[6] = (entry[6] & 0x7F) | (u8::from(patch.length > 0xFFFF) << 7);
    let old = u16::from_be_bytes([entry[21], entry[22]]);
    entry[21..23].copy_from_slice(&((old & 0xF000) | patch.counts).to_be_bytes());
}

impl Si4Writer {
    /// Removes comments and/or variations from the games of `games`
    /// (`sc_game strip`, decisions D3520 and D3531). Only games whose index
    /// entry says they have what is to be removed are read; the others are
    /// counted as unchanged. NAGs, the moves of the main line and the tags do
    /// not change. A game that cannot be read is counted and skipped.
    ///
    /// The list is sorted and deduplicated; `0` is ignored and any other
    /// number out of range fails before a byte is written. The old records
    /// stay in the `.sg4` as dead space (compact the database to free it).
    ///
    /// Cancelling is looked at for every game: the run then returns with
    /// `interrupted` set and the database exactly as it was.
    ///
    /// # Errors
    ///
    /// [`Si4Error::InvalidGameNumber`], [`Si4Error::GamesFileFull`], an I/O
    /// error (the writer becomes unusable only if it hit the index) or an
    /// injected failure of a test. An error before the entries are patched
    /// leaves the database as it was.
    pub fn strip_games(
        &mut self,
        games: &[u32],
        what: StripWhat,
        cancel: &CancelToken,
        progress: &mut dyn ProgressSink,
    ) -> Result<StripReport> {
        self.rewrite_records(
            games,
            cancel,
            progress,
            |entry| {
                (what.comments() && entry.comment_count > 0)
                    || (what.variations() && entry.variation_count > 0)
            },
            |raw| strip_tree(raw, what),
        )
    }

    /// Adds, replaces or removes an extra tag in the games of `games`
    /// (`sc_base tag add|strip`, decision D3521, rules 34 and 35). A game
    /// that already has 40 extra tags cannot receive a new one, and a record
    /// that would pass 131,071 bytes cannot grow: both are counted in
    /// `skipped`. Everything else is as in [`Si4Writer::strip_games`].
    ///
    /// # Errors
    ///
    /// [`Si4Error::InvalidTag`] for a name that is empty, not ASCII letters,
    /// digits or `_`, reserved (`ECO`, `Round`, `Date`, `Result`, `SetUp`,
    /// `FEN`, the Elo tags...), or a value longer than 240 bytes or with a
    /// line break; the rest as [`Si4Writer::strip_games`].
    pub fn edit_tags(
        &mut self,
        games: &[u32],
        op: &TagOp,
        cancel: &CancelToken,
        progress: &mut dyn ProgressSink,
    ) -> Result<TagReport> {
        match op {
            TagOp::Add { name, value } => {
                check_tag_name(name, true)?;
                check_tag_value(name, value)?;
            }
            TagOp::Strip { name } => check_tag_name(name, false)?,
        }
        self.rewrite_records(
            games,
            cancel,
            progress,
            |_| true,
            |raw| edit_tag_list(raw, op),
        )
    }

    /// The engine of the two operations above.
    fn rewrite_records(
        &mut self,
        games: &[u32],
        cancel: &CancelToken,
        progress: &mut dyn ProgressSink,
        wants: impl Fn(&IndexEntry) -> bool,
        edit: impl FnMut(&mut RawGame) -> Edit,
    ) -> Result<RewriteReport> {
        self.check_alive()?;
        let len = self.len();
        let mut numbers: Vec<u32> = games.iter().copied().filter(|&n| n != 0).collect();
        numbers.sort_unstable();
        numbers.dedup();
        if let Some(&number) = numbers.iter().find(|&&n| n > len) {
            return Err(Si4Error::InvalidGameNumber { number, len });
        }
        let total = u64::try_from(numbers.len()).unwrap_or(u64::MAX);
        let start = self
            .games_file
            .metadata()
            .map_err(io_error(&self.games_path))?
            .len();
        let mut job = Append {
            start,
            end: start,
            flushed: 0,
            buffer: Vec::new(),
        };
        let mut pending: Vec<Patch> = Vec::new();
        let mut report = RewriteReport::default();

        let scanned = self.scan_for_rewrite(
            &numbers,
            cancel,
            progress,
            &mut job,
            &mut pending,
            &mut report,
            wants,
            edit,
        );
        if let Err(error) = scanned {
            self.cut_games_file(&job);
            return Err(error);
        }
        if report.interrupted {
            self.cut_games_file(&job);
            report.changed = 0;
            return Ok(report);
        }
        if pending.is_empty() {
            progress.progress(total, total);
            return Ok(report);
        }

        // Step 1 ends: the records are on disk before any entry points to them.
        let appended = self
            .flush_append(&mut job)
            .and_then(|()| {
                self.games_file
                    .sync_all()
                    .map_err(io_error(&self.games_path))
            })
            .and_then(|()| self.stage("rewrite:appended"));
        if let Err(error) = appended {
            self.cut_games_file(&job);
            return Err(error);
        }
        report.bytes_appended = job.end - start;
        remove_cache(&self.stem);
        if let Err(error) = self.stage("rewrite:entries") {
            self.cut_games_file(&job);
            return Err(error);
        }
        // Step 2: the entries, in place. From here on each entry written
        // points to a record that is already on disk. Consecutive game
        // numbers go in one read and one write.
        let mut written = 0usize;
        let mut at = 0usize;
        while at < pending.len() {
            if written > 0 && written * 2 >= pending.len() {
                // Test hook: a process killed with half of the entries patched.
                self.stage("rewrite:mid")?;
            }
            let end = group_end(&pending, at);
            let group = pending.get(at..end).unwrap_or(&[]);
            if let Err(error) = self.patch_group(group) {
                // An entry may be half written.
                self.poisoned = true;
                return Err(error);
            }
            written += end - at;
            at = end;
        }
        self.sync_index()?;
        progress.progress(total, total);
        Ok(report)
    }

    /// Reads the entries of a run of consecutive games, points them to their
    /// new records and writes them back.
    fn patch_group(&mut self, group: &[Patch]) -> Result<()> {
        let Some(first) = group.first() else {
            return Ok(());
        };
        let mut bytes = vec![0u8; group.len() * ENTRY_LEN];
        let path = &self.index_path;
        self.index_file
            .seek(SeekFrom::Start(entry_position(first.number)))
            .and_then(|_| self.index_file.read_exact(&mut bytes))
            .map_err(io_error(path))?;
        #[allow(clippy::chunks_exact_to_as_chunks)]
        for (chunk, patch) in bytes.chunks_exact_mut(ENTRY_LEN).zip(group) {
            if let Ok(entry) = <&mut [u8; ENTRY_LEN]>::try_from(chunk) {
                patch_entry(entry, patch);
            }
        }
        self.index_file
            .seek(SeekFrom::Start(entry_position(first.number)))
            .and_then(|_| self.index_file.write_all(&bytes))
            .map_err(io_error(path))
    }

    /// Reads, edits and queues the games; fills `job`, `pending` and `report`.
    #[allow(clippy::too_many_arguments)]
    fn scan_for_rewrite(
        &mut self,
        numbers: &[u32],
        cancel: &CancelToken,
        progress: &mut dyn ProgressSink,
        job: &mut Append,
        pending: &mut Vec<Patch>,
        report: &mut RewriteReport,
        wants: impl Fn(&IndexEntry) -> bool,
        mut edit: impl FnMut(&mut RawGame) -> Edit,
    ) -> Result<()> {
        let total = u64::try_from(numbers.len()).unwrap_or(u64::MAX);
        let mut ticker = Ticker::new(REPORT_EVERY, CHECK_INTERVAL);
        let mut record: Vec<u8> = Vec::new();
        for (index, &number) in numbers.iter().enumerate() {
            let done = u64::try_from(index).unwrap_or(u64::MAX);
            if cancel.is_cancelled() {
                report.interrupted = true;
                return Ok(());
            }
            if ticker.tick() {
                progress.progress(done, total);
            }
            report.examined += 1;
            let entry_bytes = self.read_entry(number)?;
            let Ok((entry, _)) = IndexEntry::parse_lenient(&entry_bytes) else {
                report.unreadable += 1;
                continue;
            };
            if !wants(&entry) {
                report.unchanged += 1;
                continue;
            }
            if self.record_into(number, &entry, &mut record).is_err() {
                report.unreadable += 1;
                continue;
            }
            let Ok(mut raw) = RawGame::parse(&record) else {
                report.unreadable += 1;
                continue;
            };
            match edit(&mut raw) {
                Edit::Unchanged => report.unchanged += 1,
                Edit::Skipped => report.skipped += 1,
                Edit::Changed => {
                    let Ok(encoded) = encode_record(&raw) else {
                        report.skipped += 1;
                        continue;
                    };
                    let (offset, padding) = plan_append(job.end, encoded.bytes.len());
                    let offset32 = u32::try_from(offset).map_err(|_| Si4Error::GamesFileFull)?;
                    job.push(padding, &encoded.bytes);
                    pending.push(Patch::new(number, offset32, &encoded));
                    report.changed += 1;
                    if job.buffer.len() >= FLUSH_BYTES {
                        self.flush_append(job)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Writes the buffered records after the ones already flushed. A `.sg4`
    /// whose length is not the expected one was changed by someone else (a
    /// Scid ignores the advisory lock): nothing is written over it.
    fn flush_append(&mut self, job: &mut Append) -> Result<()> {
        if job.buffer.is_empty() {
            return Ok(());
        }
        let path = &self.games_path;
        let expected = job.start + job.flushed;
        let actual = self.games_file.metadata().map_err(io_error(path))?.len();
        if actual != expected {
            return Err(Si4Error::Io {
                path: path.clone(),
                source: std::io::Error::other("the games file changed during the rewrite"),
            });
        }
        self.games_file
            .seek(SeekFrom::Start(expected))
            .map_err(io_error(path))?;
        self.games_file
            .write_all(&job.buffer)
            .map_err(io_error(path))?;
        job.flushed += u64::try_from(job.buffer.len()).unwrap_or(0);
        job.buffer.clear();
        Ok(())
    }

    /// Cuts the `.sg4` back to where the rewrite began after an abandoned
    /// rewrite. Nothing refers to the bytes after that point, so a failure
    /// here only leaves dead space. The file is cut only if it still ends
    /// where this rewrite left it: if someone else appended games meanwhile,
    /// the dead space stays (a later compaction reclaims it). A simulated
    /// process death ([`FailPoint::kill_at_label`]) cuts nothing.
    fn cut_games_file(&mut self, job: &Append) {
        if self.fail_point.as_ref().is_some_and(FailPoint::was_killed) || job.flushed == 0 {
            return;
        }
        let ours = job.start + job.flushed;
        if self.games_file.metadata().map(|m| m.len()).ok() == Some(ours)
            && self.games_file.set_len(job.start).is_ok()
        {
            self.games_file.sync_all().ok();
        }
    }
}

/// The end (exclusive) of the run of consecutive game numbers that starts at
/// `at`, at most one chunk long.
fn group_end(pending: &[Patch], at: usize) -> usize {
    let mut end = at + 1;
    while end < pending.len() && end - at < CHUNK_ENTRIES {
        let (Some(prev), Some(next)) = (pending.get(end - 1), pending.get(end)) else {
            break;
        };
        if prev.number.checked_add(1) != Some(next.number) {
            break;
        }
        end += 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_names_follow_the_rules() {
        for ok in ["Source", "Time_Control", "a1", "Annotator", "Sourcey"] {
            assert!(check_tag_name(ok, true).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "a b",
            "é",
            "ECO",
            "eco",
            "SetUp",
            "Setup",
            "EventDate",
            "x-y",
        ] {
            assert!(
                matches!(check_tag_name(bad, true), Err(Si4Error::InvalidTag { .. })),
                "{bad:?}"
            );
        }
        // Stripping accepts any non-empty name.
        assert!(check_tag_name("a b", false).is_ok());
        assert!(check_tag_name("", false).is_err());
    }

    #[test]
    fn tag_values_follow_the_rules() {
        assert!(check_tag_value("T", &"x".repeat(240)).is_ok());
        assert!(check_tag_value("T", &"x".repeat(241)).is_err());
        assert!(check_tag_value("T", "a\nb").is_err());
        assert!(check_tag_value("T", "a\0b").is_err());
        assert!(check_tag_value("T", "").is_ok());
    }

    #[test]
    fn patching_an_entry_keeps_the_other_bytes() {
        let mut entry = [0xA5u8; ENTRY_LEN];
        let record = EncodedRecord {
            bytes: vec![0; 0x1_0002],
            variation_count: 3,
            comment_count: 0,
            nag_count: 11,
        };
        patch_entry(&mut entry, &Patch::new(7, 0x0102_0304, &record));
        assert_eq!(&entry[0..4], &[1, 2, 3, 4]);
        assert_eq!(&entry[4..6], &[0x00, 0x02]);
        assert_eq!(entry[6], 0xA5 | 0x80);
        // Result nibble (high 4 bits of the word) stays.
        assert_eq!(entry[21] >> 4, 0xA);
        assert_eq!(entry[21] & 0xF, 10, "11 NAGs are code 10");
        assert_eq!(entry[22], 0x03, "3 variations, no comments");
        for (i, b) in entry.iter().enumerate() {
            if !matches!(i, 0..=6 | 21 | 22) {
                assert_eq!(*b, 0xA5, "byte {i}");
            }
        }
    }
}

#[cfg(test)]
mod cut_tests {

    use super::*;

    fn writer(dir: &std::path::Path) -> Si4Writer {
        let mut writer = Si4Writer::create(&dir.join("cut"), 1).expect("create");
        writer.games_file.seek(SeekFrom::End(0)).expect("seek");
        writer
    }

    #[test]
    fn the_games_file_is_cut_only_if_it_still_ends_where_the_rewrite_left_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut writer = writer(dir.path());
        let start = writer.games_file.metadata().expect("meta").len();
        writer.games_file.write_all(&[7u8; 100]).expect("write");
        let job = Append {
            start,
            end: start + 100,
            flushed: 100,
            buffer: Vec::new(),
        };
        // Someone else appended after us: the file is left alone.
        writer.games_file.write_all(&[9u8; 50]).expect("write");
        writer.cut_games_file(&job);
        assert_eq!(
            writer.games_file.metadata().expect("meta").len(),
            start + 150
        );
        // Without the intruder the file is cut back.
        writer.games_file.set_len(start + 100).expect("len");
        writer.cut_games_file(&job);
        assert_eq!(writer.games_file.metadata().expect("meta").len(), start);
    }

    #[test]
    fn nothing_flushed_means_nothing_to_cut() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut writer = writer(dir.path());
        let start = writer.games_file.metadata().expect("meta").len();
        writer.games_file.write_all(&[9u8; 20]).expect("write");
        let job = Append {
            start,
            end: start,
            flushed: 0,
            buffer: Vec::new(),
        };
        writer.cut_games_file(&job);
        assert_eq!(
            writer.games_file.metadata().expect("meta").len(),
            start + 20
        );
    }

    #[test]
    fn consecutive_numbers_make_one_group() {
        let patch = |number| Patch {
            number,
            offset: 0,
            length: 0,
            counts: 0,
        };
        let list = [patch(1), patch(2), patch(3), patch(5), patch(6), patch(9)];
        assert_eq!(group_end(&list, 0), 3);
        assert_eq!(group_end(&list, 3), 5);
        assert_eq!(group_end(&list, 5), 6);
    }
}
