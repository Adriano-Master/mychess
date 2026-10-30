// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_search_board`,
// `sc_search_moves`, `sc_search_material`: the shared filter loop).
// Copyright (c) 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is
// distributed under the GNU General Public License (see its COPYING); this
// file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The common scan of the advanced searches (M6-1a, decisions D3201 to
//! D3207 and D3245 of `docs/specs/m6-1-buscas-avancadas.md`).
//!
//! Structure follows the loops of `sc_search_board`, `sc_search_moves` and
//! `sc_search_material` of Scid (`tkscid.cpp`), which are the same except for
//! the per-game test: the filter operation, the "no record" rule, progress and
//! cancellation. Here the per-game test is a [`GameMatcher`] and the loop is
//! shared.
//!
//! ## Blocks and threads (D3205)
//!
//! The records are read **sequentially**, in blocks of up to
//! [`BLOCK_GAMES`] games or [`BLOCK_BYTES`] bytes (whichever comes first),
//! because [`GameSource::record_bytes`] needs `&mut self`. Each block is then
//! evaluated in parallel with `rayon` and merged **in game order**, so the
//! resulting filter is byte for byte the same with one thread or many.
//!
//! ## Differences from Scid
//!
//! - A cancelled scan leaves the filter **unchanged** (D3202); Scid leaves it
//!   half updated.
//! - A game whose record cannot be read does not match and is listed in
//!   [`SearchReport::unreadable`] (D3203).

use std::time::{Duration, Instant};

use rayon::prelude::*;

use super::header::{SearchError, SearchReport};
use crate::filter::{Filter, FilterOp};
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, SEARCH_CHECK_EVERY, Ticker};
use crate::si4::{IndexEntry, RecordError};
use crate::source::GameSource;

/// Most games in a block of the parallel scan.
pub const BLOCK_GAMES: usize = 2048;

/// Most record bytes in a block of the parallel scan. A block always takes at
/// least one game, so a record bigger than this is still examined.
pub const BLOCK_BYTES: usize = 8 << 20;

/// What the index entry alone says about a game (the "shortcuts" of Scid).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    /// The game cannot match: it leaves the filter without being read.
    Skip,
    /// The record must be read and evaluated.
    Examine,
}

/// The per-game test of a scan.
///
/// A shortcut may only be an **acceleration**: when [`prefilter`] answers
/// [`Shortcut::Skip`], [`evaluate`] would have answered `Ok(None)` too
/// (D3207). The matcher is shared between threads, so it must not keep
/// mutable state.
///
/// [`prefilter`]: GameMatcher::prefilter
/// [`evaluate`]: GameMatcher::evaluate
pub trait GameMatcher: Sync {
    /// Decides from the index entry whether the record must be read.
    fn prefilter(&self, entry: &IndexEntry) -> Shortcut;

    /// Tests one game. `Ok(Some(v))` is a match with filter value `v` (ply
    /// plus 1, 1 to 255; 0 is read as 1); `Ok(None)` is no match; `Err` is a
    /// record that could not be read, which counts as no match and is
    /// reported in [`SearchReport::unreadable`].
    ///
    /// # Errors
    ///
    /// A [`RecordError`] when the record is damaged before the answer is
    /// known.
    fn evaluate(&self, entry: &IndexEntry, record: &[u8]) -> Result<Option<u8>, RecordError>;
}

/// A game queued in the current block.
struct Job {
    number: u32,
    entry: IndexEntry,
    start: usize,
    end: usize,
}

/// Runs `matcher` over every game that `op` selects and combines the result
/// with `filter` (Scid's loop, shared by the three searches).
///
/// - `And`: only games in the filter are examined; the others stay out.
/// - `Or`: only games out of the filter are examined; those in keep their
///   value.
/// - `Reset`: every game is examined.
///
/// A game that is examined and does not match gets 0 (even with `Or`); a game
/// without a record (`Length == 0`) gets 0 without being examined. Progress is
/// reported to `progress` every 5000 games or 100 ms, where `cancel` is also
/// looked at (and before every game of a parallel block). If cancelled,
/// `filter` is left as it was and [`SearchReport::interrupted`] is set.
///
/// The result does not depend on the number of `rayon` threads.
///
/// # Errors
///
/// [`SearchError::FilterSize`] if `filter` does not cover the database.
// One loop, as the three loops of Scid it replaces.
#[allow(clippy::too_many_lines)]
pub fn scan_games<D: GameSource + ?Sized, M: GameMatcher>(
    db: &mut D,
    matcher: &M,
    op: FilterOp,
    filter: &mut Filter,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<SearchReport, SearchError> {
    scan_games_with_limits(
        db,
        matcher,
        op,
        filter,
        progress,
        cancel,
        BLOCK_GAMES,
        BLOCK_BYTES,
    )
}

/// [`scan_games`] with the block limits as parameters, so tests can force
/// the cut by bytes. A block always takes at least one game; a limit of 0
/// games is read as 1.
///
/// # Errors
///
/// [`SearchError::FilterSize`] if `filter` does not cover the database.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub(crate) fn scan_games_with_limits<D: GameSource + ?Sized, M: GameMatcher>(
    db: &mut D,
    matcher: &M,
    op: FilterOp,
    filter: &mut Filter,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
    max_games: usize,
    max_bytes: usize,
) -> Result<SearchReport, SearchError> {
    let games = db.len();
    if filter.len() != games {
        return Err(SearchError::FilterSize {
            filter: filter.len(),
            games,
        });
    }
    let started = Instant::now();
    let mut report = SearchReport {
        matched: filter.count(),
        start_count: if op == FilterOp::And {
            filter.count()
        } else {
            games
        },
        elapsed: Duration::ZERO,
        interrupted: false,
        unreadable: Vec::new(),
        unreadable_total: 0,
    };
    if cancel.is_cancelled() {
        report.interrupted = true;
        report.elapsed = started.elapsed();
        return Ok(report);
    }

    let mut work = filter.clone();
    let mut op = op;
    if op == FilterOp::Reset {
        work.fill(1);
        op = FilterOp::And;
    }
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    let mut buffer: Vec<u8> = Vec::new();
    let mut jobs: Vec<Job> = Vec::new();
    let mut unreadable: Vec<u32> = Vec::new();
    let mut next = 1u32;

    'blocks: while next <= games {
        buffer.clear();
        jobs.clear();
        unreadable.clear();
        while next <= games && jobs.len() < max_games.max(1) && buffer.len() < max_bytes {
            let n = next;
            next += 1;
            if ticker.tick() {
                progress.progress(u64::from(n - 1), u64::from(games));
                if cancel.is_cancelled() {
                    report.interrupted = true;
                    break 'blocks;
                }
            }
            let was_in = work.contains(n);
            match op {
                FilterOp::And if !was_in => continue,
                FilterOp::Or if was_in => continue,
                _ => {}
            }
            let Ok(entry) = db.entry(n) else {
                work.set(n, 0);
                unreadable.push(n);
                continue;
            };
            if entry.length == 0 || matcher.prefilter(&entry) == Shortcut::Skip {
                work.set(n, 0);
                continue;
            }
            let Ok(record) = db.record_bytes(n) else {
                work.set(n, 0);
                unreadable.push(n);
                continue;
            };
            let start = buffer.len();
            buffer.extend_from_slice(record);
            jobs.push(Job {
                number: n,
                entry,
                start,
                end: buffer.len(),
            });
        }

        let results: Vec<Option<Result<Option<u8>, RecordError>>> = jobs
            .par_iter()
            .map(|job| {
                if cancel.is_cancelled() {
                    return None;
                }
                let record = buffer.get(job.start..job.end).unwrap_or(&[]);
                Some(matcher.evaluate(&job.entry, record))
            })
            .collect();
        if cancel.is_cancelled() {
            report.interrupted = true;
            break;
        }
        for (job, result) in jobs.iter().zip(results) {
            match result {
                Some(Ok(Some(value))) => {
                    work.set(job.number, value.max(1));
                }
                Some(Ok(None)) | None => {
                    work.set(job.number, 0);
                }
                Some(Err(_)) => {
                    work.set(job.number, 0);
                    unreadable.push(job.number);
                }
            }
        }
        unreadable.sort_unstable();
        for &n in &unreadable {
            report.note_unreadable(n);
        }
    }

    if !report.interrupted {
        progress.progress(u64::from(games), u64::from(games));
        *filter = work;
        report.matched = filter.count();
    }
    report.elapsed = started.elapsed();
    Ok(report)
}

#[cfg(test)]
mod tests;
