// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_name_edit`, 12250-12552),
// tcl/end.tcl (`nameEditor`) and src/index.h (`SetDate`, `SetEventDate`).
// Copyright notices of src/tkscid.cpp: Copyright (c) 1999-2004 Shane Hudson;
// Copyright (c) 2006-2007 Pascal Georges; Copyright (c) 2009-2013 Stevenaaus.
// Scid is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The name editor (`sc_name edit`; decision D3523 and rules 27 to 33 of
//! `docs/specs/m7-1-manutencao.md`): rename or merge a player, event, site or
//! round name, set the rating of a player, or change a date or an event date,
//! in all the games or in a list of games.
//!
//! It shares its machinery with the corrections of the spelling checker
//! ([`Si4Writer::apply_name_changes`]): the new name goes to the `.sn4`
//! first (so the database stays valid), then the entries are patched in
//! place in chunks with one `fsync`, then the frequencies, kept up to date as
//! the entries change, are written with a second `.sn4` replacement.
//!
//! Unlike the Scid, the work is done in two passes over the entries: the
//! first only counts (and can be cancelled: nothing is written), the second
//! writes. So a cancelled run leaves the database as it was.

use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};

use mychess_core::{MAX_ELO, PartialDate};

use super::elo::word;
use super::names::{CHUNK_ENTRIES, read_ids, write_ids};
use super::{Si4Writer, entry_position, io_error, remove_cache, replace_file};
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::bytes_to_string;
use crate::si4::derive::event_date_field;
use crate::si4::entry::IndexEntry;
use crate::si4::error::{Result, Si4Error};
use crate::si4::header::ENTRY_LEN;
use crate::si4::names::NameKind;
use crate::si4::names_write::MAX_NAME_LEN;

/// What the name editor changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameEditTarget {
    /// Replaces a name of `kind` by another one. A `new` name that already
    /// exists merges the two (its ID is reused); otherwise it is added.
    Name {
        /// Player, event, site or round.
        kind: NameKind,
        /// The name to replace (a pattern when [`NameEdit::pattern`]).
        old: String,
        /// The new name: 1 to 255 bytes.
        new: String,
    },
    /// Sets the rating of a player, on the side (White, Black or both) where
    /// the player appears.
    Rating {
        /// The player (a pattern when [`NameEdit::pattern`]).
        player: String,
        /// The Elo, `0..=4000`.
        elo: u16,
        /// The rating type, `0..=15` (0 = Elo).
        rating_type: u8,
    },
    /// Replaces the date of the games whose date is `old`.
    Date {
        /// `YYYY.MM.DD` with `?` for unknown parts, or `*` for any date.
        old: String,
        /// The new date, in the same format.
        new: String,
    },
    /// Replaces the event date of the games whose event date is `old`. The
    /// index keeps it as an offset of -3 to +3 years from the game date, so a
    /// new event date that does not fit is not written (counted in
    /// [`NameEditReport::skipped`]).
    ///
    /// Known limit (D3577): only the index entry is read and written. A game
    /// whose event date is more than 3 years from the game date, as the Scid
    /// writes it, keeps the real date in the tag 255 of its record and `0` in
    /// the entry; such a game never matches `old` (except `*` and unknown),
    /// and a new value written to its entry is hidden on reading by the date
    /// of the record, which wins.
    EventDate {
        /// `YYYY.MM.DD` with `?` for unknown parts, or `*` for any.
        old: String,
        /// The new event date.
        new: String,
    },
}

/// A request to the name editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameEdit {
    /// What to change.
    pub target: NameEditTarget,
    /// Whether the old name is a pattern: `*` is any sequence of characters,
    /// `?` one character and `\` escapes the next one (D3533: no `[...]`).
    /// Case matters. Ignored by the date targets.
    pub pattern: bool,
}

/// Which games the name editor looks at.
#[derive(Debug, Clone, Copy)]
pub enum NameScope<'a> {
    /// Every game; `*` as the old value is refused.
    All,
    /// These games (the filter or a tournament); sorted and deduplicated
    /// here, `0` ignored, any other number out of range is an error.
    Games(&'a [u32]),
}

/// What the name editor did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NameEditReport {
    /// Games in scope.
    pub games_in_scope: u32,
    /// Index entries rewritten.
    pub games_changed: u32,
    /// Changes made: a game with the same player on both sides counts two
    /// (the number of the Scid message "Changed N of ...").
    pub edits: u32,
    /// Names added to the name file (0 or 1).
    pub names_added: u32,
    /// Event dates not written because they do not fit the index.
    pub skipped: u32,
    /// Games whose event date was dropped because the new game date is more
    /// than 3 years from it (or unknown).
    pub event_dates_lost: u32,
    /// The run was cancelled; nothing was written.
    pub interrupted: bool,
}

/// Why the name editor refused a request or failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NameEditError {
    /// The new name is empty.
    #[error("the new value is empty")]
    EmptyNew,
    /// The new name does not fit the name file.
    #[error("the new name has {len} bytes, more than the limit of {max}")]
    NameTooLong {
        /// Bytes in the name.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// The new name has a NUL character.
    #[error("the new name contains a NUL character")]
    NulInName,
    /// The old name is not in the database.
    #[error("the {kind} name {name:?} does not exist")]
    NameNotFound {
        /// The kind of name.
        kind: NameKind,
        /// The name asked for.
        name: String,
    },
    /// A pattern that matches every name (`*`, `**`, `*?*`...) was used with
    /// players.
    #[error("using '*' to match player names is not allowed")]
    GlobPlayer,
    /// `*` was used with all the games.
    #[error("using '*' is potentially harmful and not allowed with all the games")]
    GlobAll,
    /// A date that is not `YYYY.MM.DD` (with `?`).
    #[error("invalid date {text:?}")]
    BadDate {
        /// The text.
        text: String,
    },
    /// A rating out of range.
    #[error("invalid rating {elo} with type {rating_type}")]
    BadRating {
        /// The Elo asked for.
        elo: u16,
        /// The rating type asked for.
        rating_type: u8,
    },
    /// A database error.
    #[error(transparent)]
    Si4(#[from] Si4Error),
}

/// One piece of a compiled pattern.
#[derive(Debug, Clone, Copy)]
enum Tok {
    Star,
    One,
    Lit(char),
}

fn compile_pattern(pattern: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        out.push(match c {
            '*' => Tok::Star,
            '?' => Tok::One,
            '\\' => Tok::Lit(chars.next().unwrap_or('\\')),
            other => Tok::Lit(other),
        });
    }
    out
}

/// Whether a pattern has a `*` and no literal character (`*`, `**`, `*?*`,
/// `?*`...): it matches every name, or every name of a minimum length, so it
/// gets the same treatment as a lone `*` (D3578). The Scid only refuses the
/// exact `*`; this is stricter on purpose.
fn matches_everything(pattern: &str) -> bool {
    let tokens = compile_pattern(pattern);
    tokens.iter().any(|t| matches!(t, Tok::Star))
        && !tokens.iter().any(|t| matches!(t, Tok::Lit(_)))
}

/// Whether `text` matches the compiled pattern (linear in the product of the
/// lengths at worst, no recursion).
fn glob_match(pattern: &[Tok], text: &[char]) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    let mut backtrack: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some(Tok::Star) => {
                backtrack = Some((p, t));
                p += 1;
            }
            Some(Tok::One) => {
                p += 1;
                t += 1;
            }
            Some(Tok::Lit(c)) if text.get(t) == Some(c) => {
                p += 1;
                t += 1;
            }
            _ => match backtrack {
                Some((star, at)) => {
                    backtrack = Some((star, at + 1));
                    p = star + 1;
                    t = at + 1;
                }
                None => return false,
            },
        }
    }
    pattern
        .get(p..)
        .is_some_and(|rest| rest.iter().all(|t| matches!(t, Tok::Star)))
}

/// What the entries are patched with.
enum Plan {
    Names {
        kind: NameKind,
        olds: HashSet<u32>,
        new_text: String,
        /// The ID of the new name when it is already in the name file.
        existing: Option<u32>,
    },
    Rating {
        olds: HashSet<u32>,
        word: [u8; 2],
    },
    Date {
        /// `None` = any date.
        old: Option<u32>,
        new: PartialDate,
    },
    EventDate {
        old: Option<PartialDate>,
        new: PartialDate,
    },
}

/// Counters of one pass.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    edits: u32,
    games: u32,
    skipped: u32,
    lost: u32,
}

fn dates_word(e: &[u8; ENTRY_LEN]) -> u32 {
    u32::from_be_bytes([e[25], e[26], e[27], e[28]])
}

/// Patches one entry for `plan`; says whether it changed. `deltas` gets the
/// `(old ID, new ID)` pairs of name changes.
fn edit_entry(
    plan: &Plan,
    new_id: u32,
    entry: &mut [u8; ENTRY_LEN],
    tally: &mut Tally,
    deltas: &mut Vec<(u32, u32)>,
) -> bool {
    let edits_before = tally.edits;
    match plan {
        Plan::Names { kind, olds, .. } => {
            let mut ids = read_ids(entry);
            let fields: &[usize] = match kind {
                NameKind::Player => &[0, 1],
                NameKind::Event => &[2],
                NameKind::Site => &[3],
                NameKind::Round => &[4],
            };
            for &field in fields {
                let Some(slot) = ids.get_mut(field) else {
                    continue;
                };
                if olds.contains(slot) {
                    deltas.push((*slot, new_id));
                    *slot = new_id;
                    tally.edits += 1;
                }
            }
            if tally.edits != edits_before {
                write_ids(entry, ids);
            }
        }
        Plan::Rating { olds, word: new } => {
            let ids = read_ids(entry);
            for (side, at) in [(0usize, 29usize), (1, 31)] {
                let matches = ids.get(side).is_some_and(|id| olds.contains(id));
                if let Some(dst) = entry.get_mut(at..at + 2)
                    && matches
                    && dst != new.as_slice()
                {
                    dst.copy_from_slice(new);
                    tally.edits += 1;
                }
            }
        }
        Plan::Date { old, new } => {
            let word = dates_word(entry);
            if old.is_none_or(|bits| word & 0x000F_FFFF == bits)
                && let Ok((parsed, _)) = IndexEntry::parse_lenient(entry)
            {
                let field = event_date_field(*new, parsed.event_date);
                let combined = (new.to_scid_u32() & 0x000F_FFFF) | (field << 20);
                if combined != word {
                    entry[25..29].copy_from_slice(&combined.to_be_bytes());
                    tally.edits += 1;
                    if !parsed.event_date.is_unknown() && field == 0 {
                        tally.lost += 1;
                    }
                }
            }
        }
        Plan::EventDate { old, new } => {
            if let Ok((parsed, _)) = IndexEntry::parse_lenient(entry)
                && old.is_none_or(|date| parsed.event_date == date)
            {
                let field = if new.is_unknown() {
                    0
                } else {
                    event_date_field(parsed.date, *new)
                };
                if field == 0 && !new.is_unknown() {
                    tally.skipped += 1;
                } else {
                    let word = dates_word(entry);
                    let combined = (word & 0x000F_FFFF) | (field << 20);
                    if combined != word {
                        entry[25..29].copy_from_slice(&combined.to_be_bytes());
                        tally.edits += 1;
                    }
                }
            }
        }
    }
    let changed = tally.edits != edits_before;
    if changed {
        tally.games += 1;
    }
    changed
}

/// Runs of consecutive game numbers `(first, count)` and the number of games.
fn runs_of(scope: NameScope<'_>, len: u32) -> Result<(Vec<(u32, u32)>, u32)> {
    match scope {
        NameScope::All => Ok((if len == 0 { vec![] } else { vec![(1, len)] }, len)),
        NameScope::Games(games) => {
            let mut numbers: Vec<u32> = games.iter().copied().filter(|&n| n != 0).collect();
            numbers.sort_unstable();
            numbers.dedup();
            if let Some(&number) = numbers.iter().find(|&&n| n > len) {
                return Err(Si4Error::InvalidGameNumber { number, len });
            }
            let mut runs: Vec<(u32, u32)> = Vec::new();
            for &n in &numbers {
                match runs.last_mut() {
                    Some((first, count)) if *first + *count == n => *count += 1,
                    _ => runs.push((n, 1)),
                }
            }
            Ok((runs, u32::try_from(numbers.len()).unwrap_or(u32::MAX)))
        }
    }
}

fn parse_edit_date(text: &str) -> std::result::Result<PartialDate, NameEditError> {
    if PartialDate::is_valid_str(text) {
        Ok(PartialDate::parse_lenient(text))
    } else {
        Err(NameEditError::BadDate {
            text: text.to_owned(),
        })
    }
}

impl Si4Writer {
    /// Runs the name editor (`sc_name edit`).
    ///
    /// Name targets need the old name to exist (unless `pattern`, where no
    /// match is not an error: nothing changes); the new name is reused if it
    /// exists, otherwise it is added. With a pattern both the White and the
    /// Black side of a game can change (the Scid only changes one, rule 29).
    /// A pattern with a `*` and no literal character (`*`, `**`, `*?*`...) is
    /// refused for players and ratings, and for [`NameScope::All`] (D3578). An
    /// old name without a pattern longer than 255 bytes is
    /// [`NameEditError::NameNotFound`]. Frequencies are kept right; names left with
    /// frequency 0 stay in the name file until it is compacted.
    ///
    /// Event dates are the entry's only (D3577): see
    /// [`NameEditTarget::EventDate`] for the tag 255 of the Scid records.
    ///
    /// Cancelling is looked at between chunks of the first pass: the report
    /// then has `interrupted` set and nothing was written. Progress counts
    /// both passes and never goes back.
    ///
    /// # Errors
    ///
    /// [`NameEditError`]: a refused request (before any write), a full name
    /// kind ([`Si4Error::NameLimit`]), an I/O error or an injected failure.
    /// A failure after the new name was written leaves a database that opens
    /// with every name ID valid.
    pub fn edit_names(
        &mut self,
        edit: &NameEdit,
        scope: NameScope<'_>,
        cancel: &CancelToken,
        progress: &mut dyn ProgressSink,
    ) -> std::result::Result<NameEditReport, NameEditError> {
        self.check_alive()?;
        let (runs, in_scope) = runs_of(scope, self.len())?;
        let plan = self.plan_name_edit(edit, matches!(scope, NameScope::All))?;
        let total = u64::from(in_scope) * 2;
        let mut report = NameEditReport {
            games_in_scope: in_scope,
            ..NameEditReport::default()
        };

        // Pass 1: count, nothing written.
        let mut tally = Tally::default();
        let cancelled = self.scan_name_edit(
            &runs,
            &plan,
            u32::MAX,
            false,
            (cancel, progress, 0, total),
            &mut tally,
        )?;
        report.interrupted = cancelled;
        report.skipped = tally.skipped;
        report.event_dates_lost = tally.lost;
        if cancelled {
            report.event_dates_lost = 0;
            return Ok(report);
        }
        report.games_changed = tally.games;
        report.edits = tally.edits;
        if tally.games == 0 {
            // A run killed after the entries but before the final `.sn4` leaves
            // stale frequencies on disk; the writer recounted them when it
            // opened, so repeating the edit puts them right even though no
            // entry changes.
            if matches!(plan, Plan::Names { .. }) && self.names_file_is_stale() {
                replace_file(&self.names_path, &self.names.to_bytes())?;
            }
            progress.progress(total, total);
            return Ok(report);
        }

        // The new name, on disk before anything points to it.
        let mut new_id = u32::MAX;
        if let Plan::Names {
            kind,
            new_text,
            existing,
            ..
        } = &plan
        {
            if let Some(id) = existing {
                new_id = *id;
            } else {
                self.names.begin();
                match self.names.add_name(*kind, new_text.as_bytes()) {
                    Ok(id) => new_id = id,
                    Err(error) => {
                        self.names.rollback();
                        return Err(error.into());
                    }
                }
                let superset = self
                    .stage("names:superset")
                    .and_then(|()| replace_file(&self.names_path, &self.names.to_bytes()));
                if let Err(error) = superset {
                    self.names.rollback();
                    return Err(error.into());
                }
                self.names.commit();
                report.names_added = 1;
            }
        }

        // Pass 2: write.
        remove_cache(&self.stem);
        self.stage("names:entries")?;
        let mut written = Tally::default();
        let rewritten = self.scan_name_edit(
            &runs,
            &plan,
            new_id,
            true,
            (cancel, progress, u64::from(in_scope), total),
            &mut written,
        );
        let synced = if written.games > 0 {
            self.sync_index()
        } else {
            Ok(())
        };
        rewritten?;
        synced?;
        if matches!(plan, Plan::Names { .. }) {
            // Frequencies were kept right while the entries changed.
            self.stage("names:final")?;
            replace_file(&self.names_path, &self.names.to_bytes())?;
        }
        progress.progress(total, total);
        Ok(report)
    }

    /// Whether the `.sn4` on disk differs from what the recounted name base
    /// would write (an unreadable file counts as not stale: the error will
    /// show where it matters).
    fn names_file_is_stale(&self) -> bool {
        std::fs::read(&self.names_path).is_ok_and(|disk| disk != self.names.to_bytes())
    }

    /// Validates the request and resolves the old names to IDs.
    fn plan_name_edit(
        &self,
        edit: &NameEdit,
        all: bool,
    ) -> std::result::Result<Plan, NameEditError> {
        let glob_name = |text: &str| edit.pattern && matches_everything(text);
        match &edit.target {
            NameEditTarget::Name { kind, old, new } => {
                if new.is_empty() {
                    return Err(NameEditError::EmptyNew);
                }
                if new.len() > MAX_NAME_LEN {
                    return Err(NameEditError::NameTooLong {
                        len: new.len(),
                        max: MAX_NAME_LEN,
                    });
                }
                if new.contains('\0') {
                    return Err(NameEditError::NulInName);
                }
                if glob_name(old) {
                    if *kind == NameKind::Player {
                        return Err(NameEditError::GlobPlayer);
                    }
                    if all {
                        return Err(NameEditError::GlobAll);
                    }
                }
                let mut olds = self.resolve_old_names(*kind, old, edit.pattern)?;
                let existing = self.find_name_id(*kind, new);
                if let Some(id) = existing {
                    olds.remove(&id);
                }
                Ok(Plan::Names {
                    kind: *kind,
                    olds,
                    new_text: new.clone(),
                    existing,
                })
            }
            NameEditTarget::Rating {
                player,
                elo,
                rating_type,
            } => {
                if *elo > MAX_ELO || *rating_type > 15 {
                    return Err(NameEditError::BadRating {
                        elo: *elo,
                        rating_type: *rating_type,
                    });
                }
                if glob_name(player) {
                    return Err(NameEditError::GlobPlayer);
                }
                let olds = self.resolve_old_names(NameKind::Player, player, edit.pattern)?;
                Ok(Plan::Rating {
                    olds,
                    word: word(*elo, *rating_type),
                })
            }
            NameEditTarget::Date { old, new } | NameEditTarget::EventDate { old, new } => {
                let any = old == "*";
                if any && all {
                    return Err(NameEditError::GlobAll);
                }
                let new_date = parse_edit_date(new)?;
                let old_date = if any {
                    None
                } else {
                    Some(parse_edit_date(old)?)
                };
                Ok(if matches!(edit.target, NameEditTarget::Date { .. }) {
                    Plan::Date {
                        old: old_date.map(PartialDate::to_scid_u32),
                        new: new_date,
                    }
                } else {
                    Plan::EventDate {
                        old: old_date,
                        new: new_date,
                    }
                })
            }
        }
    }

    /// The IDs of the names `old` stands for.
    fn resolve_old_names(
        &self,
        kind: NameKind,
        old: &str,
        pattern: bool,
    ) -> std::result::Result<HashSet<u32>, NameEditError> {
        let mut ids = HashSet::new();
        if pattern {
            let compiled = compile_pattern(old);
            let mut text: Vec<char> = Vec::new();
            for id in 0..self.names.num_names(kind) {
                let Some(bytes) = self.names.get(kind, id) else {
                    continue;
                };
                text.clear();
                text.extend(bytes_to_string(bytes).chars());
                if glob_match(&compiled, &text) {
                    ids.insert(id);
                }
            }
        } else {
            // `find_name_id` cuts the text to the name limit: a longer old
            // name would match the name that is its prefix.
            let id = Some(old)
                .filter(|text| text.len() <= MAX_NAME_LEN)
                .and_then(|text| self.find_name_id(kind, text))
                .ok_or_else(|| NameEditError::NameNotFound {
                    kind,
                    name: old.to_owned(),
                })?;
            ids.insert(id);
        }
        Ok(ids)
    }

    /// One pass over the runs. Returns whether it was cancelled (only the
    /// counting pass looks at the token). `pass` is the token, the progress
    /// sink, the progress already done and the progress total.
    fn scan_name_edit(
        &mut self,
        runs: &[(u32, u32)],
        plan: &Plan,
        new_id: u32,
        write: bool,
        pass: (&CancelToken, &mut dyn ProgressSink, u64, u64),
        tally: &mut Tally,
    ) -> Result<bool> {
        let (cancel, progress, base, total) = pass;
        let kind = match plan {
            Plan::Names { kind, .. } => Some(*kind),
            _ => None,
        };
        let mut buffer = vec![0u8; CHUNK_ENTRIES * ENTRY_LEN];
        let mut done = base;
        for &(first, count) in runs {
            let mut at = 0u32;
            while at < count {
                if !write && cancel.is_cancelled() {
                    return Ok(true);
                }
                let n = usize::try_from(count - at)
                    .unwrap_or(CHUNK_ENTRIES)
                    .min(CHUNK_ENTRIES);
                let start = first + at;
                let bytes = buffer.get_mut(..n * ENTRY_LEN).unwrap_or(&mut []);
                let path = &self.index_path;
                self.index_file
                    .seek(SeekFrom::Start(entry_position(start)))
                    .map_err(io_error(path))?;
                self.index_file.read_exact(bytes).map_err(io_error(path))?;

                let mut dirty: Vec<usize> = Vec::new();
                let mut deltas: Vec<(u32, u32)> = Vec::new();
                #[allow(clippy::chunks_exact_to_as_chunks)]
                for (i, chunk) in bytes.chunks_exact_mut(ENTRY_LEN).enumerate() {
                    let Ok(entry) = <&mut [u8; ENTRY_LEN]>::try_from(chunk) else {
                        continue;
                    };
                    if edit_entry(plan, new_id, entry, tally, &mut deltas) {
                        dirty.push(i);
                    }
                }
                if write {
                    self.write_dirty(start, bytes, &dirty)?;
                    // Test hook: a process killed between two chunks.
                    self.stage("names:chunk")?;
                    if let Some(kind) = kind {
                        for (old, new) in deltas {
                            self.names.inc_frequency(kind, old, -1);
                            self.names.inc_frequency(kind, new, 1);
                        }
                    }
                }
                let step = u32::try_from(n).unwrap_or(0);
                at += step;
                done += u64::from(step);
                progress.progress(done, total);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        glob_match(&compile_pattern(pattern), &chars)
    }

    #[test]
    fn patterns_use_star_question_and_backslash_only() {
        assert!(matches("Gam*", "Gama, G"));
        assert!(!matches("Gam*", "gama"));
        assert!(matches("*a", "Gama"));
        assert!(matches("G?ma", "Gama"));
        assert!(!matches("G?ma", "Gma"));
        assert!(matches("*", ""));
        assert!(matches("", ""));
        assert!(!matches("", "a"));
        assert!(matches("a*b*c", "aXXbYYc"));
        assert!(!matches("a*b*c", "aXXbYY"));
        // Brackets are plain characters (D3533).
        assert!(matches("[ab]", "[ab]"));
        assert!(!matches("[ab]", "a"));
        // Escapes.
        assert!(matches(r"a\*b", "a*b"));
        assert!(!matches(r"a\*b", "aXb"));
        assert!(matches(r"a\", r"a\"));
        // Unicode counts by character.
        assert!(matches("Á?varo", "Álvaro"));
        assert!(matches("*ção", "Ação"));
    }

    #[test]
    fn runs_join_consecutive_numbers() {
        let (runs, count) = runs_of(NameScope::Games(&[5, 3, 4, 0, 9, 3]), 10).unwrap();
        assert_eq!(runs, vec![(3, 3), (9, 1)]);
        assert_eq!(count, 4);
        assert!(matches!(
            runs_of(NameScope::Games(&[11]), 10),
            Err(Si4Error::InvalidGameNumber {
                number: 11,
                len: 10
            })
        ));
        assert_eq!(runs_of(NameScope::All, 0).unwrap().0, vec![]);
        assert_eq!(runs_of(NameScope::All, 7).unwrap(), (vec![(1, 7)], 7));
    }

    #[test]
    fn dates_are_validated() {
        assert!(parse_edit_date("2020.??.??").is_ok());
        assert!(parse_edit_date("2020.13.01").is_err());
        assert!(parse_edit_date("abc").is_err());
        assert_eq!(
            parse_edit_date("").unwrap(),
            PartialDate::UNKNOWN,
            "empty means unknown"
        );
    }
}
