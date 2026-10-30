// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_filter_stats`,
// `sc_filter_freq`) and tcl/windows/stats.tcl, tcl/tools/graphs.tcl (the two
// Tcl files carry no copyright notice).
// Copyright notices of src/tkscid.cpp: Copyright (c) 1999-2004 Shane Hudson;
// Copyright (c) 2006-2007 Pascal Georges; Copyright (c) 2009-2013 Stevenaaus.
// Scid is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Statistics and graphs of the filter (milestone M6, slice 4b; decisions
//! D3370 to D3376 of `docs/specs/m6-4-tabela-estatisticas-rastreador.md`).
//!
//! - [`filter_stats`] and [`filter_stats_rows`]: the counts of results of the
//!   statistics window (`sc_filter stats`), with [`StatsConfig`] and
//!   [`filter_stats_table`] for the lines of the window (spec 2.2).
//! - [`filter_histogram`]: the frequency per decade, year, rating and number
//!   of moves of the two filter graphs (`sc_filter freq`, spec 2.3), as a
//!   [`Histogram`] that gives the absolute and the relative
//!   [`HistogramView`].
//!
//! Everything reads **only the index**, in one pass over the database
//! (`O(games)`, never `O(bins x games)`, D3374): blocks of entries are read in
//! order and counted in parallel with `rayon`. The counters are integers that
//! are added in block order, so the result is the same with one thread or
//! many. Progress goes to a [`ProgressSink`] and a [`CancelToken`] stops the
//! scan with [`StatsError::Cancelled`] (nothing partial is returned).
//!
//! The dates, ratings and the number of moves are those of the index entry.
//! The current year is never read from the clock here: it is an argument
//! ([`StatsConfig::new`], [`FilterGraphConfig::normalized`]).

mod histogram;
mod table;

use rayon::prelude::*;
use thiserror::Error;

use crate::filter::Filter;
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::IndexEntry;
use crate::source::GameSource;
use mychess_core::GameResult;

pub use histogram::{
    FilterGraphConfig, GraphKind, Histogram, HistogramBin, HistogramView, MAX_GRAPH_BINS,
    MAX_GRAPH_FIELD, ViewPoint, filter_histogram,
};
pub use table::{StatsConfig, StatsLine, filter_stats_table};

/// Games whose index entries are read, in order, before they are counted in
/// parallel.
const INDEX_BLOCK: u32 = 16_384;

/// Entries counted by one `rayon` task.
const TASK_ENTRIES: usize = 2048;

/// Why a statistic could not be computed.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatsError {
    /// The filter does not cover the database (it belongs to another one).
    #[error("the filter has {filter} games but the database has {games}")]
    FilterSize {
        /// Games in the filter.
        filter: u32,
        /// Games in the database.
        games: u32,
    },
    /// The operation was cancelled.
    #[error("cancelled")]
    Cancelled,
}

/// Checks that `filter` covers `db`.
pub(crate) fn check_filter<D: GameSource + ?Sized>(
    db: &D,
    filter: &Filter,
) -> Result<(), StatsError> {
    if filter.len() == db.len() {
        Ok(())
    } else {
        Err(StatsError::FilterSize {
            filter: filter.len(),
            games: db.len(),
        })
    }
}

/// Reads the index of `db` in blocks and folds every entry into an
/// accumulator `A`: `add(&mut acc, game_number, entry)`. Each parallel task
/// starts from `empty()`; the partial accumulators are merged **in game
/// order** with `merge`, so a non-commutative merge (the last game of a month,
/// say) is also deterministic.
///
/// Returns the accumulator and the number of entries that could not be read
/// (they are skipped). Progress is reported once per block.
///
/// # Errors
///
/// [`StatsError::Cancelled`] if the token is set between blocks.
pub(crate) fn fold_index<D, A, E, F, M>(
    db: &D,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
    empty: E,
    add: F,
    merge: M,
) -> Result<(A, u32), StatsError>
where
    D: GameSource + ?Sized,
    A: Send,
    E: Fn() -> A + Sync,
    F: Fn(&mut A, u32, &IndexEntry) + Sync,
    M: Fn(&mut A, A),
{
    let games = db.len();
    let mut total = empty();
    let mut unreadable = 0u32;
    let mut next = 1u32;
    while next <= games {
        if cancel.is_cancelled() {
            return Err(StatsError::Cancelled);
        }
        let end = next.saturating_add(INDEX_BLOCK - 1).min(games);
        let mut entries: Vec<(u32, IndexEntry)> = Vec::new();
        for n in next..=end {
            match db.entry(n) {
                Ok(entry) => entries.push((n, entry)),
                Err(_) => unreadable = unreadable.saturating_add(1),
            }
        }
        let parts: Vec<A> = entries
            .par_chunks(TASK_ENTRIES)
            .map(|chunk| {
                let mut acc = empty();
                for (n, entry) in chunk {
                    add(&mut acc, *n, entry);
                }
                acc
            })
            .collect();
        for part in parts {
            merge(&mut total, part);
        }
        progress.progress(u64::from(end), u64::from(games));
        next = end + 1;
    }
    Ok((total, unreadable))
}

/// The year of an entry; 0 when it is unknown (as `date_GetYear` of Scid).
pub(crate) fn year_of(entry: &IndexEntry) -> u32 {
    u32::from(entry.date.year().unwrap_or(0))
}

/// The year that closes the last interval of the statistics (Scid: 2099).
pub const OPEN_YEAR_LIMIT: u16 = 2099;

/// One criterion of a line of the statistics window (`sc_filter stats`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatsQuery {
    /// Every game of the filter.
    All,
    /// Both players have a rating of at least this value. A game with a
    /// rating 0 (none) does not pass.
    BothRated(u16),
    /// Games with `from <= year < to`; `to: None` is the open end, which
    /// Scid writes as 2099 ([`OPEN_YEAR_LIMIT`]). A game without a year (year
    /// 0) only enters when `from` is 0.
    Years {
        /// First year.
        from: u16,
        /// First year left out, or `None` for the open end.
        to: Option<u16>,
    },
    /// Games with `year >= from` (the "since" lines).
    Since(u16),
}

impl StatsQuery {
    /// Whether the entry of a game in the filter counts for this criterion.
    pub fn matches(self, entry: &IndexEntry) -> bool {
        match self {
            StatsQuery::All => true,
            StatsQuery::BothRated(min) => entry.white_elo >= min && entry.black_elo >= min,
            StatsQuery::Years { from, to } => {
                let year = year_of(entry);
                year >= u32::from(from) && year < u32::from(to.unwrap_or(OPEN_YEAR_LIMIT))
            }
            StatsQuery::Since(from) => year_of(entry) >= u32::from(from),
        }
    }
}

/// The counts of one line of the statistics window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FilterStatsRow {
    /// Games counted.
    pub total: u32,
    /// Wins of White (`1-0`).
    pub white: u32,
    /// Draws.
    pub draw: u32,
    /// Wins of Black (`0-1`).
    pub black: u32,
    /// Games without a result (`*`).
    pub none: u32,
    /// Index entries of the database that could not be read and so were
    /// left out of every count of the pass (the same number in every row of
    /// one call; 0 for a healthy database, D3443).
    pub skipped_entries: u32,
}

impl FilterStatsRow {
    /// Adds a game with `result`.
    pub fn add(&mut self, result: GameResult) {
        self.total = self.total.saturating_add(1);
        let slot = match result {
            GameResult::White => &mut self.white,
            GameResult::Draw => &mut self.draw,
            GameResult::Black => &mut self.black,
            _ => &mut self.none,
        };
        *slot = slot.saturating_add(1);
    }

    /// Adds another row (block merge).
    pub(crate) fn merge(&mut self, other: FilterStatsRow) {
        self.total = self.total.saturating_add(other.total);
        self.white = self.white.saturating_add(other.white);
        self.draw = self.draw.saturating_add(other.draw);
        self.black = self.black.saturating_add(other.black);
        self.none = self.none.saturating_add(other.none);
        self.skipped_entries = self.skipped_entries.saturating_add(other.skipped_entries);
    }

    /// The score in tenths of a per cent: `(2 White + Draw + None) * 500 /
    /// total`, truncated; 0 when there are no games (rule S1). `*` counts as
    /// half a point. `score_tenths() / 10` and `% 10` are the two parts of
    /// the `%3u.%u%%` of Scid.
    pub fn score_tenths(&self) -> u32 {
        crate::tree::score_permille(
            [self.white, self.black, self.draw, self.none],
            u64::from(self.total),
        )
    }
}

/// The lines of several criteria at once, one pass over the index.
/// `rows[i]` answers `queries[i]`. Only the games in `filter` count.
///
/// # Errors
///
/// [`StatsError::FilterSize`] if `filter` does not cover `db`;
/// [`StatsError::Cancelled`] if `cancel` is set.
pub fn filter_stats_rows<D: GameSource + ?Sized>(
    db: &D,
    filter: &Filter,
    queries: &[StatsQuery],
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<Vec<FilterStatsRow>, StatsError> {
    check_filter(db, filter)?;
    let (mut rows, unreadable) = fold_index(
        db,
        progress,
        cancel,
        || vec![FilterStatsRow::default(); queries.len()],
        |rows: &mut Vec<FilterStatsRow>, n, entry| {
            if !filter.contains(n) {
                return;
            }
            for (row, query) in rows.iter_mut().zip(queries) {
                if query.matches(entry) {
                    row.add(entry.result);
                }
            }
        },
        |total, part| {
            for (row, other) in total.iter_mut().zip(part) {
                row.merge(other);
            }
        },
    )?;
    for row in &mut rows {
        row.skipped_entries = unreadable;
    }
    Ok(rows)
}

/// One line of the statistics window (`sc_filter stats`).
///
/// # Errors
///
/// Same as [`filter_stats_rows`].
pub fn filter_stats<D: GameSource + ?Sized>(
    db: &D,
    filter: &Filter,
    query: StatsQuery,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<FilterStatsRow, StatsError> {
    Ok(filter_stats_rows(db, filter, &[query], progress, cancel)?
        .first()
        .copied()
        .unwrap_or_default())
}
