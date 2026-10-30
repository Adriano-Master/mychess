// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_name_correct`,
// `recalcNameFrequencies`) and src/namebase.cpp (`WriteNameFile`).
// Copyright notices of src/tkscid.cpp: Copyright (c) 1999-2004 Shane Hudson;
// Copyright (c) 2006-2007 Pascal Georges; Copyright (c) 2009-2013 Stevenaaus.
// Copyright notice of src/namebase.cpp: Copyright (c) 2001 Shane Hudson.
// Scid is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Name corrections of [`Si4Writer`] (`docs/specs/m6-3-jogadores-torneios-spelling.md`,
//! rules 48 to 59 and decisions D3327 and D3328).
//!
//! The safe order (rule 56): the new names go to the `.sn4` first (a superset
//! of the old file, so the database stays valid); then the entries of the
//! index that change are rewritten in place, in chunks, one `fsync` at the end;
//! then the frequencies are recounted and the `.sn4` is written again through
//! a temporary file in the same directory and `rename`. A failure or a
//! cancellation in the middle leaves a database that opens, whose
//! entries are all complete and whose name IDs all exist.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};

use mychess_core::PartialDate;

use super::{Si4Writer, entry_position, io_error, remove_cache, replace_file};
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::names_write::truncate_name;
use crate::si4::{ENTRY_LEN, NameKind, Result, Si4Error};

/// Entries read, patched and written back at a time.
pub(super) const CHUNK_ENTRIES: usize = 4096;

/// One correction: the name in the database, the name it becomes and, for
/// players, the life span that limits the games it applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameChange {
    /// The name now in the database (exact).
    pub old: String,
    /// The correct name.
    pub new: String,
    /// Birth date of the player; unknown = no lower limit.
    pub born: PartialDate,
    /// Death date of the player; unknown parts count as the latest possible
    /// (D3328); an unknown year = no upper limit.
    pub died: PartialDate,
}

impl NameChange {
    /// A correction without life span.
    pub fn new(old: impl Into<String>, new: impl Into<String>) -> NameChange {
        NameChange {
            old: old.into(),
            new: new.into(),
            born: PartialDate::UNKNOWN,
            died: PartialDate::UNKNOWN,
        }
    }
}

/// What [`Si4Writer::apply_name_changes`] did (rule 54).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NameChangeReport {
    /// Corrections that map a name to a different one.
    pub mapped: u32,
    /// Name IDs replaced in the index (one per side or field).
    pub games_changed: u32,
    /// Corrections whose old name is not in the database.
    pub names_not_found: u32,
    /// Corrections skipped because the kind reached its limit of names.
    pub skipped_full: u32,
    /// Player sides left alone because of the life span.
    pub skipped_by_date: u32,
    /// The run was cancelled in the middle of the index.
    pub interrupted: bool,
}

/// Whether a game on `date_bits` (Scid's 20-bit date, unknown parts 0) may
/// take the corrected player name (rule 53): the date is completely unknown
/// or it is not before the birth and not after the death. Unknown parts of
/// the birth count as 0; unknown month and day of a death count as 12 and
/// 31 (D3328); a death without year is no limit.
pub fn life_span_allows(date_bits: u32, born: PartialDate, died: PartialDate) -> bool {
    date_bits == 0 || span_bits(born, died).allows(date_bits)
}

/// The birth and death as numbers to compare with the dates of the index.
#[derive(Debug, Clone, Copy)]
struct SpanBits {
    born: u32,
    died: u32,
}

impl SpanBits {
    fn allows(self, date: u32) -> bool {
        (self.born == 0 || date >= self.born) && (self.died == 0 || date <= self.died)
    }
}

fn span_bits(born: PartialDate, died: PartialDate) -> SpanBits {
    let died_bits = match died.year() {
        None => 0,
        Some(year) => {
            u32::from(year) << 9
                | u32::from(died.month().unwrap_or(12)) << 5
                | u32::from(died.day().unwrap_or(31))
        }
    };
    SpanBits {
        born: born.to_scid_u32(),
        died: died_bits,
    }
}

/// The five name IDs of an entry: White, Black, event, site, round.
pub(super) fn read_ids(e: &[u8; ENTRY_LEN]) -> [u32; 5] {
    let word = |at: usize| u32::from(u16::from_be_bytes([e[at], e[at + 1]]));
    [
        u32::from(e[9] >> 4) << 16 | word(10),
        u32::from(e[9] & 0xF) << 16 | word(12),
        u32::from(e[14] >> 5) << 16 | word(15),
        u32::from((e[14] >> 2) & 7) << 16 | word(17),
        u32::from(e[14] & 3) << 16 | word(19),
    ]
}

/// Writes the five IDs back, leaving every other bit as it is.
pub(super) fn write_ids(e: &mut [u8; ENTRY_LEN], ids: [u32; 5]) {
    // Each ID was read from, or checked against, the width of its field.
    let low = |id: u32| ((id & 0xFFFF) as u16).to_be_bytes();
    let high = |id: u32| u8::try_from(id >> 16).unwrap_or(0);
    e[9] = high(ids[0]) << 4 | (high(ids[1]) & 0xF);
    e[10..12].copy_from_slice(&low(ids[0]));
    e[12..14].copy_from_slice(&low(ids[1]));
    e[14] = high(ids[2]) << 5 | (high(ids[3]) & 7) << 2 | (high(ids[4]) & 3);
    e[15..17].copy_from_slice(&low(ids[2]));
    e[17..19].copy_from_slice(&low(ids[3]));
    e[19..21].copy_from_slice(&low(ids[4]));
}

/// The target of one old name.
#[derive(Debug, Clone, Copy)]
struct Target {
    new: u32,
    span: SpanBits,
}

impl Si4Writer {
    /// The ID of a name given as text: UTF-8 first, then Windows-1252 for
    /// databases with old names.
    pub(super) fn find_name_id(&self, kind: NameKind, text: &str) -> Option<u32> {
        let (text, _) = truncate_name(text);
        if let Some(id) = self.names.find(kind, text.as_bytes()) {
            return Some(id);
        }
        let (bytes, _, unmappable) = encoding_rs::WINDOWS_1252.encode(text);
        if unmappable {
            return None;
        }
        self.names.find(kind, &bytes)
    }

    /// Applies name corrections of one kind (`sc_name correct`, rules 48 to
    /// 59). Old names that do not exist are counted and skipped; a new name
    /// that exists is reused, otherwise it is added; when the kind is full
    /// the correction is skipped. Nothing is written when no correction maps
    /// to a different name. Only the name IDs of the entries that change are
    /// written, deleted games included; for players White and Black are
    /// tested separately against the life span. Afterwards the frequencies
    /// are recounted and the `.sn4` is written again.
    ///
    /// Cancelling is looked at between chunks of the index; the report then
    /// has `interrupted` set and what was written stays valid.
    ///
    /// # Errors
    ///
    /// An I/O error (the writer becomes unusable if it hit the index) or an
    /// injected failure of a test.
    pub fn apply_name_changes(
        &mut self,
        kind: NameKind,
        changes: &[NameChange],
        cancel: &CancelToken,
        progress: &mut dyn ProgressSink,
    ) -> Result<NameChangeReport> {
        self.check_alive()?;
        let mut report = NameChangeReport::default();
        self.names.begin();
        let mut map: HashMap<u32, Target> = HashMap::new();
        for change in changes {
            if change.new.is_empty() {
                continue;
            }
            let Some(old) = self.find_name_id(kind, &change.old) else {
                report.names_not_found = report.names_not_found.saturating_add(1);
                continue;
            };
            let new = match self.find_name_id(kind, &change.new) {
                Some(id) => id,
                None => match self
                    .names
                    .add_name(kind, truncate_name(&change.new).0.as_bytes())
                {
                    Ok(id) => id,
                    Err(Si4Error::NameLimit { .. }) => {
                        report.skipped_full = report.skipped_full.saturating_add(1);
                        continue;
                    }
                    Err(error) => {
                        self.names.rollback();
                        return Err(error);
                    }
                },
            };
            // The last line for a name wins.
            map.insert(
                old,
                Target {
                    new,
                    span: span_bits(change.born, change.died),
                },
            );
        }
        map.retain(|old, target| *old != target.new);
        report.mapped = u32::try_from(map.len()).unwrap_or(u32::MAX);
        if map.is_empty() {
            self.names.rollback();
            return Ok(report);
        }

        remove_cache(&self.stem);
        // Step 2: the new names, on disk before anything points to them.
        let superset = self
            .stage("names:superset")
            .and_then(|()| replace_file(&self.names_path, &self.names.to_bytes()));
        if let Err(error) = superset {
            self.names.rollback();
            return Err(error);
        }
        self.names.commit();

        // Step 3: the index.
        self.stage("names:entries")?;
        let rewritten = self.rewrite_ids(kind, &map, &mut report, cancel, progress);
        let synced = if report.games_changed > 0 {
            self.sync_index()
        } else {
            Ok(())
        };
        rewritten.and(synced)?;

        // Step 4: the recounted frequencies.
        self.stage("names:final")?;
        replace_file(&self.names_path, &self.names.to_bytes())?;
        Ok(report)
    }

    /// Walks the index in chunks and patches the IDs of `kind`.
    fn rewrite_ids(
        &mut self,
        kind: NameKind,
        map: &HashMap<u32, Target>,
        report: &mut NameChangeReport,
        cancel: &CancelToken,
        progress: &mut dyn ProgressSink,
    ) -> Result<()> {
        let total = self.len();
        let mut buffer = vec![0u8; CHUNK_ENTRIES * ENTRY_LEN];
        let mut next = 1u32;
        while next <= total {
            if cancel.is_cancelled() {
                report.interrupted = true;
                break;
            }
            let count = usize::try_from(total - next + 1)
                .unwrap_or(CHUNK_ENTRIES)
                .min(CHUNK_ENTRIES);
            let bytes = buffer.get_mut(..count * ENTRY_LEN).unwrap_or(&mut []);
            let path = &self.index_path;
            self.index_file
                .seek(SeekFrom::Start(entry_position(next)))
                .map_err(io_error(path))?;
            self.index_file.read_exact(bytes).map_err(io_error(path))?;

            let mut dirty: Vec<usize> = Vec::new();
            let mut deltas: Vec<(u32, u32)> = Vec::new();
            #[allow(clippy::chunks_exact_to_as_chunks)]
            for (i, chunk) in bytes.chunks_exact_mut(ENTRY_LEN).enumerate() {
                let Ok(entry) = <&mut [u8; ENTRY_LEN]>::try_from(chunk) else {
                    continue;
                };
                if patch_entry(kind, entry, map, report, &mut deltas) {
                    dirty.push(i);
                }
            }
            self.write_dirty(next, bytes, &dirty)?;
            for (old, new) in deltas {
                self.names.inc_frequency(kind, old, -1);
                self.names.inc_frequency(kind, new, 1);
            }
            next += u32::try_from(count).unwrap_or(0);
            progress.progress(u64::from(next - 1), u64::from(total));
        }
        Ok(())
    }

    /// Writes the entries listed in `dirty` (indices into `bytes`, which
    /// starts at game `first`), joining neighbours into one write.
    pub(super) fn write_dirty(&mut self, first: u32, bytes: &[u8], dirty: &[usize]) -> Result<()> {
        let mut i = 0;
        while let Some(&start) = dirty.get(i) {
            let mut end = start;
            while dirty.get(i + 1) == Some(&(end + 1)) {
                i += 1;
                end += 1;
            }
            i += 1;
            let Some(data) = bytes.get(start * ENTRY_LEN..(end + 1) * ENTRY_LEN) else {
                continue;
            };
            let first_game = first + u32::try_from(start).unwrap_or(0);
            let path = &self.index_path;
            let written = self
                .index_file
                .seek(SeekFrom::Start(entry_position(first_game)))
                .and_then(|_| self.index_file.write_all(data))
                .map_err(io_error(path));
            if let Err(error) = written {
                // An entry may be half written.
                self.poisoned = true;
                return Err(error);
            }
        }
        Ok(())
    }
}

/// Patches one entry; returns whether it changed. Frequency changes are
/// appended to `deltas` as `(old ID, new ID)`.
fn patch_entry(
    kind: NameKind,
    entry: &mut [u8; ENTRY_LEN],
    map: &HashMap<u32, Target>,
    report: &mut NameChangeReport,
    deltas: &mut Vec<(u32, u32)>,
) -> bool {
    let mut ids = read_ids(entry);
    let date = u32::from_be_bytes([entry[25], entry[26], entry[27], entry[28]]) & 0x000F_FFFF;
    let fields: &[usize] = match kind {
        NameKind::Player => &[0, 1],
        NameKind::Event => &[2],
        NameKind::Site => &[3],
        NameKind::Round => &[4],
    };
    let mut changed = false;
    for &field in fields {
        let Some(&old) = ids.get(field) else {
            continue;
        };
        let Some(target) = map.get(&old) else {
            continue;
        };
        if kind == NameKind::Player && date != 0 && !target.span.allows(date) {
            report.skipped_by_date = report.skipped_by_date.saturating_add(1);
            continue;
        }
        if let Some(slot) = ids.get_mut(field) {
            *slot = target.new;
        }
        deltas.push((old, target.new));
        report.games_changed = report.games_changed.saturating_add(1);
        changed = true;
    }
    if changed {
        write_ids(entry, ids);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pd(text: &str) -> PartialDate {
        PartialDate::parse_lenient(text)
    }

    fn bits(text: &str) -> u32 {
        pd(text).to_scid_u32()
    }

    #[test]
    fn ids_round_trip_through_the_entry_bytes() {
        let mut e = [0xA5u8; ENTRY_LEN];
        let ids = [0xF_FFFF, 0x1_2345, 0x7_FFFF, 0x5_4321, 0x3_FFFF];
        write_ids(&mut e, ids);
        assert_eq!(read_ids(&e), ids);
        // Nothing outside the id bytes moved.
        for (i, b) in e.iter().enumerate() {
            if !matches!(i, 9..=20) {
                assert_eq!(*b, 0xA5, "byte {i}");
            }
        }
    }

    #[test]
    fn life_span_rules() {
        let born = pd("1900.??.??");
        let died = pd("1970.04.??");
        assert!(life_span_allows(0, born, died), "unknown date passes");
        assert!(life_span_allows(bits("1970.04.20"), born, died));
        assert!(!life_span_allows(bits("1975.01.01"), born, died));
        assert!(!life_span_allows(bits("1899.12.31"), born, died));
        assert!(life_span_allows(bits("1900.01.01"), born, died));
        // Death with only the year does not block games of that year.
        assert!(life_span_allows(
            bits("1979.06.10"),
            PartialDate::UNKNOWN,
            pd("1979")
        ));
        assert!(!life_span_allows(
            bits("1980.01.01"),
            PartialDate::UNKNOWN,
            pd("1979")
        ));
        // Unknown death year: no limit.
        assert!(life_span_allows(
            bits("2020.01.01"),
            born,
            PartialDate::UNKNOWN
        ));
    }

    #[test]
    fn a_full_kind_skips_the_correction_without_writing() {
        use crate::progress::NoProgress;
        let dir = tempfile::tempdir().unwrap();
        let mut w = Si4Writer::create(&dir.path().join("full"), 1).unwrap();
        let limit = crate::si4::NAME_MAX_ID[NameKind::Round.index()];
        for i in 0..limit {
            w.names
                .add_name(NameKind::Round, format!("r{i}").as_bytes())
                .unwrap();
        }
        let report = w
            .apply_name_changes(
                NameKind::Round,
                &[
                    NameChange::new("r0", "brand new"),
                    // An existing name is still reusable when the kind is full.
                    NameChange::new("r1", "r2"),
                ],
                &CancelToken::new(),
                &mut NoProgress,
            )
            .unwrap();
        assert_eq!((report.skipped_full, report.mapped), (1, 1));
        assert_eq!(
            w.names.num_names(NameKind::Round),
            limit,
            "nothing was added"
        );
    }
}
