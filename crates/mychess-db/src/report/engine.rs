// Scid: src/tkscid.cpp `sc_report_create` 14514-14611 (the pass over the
// filter), src/optable.cpp `OpLine::Init` 83-182, `OpTable::Add` 676-721,
// `AddMoveOrder` 1944-2000, `AddEndMaterial` and `setupRatios` of
// tcl/tools/optable.tcl 527-566. Copyright (c) 2001-2003 Shane Hudson;
// Scid vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The generation of the shared part of a report (decisions D3601, D3602,
//! D3609 and D3625): one block-parallel pass over the games of the filter,
//! one pass over the index of the whole database, and the sequential
//! decoding of the games of the theory table.
//!
//! The result does not depend on the number of threads: the blocks are
//! cut by game number and merged in order, and every tie has a fixed rule.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::fmt::Write as _;
use std::ops::ControlFlow;

use mychess_core::{Color, GameResult, Move, PartialDate, Position, SanSuffix};
use rayon::prelude::*;

use super::ReportError;
use super::model::{
    DatabaseTotals, EndTheme, MoveOrder, NUM_THEMES, Popularity, RANGE_COUNT, RANGES, ReportCore,
    ReportLine, THEME_PLIES, THEME_THRESHOLD, YearTrend,
};
use super::options::ReportOptions;
use super::table;
use super::themes::{add_positional_themes, end_theme};
use crate::crosstable::ResultCounts;
use crate::filter::Filter;
use crate::filter_stats::{FilterStatsRow, StatsLine, StatsQuery, fold_index};
use crate::player_stats::PlayerStats;
use crate::progress::{CancelToken, ProgressSink};
use crate::search::{BLOCK_BYTES, BLOCK_GAMES};
use crate::si4::{
    GameFlags, IndexEntry, NameKind, RawMove, bytes_to_string, mainline_of_record,
    start_fen_of_record, walk_mainline,
};
use crate::source::GameSource;

/// What the shared generation needs (the opening report fills it from the
/// position, the player report of milestone M7-3 from the player).
#[derive(Debug, Clone, Copy)]
pub struct CoreInput<'a> {
    /// The games of the report. The value of each game is the ply of the
    /// reference position in it, plus one (the tree filter).
    pub filter: &'a Filter,
    /// The side to move in the reference position.
    pub turn: Color,
    /// With Black to move, the last move of the stem in SAN (without `-` and
    /// `=`): the first cell of every line. `None` leaves it empty.
    pub stem_last: Option<&'a str>,
    /// The move of the position left out of the theory table.
    pub exclude: Option<RawMove>,
    /// The options; they are normalised here.
    pub options: &'a ReportOptions,
    /// Estimated ratings, for the average rating of a game.
    pub player_stats: &'a PlayerStats,
    /// The lines of the statistics block; only the "all games", "both rated"
    /// and "since" ones are shown.
    pub stats_queries: &'a [StatsQuery],
    /// Today, for the popularity of the last years (injected, D3608).
    pub today: PartialDate,
}

/// A candidate line: what is kept of a game before the strings are made.
#[derive(Debug, Clone, Copy)]
struct Cand {
    game: u32,
    ply: u16,
    avg_elo: u16,
    white_id: u32,
    black_id: u32,
    site_id: u32,
    white_elo: u16,
    black_elo: u16,
    result: GameResult,
    half_moves: u16,
    date: PartialDate,
    eco: u16,
    themes: [u8; NUM_THEMES],
    end_theme: EndTheme,
    order: u32,
}

/// Heap order: the worst candidate (lowest rating, then highest number) is
/// the greatest, so it is the one dropped when the heap is full.
struct Worst(Cand);

impl PartialEq for Worst {
    fn eq(&self, other: &Worst) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Worst {}
impl PartialOrd for Worst {
    fn partial_cmp(&self, other: &Worst) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Worst {
    fn cmp(&self, other: &Worst) -> Ordering {
        other
            .0
            .avg_elo
            .cmp(&self.0.avg_elo)
            .then(self.0.game.cmp(&other.0.game))
    }
}

/// A way of reaching the position: the start and the moves.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OrderKey {
    fen: Option<Vec<u8>>,
    moves: Vec<RawMove>,
}

/// A game read for a block.
struct Item {
    game: u32,
    ply: u16,
    entry: IndexEntry,
    start: usize,
    end: usize,
}

/// What the parallel step learns of a game.
struct Fact {
    has_more: bool,
    next: Option<RawMove>,
    themes: [u8; NUM_THEMES],
    /// Index in the chunk's keys; only when `has_more`.
    order_local: usize,
}

/// The move orders met by a chunk, found by the moves without allocating.
#[derive(Default)]
struct LocalOrders {
    scratch: Vec<RawMove>,
    plain: HashMap<Vec<RawMove>, usize>,
    started: HashMap<(Vec<u8>, Vec<RawMove>), usize>,
    keys: Vec<OrderKey>,
}

impl LocalOrders {
    fn intern(&mut self, fen: Option<Vec<u8>>) -> usize {
        match fen {
            None => {
                if let Some(&i) = self.plain.get(self.scratch.as_slice()) {
                    return i;
                }
                let i = self.keys.len();
                self.keys.push(OrderKey {
                    fen: None,
                    moves: self.scratch.clone(),
                });
                self.plain.insert(self.scratch.clone(), i);
                i
            }
            Some(fen) => {
                let key = (fen, self.scratch.clone());
                if let Some(&i) = self.started.get(&key) {
                    return i;
                }
                let i = self.keys.len();
                self.keys.push(OrderKey {
                    fen: Some(key.0.clone()),
                    moves: key.1.clone(),
                });
                self.started.insert(key, i);
                i
            }
        }
    }
}

/// Walks the main line of one game: the themes of the first 20 moves, the
/// moves up to the position and the move played from it (Scid:
/// `OpLine::Init` and `AddMoveOrder`).
fn analyze(
    record: &[u8],
    entry: &IndexEntry,
    ply: usize,
    orders: &mut LocalOrders,
) -> Option<Fact> {
    orders.scratch.clear();
    let mut themes = [0u8; NUM_THEMES];
    let mut visits = 0usize;
    let mut found: Option<(bool, Option<RawMove>)> = None;
    let theme_limit = usize::try_from(THEME_PLIES).unwrap_or(40);
    let stop = ply.max(theme_limit);
    let walked = walk_mainline(record, |pos, next| {
        if (1..=theme_limit).contains(&visits) {
            add_positional_themes(pos, &mut themes);
        }
        if visits < ply {
            if let Some(mv) = next {
                orders.scratch.push(mv);
            }
        } else if visits == ply {
            found = Some((next.is_some(), next));
        }
        if visits >= stop || next.is_none() {
            return ControlFlow::Break(());
        }
        visits += 1;
        ControlFlow::Continue(())
    });
    walked.ok()?;
    let (has_more, next) = found?;
    let order_local = if has_more {
        let fen = if entry.flags.contains(GameFlags::START) {
            Some(start_fen_of_record(record).ok()??)
        } else {
            None
        };
        orders.intern(fen)
    } else {
        0
    };
    Some(Fact {
        has_more,
        next,
        themes,
        order_local,
    })
}

/// The results of a chunk, in the order of its items.
struct ChunkOut {
    facts: Vec<Option<Fact>>,
    keys: Vec<OrderKey>,
}

fn process_chunk(items: &[Item], arena: &[u8]) -> ChunkOut {
    let mut orders = LocalOrders::default();
    let facts = items
        .iter()
        .map(|it| {
            let record = arena.get(it.start..it.end)?;
            analyze(record, &it.entry, usize::from(it.ply), &mut orders)
        })
        .collect();
    ChunkOut {
        facts,
        keys: orders.keys,
    }
}

fn slot(result: GameResult) -> usize {
    match result {
        GameResult::White => 0,
        GameResult::Black => 1,
        GameResult::Draw => 2,
        _ => 3,
    }
}

fn add_result(counts: &mut ResultCounts, result: GameResult) {
    let field = match result {
        GameResult::White => &mut counts.white,
        GameResult::Black => &mut counts.black,
        GameResult::Draw => &mut counts.draws,
        _ => &mut counts.unknown,
    };
    *field = field.saturating_add(1);
}

/// The average rating that ranks a game: a missing rating is the estimate of
/// the player (Scid: `Game::GetAverageElo`), else 0.
pub(crate) fn average_elo(entry: &IndexEntry, stats: &PlayerStats) -> u16 {
    let side = |elo: u16, id: u32| if elo == 0 { stats.max_elo(id) } else { elo };
    let (w, b) = (
        side(entry.white_elo, entry.white_id),
        side(entry.black_elo, entry.black_id),
    );
    u16::midpoint(w, b)
}

/// The state of pass A, built game by game in game order.
struct Collector {
    cap: usize,
    exclude: Option<RawMove>,
    stats_rows: Vec<FilterStatsRow>,
    report_filter: Filter,
    heap: BinaryHeap<Worst>,
    total: ResultCounts,
    theory: ResultCounts,
    themes: [u32; NUM_THEMES],
    endgame_report: [u32; 8],
    order_ids: HashMap<OrderKey, u32>,
    order_keys: Vec<OrderKey>,
    order_counts: Vec<u32>,
    skipped: u32,
}

impl Collector {
    fn order_id(&mut self, key: &OrderKey) -> u32 {
        if let Some(&id) = self.order_ids.get(key) {
            return id;
        }
        if self.order_keys.len() >= self.cap {
            return 0;
        }
        self.order_keys.push(key.clone());
        self.order_counts.push(0);
        let id = u32::try_from(self.order_keys.len()).unwrap_or(u32::MAX);
        self.order_ids.insert(key.clone(), id);
        id
    }

    fn add(
        &mut self,
        item: &Item,
        fact: &Fact,
        chunk_keys: &[OrderKey],
        input: &CoreInput<'_>,
        local_ids: &mut HashMap<usize, u32>,
    ) {
        if !fact.has_more {
            return;
        }
        let entry = &item.entry;
        self.report_filter.set(item.game, 1);
        for (row, query) in self.stats_rows.iter_mut().zip(input.stats_queries) {
            if query.matches(entry) {
                row.add(entry.result);
            }
        }
        add_result(&mut self.total, entry.result);
        for (count, &n) in self.themes.iter_mut().zip(&fact.themes) {
            if n >= THEME_THRESHOLD {
                *count = count.saturating_add(1);
            }
        }
        let theme = end_theme(entry.final_mat_sig);
        let c = &mut self.endgame_report[theme.index()];
        *c = c.saturating_add(1);

        let order = *local_ids.entry(fact.order_local).or_insert_with(|| {
            chunk_keys
                .get(fact.order_local)
                .map_or(0, |key| self.order_id(key))
        });
        if order != 0
            && let Some(n) = usize::try_from(order - 1)
                .ok()
                .and_then(|i| self.order_counts.get_mut(i))
        {
            *n = n.saturating_add(1);
        }
        if self.exclude.is_some() && fact.next == self.exclude {
            return;
        }
        add_result(&mut self.theory, entry.result);
        self.heap.push(Worst(Cand {
            game: item.game,
            ply: item.ply,
            avg_elo: average_elo(entry, input.player_stats),
            white_id: entry.white_id,
            black_id: entry.black_id,
            site_id: entry.site_id,
            white_elo: entry.white_elo,
            black_elo: entry.black_elo,
            result: entry.result,
            half_moves: entry.num_half_moves,
            date: entry.date,
            eco: entry.eco_code,
            themes: fact.themes,
            end_theme: theme,
            order,
        }));
        if self.heap.len() > self.cap {
            self.heap.pop();
        }
    }
}

/// Progress shifted into a larger range, so two passes report one monotonic
/// progress.
pub(super) struct Shifted<'a> {
    pub inner: &'a mut dyn ProgressSink,
    pub offset: u64,
    pub total: u64,
}

impl ProgressSink for Shifted<'_> {
    fn progress(&mut self, done: u64, _total: u64) {
        self.inner
            .progress(self.offset.saturating_add(done).min(self.total), self.total);
    }
}

/// Reads the next block of games of the filter.
fn read_block<D: GameSource + ?Sized>(
    db: &mut D,
    games: &mut impl Iterator<Item = u32>,
    filter: &Filter,
    skipped: &mut u32,
) -> (Vec<Item>, Vec<u8>) {
    let mut items: Vec<Item> = Vec::new();
    let mut arena: Vec<u8> = Vec::new();
    for n in games.by_ref() {
        let ply = u16::from(filter.ply(n));
        let Ok(entry) = db.entry(n) else {
            *skipped = skipped.saturating_add(1);
            continue;
        };
        let Ok(record) = db.record_bytes(n) else {
            *skipped = skipped.saturating_add(1);
            continue;
        };
        let start = arena.len();
        arena.extend_from_slice(record);
        items.push(Item {
            game: n,
            ply,
            entry,
            start,
            end: arena.len(),
        });
        if items.len() >= BLOCK_GAMES || arena.len() >= BLOCK_BYTES {
            break;
        }
    }
    (items, arena)
}

/// The numbered SAN text of a move order.
fn order_text(key: &OrderKey) -> String {
    let mut pos = match &key.fen {
        Some(fen) => match Position::from_fen(&bytes_to_string(fen)) {
            Ok(p) => p,
            Err(_) => return String::new(),
        },
        None => Position::new(),
    };
    let mut out = String::new();
    for (i, raw) in key.moves.iter().enumerate() {
        let Some(mv) = core_move(&pos, *raw) else {
            break;
        };
        let Ok(san) = pos.san_with(mv, SanSuffix::CheckOrMate) else {
            break;
        };
        if i > 0 {
            out.push(' ');
        }
        if i == 0 || pos.turn() == Color::White {
            let dots = if pos.turn() == Color::White {
                "."
            } else {
                "..."
            };
            let _ = write!(out, "{}{dots}", pos.fullmove_number());
        }
        out.push_str(&san);
        if pos.play(mv).is_err() {
            break;
        }
    }
    out
}

/// A raw move of the index as a move of `pos`.
pub(crate) fn core_move(pos: &Position, raw: RawMove) -> Option<Move> {
    if raw.is_null() {
        Some(Move::Null)
    } else {
        pos.find_move(raw.from, raw.to, raw.promotion)
    }
}

fn strip_marks(san: &str) -> String {
    san.chars().filter(|c| *c != '-' && *c != '=').collect()
}

/// Fills the SAN columns of a line of the theory table. `false` when the
/// game cannot be replayed.
fn fill_moves<D: GameSource + ?Sized>(
    db: &mut D,
    line: &mut ReportLine,
    input: &CoreInput<'_>,
    max_moves: usize,
) -> bool {
    let Ok(record) = db.record_bytes(line.game) else {
        return false;
    };
    let (Ok(raws), Ok(start)) = (mainline_of_record(record), start_fen_of_record(record)) else {
        return false;
    };
    let mut pos = match start {
        Some(fen) => match Position::from_fen(&bytes_to_string(&fen)) {
            Ok(p) => p,
            Err(_) => return false,
        },
        None => Position::new(),
    };
    let ply = usize::from(line.start_ply);
    for raw in raws.iter().take(ply) {
        match core_move(&pos, *raw) {
            Some(mv) if pos.play(mv).is_ok() => {}
            _ => return false,
        }
    }
    let mut moves: Vec<String> = Vec::new();
    let black_to_move = input.turn == Color::Black;
    if black_to_move {
        moves.push(input.stem_last.unwrap_or("").to_owned());
    }
    let mut taken = 0usize;
    for raw in raws.iter().skip(ply) {
        if moves.len() >= max_moves {
            break;
        }
        let Some(mv) = core_move(&pos, *raw) else {
            return false;
        };
        let Ok(san) = pos.san_with(mv, SanSuffix::CheckOrMate) else {
            return false;
        };
        moves.push(strip_marks(&san));
        taken += 1;
        if pos.play(mv).is_err() {
            return false;
        }
    }
    line.short_game = ply + taken >= raws.len();
    line.moves = moves;
    true
}

fn lines_from<D: GameSource + ?Sized>(db: &D, cands: &[Cand]) -> Vec<ReportLine> {
    let name = |kind: NameKind, id: u32| {
        db.names()
            .get(kind, id)
            .map(|b| bytes_to_string(b).into_owned())
            .unwrap_or_default()
    };
    cands
        .iter()
        .map(|c| ReportLine {
            game: c.game,
            white_id: c.white_id,
            black_id: c.black_id,
            white: name(NameKind::Player, c.white_id),
            black: name(NameKind::Player, c.black_id),
            white_elo: c.white_elo,
            black_elo: c.black_elo,
            avg_elo: c.avg_elo,
            result: c.result,
            half_moves: c.half_moves,
            date: c.date,
            site: name(NameKind::Site, c.site_id),
            eco: mychess_core::Eco::from_scid_code(c.eco),
            start_ply: c.ply,
            moves: Vec::new(),
            short_game: false,
            themes: c.themes,
            end_theme: c.end_theme,
            move_order: c.order,
            note: 0,
        })
        .collect()
}

/// The accumulator of the pass over the index of the whole database.
#[derive(Default)]
struct IndexAcc {
    results: ResultCounts,
    length_sum: [u64; 4],
    length_count: [u32; 4],
    endgames: [u32; 8],
    in_filter: [u32; BUCKETS],
    in_db: [u32; BUCKETS],
}

/// Popularity buckets: the 9 ranges, all dates, then the last 10, 5 and 1
/// years.
const BUCKETS: usize = RANGE_COUNT + 4;
const ALL_BUCKET: usize = RANGE_COUNT;

fn encode_date(year: u32, month: u32, day: u32) -> u32 {
    (year << 9) | (month << 5) | day
}

/// `(first, last)` encoded dates of each popularity bucket.
fn bucket_bounds(today: PartialDate) -> [(u32, u32); BUCKETS] {
    let latest = encode_date(u32::from(mychess_core::MAX_YEAR), 12, 31);
    let mut out = [(0, latest); BUCKETS];
    for (i, (from, to)) in RANGES.iter().enumerate() {
        out[i] = (
            encode_date(u32::from(*from), 0, 0),
            encode_date(u32::from(*to), 12, 31),
        );
    }
    out[ALL_BUCKET] = (0, latest);
    let (y, m, d) = (
        u32::from(today.year().unwrap_or(0)),
        u32::from(today.month().unwrap_or(0)),
        u32::from(today.day().unwrap_or(0)),
    );
    for (k, years) in [10u32, 5, 1].into_iter().enumerate() {
        out[ALL_BUCKET + 1 + k] = (encode_date(y.saturating_sub(years), m, d), latest);
    }
    out
}

fn popularity(acc: &IndexAcc) -> Popularity {
    let ratio = |i: usize| {
        let (f, a) = (acc.in_filter[i], acc.in_db[i]);
        a.checked_div(f).unwrap_or(0)
    };
    let ratio_all = ratio(ALL_BUCKET);
    let mut ranges = [None; RANGE_COUNT];
    for (i, r) in ranges.iter_mut().enumerate() {
        *r = (acc.in_filter[i] != 0).then(|| ratio(i));
    }
    let mut recent = [10u8, 5, 1].map(|years| YearTrend {
        years,
        ratio: 0,
        delta: 0,
    });
    for (k, t) in recent.iter_mut().enumerate() {
        t.ratio = ratio(ALL_BUCKET + 1 + k);
        t.delta = if t.ratio == 0 {
            0
        } else {
            // int((all / y - 1) * 100 + 0.5), truncated toward zero, in
            // exact integers.
            let (a, y) = (i64::from(ratio_all), i64::from(t.ratio));
            i32::try_from(((a - y) * 200 + y) / (2 * y)).unwrap_or(0)
        };
    }
    Popularity {
        ratio_all,
        ranges,
        recent,
    }
}

/// Generates the shared part of a report.
///
/// # Errors
///
/// [`ReportError::FilterSize`] when the filter does not cover the database.
/// A cancelled run is not an error: the result has
/// [`ReportCore::interrupted`] set.
// The three passes read best in one place.
#[allow(clippy::too_many_lines)]
pub fn build_report_core<D: GameSource + ?Sized>(
    db: &mut D,
    input: &CoreInput<'_>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<ReportCore, ReportError> {
    let games = db.len();
    if input.filter.len() != games {
        return Err(ReportError::FilterSize {
            filter: input.filter.len(),
            games,
        });
    }
    let opts = input.options.normalized();
    let cap = usize::try_from(opts.max_games).unwrap_or(usize::MAX);
    let work_total = u64::from(games) + u64::from(input.filter.count());
    let mut col = Collector {
        cap,
        exclude: input.exclude,
        stats_rows: vec![FilterStatsRow::default(); input.stats_queries.len()],
        report_filter: {
            let mut f = Filter::new(games);
            f.fill(0);
            f
        },
        heap: BinaryHeap::new(),
        total: ResultCounts::default(),
        theory: ResultCounts::default(),
        themes: [0; NUM_THEMES],
        endgame_report: [0; 8],
        order_ids: HashMap::new(),
        order_keys: Vec::new(),
        order_counts: Vec::new(),
        skipped: 0,
    };
    let mut interrupted = false;

    // Pass A: the games of the filter.
    let mut pending = input.filter.iter_games();
    let mut done = 0u64;
    loop {
        if cancel.is_cancelled() {
            interrupted = true;
            break;
        }
        let (items, arena) = read_block(db, &mut pending, input.filter, &mut col.skipped);
        if items.is_empty() {
            break;
        }
        let outs: Vec<ChunkOut> = items
            .par_chunks(256)
            .map(|chunk| process_chunk(chunk, &arena))
            .collect();
        for (chunk, out) in items.chunks(256).zip(&outs) {
            let mut local_ids: HashMap<usize, u32> = HashMap::new();
            for (item, fact) in chunk.iter().zip(&out.facts) {
                match fact {
                    Some(fact) => col.add(item, fact, &out.keys, input, &mut local_ids),
                    None => col.skipped = col.skipped.saturating_add(1),
                }
            }
        }
        done += items.len() as u64;
        progress.progress(done.min(work_total), work_total);
    }

    // The lines kept, in game order.
    let mut kept: Vec<Cand> = col.heap.into_vec().into_iter().map(|w| w.0).collect();
    kept.sort_by_key(|c| c.game);
    let mut lines = lines_from(db, &kept);

    // Move orders, most frequent first.
    let mut move_orders: Vec<MoveOrder> = col
        .order_keys
        .iter()
        .zip(&col.order_counts)
        .enumerate()
        .map(|(i, (key, &count))| MoveOrder {
            id: u32::try_from(i + 1).unwrap_or(u32::MAX),
            moves: order_text(key),
            count,
        })
        .collect();
    move_orders.sort_by(|a, b| b.count.cmp(&a.count).then(a.moves.cmp(&b.moves)));

    // The theory table: the highest-rated lines, decoded one by one.
    let (mut order, mut rows, mut notes) = (Vec::new(), Vec::new(), Vec::new());
    let table_cap = usize::try_from(opts.max_table_games).unwrap_or(usize::MAX);
    if table_cap > 0 && !lines.is_empty() && !cancel.is_cancelled() {
        let mut ranking: Vec<usize> = (0..lines.len()).collect();
        ranking.sort_by(|&a, &b| {
            lines[b]
                .avg_elo
                .cmp(&lines[a].avg_elo)
                .then(lines[a].game.cmp(&lines[b].game))
        });
        ranking.truncate(table_cap);
        let max_moves = 2 * (super::model::COLUMNS + usize::from(opts.extra_moves));
        let mut chosen = Vec::with_capacity(ranking.len());
        for i in ranking {
            if cancel.is_cancelled() {
                interrupted = true;
                break;
            }
            if fill_moves(db, &mut lines[i], input, max_moves) {
                chosen.push(i);
            } else {
                col.skipped = col.skipped.saturating_add(1);
            }
        }
        let target = if opts.table_rows > 0 {
            usize::from(opts.table_rows)
        } else {
            table::guess_rows(chosen.len())
        };
        let built = table::build(&lines, &chosen, target);
        for (line, &n) in lines.iter_mut().zip(&built.note_of) {
            line.note = n;
        }
        order = built.order;
        rows = built.rows;
        notes = built.notes;
    }

    // Pass B: the index of the whole database.
    let bounds = bucket_bounds(input.today);
    let mut database = DatabaseTotals {
        games,
        ..DatabaseTotals::default()
    };
    let mut endgames_all = [0u32; 8];
    let mut pop = IndexAcc::default();
    if !interrupted && !cancel.is_cancelled() {
        let report_filter = &col.report_filter;
        let mut shifted = Shifted {
            inner: progress,
            offset: done,
            total: work_total,
        };
        let pass = fold_index(
            &*db,
            &mut shifted,
            cancel,
            IndexAcc::default,
            |acc: &mut IndexAcc, n, entry| {
                add_result(&mut acc.results, entry.result);
                let i = slot(entry.result);
                acc.length_sum[i] += u64::from(entry.num_half_moves).div_ceil(2);
                acc.length_count[i] = acc.length_count[i].saturating_add(1);
                let t = end_theme(entry.final_mat_sig).index();
                acc.endgames[t] = acc.endgames[t].saturating_add(1);
                let date = entry.date.to_scid_u32();
                let in_report = report_filter.contains(n);
                for (b, (from, to)) in bounds.iter().enumerate() {
                    if date >= *from && date <= *to {
                        acc.in_db[b] += 1;
                        if in_report {
                            acc.in_filter[b] += 1;
                        }
                    }
                }
            },
            |total, part| {
                total.results.white += part.results.white;
                total.results.black += part.results.black;
                total.results.draws += part.results.draws;
                total.results.unknown += part.results.unknown;
                for i in 0..4 {
                    total.length_sum[i] += part.length_sum[i];
                    total.length_count[i] += part.length_count[i];
                }
                for i in 0..8 {
                    total.endgames[i] += part.endgames[i];
                }
                for i in 0..BUCKETS {
                    total.in_filter[i] += part.in_filter[i];
                    total.in_db[i] += part.in_db[i];
                }
            },
        );
        match pass {
            Ok((acc, _unreadable)) => {
                database.results = acc.results;
                database.length_sum = acc.length_sum;
                database.length_count = acc.length_count;
                endgames_all = acc.endgames;
                pop = acc;
            }
            Err(_) => interrupted = true,
        }
    } else {
        interrupted = true;
    }
    progress.progress(work_total, work_total);

    let stats = input
        .stats_queries
        .iter()
        .zip(&col.stats_rows)
        .map(|(&query, &row)| StatsLine { query, row })
        .collect();
    Ok(ReportCore {
        total: col.total,
        theory: col.theory,
        lines,
        table_order: order,
        rows,
        notes,
        stats,
        popularity: popularity(&pop),
        move_orders,
        themes: col.themes,
        endgames: super::model::EndgameCounts {
            report: col.endgame_report,
            all: endgames_all,
        },
        database,
        skipped: col.skipped,
        interrupted,
    })
}
