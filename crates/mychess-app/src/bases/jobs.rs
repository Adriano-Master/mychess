//! The blocking work behind the list effects of slice 4c: quick search,
//! sort, copy to the Clipbase and flag changes (spec M3-4; D556 onwards).
//!
//! `perform` runs these on `spawn_blocking`; `update` never calls them.
//! Long ones report to an [`OpenControl`] (games done out of games to do)
//! and stop when it is cancelled, leaving everything as it was.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mychess_core::GameResult;
use mychess_db::si4::{
    FlagOp, GameFlags, IndexEntry, MAX_GAMES, NameBase, NameKind, PreparedGame, Si4Database,
    Si4Writer, bytes_to_string,
};
use mychess_db::{Filter, GameSource, TextMatcher};

use super::columns::ListColumn;
use super::error::BaseError;
use super::list_message::{
    CopyList, CopyOutcome, CopyTarget, FlagOutcome, Reloaded, SearchKind, SearchOutcome, SearchSpec,
};
use super::open::{FileStamp, OpenControl};
use super::rows::country_of;
use super::source::{BaseSource, SharedSource};

/// Entries read under one lock while sorting: the lock is released between
/// chunks so the list can be drawn.
const SORT_CHUNK: u32 = 16_384;

fn cancelled(control: &OpenControl) -> Result<(), BaseError> {
    if control.is_cancelled() {
        Err(BaseError::Cancelled)
    } else {
        Ok(())
    }
}

/// Runs `visit` on the games `next_game` yields, a chunk at a time: the read
/// lock on the database is taken for one chunk and let go between chunks, so
/// a long search never keeps the list (drawn by `view`) or a game load
/// waiting for more than a chunk (D570). Progress and cancellation are
/// looked at between chunks.
fn scan_chunks(
    source: &SharedSource,
    control: &OpenControl,
    total: u64,
    mut next_game: impl FnMut() -> Option<u32>,
    mut visit: impl FnMut(&BaseSource, u32) -> bool,
) -> Result<(), BaseError> {
    let mut progress = control.reporter();
    let mut done = 0u64;
    loop {
        cancelled(control)?;
        progress(done, total);
        let db = source.read();
        for _ in 0..SORT_CHUNK {
            let Some(game) = next_game() else {
                progress(total, total);
                return Ok(());
            };
            done += 1;
            if !visit(&db, game) {
                return Ok(());
            }
        }
    }
}

/// The quick search of the list.
///
/// `Find` looks for the next game after position `from` of the view (0 =
/// before the first); `tail`, when the view is sorted, holds the games of
/// the view after the current one, in view order. `Filter` applies the
/// `+`-separated terms of `text` one after the other to a copy of `filter`.
/// The database is read a chunk at a time (see [`scan_chunks`]).
///
/// # Errors
///
/// [`BaseError::Cancelled`] when the control was cancelled.
pub(crate) fn find_text(
    source: &SharedSource,
    filter: &Filter,
    spec: &SearchSpec,
    control: &OpenControl,
) -> Result<SearchOutcome, BaseError> {
    match spec.kind {
        SearchKind::Find => {
            let needle = spec.text.trim();
            if needle.is_empty() {
                return Ok(SearchOutcome::Found(None));
            }
            let mut matcher = TextMatcher::new(needle, spec.ignore_case);
            let mut found = None;
            if let Some(tail) = &spec.tail {
                // A sorted view: the first hit in view order.
                let mut games = tail.iter().copied();
                scan_chunks(
                    source,
                    control,
                    tail.len() as u64,
                    || games.next(),
                    |db, game| {
                        if matcher.matches(db, game) == Some(true) {
                            found = Some(game);
                            return false;
                        }
                        true
                    },
                )?;
            } else {
                let mut last = filter.nth(spec.from).unwrap_or(0);
                scan_chunks(
                    source,
                    control,
                    u64::from(filter.count()),
                    || {
                        let next = filter.next_after(last)?;
                        last = next;
                        Some(next)
                    },
                    |db, game| {
                        if matcher.matches(db, game) == Some(true) {
                            found = Some(game);
                            return false;
                        }
                        true
                    },
                )?;
            }
            Ok(SearchOutcome::Found(found))
        }
        SearchKind::Filter => {
            let mut work = filter.clone();
            let mut removed = 0u32;
            let mut unreadable = 0u64;
            for term in spec
                .text
                .split('+')
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                let base = work.clone();
                let mut matcher = TextMatcher::new(term, spec.ignore_case);
                let mut last = 0u32;
                scan_chunks(
                    source,
                    control,
                    u64::from(base.count()),
                    || {
                        let next = base.next_after(last)?;
                        last = next;
                        Some(next)
                    },
                    |db, game| {
                        match matcher.matches(db, game) {
                            Some(true) => {}
                            other => {
                                if other.is_none() {
                                    unreadable += 1;
                                }
                                work.set(game, 0);
                                removed = removed.saturating_add(1);
                            }
                        }
                        true
                    },
                )?;
            }
            Ok(SearchOutcome::Filtered {
                filter: Box::new(work),
                removed,
                unreadable,
            })
        }
    }
}

/// The order of every game of `source` by `column`, smallest first; games
/// with equal keys keep the database order.
///
/// # Errors
///
/// [`BaseError::Cancelled`]; [`BaseError::Internal`] for a column that does
/// not sort.
pub(crate) fn sort_order(
    source: &SharedSource,
    column: ListColumn,
    control: &OpenControl,
) -> Result<Arc<Vec<u32>>, BaseError> {
    if !column.is_sortable() {
        return Err(BaseError::Internal);
    }
    let mut progress = control.reporter();
    let (len, ranks) = {
        let db = source.read();
        (GameSource::len(&*db), Ranks::build(column, db.names()))
    };
    let mut keyed: Vec<(u64, u32)> = Vec::with_capacity(len as usize);
    let mut next = 1u32;
    while next <= len {
        cancelled(control)?;
        progress(u64::from(next - 1), u64::from(len));
        let end = next.saturating_add(SORT_CHUNK - 1).min(len);
        let db = source.read();
        for number in next..=end {
            let key = db
                .entry(number)
                .map_or(u64::MAX, |entry| key_of(column, &entry, &ranks));
            keyed.push((key, number));
        }
        drop(db);
        next = end.saturating_add(1);
        if end == u32::MAX {
            break;
        }
    }
    cancelled(control)?;
    keyed.sort_unstable();
    progress(u64::from(len), u64::from(len));
    Ok(Arc::new(keyed.into_iter().map(|(_, n)| n).collect()))
}

/// Rank tables for the columns that sort by name: `rank[id]` is the
/// position of the name `id` among its kind.
struct Ranks {
    white: Vec<u32>,
    black: Vec<u32>,
    event: Vec<u32>,
    site: Vec<u32>,
    round: Vec<u32>,
    country: Vec<u32>,
}

impl Ranks {
    fn build(column: ListColumn, names: &NameBase) -> Ranks {
        let player = |wanted: bool| {
            if wanted {
                name_ranks(names, NameKind::Player, cmp_text)
            } else {
                Vec::new()
            }
        };
        let both = matches!(column, ListColumn::White | ListColumn::Black);
        let players = player(both);
        Ranks {
            white: players.clone(),
            black: players,
            event: if column == ListColumn::Event {
                name_ranks(names, NameKind::Event, cmp_text)
            } else {
                Vec::new()
            },
            site: if column == ListColumn::Site {
                name_ranks(names, NameKind::Site, cmp_text)
            } else {
                Vec::new()
            },
            round: if column == ListColumn::Round {
                name_ranks(names, NameKind::Round, cmp_round)
            } else {
                Vec::new()
            },
            country: if column == ListColumn::Country {
                country_ranks(names)
            } else {
                Vec::new()
            },
        }
    }
}

fn rank(table: &[u32], id: u32) -> u64 {
    usize::try_from(id)
        .ok()
        .and_then(|i| table.get(i))
        .map_or(u64::MAX, |r| u64::from(*r))
}

fn key_of(column: ListColumn, entry: &IndexEntry, ranks: &Ranks) -> u64 {
    match column {
        ListColumn::White => rank(&ranks.white, entry.white_id),
        ListColumn::Black => rank(&ranks.black, entry.black_id),
        ListColumn::Event => rank(&ranks.event, entry.event_id),
        ListColumn::Site => rank(&ranks.site, entry.site_id),
        ListColumn::Round => rank(&ranks.round, entry.round_id),
        ListColumn::Country => rank(&ranks.country, entry.site_id),
        ListColumn::Date => u64::from(entry.date.to_scid_u32()),
        ListColumn::EventDate => u64::from(entry.event_date.to_scid_u32()),
        // Scid index.cpp 731-740: 1-0, 0-1, 1/2-1/2, then the unfinished.
        ListColumn::Result => match entry.result {
            GameResult::White => 1,
            GameResult::Black => 2,
            GameResult::Draw => 3,
            _ => 4,
        },
        ListColumn::Length => u64::from(entry.num_half_moves),
        // Scid index.cpp 748-754 and 795-801: the first click lists the
        // highest rating (and the largest count) first, so the ascending
        // order of the key is the descending order of the value (D571).
        ListColumn::WhiteElo => u64::MAX - u64::from(entry.white_elo),
        ListColumn::BlackElo => u64::MAX - u64::from(entry.black_elo),
        ListColumn::Deleted => u64::from(entry.flags.contains(GameFlags::DELETE)),
        ListColumn::Variations => u64::MAX - u64::from(entry.variation_count),
        ListColumn::Comments => u64::MAX - u64::from(entry.comment_count),
        ListColumn::Eco => u64::from(entry.eco_code),
        _ => 0,
    }
}

/// Text order without regard to ASCII case, then by bytes.
fn cmp_text(a: &[u8], b: &[u8]) -> Ordering {
    a.iter()
        .map(u8::to_ascii_lowercase)
        .cmp(b.iter().map(u8::to_ascii_lowercase))
        .then_with(|| a.cmp(b))
}

/// The order of round names (Scid `strCompareRound`): the integer at the
/// start decides first, and a name that does not start with a digit (`?`,
/// empty) reads as 0, so it comes before round 1; then digit runs compare
/// as numbers (`3.2` before `3.10`).
fn cmp_round(a: &[u8], b: &[u8]) -> Ordering {
    let lead = |x: &[u8]| split_number(x).0;
    lead(a).cmp(&lead(b)).then_with(|| cmp_natural(a, b))
}

/// Text order where runs of digits compare as numbers (`2` before `10`,
/// `3.2` before `3.10`).
fn cmp_natural(a: &[u8], b: &[u8]) -> Ordering {
    let (mut a, mut b) = (a, b);
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (na, rest_a) = split_number(a);
                let (nb, rest_b) = split_number(b);
                match na.cmp(&nb) {
                    Ordering::Equal => {}
                    other => return other,
                }
                a = rest_a;
                b = rest_b;
            }
            (Some(x), Some(y)) => {
                match x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase()) {
                    Ordering::Equal => {}
                    other => return other,
                }
                a = a.get(1..).unwrap_or_default();
                b = b.get(1..).unwrap_or_default();
            }
        }
    }
}

/// The leading digits as a number (saturating) and what follows them.
fn split_number(bytes: &[u8]) -> (u64, &[u8]) {
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    let value = bytes.iter().take(digits).fold(0u64, |acc, d| {
        acc.saturating_mul(10).saturating_add(u64::from(d - b'0'))
    });
    (value, bytes.get(digits..).unwrap_or_default())
}

fn name_ranks(names: &NameBase, kind: NameKind, cmp: fn(&[u8], &[u8]) -> Ordering) -> Vec<u32> {
    let count = names.num_names(kind);
    let mut ids: Vec<u32> = (0..count).collect();
    let text = |id: u32| names.get(kind, id).unwrap_or_default();
    ids.sort_by(|a, b| cmp(text(*a), text(*b)).then(a.cmp(b)));
    let mut ranks = vec![0u32; count as usize];
    for (position, id) in (0u32..).zip(ids) {
        if let Some(slot) = ranks.get_mut(id as usize) {
            *slot = position;
        }
    }
    ranks
}

/// Ranks of the sites by the country code they end with (`?` when none).
fn country_ranks(names: &NameBase) -> Vec<u32> {
    let count = names.num_names(NameKind::Site);
    let countries: Vec<String> = (0..count)
        .map(|id| {
            names
                .get(NameKind::Site, id)
                .map_or_else(String::new, |b| country_of(&bytes_to_string(b)))
        })
        .collect();
    let mut ids: Vec<u32> = (0..count).collect();
    ids.sort_by(|a, b| {
        let key = |id: &u32| countries.get(*id as usize).map_or("", String::as_str);
        key(a).cmp(key(b)).then(a.cmp(b))
    });
    // Equal countries must share a rank: sites of one country sort together
    // and keep the database order among themselves.
    let mut ranks = vec![0u32; count as usize];
    let mut rank_value = 0u32;
    let mut previous: Option<&str> = None;
    for id in ids {
        let current = countries.get(id as usize).map_or("", String::as_str);
        if previous.is_some_and(|p| p != current) {
            rank_value += 1;
        }
        previous = Some(current);
        if let Some(slot) = ranks.get_mut(id as usize) {
            *slot = rank_value;
        }
    }
    ranks
}

/// Games prepared before they are written in one go. The `.sn4` of an SI4
/// target is rewritten once per batch (D511), so the batch is large; the
/// work between two batches is what bounds how much a cancel can leave
/// unwritten (nothing: the pending batch is written too, D578).
pub(super) const COPY_BATCH: usize = 5_000;

/// Reads game `number` of `from` and encodes it for the target, keeping the
/// user flags of its index entry (the Scid copies the whole entry, so a game
/// marked deleted stays marked: `sc_savegame`, D581).
fn prepare_game(from: &SharedSource, number: u32) -> Result<PreparedGame, BaseError> {
    let unreadable = |_| BaseError::GameUnreadable { number };
    let (game, flags) = {
        let mut db = from.lock();
        let flags = db.entry(number).map_err(unreadable)?.flags;
        (db.game(number).map_err(unreadable)?, flags)
    };
    PreparedGame::from_game(&game, flags).map_err(|e| {
        tracing::warn!(game = number, error = ?e, "a game cannot be encoded for the copy");
        BaseError::from_encode(&e)
    })
}

/// The loop of a copy: reads, encodes and hands `write` a batch at a time.
/// Returns the games written, how many of them had a name shortened and why
/// it stopped early, if it did.
///
/// A cancel or a game that cannot be read ends the loop, and the games
/// already prepared are still written: the copy keeps everything before the
/// problem. A batch the target refuses is not written at all (all or none
/// per batch) and ends the copy.
pub(super) fn run_copy(
    from: &SharedSource,
    numbers: impl Iterator<Item = u32>,
    control: &OpenControl,
    total: u64,
    write: &mut impl FnMut(&[PreparedGame]) -> Result<(), BaseError>,
) -> (u32, u32, Option<BaseError>) {
    let mut progress = control.reporter();
    let mut truncated = 0u32;
    let mut batch: Vec<PreparedGame> = Vec::with_capacity(COPY_BATCH);
    let mut copied = 0u32;
    let mut error = None;
    let mut done = 0u64;
    let mut flush = |batch: &mut Vec<PreparedGame>, copied: &mut u32| -> Result<(), BaseError> {
        let added = u32::try_from(batch.len()).unwrap_or(u32::MAX);
        let result = write(batch);
        batch.clear();
        result?;
        *copied = copied.saturating_add(added);
        Ok(())
    };
    for number in numbers {
        if control.is_cancelled() {
            error = Some(BaseError::Cancelled);
            break;
        }
        progress(done, total);
        match prepare_game(from, number) {
            Ok(prepared) => {
                if prepared.names_truncated() {
                    truncated = truncated.saturating_add(1);
                }
                batch.push(prepared);
            }
            Err(e) => {
                error = Some(e);
                break;
            }
        }
        done += 1;
        if batch.len() >= COPY_BATCH
            && let Err(e) = flush(&mut batch, &mut copied)
        {
            error = Some(e);
            break;
        }
    }
    if !batch.is_empty()
        && let Err(e) = flush(&mut batch, &mut copied)
    {
        error = error.or(Some(e));
    }
    progress(done, total);
    (copied, truncated, error)
}

/// Copies the games of `list` from `from` to the end of `target`, in the
/// order of the source (slice 4d, D577).
///
/// - Clipbase target: the limit is checked before any game is read
///   ([`BaseError::ClipbaseLimit`]); games are appended in batches with
///   [`Clipbase::push_many`](super::Clipbase::push_many).
/// - `.si4` target: the file must still be the one that was read (size and
///   time, then the number of games the writer finds), else nothing is
///   written and the outcome carries [`BaseError::Stale`]. A short-lived
///   writer adds the games in batches with `add_prepared_batch` (names
///   written once per batch; a full name table or game table stops the copy
///   with an explicit error), is dropped, and the database is opened again so
///   the slot shows what is on disk, whatever happened.
///
/// # Errors
///
/// Only when nothing was written and nothing needs reloading:
/// [`BaseError::Cancelled`] before the start, [`BaseError::ClipbaseLimit`],
/// [`BaseError::TooManyGames`] (the target cannot hold them all), and the
/// errors of opening the writer ([`BaseError::AlreadyOpen`],
/// [`BaseError::File`]). Everything else is in [`CopyOutcome::error`].
pub(crate) fn copy_games_to(
    from: &SharedSource,
    source_stamp: Option<&(PathBuf, FileStamp)>,
    list: &CopyList,
    target: &CopyTarget,
    control: &OpenControl,
) -> Result<CopyOutcome, BaseError> {
    cancelled(control)?;
    // An `.si4` source must still be the file that was read (D590).
    if source_stamp.is_some_and(|(path, stamp)| FileStamp::of(path) != Some(*stamp)) {
        return Err(BaseError::Stale);
    }
    let count = list.count();
    let total = u64::from(count);
    match target {
        CopyTarget::Clipbase(to) => {
            {
                let target = to.read();
                let clip = target.clipbase().ok_or(BaseError::Internal)?;
                let room =
                    u64::from(clip.limit()).saturating_sub(u64::from(GameSource::len(&*target)));
                if total > room {
                    return Err(BaseError::ClipbaseLimit {
                        limit: clip.limit(),
                    });
                }
            }
            let mut write = |batch: &[PreparedGame]| -> Result<(), BaseError> {
                to.lock()
                    .clipbase_mut()
                    .ok_or(BaseError::Internal)?
                    .push_many(batch)
                    .map(drop)
            };
            let (copied, truncated, error) = match list {
                CopyList::Filter(filter) => {
                    run_copy(from, filter.iter_games(), control, total, &mut write)
                }
                CopyList::Numbers(numbers) => {
                    run_copy(from, numbers.iter().copied(), control, total, &mut write)
                }
            };
            Ok(CopyOutcome {
                copied,
                error,
                truncated,
                reloaded: None,
            })
        }
        CopyTarget::Si4 {
            path,
            expected_total,
            stamp,
        } => copy_to_si4(from, list, path, *expected_total, *stamp, control),
    }
}

fn copy_to_si4(
    from: &SharedSource,
    list: &CopyList,
    path: &Path,
    expected_total: u32,
    stamp: Option<FileStamp>,
    control: &OpenControl,
) -> Result<CopyOutcome, BaseError> {
    let total = u64::from(list.count());
    let (copied, truncated, mut error) =
        if stamp.is_some_and(|expected| FileStamp::of(path) != Some(expected)) {
            (0, 0, Some(BaseError::Stale))
        } else {
            let mut writer = Si4Writer::open(path).map_err(|e| BaseError::from_si4(&e))?;
            let result = if writer.len() != expected_total {
                (0, 0, Some(BaseError::Stale))
            } else if u64::from(writer.len()).saturating_add(total) > u64::from(MAX_GAMES) {
                // Checked up front so a copy that cannot fit writes nothing.
                return Err(BaseError::TooManyGames { limit: MAX_GAMES });
            } else {
                let mut write = |batch: &[PreparedGame]| -> Result<(), BaseError> {
                    writer.add_prepared_batch(batch).map(drop).map_err(|e| {
                        // Logged once here, at the border, with the original
                        // error before it becomes a `BaseError` (D590).
                        tracing::warn!(
                            error = ?e,
                            path = %path.display(),
                            "a batch of the copy was refused"
                        );
                        BaseError::from_si4(&e)
                    })
                };
                match list {
                    CopyList::Filter(filter) => {
                        run_copy(from, filter.iter_games(), control, total, &mut write)
                    }
                    CopyList::Numbers(numbers) => {
                        run_copy(from, numbers.iter().copied(), control, total, &mut write)
                    }
                }
            };
            // On Windows a live writer blocks reads of the index: drop it
            // before the reader opens the files again.
            drop(writer);
            result
        };
    let reloaded = match Si4Database::open(path) {
        Ok(db) => Some(Reloaded {
            source: SharedSource::new(BaseSource::Si4(Box::new(db))),
            stamp: FileStamp::of(path),
        }),
        Err(e) => {
            tracing::error!(error = ?e, path = %path.display(), copied, "the target cannot be opened again after a copy");
            error = error.or(Some(BaseError::from_si4(&e)));
            None
        }
    };
    Ok(CopyOutcome {
        copied,
        error,
        truncated,
        reloaded,
    })
}

/// Toggles the "deleted" flag of games `numbers` in the `.si4` file at
/// `path` and opens the database again (the old one is stale).
///
/// Before anything is written the file must still be the one that was read:
/// the size and time of the file (`stamp`) and the number of games the writer
/// finds (`expected_total`) must match, else nothing is written and the
/// outcome carries [`BaseError::Stale`] (D573). A failure in the middle keeps
/// the flags already written, and the database is reopened in every case
/// where the writer could be opened, so the slot shows what is on disk.
///
/// A short-lived [`Si4Writer`] does the work and is dropped before the
/// reader is opened: on Windows a live writer blocks reads of the index.
///
/// # Errors
///
/// [`BaseError::AlreadyOpen`] when another writer holds the database;
/// [`BaseError::File`] for permission and I/O failures when nothing could be
/// done; [`BaseError::Cancelled`] before anything is written.
pub(crate) fn toggle_deleted(
    path: &Path,
    numbers: &[u32],
    expected_total: u32,
    stamp: Option<FileStamp>,
    control: &OpenControl,
) -> Result<FlagOutcome, BaseError> {
    cancelled(control)?;
    let mut progress = control.reporter();
    let total = numbers.len() as u64;
    let mut changed = 0u32;
    let mut error = None;
    if stamp.is_some_and(|expected| FileStamp::of(path) != Some(expected)) {
        error = Some(BaseError::Stale);
    } else {
        let mut writer = Si4Writer::open(path).map_err(|e| BaseError::from_si4(&e))?;
        if writer.len() == expected_total {
            for (done, number) in (0u64..).zip(numbers) {
                progress(done, total);
                match writer.set_flag(*number, GameFlags::DELETE, FlagOp::Toggle) {
                    Ok(true) => changed += 1,
                    Ok(false) => {}
                    Err(e) => {
                        error = Some(BaseError::from_si4(&e));
                        break;
                    }
                }
            }
        } else {
            error = Some(BaseError::Stale);
        }
    }
    // A failed reopening keeps what was changed: the outcome has no source
    // and says why (D592).
    let source = match Si4Database::open(path) {
        Ok(db) => Some(SharedSource::new(BaseSource::Si4(Box::new(db)))),
        Err(e) => {
            tracing::error!(error = ?e, path = %path.display(), changed, "the database cannot be opened again after a flag change");
            error = error.or(Some(BaseError::from_si4(&e)));
            None
        }
    };
    progress(total, total);
    Ok(FlagOutcome {
        source,
        changed,
        error,
        stamp: FileStamp::of(path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_compares_numbers_inside_text() {
        assert_eq!(cmp_natural(b"2", b"10"), Ordering::Less);
        assert_eq!(cmp_natural(b"3.2", b"3.10"), Ordering::Less);
        assert_eq!(cmp_natural(b"3.10", b"3.2"), Ordering::Greater);
        assert_eq!(cmp_natural(b"a", b"A"), Ordering::Equal);
        assert_eq!(cmp_natural(b"", b"1"), Ordering::Less);
        assert_eq!(
            cmp_natural(b"99999999999999999999999", b"1"),
            Ordering::Greater
        );
    }

    #[test]
    fn text_order_ignores_ascii_case_first() {
        assert_eq!(cmp_text(b"abc", b"ABD"), Ordering::Less);
        assert_eq!(cmp_text(b"b", b"A"), Ordering::Greater);
        assert_eq!(cmp_text(b"a", b"A"), Ordering::Greater, "ties by bytes");
    }
}
