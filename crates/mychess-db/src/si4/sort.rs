// Derived from Scid vs PC 4.27, src/index.cpp (IndexEntry::Compare 679-820,
// Index::WriteSorted, Index::ParseSortCriteria) and src/scidt.cpp (`-S`/`-R`).
// Copyright (c) 1999-2001 Shane Hudson. All rights reserved (notices of the
// Scid sources); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Permanent sort of the index (`scidt -S` / `-R`,
//! `docs/specs/m3-5-mychess-cli.md`).
//!
//! Only the `.si4` is rewritten: the entries are permuted and the `.sg4`
//! and the `.sn4` are untouched, as in Scid (games stay "scrambled" in the
//! game file until the database is compacted). The new index replaces the
//! old one through the journaled transaction of [`super::journal`], which is
//! more than Scid does (it sorts in place, without atomicity).
//!
//! Differences from Scid, all on purpose:
//! - the sort is stable (equal games keep their order), where the Scid
//!   heap sort is not;
//! - the `Country` comparison is made a total order (Scid's returns "greater"
//!   for two games without a country, which is not consistent and may make
//!   [`slice::sort_by`] panic);
//! - `Random` is not offered. Consequence: in the Scid `ra` is ambiguous
//!   (`Random` and `Rating`) and `random` sorts shuffled; here `ra` is
//!   `rating` and `random` is an unknown field;
//! - at most [`MAX_SORT_KEYS`] (15) criteria: `INDEX_MaxSortCriteria` is 16
//!   in the Scid, but one slot holds the sentinel and `ParseSortCriteria`
//!   refuses the 16th name.
//!
//! Compatibility kept on purpose (spec decisions D630, D632):
//! - the result order follows the *code* (`common.h`: none = 0, white = 1,
//!   black = 2, draw = 3; `SORT_result` turns "none" into 4), not the comment
//!   of the Scid: `1-0`, `0-1`, `1/2-1/2`, `*`;
//! - names are compared with `strCompare` (`misc.cpp:289`), which subtracts
//!   *signed* `char` values: bytes at or above 0x80 (the lead byte of any
//!   non-ASCII UTF-8 letter, such as the `E` acute of "Eric") sort **before**
//!   every ASCII byte. This differs from the order of the `.sn4` (which
//!   `name_order` follows and where the
//!   first byte is compared unsigned); only the sort uses this one. It is
//!   still a total order, so [`slice::sort_by`] cannot panic.

use std::cmp::Ordering;
use std::fmt;
use std::io::Write;

use super::entry::{GameFlags, IndexEntry};
use super::error::{Result, Si4Error};
use super::header::ENTRY_LEN;
use super::journal::{FailPoint, run_transaction};
use super::names::NameKind;
use super::names_write::NameBuilder;
use super::write::{Si4Writer, io_error, remove_cache};
use crate::progress::{CancelToken, NoProgress, ProgressSink};

/// Most criteria in one sort: `INDEX_MaxSortCriteria` (16) minus the slot of
/// the sentinel, as `Index::ParseSortCriteria` enforces.
pub const MAX_SORT_KEYS: usize = 15;

/// A sort criterion (the names of `sortCriteriaNames` of the Scid, without
/// `Random`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SortKey {
    /// The full date (unknown parts first).
    Date,
    /// The year of the date.
    Year,
    /// The event name.
    Event,
    /// The site name.
    Site,
    /// The round name; numbers compare as numbers (`strCompareRound`).
    Round,
    /// The white player name.
    White,
    /// The black player name.
    Black,
    /// The ECO code.
    Eco,
    /// The result: `1-0`, `0-1`, `1/2-1/2`, then `*`.
    Result,
    /// The number of half moves, fewest first.
    Length,
    /// The average rating, highest first.
    Rating,
    /// White's rating, highest first.
    WhiteElo,
    /// Black's rating, highest first.
    BlackElo,
    /// The last three letters of the site (a country code), games without one last.
    Country,
    /// The month of the date.
    Month,
    /// Games without the `D` flag first.
    Deleted,
    /// The event date (the date when it is unknown).
    EventDate,
    /// Most variations first.
    Variations,
    /// Most comments first.
    Comments,
}

impl SortKey {
    /// Every criterion, in the order of the Scid table.
    pub const ALL: [SortKey; 19] = [
        SortKey::Date,
        SortKey::Year,
        SortKey::Event,
        SortKey::Site,
        SortKey::Round,
        SortKey::White,
        SortKey::Black,
        SortKey::Eco,
        SortKey::Result,
        SortKey::Length,
        SortKey::Rating,
        SortKey::WhiteElo,
        SortKey::BlackElo,
        SortKey::Country,
        SortKey::Month,
        SortKey::Deleted,
        SortKey::EventDate,
        SortKey::Variations,
        SortKey::Comments,
    ];

    /// The name accepted by [`SortKey::parse_list`] (lower case).
    pub fn name(self) -> &'static str {
        match self {
            SortKey::Date => "date",
            SortKey::Year => "year",
            SortKey::Event => "event",
            SortKey::Site => "site",
            SortKey::Round => "round",
            SortKey::White => "white",
            SortKey::Black => "black",
            SortKey::Eco => "eco",
            SortKey::Result => "result",
            SortKey::Length => "length",
            SortKey::Rating => "rating",
            SortKey::WhiteElo => "welo",
            SortKey::BlackElo => "belo",
            SortKey::Country => "country",
            SortKey::Month => "month",
            SortKey::Deleted => "deleted",
            SortKey::EventDate => "eventdate",
            SortKey::Variations => "variations",
            SortKey::Comments => "comments",
        }
    }

    /// Reads a list of criteria separated by `,`, `.` or spaces
    /// (`Index::ParseSortCriteria`). Names are case-insensitive; an exact
    /// name wins, otherwise a unique prefix is accepted (`Sortkey::parse_list("w")`
    /// is ambiguous: `white` or `welo`).
    ///
    /// # Errors
    ///
    /// [`SortKeyError`] for an empty list, an unknown or ambiguous name, an
    /// invalid character, or more than [`MAX_SORT_KEYS`] criteria.
    pub fn parse_list(text: &str) -> std::result::Result<Vec<SortKey>, SortKeyError> {
        let mut keys = Vec::new();
        let mut word = String::new();
        let flush = |word: &mut String, keys: &mut Vec<SortKey>| {
            if word.is_empty() {
                return Ok(());
            }
            if keys.len() >= MAX_SORT_KEYS {
                return Err(SortKeyError::TooMany);
            }
            keys.push(SortKey::lookup(word)?);
            word.clear();
            Ok(())
        };
        for c in text.chars() {
            if c.is_ascii_alphabetic() {
                word.push(c.to_ascii_lowercase());
            } else if matches!(c, ',' | '.' | ' ') {
                flush(&mut word, &mut keys)?;
            } else {
                return Err(SortKeyError::InvalidChar(c));
            }
        }
        flush(&mut word, &mut keys)?;
        if keys.is_empty() {
            return Err(SortKeyError::Empty);
        }
        Ok(keys)
    }

    fn lookup(word: &str) -> std::result::Result<SortKey, SortKeyError> {
        if let Some(key) = SortKey::ALL.iter().find(|k| k.name() == word) {
            return Ok(*key);
        }
        let mut matches = SortKey::ALL.iter().filter(|k| k.name().starts_with(word));
        match (matches.next(), matches.next()) {
            (Some(key), None) => Ok(*key),
            (Some(_), Some(_)) => Err(SortKeyError::Ambiguous(word.to_owned())),
            _ => Err(SortKeyError::Unknown(word.to_owned())),
        }
    }
}

impl fmt::Display for SortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A list of sort criteria that cannot be understood.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SortKeyError {
    /// No criterion was given.
    #[error("no sort field given")]
    Empty,
    /// A name that matches no criterion.
    #[error("invalid sort field: {0}")]
    Unknown(String),
    /// A prefix that matches several criteria.
    #[error("ambiguous sort field: {0}")]
    Ambiguous(String),
    /// A character that is not a letter or a separator.
    #[error("invalid character in sort field list: {0:?}")]
    InvalidChar(char),
    /// More than [`MAX_SORT_KEYS`] criteria.
    #[error("too many fields in sort criteria (at most {MAX_SORT_KEYS})")]
    TooMany,
}

/// What [`sort_games`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortReport {
    /// Games in the index.
    pub games: u32,
    /// Games that changed position.
    pub moved: u32,
    /// The permutation applied: `order[i]` is the new position (0-based) of
    /// the game that was at position `i` (0-based) before the sort. It has
    /// `games` items and is the identity when nothing moved or the run was
    /// interrupted. The application uses it to carry the filter, the
    /// selection and the open game to the new numbers.
    pub order: Vec<u32>,
    /// The run was cancelled before the swap: the database is as it was.
    pub interrupted: bool,
}

/// Sorts the index by `keys` (the first key decides, the next ones break
/// ties); `reverse` inverts the whole order (`scidt -R`). Consumes the writer
/// and returns a new one on the sorted database. Equal games keep their
/// relative order, also with `reverse`.
///
/// # Errors
///
/// The errors of the transaction and [`Si4Error::CorruptIndex`] for an entry
/// that cannot be read. Nothing is changed on failure.
pub fn sort_games(
    writer: Si4Writer,
    keys: &[SortKey],
    reverse: bool,
) -> Result<(Si4Writer, SortReport)> {
    sort_games_with_hook(writer, keys, reverse, &mut FailPoint::never())
}

/// [`sort_games`] with a [`FailPoint`] to simulate a crash at each stage.
pub fn sort_games_with_hook(
    writer: Si4Writer,
    keys: &[SortKey],
    reverse: bool,
    hook: &mut FailPoint,
) -> Result<(Si4Writer, SortReport)> {
    sort_games_controlled(
        writer,
        keys,
        reverse,
        &mut NoProgress,
        &CancelToken::new(),
        hook,
    )
}

/// [`sort_games`] with progress and cancellation (decision D3580). Progress
/// counts the entries read and then the entries written; `cancel` is looked
/// at while reading, after the sort and while writing. A cancelled run
/// changes nothing: it returns a writer on the same database and a report
/// with `interrupted` set and the identity permutation.
#[allow(clippy::too_many_lines)]
pub fn sort_games_controlled(
    writer: Si4Writer,
    keys: &[SortKey],
    reverse: bool,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
    hook: &mut FailPoint,
) -> Result<(Si4Writer, SortReport)> {
    writer.check_alive()?;
    let Si4Writer {
        index_path,
        stem,
        mut index_file,
        header_bytes,
        header,
        names,
        ..
    } = writer;
    let count = header.num_games();
    let reopen = stem.clone();
    let stem_copy = stem.clone();
    let keys = keys.to_vec();
    let total = u64::from(count) * 2;
    let cancelled = std::cell::Cell::new(false);
    let cancelled_ref = &cancelled;
    let outcome = run_transaction(&reopen, "sort", hook, move |txn| {
        remove_cache(&stem);
        let raw = super::compact::read_entries(&mut index_file, &index_path, count)?;
        // `index` is the 0-based position within the current phase; `done`
        // is the progress across both phases.
        let stop = |index: u64, done: u64, progress: &mut dyn ProgressSink| {
            if index.is_multiple_of(256) {
                progress.progress(done, total);
                if cancel.is_cancelled() {
                    cancelled_ref.set(true);
                    return true;
                }
            }
            false
        };
        let interrupted = || Si4Error::Io {
            path: index_path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::Interrupted, "sort cancelled"),
        };
        let mut entries: Vec<IndexEntry> = Vec::with_capacity(raw.len() / ENTRY_LEN);
        for (n, bytes) in (1..=count).zip(raw.as_chunks::<ENTRY_LEN>().0) {
            if stop(u64::from(n - 1), u64::from(n), progress) {
                return Err(interrupted());
            }
            let (entry, _) = IndexEntry::parse_lenient(bytes.as_slice()).map_err(|e| {
                Si4Error::CorruptIndex {
                    game: n,
                    reason: e.reason(),
                }
            })?;
            entries.push(entry);
        }
        let mut order: Vec<usize> = (0..entries.len()).collect();
        // `sort_by` is stable: equal games keep their order, also reversed
        // (the reversed comparison of equal games is still equal).
        order.sort_by(|&a, &b| {
            let ordering = compare(&entries[a], &entries[b], &keys, &names);
            if reverse {
                ordering.reverse()
            } else {
                ordering
            }
        });
        // The sort itself cannot be interrupted: look at the token once it is
        // done, after telling the caller that the reading phase is complete.
        progress.progress(u64::from(count), total);
        if cancel.is_cancelled() {
            cancelled_ref.set(true);
            return Err(interrupted());
        }
        let chunks = raw.as_chunks::<ENTRY_LEN>().0;
        let mut sorted: Vec<u8> = Vec::with_capacity(raw.len());
        let mut moved = 0u32;
        let mut permutation = vec![0u32; order.len()];
        for (position, &source) in order.iter().enumerate() {
            if stop(
                position as u64,
                u64::from(count) + position as u64 + 1,
                progress,
            ) {
                return Err(interrupted());
            }
            if position != source {
                moved += 1;
            }
            if let Some(slot) = permutation.get_mut(source) {
                *slot = u32::try_from(position).unwrap_or(u32::MAX);
            }
            if let Some(chunk) = chunks.get(source) {
                sorted.extend_from_slice(chunk);
            }
        }
        txn.stage_file(&index_path, |out, tmp| {
            out.write_all(header_bytes.as_bytes())
                .map_err(io_error(tmp))?;
            out.write_all(&sorted).map_err(io_error(tmp))
        })?;
        drop(index_file);
        progress.progress(total, total);
        Ok(SortReport {
            games: count,
            moved,
            order: permutation,
            interrupted: false,
        })
    });
    match outcome {
        Ok(report) => Ok((Si4Writer::open(&stem_copy)?, report)),
        Err(_) if cancelled.get() => Ok((
            Si4Writer::open(&stem_copy)?,
            SortReport {
                games: count,
                moved: 0,
                order: (0..count).collect(),
                interrupted: true,
            },
        )),
        Err(error) => Err(error),
    }
}

/// `IndexEntry::Compare` without the reversal, which the caller applies to
/// the final result (the same as negating the first difference).
fn compare(a: &IndexEntry, b: &IndexEntry, keys: &[SortKey], names: &NameBuilder) -> Ordering {
    for key in keys {
        let ordering = compare_key(a, b, *key, names);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn name(names: &NameBuilder, kind: NameKind, id: u32) -> &[u8] {
    names.get(kind, id).unwrap_or(&[])
}

fn compare_key(a: &IndexEntry, b: &IndexEntry, key: SortKey, names: &NameBuilder) -> Ordering {
    let date = |e: &IndexEntry| e.date.to_scid_u32();
    let event_date = |e: &IndexEntry| {
        let d = e.event_date.to_scid_u32();
        if d == 0 { date(e) } else { d }
    };
    // `common.h`: none = 0, white = 1, black = 2, draw = 3; `SORT_result`
    // moves "none" to draw + 1.
    let result = |e: &IndexEntry| match e.result {
        mychess_core::GameResult::White => 1u8,
        mychess_core::GameResult::Black => 2,
        mychess_core::GameResult::Draw => 3,
        _ => 4,
    };
    let average = |e: &IndexEntry| u32::midpoint(u32::from(e.white_elo), u32::from(e.black_elo));
    match key {
        SortKey::Date => date(a).cmp(&date(b)),
        SortKey::Year => a.date.year().unwrap_or(0).cmp(&b.date.year().unwrap_or(0)),
        SortKey::Month => a
            .date
            .month()
            .unwrap_or(0)
            .cmp(&b.date.month().unwrap_or(0)),
        SortKey::Event => str_compare(
            name(names, NameKind::Event, a.event_id),
            name(names, NameKind::Event, b.event_id),
        ),
        SortKey::Site => str_compare(
            name(names, NameKind::Site, a.site_id),
            name(names, NameKind::Site, b.site_id),
        ),
        SortKey::Round => round_order(
            name(names, NameKind::Round, a.round_id),
            name(names, NameKind::Round, b.round_id),
        ),
        SortKey::White => str_compare(
            name(names, NameKind::Player, a.white_id),
            name(names, NameKind::Player, b.white_id),
        ),
        SortKey::Black => str_compare(
            name(names, NameKind::Player, a.black_id),
            name(names, NameKind::Player, b.black_id),
        ),
        SortKey::Eco => a.eco_code.cmp(&b.eco_code),
        SortKey::Result => result(a).cmp(&result(b)),
        SortKey::Length => a.num_half_moves.cmp(&b.num_half_moves),
        SortKey::Rating => average(b).cmp(&average(a)),
        SortKey::WhiteElo => b.white_elo.cmp(&a.white_elo),
        SortKey::BlackElo => b.black_elo.cmp(&a.black_elo),
        SortKey::Country => country_order(
            name(names, NameKind::Site, a.site_id),
            name(names, NameKind::Site, b.site_id),
        ),
        SortKey::Deleted => a
            .flags
            .contains(GameFlags::DELETE)
            .cmp(&b.flags.contains(GameFlags::DELETE)),
        SortKey::EventDate => event_date(a).cmp(&event_date(b)),
        SortKey::Variations => b.variation_count.cmp(&a.variation_count),
        SortKey::Comments => b.comment_count.cmp(&a.comment_count),
    }
}

/// `strCompare`: byte by byte as *signed* characters, the NUL terminator
/// counting as a byte (so a prefix comes first).
fn str_compare(a: &[u8], b: &[u8]) -> Ordering {
    let mut i = 0;
    loop {
        let ca = a.get(i).copied().unwrap_or(0);
        let cb = b.get(i).copied().unwrap_or(0);
        if ca != cb {
            return i8::from_ne_bytes([ca]).cmp(&i8::from_ne_bytes([cb]));
        }
        if ca == 0 {
            return Ordering::Equal;
        }
        i += 1;
    }
}

/// `strtol(text, NULL, 10)`: optional white space and sign, then digits;
/// anything else reads as 0. Saturates instead of overflowing.
fn leading_integer(text: &[u8]) -> i64 {
    let mut rest = text;
    while let [first, tail @ ..] = rest
        && first.is_ascii_whitespace()
    {
        rest = tail;
    }
    let negative = match rest {
        [b'-', tail @ ..] => {
            rest = tail;
            true
        }
        [b'+', tail @ ..] => {
            rest = tail;
            false
        }
        _ => false,
    };
    let value = rest
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .fold(0i64, |acc, d| {
            acc.saturating_mul(10).saturating_add(i64::from(d - b'0'))
        });
    if negative { -value } else { value }
}

/// `strCompareRound`: the integer at the start, then (when both names are
/// equal up to the first dot) the integer after the dot, then the byte order.
fn round_order(a: &[u8], b: &[u8]) -> Ordering {
    let lead = leading_integer(a).cmp(&leading_integer(b));
    if lead != Ordering::Equal {
        return lead;
    }
    let mut i = 0;
    while let (Some(&x), Some(&y)) = (a.get(i), b.get(i)) {
        if x != y {
            break;
        }
        if x == b'.' {
            let after = leading_integer(&a[i + 1..]).cmp(&leading_integer(&b[i + 1..]));
            if after != Ordering::Equal {
                return after;
            }
            break;
        }
        i += 1;
    }
    str_compare(a, b)
}

/// The three-letter country code at the end of a site (`Paris FRA`), or
/// `None` when the site does not end that way.
fn country_code(site: &[u8]) -> Option<&[u8]> {
    let len = site.len();
    if len < 4 || site[len - 4] != b' ' || !site[len - 3].is_ascii_uppercase() {
        return None;
    }
    Some(&site[len - 3..])
}

fn country_order(a: &[u8], b: &[u8]) -> Ordering {
    match (country_code(a), country_code(b)) {
        (Some(x), Some(y)) => str_compare(x, y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::si4::Si4Database;
    use mychess_core::{Game, GameResult, PartialDate};
    use pretty_assertions::assert_eq;

    fn game(white: &str, round: &str, date: &str, plies: usize, result: GameResult) -> Game {
        let mut g = Game::new();
        let mut node = g.root();
        for uci in ["e2e4", "e7e5", "g1f3", "b8c6"].iter().take(plies) {
            let mv = g.position_at(node).unwrap().parse_uci(uci).unwrap();
            node = g.add_move(node, mv).unwrap();
        }
        let tags = g.tags_mut();
        tags.white = white.into();
        tags.black = "Z".into();
        tags.event = "E".into();
        tags.site = "S".into();
        tags.round = round.into();
        tags.date = PartialDate::parse_lenient(date);
        tags.result = result;
        g
    }

    fn make(dir: &tempfile::TempDir, games: &[Game]) -> Si4Writer {
        let mut w = Si4Writer::create(&dir.path().join("db"), 0).unwrap();
        for g in games {
            w.add_game(g, GameFlags::empty()).unwrap();
        }
        w
    }

    fn whites(dir: &tempfile::TempDir) -> Vec<String> {
        let mut db = Si4Database::open(&dir.path().join("db")).unwrap();
        (1..=db.len())
            .map(|n| db.game(n).unwrap().tags().white.clone())
            .collect()
    }

    #[test]
    fn parses_lists_like_scid() {
        assert_eq!(
            SortKey::parse_list("Year,event.round date").unwrap(),
            vec![SortKey::Year, SortKey::Event, SortKey::Round, SortKey::Date]
        );
        assert_eq!(
            SortKey::parse_list("WElo").unwrap(),
            vec![SortKey::WhiteElo]
        );
        assert_eq!(SortKey::parse_list("ra").unwrap(), vec![SortKey::Rating]);
        assert_eq!(
            SortKey::parse_list("w").unwrap_err(),
            SortKeyError::Ambiguous("w".into())
        );
        assert_eq!(
            SortKey::parse_list("nope").unwrap_err(),
            SortKeyError::Unknown("nope".into())
        );
        assert_eq!(SortKey::parse_list("").unwrap_err(), SortKeyError::Empty);
        assert_eq!(
            SortKey::parse_list("date;year").unwrap_err(),
            SortKeyError::InvalidChar(';')
        );
        assert_eq!(
            SortKey::parse_list(&"date,".repeat(17)).unwrap_err(),
            SortKeyError::TooMany
        );
    }

    #[test]
    fn rounds_compare_as_numbers() {
        assert_eq!(round_order(b"2", b"10"), Ordering::Less);
        assert_eq!(round_order(b"3.2", b"3.10"), Ordering::Less);
        assert_eq!(round_order(b"?", b"1"), Ordering::Less);
        assert_eq!(round_order(b"4", b"4"), Ordering::Equal);
    }

    #[test]
    fn country_is_a_total_order() {
        assert_eq!(country_order(b"Paris FRA", b"Rome ITA"), Ordering::Less);
        assert_eq!(country_order(b"Paris", b"Rome ITA"), Ordering::Greater);
        assert_eq!(country_order(b"Rome ITA", b"Paris"), Ordering::Less);
        assert_eq!(country_order(b"Paris", b"Rome"), Ordering::Equal);
    }

    #[test]
    fn sorts_by_date_then_round_and_keeps_games_intact() {
        let dir = tempfile::tempdir().unwrap();
        let writer = make(
            &dir,
            &[
                game("C", "10", "2020.01.01", 2, GameResult::White),
                game("A", "2", "2020.01.01", 4, GameResult::Draw),
                game("B", "1", "2019.05.05", 0, GameResult::Black),
            ],
        );
        let (writer, report) = sort_games(writer, &[SortKey::Date, SortKey::Round], false).unwrap();
        assert_eq!(report.games, 3);
        assert_eq!(report.moved, 2);
        assert_eq!(writer.len(), 3);
        drop(writer);
        assert_eq!(whites(&dir), ["B", "A", "C"]);
    }

    #[test]
    fn reverse_inverts_and_is_stable_for_ties() {
        let dir = tempfile::tempdir().unwrap();
        let writer = make(
            &dir,
            &[
                game("A", "1", "2020.01.01", 2, GameResult::White),
                game("B", "1", "2020.01.01", 2, GameResult::White),
                game("C", "1", "2021.01.01", 2, GameResult::White),
            ],
        );
        let (writer, _) = sort_games(writer, &[SortKey::Year], true).unwrap();
        drop(writer);
        assert_eq!(whites(&dir), ["C", "A", "B"]);
    }

    #[test]
    fn already_sorted_moves_nothing_and_length_orders_by_half_moves() {
        let dir = tempfile::tempdir().unwrap();
        let writer = make(
            &dir,
            &[
                game("A", "1", "2020.01.01", 0, GameResult::White),
                game("B", "1", "2020.01.01", 2, GameResult::White),
                game("C", "1", "2020.01.01", 4, GameResult::White),
            ],
        );
        let (writer, report) = sort_games(writer, &[SortKey::Length], false).unwrap();
        assert_eq!(report.moved, 0);
        let (writer, report) = sort_games(writer, &[SortKey::Length], true).unwrap();
        assert_eq!(report.moved, 2);
        drop(writer);
        assert_eq!(whites(&dir), ["C", "B", "A"]);
    }

    #[test]
    fn a_failure_in_the_transaction_keeps_the_old_order() {
        let dir = tempfile::tempdir().unwrap();
        let writer = make(
            &dir,
            &[
                game("B", "1", "2021.01.01", 2, GameResult::White),
                game("A", "1", "2020.01.01", 2, GameResult::White),
            ],
        );
        let result = sort_games_with_hook(
            writer,
            &[SortKey::Date],
            false,
            &mut FailPoint::at_label("begin"),
        );
        assert!(result.is_err());
        // The next writer finishes or drops what the failure left.
        drop(Si4Writer::open(&dir.path().join("db")).unwrap());
        assert_eq!(whites(&dir), ["B", "A"]);
    }
}
