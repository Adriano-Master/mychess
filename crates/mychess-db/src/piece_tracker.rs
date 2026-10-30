// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_base_piecetrack`) and
// tcl/tools/ptracker.tcl. Copyright (c) 1999-2004 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License (see
// its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The piece tracker (milestone M6, slice 4c; spec 2.5, rules P1 to P13 and
//! decisions D3385 to D3390).
//!
//! [`piece_track`] follows one or more pieces, given by their **starting**
//! squares, through the main line of every game of the filter, and counts for
//! each square either in how many games a followed piece moved there
//! ([`TrackMode::Games`]) or how long a followed piece stayed there
//! ([`TrackMode::Time`]). The result is plain data: [`PieceTrackResult`] with
//! the counts, [`PieceTrackResult::grid`] for the heat map,
//! [`PieceTrackResult::ranking`] for the list of the 64 squares and
//! [`ColorScheme`] for the colours. Nothing here depends on a toolkit.
//!
//! ## How the games are read
//!
//! The records are read in order in blocks of [`TRACK_BLOCK_GAMES`] games
//! (reading needs `&mut`), followed in parallel with `rayon` and added in
//! game order; the counters are integers, so the result is the same with one
//! thread or many. Games that cannot be followed are not an error
//! (rule P3): a game with its own start position (`START`), without a record,
//! with fewer half-moves than the first counted ply, or with a damaged record
//! only counts in [`PieceTrackResult::games_skipped`]. Variations are
//! ignored.
//!
//! ## Differences from Scid
//!
//! - Castling moves the king **and** the rook, and *en passant* removes the
//!   captured pawn (D3390). Scid treats castling as the rook's move when the
//!   rook is followed and leaves the pawn captured *en passant* "ghosting" on
//!   its square. For any selection the window allows (one piece, or a pair
//!   or the pawns of one colour) the result is the same.
//! - The main line is walked with the list position of the SI4 decoder
//!   (`ListPosition`), which validates each move, instead of a second 64-square
//!   board kept here; a record that makes an impossible move is a skipped
//!   game, where Scid stops following it and keeps what it had counted.
//! - A null move is a ply and moves nothing (Scid takes a "capture" of the
//!   square the king stands on).
//! - Cancelling returns the **partial** result with
//!   [`PieceTrackResult::interrupted`] set (rule P12); the games read so far
//!   are all counted.
//!
//! Time mode adds one per ply and halves the total at the end, as Scid does
//! (rule P6): [`PieceTrackResult::raw`] keeps the sums before the halving.

mod colors;
mod track;

use mychess_core::Square;
use rayon::prelude::*;

use crate::filter::Filter;
use crate::filter_stats::{StatsError, check_filter};
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, SEARCH_CHECK_EVERY, Ticker};
use crate::si4::GameFlags;
use crate::source::GameSource;

pub use colors::ColorScheme;
pub use track::piece_track_plies;

/// Games read before a block is followed in parallel (D3388).
pub const TRACK_BLOCK_GAMES: usize = 100;

/// What the counts mean (`-games` or `-time` of `sc_base piecetrack`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TrackMode {
    /// In how many games a followed piece moved to the square.
    #[default]
    Games,
    /// How many moves a followed piece spent on the square.
    Time,
}

/// What to follow and for which moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceTrackQuery {
    /// The **starting** squares of the pieces to follow (`d1` for the queen
    /// of White). Repeated squares count once.
    pub squares: Vec<Square>,
    /// First move counted, 0 to 999 (move 0 is read as 1). The ply is that
    /// of White's move.
    pub start_move: u16,
    /// Last move counted (the ply of Black's move); below `start_move` it is
    /// read as `start_move`.
    pub end_move: u16,
    /// Games or time.
    pub mode: TrackMode,
}

impl PieceTrackQuery {
    /// The last move after the window's rule (`end >= start`).
    pub fn effective_end(&self) -> u16 {
        self.end_move.max(self.start_move)
    }
}

/// The answer of [`piece_track`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceTrackResult {
    /// The count of each square (a1 = 0 to h8 = 63); in time mode already
    /// halved.
    pub counts: [u32; 64],
    /// The sums before the halving of time mode (equal to `counts` in games
    /// mode).
    pub raw: [u64; 64],
    /// What the counts mean.
    pub mode: TrackMode,
    /// First move of the window as given.
    pub start_move: u16,
    /// Last move of the window, never below `start_move`.
    pub end_move: u16,
    /// Games in the filter: the base of the percentages (rule P13), also for
    /// the games that were skipped.
    pub games: u32,
    /// Games that were followed.
    pub games_used: u32,
    /// Games of the filter that were not followed (rule P3).
    pub games_skipped: u32,
    /// The scan was cancelled: the counts are partial.
    pub interrupted: bool,
}

/// A square of the ranking of [`PieceTrackResult::ranking`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankedSquare {
    /// The square.
    pub square: Square,
    /// Its count.
    pub count: u32,
    /// Its share, in per cent with two decimals (rule P9).
    pub percent: f64,
}

/// Heat map data (rule P9): the value of each square as a fraction of the
/// largest one.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackerGrid {
    /// `count / max` of each square, 0 to 1 (a1 = 0 to h8 = 63).
    pub squares: [f32; 64],
    /// The largest count, at least 1 (so an empty map is all zeros).
    pub max: u32,
    /// Games in the filter.
    pub games: u32,
    counts: [u32; 64],
}

impl TrackerGrid {
    /// The colour of a square (`x = count * 100 / max`, rule P10). An index
    /// above 63 gives the colour of 0.
    pub fn rgb(&self, scheme: ColorScheme, square: usize) -> [u8; 3] {
        let count = self.counts.get(square).copied().unwrap_or(0);
        scheme.rgb_percent(f64::from(count) * 100.0 / f64::from(self.max))
    }
}

impl PieceTrackResult {
    fn empty(query: &PieceTrackQuery, games: u32) -> PieceTrackResult {
        PieceTrackResult {
            counts: [0; 64],
            raw: [0; 64],
            mode: query.mode,
            start_move: query.start_move,
            end_move: query.effective_end(),
            games,
            games_used: 0,
            games_skipped: 0,
            interrupted: false,
        }
    }

    /// The divisor of the percentages (rule P9): the games of the filter, or
    /// in time mode that times `end + 1 - start`; at least 1.
    pub fn divisor(&self) -> u64 {
        let games = u64::from(self.games);
        let divisor = match self.mode {
            TrackMode::Games => games,
            TrackMode::Time => {
                games
                    * (u64::from(self.end_move) + 1 - u64::from(self.start_move.min(self.end_move)))
            }
        };
        divisor.max(1)
    }

    /// The share of a square in per cent, `round(count * 10000 / divisor) /
    /// 100` (rule P9).
    #[allow(clippy::cast_precision_loss)]
    pub fn percent(&self, square: Square) -> f64 {
        let count = self.counts.get(square.to_usize()).copied().unwrap_or(0);
        (f64::from(count) * 10_000.0 / self.divisor() as f64).round() / 100.0
    }

    /// The data of the heat map.
    #[allow(clippy::cast_possible_truncation)]
    pub fn grid(&self) -> TrackerGrid {
        let max = self.counts.iter().copied().max().unwrap_or(0).max(1);
        let mut squares = [0.0f32; 64];
        for (slot, &count) in squares.iter_mut().zip(&self.counts) {
            *slot = (f64::from(count) / f64::from(max)) as f32;
        }
        TrackerGrid {
            squares,
            max,
            games: self.games,
            counts: self.counts,
        }
    }

    /// The 64 squares by decreasing count; squares with the same count keep
    /// the order a1 to h8 (rule P11).
    pub fn ranking(&self) -> Vec<RankedSquare> {
        let mut ranking: Vec<RankedSquare> = (0..64u32)
            .map(Square::new)
            .map(|square| RankedSquare {
                square,
                count: self.counts.get(square.to_usize()).copied().unwrap_or(0),
                percent: self.percent(square),
            })
            .collect();
        ranking.sort_by_key(|r| std::cmp::Reverse(r.count));
        ranking
    }
}

/// Follows the pieces of `query` through the filter games of `db`.
///
/// With no square, an empty filter or an empty database the result is 64
/// zeros (rule P2). If `cancel` is set the scan stops and the partial counts
/// come back with `interrupted` (rule P12).
///
/// # Errors
///
/// [`StatsError::FilterSize`] if `filter` does not cover `db`. Cancellation is
/// not an error here.
pub fn piece_track<D: GameSource + ?Sized>(
    db: &mut D,
    filter: &Filter,
    query: &PieceTrackQuery,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<PieceTrackResult, StatsError> {
    piece_track_with(
        db,
        filter,
        query,
        progress,
        cancel,
        TRACK_BLOCK_GAMES,
        SEARCH_CHECK_EVERY,
    )
}

/// A game read and waiting for its block.
struct Job {
    start: usize,
    end: usize,
}

/// [`piece_track`] with the size of the block and the progress interval as
/// parameters, so tests can cancel in the middle of a small database.
pub(crate) fn piece_track_with<D: GameSource + ?Sized>(
    db: &mut D,
    filter: &Filter,
    query: &PieceTrackQuery,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
    block_games: usize,
    check_every: u32,
) -> Result<PieceTrackResult, StatsError> {
    check_filter(&*db, filter)?;
    let mut result = PieceTrackResult::empty(query, filter.count());
    let mask = query
        .squares
        .iter()
        .fold(0u64, |mask, &square| mask | track::bit(square));
    if mask == 0 || filter.count() == 0 {
        return Ok(result);
    }
    if cancel.is_cancelled() {
        result.interrupted = true;
        return Ok(result);
    }
    let (first, last) = piece_track_plies(query.start_move, query.end_move);
    let mode = query.mode;
    let total = u64::from(filter.count());
    let mut ticker = Ticker::new(check_every, CHECK_INTERVAL);
    let mut buffer: Vec<u8> = Vec::new();
    let mut jobs: Vec<Job> = Vec::new();
    let mut raw = [0u64; 64];
    let mut seen = 0u64;

    // Follows the games waiting in `jobs` and adds them in order.
    let mut flush = |buffer: &mut Vec<u8>, jobs: &mut Vec<Job>, result: &mut PieceTrackResult| {
        let parts: Vec<Option<[u64; 64]>> = jobs
            .par_iter()
            .map(|job| {
                let record = buffer.get(job.start..job.end)?;
                let mut counts = [0u64; 64];
                track::track_game(record, first, last, mode, mask, &mut counts)
                    .ok()
                    .map(|()| counts)
            })
            .collect();
        for part in parts {
            if let Some(counts) = part {
                result.games_used = result.games_used.saturating_add(1);
                for (sum, add) in raw.iter_mut().zip(counts) {
                    *sum += add;
                }
            } else {
                result.games_skipped = result.games_skipped.saturating_add(1);
            }
        }
        buffer.clear();
        jobs.clear();
    };

    for n in filter.iter_games() {
        seen += 1;
        if ticker.tick() {
            progress.progress(seen - 1, total);
            if cancel.is_cancelled() {
                result.interrupted = true;
                break;
            }
        }
        let Ok(entry) = db.entry(n) else {
            result.games_skipped = result.games_skipped.saturating_add(1);
            continue;
        };
        if entry.flags.contains(GameFlags::START)
            || entry.length == 0
            || u32::from(entry.num_half_moves) < first
        {
            result.games_skipped = result.games_skipped.saturating_add(1);
            continue;
        }
        let Ok(record) = db.record_bytes(n) else {
            result.games_skipped = result.games_skipped.saturating_add(1);
            continue;
        };
        let start = buffer.len();
        buffer.extend_from_slice(record);
        jobs.push(Job {
            start,
            end: buffer.len(),
        });
        if jobs.len() >= block_games.max(1) {
            flush(&mut buffer, &mut jobs, &mut result);
            if cancel.is_cancelled() {
                result.interrupted = true;
                break;
            }
        }
    }
    flush(&mut buffer, &mut jobs, &mut result);
    if !result.interrupted {
        progress.progress(total, total);
    }
    result.raw = raw;
    for (count, &sum) in result.counts.iter_mut().zip(&raw) {
        let value = if mode == TrackMode::Time {
            sum / 2
        } else {
            sum
        };
        *count = u32::try_from(value).unwrap_or(u32::MAX);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
