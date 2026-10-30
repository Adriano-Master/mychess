// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_tree_search`,
// `sc_tree_clean`, `sc_tree_move`, `updateMainFilter`), src/tree.h and
// src/game.cpp (`Game::ExactMatch`). Copyright (c) 1999-2002 Shane Hudson;
// Scid vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The opening tree engine (`docs/specs/m5-1-motor-da-arvore.md`, decisions
//! D940 to D959): for a position, the moves played from it in the games of a
//! database, with statistics per move.
//!
//! [`search`] scans the main lines of all the games of a [`GameSource`] and
//! returns a [`TreeResult`] with one [`TreeRow`] per distinct move, a
//! [`TreeTotal`] and, optionally, the new main filter ("adjust"). It produces
//! numbers only; the text of the Scid's tree window is the job of the
//! interface, which can use the rounding helpers of [`TreeRow`] and
//! [`TreeTotal`].
//!
//! Positions match by squares and side to move, ignoring castling rights and
//! en passant (rule 1). The match is the first one in the main line of each
//! game (variations are not searched).
//!
//! The per-database state of the Scid (tree filter, filter copy for
//! "adjust", fast-mode copy) lives in [`TreeState`], which the caller keeps
//! with the database. A search that is cancelled or fails leaves it as it was.
//!
//! Other parts: [`best_games`] (the "best games" list), [`random_move`] and
//! [`move_by_index`] (training), [`performance`] and the [`TreeCache`] hook
//! for the cache file of milestone M5-3.

mod best;
pub mod mask;
mod stats;
pub mod stc;

use std::ops::ControlFlow;
use std::time::{Duration, Instant};

use mychess_core::{Color, Eco, EcoBook, GameResult, Position, Square};
use thiserror::Error;

pub use best::{
    BestGame, BestOptions, BestOrder, MAX_BEST_GAMES, ResultFilter, best_games, move_by_index,
    random_move,
};
pub use stats::performance;
pub(crate) use stats::{PERF_ELODIFF, score_permille};
pub use stc::{
    PrimeError, PrimeReport, STANDARD_LINES, StcCache, StcError, prime_with_filter,
    prime_with_game, prime_with_lines, remove_stc, stc_path,
};

use crate::filter::{Filter, MAX_UNREADABLE};
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, SEARCH_CHECK_EVERY, Ticker};
use crate::si4::{IndexEntry, ListPosition, RawMove, RecordError, Si4Error, walk_mainline};
use crate::source::GameSource;

/// The most distinct moves a tree can have (Scid: `MAX_TREE_NODES`).
pub const MAX_TREE_NODES: usize = 60;

/// How the rows are ordered (`-sort`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Sort {
    /// By SAN text, ascending, byte by byte.
    Alpha,
    /// By ECO code, descending; moves without ECO last.
    Eco,
    /// By number of games, descending.
    #[default]
    Frequency,
    /// By score, descending for White to move and ascending for Black.
    Score,
}

/// How much of the database a search examines (`-fastmode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FastMode {
    /// Examine every game.
    #[default]
    Slow,
    /// Examine only the games found by the last full search, when that copy
    /// is still valid. The result can be imprecise: transpositions into the
    /// position that the full search did not see are missed. The cache is
    /// still read (a hit is served as in a full search) but a fast scan never
    /// writes to it.
    Fast,
    /// A fast pass now, to be followed by a [`FastMode::Slow`] call that the
    /// application makes. For [`search`] it behaves as [`FastMode::Fast`].
    FastThenSlow,
}

impl FastMode {
    /// Whether [`search`] takes the fast path.
    pub fn is_fast(self) -> bool {
        !matches!(self, FastMode::Slow)
    }
}

/// Options of [`search`].
#[derive(Debug, Clone, Copy, Default)]
pub struct TreeOptions<'a> {
    /// The order of the rows.
    pub sort: Sort,
    /// Training: marks the rows as [`TreeRow::hidden`].
    pub hide_moves: bool,
    /// Compute the new main filter ([`TreeResult::adjusted_filter`]).
    pub adjust: bool,
    /// The search mode.
    pub fast: FastMode,
    /// The ECO book for the ECO code of each move (decision D950).
    pub eco_book: Option<&'a EcoBook>,
}

/// What [`search`] is asked about.
#[derive(Debug, Clone, Copy)]
pub struct TreeQuery<'a> {
    /// The position; compared by squares and side to move.
    pub position: &'a Position,
    /// The ply of the position in the current game (`GetCurrentPly`); only
    /// the validity rule of the fast mode uses it (rule 15).
    pub ply: u32,
    /// The current main filter; read only by the first search of a database
    /// (it becomes the filter of the database, rule 16).
    pub main_filter: &'a Filter,
}

/// One move of the tree with the raw counters, the part that is cached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    /// The move; `None` for the end of the game (`[end]`).
    pub mv: Option<RawMove>,
    /// Number of games.
    pub total: u32,
    /// Games by result: White, Black, draw, no result.
    pub freq: [u32; 4],
    /// Sum of the ratings of the side that moves (only ratings above 0).
    pub elo_sum: u64,
    /// How many ratings are in `elo_sum`.
    pub elo_count: u32,
    /// Sum of the ratings of the opponents (only ratings above 0).
    pub perf_sum: u64,
    /// How many ratings are in `perf_sum`.
    pub perf_count: u32,
    /// Sum of the years (only known years).
    pub year_sum: u64,
    /// How many years are in `year_sum`.
    pub year_count: u32,
}

impl TreeNode {
    fn new(mv: Option<RawMove>) -> TreeNode {
        TreeNode {
            mv,
            total: 0,
            freq: [0; 4],
            elo_sum: 0,
            elo_count: 0,
            perf_sum: 0,
            perf_count: 0,
            year_sum: 0,
            year_count: 0,
        }
    }

    /// Counts a game of index entry `entry` where `turn` moves (rule 6).
    fn add(&mut self, entry: &IndexEntry, turn: Color) {
        self.total = self.total.saturating_add(1);
        let slot = match entry.result {
            GameResult::White => 0,
            GameResult::Black => 1,
            GameResult::Draw => 2,
            _ => 3,
        };
        if let Some(f) = self.freq.get_mut(slot) {
            *f = f.saturating_add(1);
        }
        let (elo, opp) = match turn {
            Color::White => (entry.white_elo, entry.black_elo),
            Color::Black => (entry.black_elo, entry.white_elo),
        };
        if elo > 0 {
            self.elo_sum += u64::from(elo);
            self.elo_count = self.elo_count.saturating_add(1);
        }
        if opp > 0 {
            self.perf_sum += u64::from(opp);
            self.perf_count = self.perf_count.saturating_add(1);
        }
        if let Some(year) = entry.date.year().filter(|y| *y != 0) {
            self.year_sum += u64::from(year);
            self.year_count = self.year_count.saturating_add(1);
        }
    }
}

/// A row of the tree: one move with its statistics (rules 5 to 12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    /// The move in SAN, or `[end]` when games end here.
    pub san: String,
    /// The move in UCI; `None` for `[end]`.
    pub uci: Option<String>,
    /// The move as stored; `None` for `[end]`.
    pub mv: Option<RawMove>,
    /// Number of games.
    pub total: u32,
    /// Games by result: White, Black, draw, no result.
    pub freq: [u32; 4],
    /// Expected score for White, per thousand (rule 7).
    pub score_permille: u32,
    /// Average rating of the side that moves; `None` with fewer than 10.
    pub avg_elo: Option<u32>,
    /// Performance rating; `None` with fewer than 10 rated opponents.
    pub perf: Option<u32>,
    /// Average year (rounded); `None` when no game has a year.
    pub avg_year: Option<u32>,
    /// The ECO code of the position after the move (decision D950).
    pub eco: Option<Eco>,
    /// Training mode: the interface hides the SAN and the ECO code.
    pub hidden: bool,
}

impl TreeRow {
    /// Whether this is the `[end]` row (no move).
    pub fn is_end(&self) -> bool {
        self.mv.is_none()
    }

    /// The share of the games as `(percent, tenths)`, both truncated:
    /// `100*total/n` and `(1000*total/n) % 10` (rule 12). `n` is
    /// [`TreeTotal::total`].
    pub fn share_tenths(&self, n: u32) -> (u32, u32) {
        share_tenths(self.total, n)
    }

    /// The score as `(percent, tenths)`: `score/10` and `score%10`.
    pub fn score_tenths(&self) -> (u32, u32) {
        (self.score_permille / 10, self.score_permille % 10)
    }

    /// The draws as a rounded whole percentage.
    pub fn draw_percent(&self) -> u32 {
        draw_percent(self.freq[2], self.total)
    }
}

/// The totals line of the tree (rule 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeTotal {
    /// Number of games in all the rows.
    pub total: u32,
    /// Games by result: White, Black, draw, no result.
    pub freq: [u32; 4],
    /// Expected score for White, per thousand.
    pub score_permille: u32,
    /// Average rating over all rows; `None` with fewer than 10.
    pub avg_elo: Option<u32>,
    /// Performance rating over all rows.
    pub perf: Option<u32>,
    /// Average year over all rows.
    pub avg_year: Option<u32>,
}

impl TreeTotal {
    /// The score as `(percent, tenths)`: `score/10` and `score%10`.
    pub fn score_tenths(&self) -> (u32, u32) {
        (self.score_permille / 10, self.score_permille % 10)
    }

    /// The draws as a rounded whole percentage.
    pub fn draw_percent(&self) -> u32 {
        draw_percent(self.freq[2], self.total)
    }
}

fn share_tenths(total: u32, n: u32) -> (u32, u32) {
    if n == 0 {
        return (0, 0);
    }
    let (total, n) = (u64::from(total), u64::from(n));
    let whole = u32::try_from(100 * total / n).unwrap_or(100);
    let tenth = u32::try_from((1000 * total / n) % 10).unwrap_or(0);
    (whole, tenth)
}

fn draw_percent(draws: u32, total: u32) -> u32 {
    if total == 0 {
        return 0;
    }
    let permille = u64::from(draws) * 1000 / u64::from(total);
    u32::try_from((permille + 5) / 10).unwrap_or(100)
}

/// The result of [`search`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeResult {
    /// The rows, sorted as asked.
    pub rows: Vec<TreeRow>,
    /// The totals; `None` when no game reaches the position.
    pub total: Option<TreeTotal>,
    /// The side to move in the searched position.
    pub turn: Color,
    /// Games whose record was read in this search (0 when the result came
    /// from the cache).
    pub searched: u32,
    /// Time spent.
    pub elapsed: Duration,
    /// The new main filter (main filter AND tree filter, with the ply of the
    /// match), when [`TreeOptions::adjust`] is set.
    pub adjusted_filter: Option<Filter>,
    /// Whether the nodes came from the [`TreeCache`].
    pub from_cache: bool,
    /// Games that could not be read; they do not count. At most
    /// [`MAX_UNREADABLE`] numbers are kept.
    pub unreadable: Vec<u32>,
    /// How many games could not be read.
    pub unreadable_total: u64,
}

/// Errors of the tree engine.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TreeError {
    /// The search was cancelled; filters and state are unchanged.
    #[error("tree search cancelled")]
    Cancelled,
    /// More than [`MAX_TREE_NODES`] distinct moves.
    #[error("too many moves in the tree (more than {MAX_TREE_NODES})")]
    TooManyMoves,
    /// A filter does not cover the games of the database.
    #[error("filter has {filter} games but the database has {games}")]
    FilterSize {
        /// Length of the filter.
        filter: u32,
        /// Number of games of the database.
        games: u32,
    },
    /// [`TreeState::commit_filter`] without a previous search.
    #[error("no tree search to commit the filter of")]
    NothingToCommit,
    /// The position cannot be searched (it does not fit the SI4 piece lists,
    /// for example more than 16 pieces of a side); nothing was changed.
    #[error("the position cannot be searched in the tree")]
    UnsupportedPosition,
    /// A database error.
    #[error(transparent)]
    Source(#[from] Si4Error),
}

/// A tree kept by a [`TreeCache`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedTree {
    /// The nodes, in any order; score, ECO and order are recomputed.
    pub nodes: Vec<TreeNode>,
    /// The tree filter of that search.
    pub filter: Filter,
}

/// Where full searches are remembered (the `.stc` file of milestone M5-3).
///
/// The key is the position by squares and side to move, as in the search.
pub trait TreeCache {
    /// The remembered tree for `pos` on a database of `len` games.
    fn lookup(&mut self, pos: &Position, len: u32) -> Option<CachedTree>;
    /// Remembers the tree of a full search.
    fn store(&mut self, pos: &Position, tree: CachedTree);
}

/// A cache that remembers nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCache;

impl TreeCache for NoCache {
    fn lookup(&mut self, _pos: &Position, _len: u32) -> Option<CachedTree> {
        None
    }

    fn store(&mut self, _pos: &Position, _tree: CachedTree) {}
}

/// The state the tree keeps for one database (decision D952).
///
/// Create it with [`TreeState::default`]; call [`TreeState::invalidate`] when
/// the number or order of the games changes (rule 24).
#[derive(Debug, Clone, Default)]
pub struct TreeState {
    tree_filter: Option<Filter>,
    db_filter: Option<Filter>,
    old_data: Option<(Filter, u32)>,
}

impl TreeState {
    /// The tree filter of the last search: ply+1 of the match, 0 = out.
    pub fn tree_filter(&self) -> Option<&Filter> {
        self.tree_filter.as_ref()
    }

    /// The filter of the database that "adjust" combines with the tree.
    pub fn db_filter(&self) -> Option<&Filter> {
        self.db_filter.as_ref()
    }

    /// Forgets everything.
    pub fn invalidate(&mut self) {
        *self = TreeState::default();
    }

    /// Ends the tree: drops the tree filter and the fast-mode copy and
    /// returns the filter of the database to restore as the main filter
    /// (`sc_tree clean`). `None` if there was no search.
    pub fn clean(&mut self) -> Option<Filter> {
        self.tree_filter = None;
        self.old_data = None;
        self.db_filter.take()
    }

    /// Makes `current` (the main filter, already adjusted) the filter of the
    /// database (`sc_tree clean` followed by `updateFilter`).
    ///
    /// # Errors
    ///
    /// [`TreeError::NothingToCommit`] without a previous search;
    /// [`TreeError::FilterSize`] if `current` does not match the tree filter.
    pub fn commit_filter(&mut self, current: &Filter) -> Result<(), TreeError> {
        let Some(tree) = &self.tree_filter else {
            return Err(TreeError::NothingToCommit);
        };
        if tree.len() != current.len() {
            return Err(TreeError::FilterSize {
                filter: current.len(),
                games: tree.len(),
            });
        }
        self.db_filter = Some(current.clone());
        Ok(())
    }
}

/// Searches the opening tree of `query.position` in `db` (`sc_tree search`).
///
/// Progress goes to `progress` and the token is looked at every 5000 games
/// (and every 100 ms). On `Cancelled` and every other error `state` and the
/// filters are untouched (even a state that was stale is kept as it was).
///
/// When a search is not served by `cache` and is not fast, its tree is stored
/// in `cache` and the filter is kept for the fast mode (rule 15). A search
/// that found unreadable games ([`TreeResult::unreadable_total`] above 0) is
/// partial and is not stored in the cache (it would hide them on the next
/// lookup); the fast-mode copy is still kept.
///
/// # Errors
///
/// [`TreeError::Cancelled`], [`TreeError::TooManyMoves`],
/// [`TreeError::FilterSize`] and [`TreeError::UnsupportedPosition`]. Games that cannot be read are counted in
/// [`TreeResult::unreadable_total`], not reported as errors.
pub fn search<D: GameSource + ?Sized>(
    db: &mut D,
    query: &TreeQuery<'_>,
    opts: &TreeOptions<'_>,
    state: &mut TreeState,
    cache: &mut dyn TreeCache,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<TreeResult, TreeError> {
    let games = db.len();
    if query.main_filter.len() != games {
        return Err(TreeError::FilterSize {
            filter: query.main_filter.len(),
            games,
        });
    }
    let started = Instant::now();
    // A state from a database with another size is stale (rule 24); it is
    // dropped only in the final phase, so a failure leaves it untouched.
    let stale = state.tree_filter.as_ref().is_some_and(|f| f.len() != games)
        || state.db_filter.as_ref().is_some_and(|f| f.len() != games);
    let fast = opts.fast.is_fast();
    let pos = query.position;
    // Going back before the saved copy makes it useless (rule 15).
    let old_valid = state
        .old_data
        .as_ref()
        .filter(|(f, ply)| !stale && *ply <= query.ply && f.len() == games);

    let mut result = TreeResult {
        rows: Vec::new(),
        total: None,
        turn: pos.turn(),
        searched: 0,
        elapsed: Duration::ZERO,
        adjusted_filter: None,
        from_cache: false,
        unreadable: Vec::new(),
        unreadable_total: 0,
    };
    let (nodes, work) = match cache.lookup(pos, games).filter(|c| c.filter.len() == games) {
        Some(cached) => {
            result.from_cache = true;
            (cached.nodes, cached.filter)
        }
        None => scan(
            db,
            pos,
            fast,
            old_valid.map(|(f, _)| f),
            cancel,
            progress,
            &mut result,
        )?,
    };
    progress.progress(u64::from(games), u64::from(games));

    // Everything below cannot fail: the state is updated now.
    let mut rows: Vec<TreeRow> = nodes
        .iter()
        .map(|n| stats::build_row(pos, n, opts.eco_book, opts.hide_moves))
        .collect();
    stats::sort_rows(&mut rows, opts.sort, pos.turn());
    result.total = stats::build_total(&nodes, pos.turn());
    result.rows = rows;

    let had_old = old_valid.is_some();
    if stale {
        state.invalidate();
    }
    if state.db_filter.is_none() {
        state.db_filter = Some(query.main_filter.clone());
    }
    if !result.from_cache && !fast && result.unreadable_total == 0 {
        cache.store(
            pos,
            CachedTree {
                nodes,
                filter: work.clone(),
            },
        );
    }
    if (!result.from_cache && !fast) || (result.from_cache && fast) {
        state.old_data = Some((work.clone(), query.ply));
    } else if !had_old {
        state.old_data = None;
    }
    if opts.adjust {
        result.adjusted_filter = state
            .db_filter
            .as_ref()
            .map(|db_filter| adjust(db_filter, &work));
    }
    state.tree_filter = Some(work);
    result.elapsed = started.elapsed();
    Ok(result)
}

/// `main[i] = tree[i]` where both filters have the game, else 0 (rule 18;
/// Scid: `updateMainFilter`).
fn adjust(db_filter: &Filter, tree: &Filter) -> Filter {
    let mut out = Filter::new(tree.len());
    out.fill(0);
    for n in tree.iter_games() {
        if db_filter.contains(n) {
            out.set(n, tree.get(n));
        }
    }
    out
}

/// The scan of rules 3 to 6: reads the games and fills the nodes and the tree
/// filter. Works on local data so a cancel leaves no trace.
fn scan<D: GameSource + ?Sized>(
    db: &mut D,
    pos: &Position,
    fast: bool,
    old: Option<&Filter>,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
    result: &mut TreeResult,
) -> Result<(Vec<TreeNode>, Filter), TreeError> {
    let games = db.len();
    let mut work = Filter::new(games);
    work.fill(0);
    let mut nodes: Vec<TreeNode> = Vec::new();
    let target = ListPosition::from_fen(&pos.fen()).map_err(|_| TreeError::UnsupportedPosition)?;
    let standard = ListPosition::standard();
    let target_is_standard = same_board(&target, &standard);
    let turn = pos.turn();
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    if cancel.is_cancelled() {
        return Err(TreeError::Cancelled);
    }

    for n in 1..=games {
        if ticker.tick() {
            progress.progress(u64::from(n - 1), u64::from(games));
            if cancel.is_cancelled() {
                return Err(TreeError::Cancelled);
            }
        }
        if fast && old.is_some_and(|f| f.get(n) == 0) {
            continue;
        }
        let Ok(entry) = db.entry(n) else {
            note_unreadable(result, n);
            continue;
        };
        // Games without data, and games without moves unless the position is
        // the standard start (rule 3). Deleted games count (D954).
        if entry.length == 0 || (entry.num_half_moves == 0 && !target_is_standard) {
            continue;
        }
        let Ok(record) = db.record_bytes(n) else {
            note_unreadable(result, n);
            continue;
        };
        result.searched += 1;
        // Only the main line is decoded, move by move, and the walk stops at
        // the match: damage after it (variations, comments) does not exclude
        // the game (Scid: `Game::ExactMatch`).
        let Ok(found) = first_match(record, &target) else {
            note_unreadable(result, n);
            continue;
        };
        let Some((index, mv)) = found else {
            continue;
        };
        // Filter value: ply + 1, limited to 255 (rule 4, D948).
        let value = u8::try_from(index + 1).unwrap_or(u8::MAX);
        work.set(n, value);
        let slot = if let Some(slot) = nodes.iter().position(|node| node.mv == mv) {
            slot
        } else {
            if nodes.len() >= MAX_TREE_NODES {
                return Err(TreeError::TooManyMoves);
            }
            nodes.push(TreeNode::new(mv));
            nodes.len() - 1
        };
        if let Some(node) = nodes.get_mut(slot) {
            node.add(&entry, turn);
        }
    }
    Ok((nodes, work))
}

fn note_unreadable(result: &mut TreeResult, n: u32) {
    result.unreadable_total += 1;
    if result.unreadable.len() < MAX_UNREADABLE {
        result.unreadable.push(n);
    }
}

/// Whether two positions have the same pieces on the same squares and the
/// same side to move. `ListPosition`'s own equality also compares the order
/// of its piece lists, which differs between transpositions; castling rights
/// and en passant are not part of it either (rule 1).
pub(crate) fn same_board(a: &ListPosition, b: &ListPosition) -> bool {
    a.turn() == b.turn()
        && Square::ALL
            .iter()
            .all(|&sq| a.piece_at(sq) == b.piece_at(sq))
}

/// Plays the main line of `record` until `target` is reached. Returns the
/// number of half moves before the match and the next move (`None` = end of
/// the game); `Err` for a record whose moves cannot be replayed before any
/// match (decision D955(g)).
fn first_match(
    record: &[u8],
    target: &ListPosition,
) -> Result<Option<(usize, Option<RawMove>)>, RecordError> {
    let mut ply = 0usize;
    walk_mainline(record, |current, next| {
        if same_board(current, target) {
            return ControlFlow::Break((ply, next));
        }
        ply += 1;
        ControlFlow::Continue(())
    })
}

#[cfg(test)]
mod tests;
