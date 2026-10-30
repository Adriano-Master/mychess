//! The scan of a database for a [`Pattern`] (spec M6-2, slice b; decisions
//! D3253, D3258, D3272, D3285, D3291, D3294, D3296).
//!
//! [`search_pattern`] reuses the block-parallel scan of the advanced searches
//! ([`scan_games`]): the pattern is a [`GameMatcher`] whose `prefilter` folds
//! the `Result` conditions with the result stored in the index entry (so a
//! game that cannot match is never read) and whose `evaluate` rebuilds the
//! main line of the record and applies [`match_line`].
//!
//! The filter keeps, for every matching game, the ply of the first matching
//! position plus one (1 when that ply is 254 or more, as the overflow of the
//! Scid); the exact ply is recomputed for one game by [`hits_in_game`], which
//! also returns the highlights to draw.
//!
//! ## Determinism (D3296)
//!
//! `evaluate` is pure and the mate solver is deterministic with a per
//! position node budget, so the filter and every counter of the report are
//! the same with one or many threads (the counters are sums, which do not
//! depend on the order). There is deliberately **no wall-clock limit**: a
//! time cap would make the answer depend on the machine. The cost of the
//! `available` mate mode is bounded by [`ScanLimits::mate_nodes_cap`] and the
//! caller should ask the user first when [`estimate_cost`] says so.

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use mychess_core::{Game, GameResult, Move, Position, tactics};

use super::eval::{Frame, Highlight, Line, MAX_FRAMES, fold_constants, highlights_at};
use super::eval::{match_line, matching_plies};
use super::model::{Condition, MateMode, Pattern, PatternError, ResultKind};
use crate::filter::{Filter, FilterOp};
use crate::progress::{CancelToken, ProgressSink};
use crate::search::{GameMatcher, SearchError, Shortcut, scan_games};
use crate::si4::{IndexEntry, RawMove, RecordError, start_fen_of_record, walk_mainline};
use crate::source::GameSource;

/// Estimated positions per game used by [`estimate_cost`] (D3285).
pub const POSITIONS_PER_GAME: u64 = 60;

/// Above this many estimated positions an `available` mate search should be
/// confirmed by the user (D3285).
pub const CONFIRM_POSITIONS: u64 = 2_000_000;

/// Largest ply for which the filter value is `ply + 1`; a later match gets 1.
const LAST_DIRECT_PLY: u16 = 253;

/// Limits of a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    /// Most positions (initial one included) kept for a game; a longer game
    /// is cut and the pattern sees only the part kept. At most
    /// [`MAX_FRAMES`].
    pub max_frames: usize,
    /// Ceiling for the node budget of every `mate` condition: a pattern that
    /// asks for more is searched with this budget (at least 1000, the
    /// smallest budget a pattern may ask for).
    pub mate_nodes_cap: u32,
}

impl Default for ScanLimits {
    fn default() -> ScanLimits {
        ScanLimits {
            max_frames: MAX_FRAMES,
            mate_nodes_cap: 2_000_000,
        }
    }
}

/// Why a pattern search could not run.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ScanError {
    /// The pattern is not valid; nothing was scanned.
    #[error(transparent)]
    Pattern(#[from] PatternError),
    /// The scan itself could not start (for example the filter does not
    /// cover the database).
    #[error(transparent)]
    Search(#[from] SearchError),
}

/// The outcome of [`search_pattern`] (spec "Varredura").
///
/// New fields will be added over time; read the ones you need and do not
/// build it with a struct literal.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PatternReport {
    /// Games in the filter after the search (the old filter's count if the
    /// scan was interrupted).
    pub matched: u32,
    /// The "before" count: the filter's count for AND, the number of games
    /// otherwise.
    pub start_count: u32,
    /// Time spent.
    pub elapsed: Duration,
    /// The scan was cancelled; the filter was not changed.
    pub interrupted: bool,
    /// Numbers of the first games that could not be read (at most
    /// [`MAX_UNREADABLE`](crate::MAX_UNREADABLE)); a game whose main line
    /// is damaged or illegal counts here and does not match.
    pub unreadable: Vec<u32>,
    /// How many games could not be read in total.
    pub unreadable_total: u64,
    /// Positions evaluated (both orientations counted with `ignore_colors`).
    pub positions_examined: u64,
    /// Positions where an `Unknown` (mate search out of budget) decided the
    /// answer: they neither match nor exclude.
    pub unknown_positions: u64,
    /// Games skipped without being read because the `Result` conditions
    /// cannot hold for the result in the index (D3258).
    pub games_folded_out: u32,
    /// The pattern asks for `available` mate, the expensive mode.
    pub uses_available_mate: bool,
}

/// What a scan is expected to cost (D3285), so that the interface can ask
/// before an expensive search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanCost {
    /// The pattern has a `mate` condition in `available` mode.
    pub uses_available_mate: bool,
    /// Estimated positions to evaluate (games times
    /// [`POSITIONS_PER_GAME`]); 0 when the pattern has no `available` mate.
    pub estimated_positions: u64,
    /// The interface should show the estimate and ask for confirmation
    /// (`estimated_positions` above [`CONFIRM_POSITIONS`]).
    pub needs_confirmation: bool,
}

fn has_available_mate(cond: &Condition) -> bool {
    match cond {
        Condition::Mate(m) => m.mode == MateMode::Available,
        Condition::All { items } | Condition::Any { items } => items.iter().any(has_available_mate),
        Condition::Not { item } => has_available_mate(item),
        _ => false,
    }
}

/// Estimates the cost of scanning `games_in_scope` games with `pattern`.
pub fn estimate_cost(pattern: &Pattern, games_in_scope: u32) -> ScanCost {
    let uses_available_mate = has_available_mate(&pattern.root);
    let estimated_positions = if uses_available_mate {
        u64::from(games_in_scope) * POSITIONS_PER_GAME
    } else {
        0
    };
    ScanCost {
        uses_available_mate,
        estimated_positions,
        needs_confirmation: estimated_positions > CONFIRM_POSITIONS,
    }
}

/// The condition with every mate budget limited to `cap`.
fn capped(cond: &Condition, cap: u32) -> Condition {
    match cond {
        Condition::All { items } => Condition::All {
            items: items.iter().map(|c| capped(c, cap)).collect(),
        },
        Condition::Any { items } => Condition::Any {
            items: items.iter().map(|c| capped(c, cap)).collect(),
        },
        Condition::Not { item } => Condition::Not {
            item: Box::new(capped(item, cap)),
        },
        Condition::Mate(m) => {
            let mut m = m.clone();
            m.max_nodes = m.max_nodes.min(cap);
            Condition::Mate(m)
        }
        other => other.clone(),
    }
}

fn result_kind(result: GameResult) -> ResultKind {
    match result {
        GameResult::White => ResultKind::WhiteWins,
        GameResult::Black => ResultKind::BlackWins,
        GameResult::Draw => ResultKind::Draw,
        _ => ResultKind::Unknown,
    }
}

fn balance_of(pos: &Position) -> i16 {
    i16::try_from(tactics::material_balance(pos)).unwrap_or(0)
}

/// The filter value for a match at `ply` (D3253): `ply + 1`, or 1 from ply
/// 254 on (the overflow of the Scid).
fn filter_value(ply: u16) -> u8 {
    if ply <= LAST_DIRECT_PLY {
        u8::try_from(ply + 1).unwrap_or(1)
    } else {
        1
    }
}

fn start_position(record: &[u8]) -> Result<Position, RecordError> {
    match start_fen_of_record(record)? {
        None => Ok(Position::new()),
        Some(fen) => Position::from_fen(&String::from_utf8_lossy(&fen))
            .map_err(|_| RecordError::new(0, "start position is not usable")),
    }
}

fn to_core(pos: &Position, mv: RawMove) -> Result<Move, RecordError> {
    if mv.is_null() {
        return Ok(Move::Null);
    }
    pos.find_move(mv.from, mv.to, mv.promotion)
        .ok_or(RecordError::new(0, "move is not legal"))
}

/// Why the walk over the main line stopped early.
enum Stop {
    Full,
    Bad(RecordError),
}

/// The main line of a raw record as a [`Line`], cut at `max_frames`.
/// `plies_hint` (the half-move count of the index entry) only sizes the
/// buffer.
///
/// # Errors
///
/// A [`RecordError`] when the start position or a move of the main line
/// cannot be read (the game counts as unreadable, D3299 item 11).
fn line_from_record(
    record: &[u8],
    result: ResultKind,
    max_frames: usize,
    plies_hint: usize,
) -> Result<Line, RecordError> {
    let limit = max_frames.clamp(1, MAX_FRAMES);
    let start = start_position(record)?;
    let mut frames = Vec::with_capacity(plies_hint.saturating_add(1).min(limit));
    frames.push(Frame {
        balance: balance_of(&start),
        pos: start,
        mv: None,
    });
    let walked = walk_mainline(record, |_, next| {
        let Some(raw) = next else {
            return ControlFlow::Continue(());
        };
        if frames.len() >= limit {
            return ControlFlow::Break(Stop::Full);
        }
        // The last frame is the current position: no second copy is kept.
        let step = frames
            .last()
            .ok_or(RecordError::new(0, "empty line"))
            .and_then(|last| {
                let mv = to_core(&last.pos, raw)?;
                last.pos
                    .after(mv)
                    .map(|p| (mv, p))
                    .map_err(|_| RecordError::new(0, "move is not legal"))
            });
        match step {
            Ok((mv, next_pos)) => {
                frames.push(Frame {
                    balance: balance_of(&next_pos),
                    pos: next_pos,
                    mv: Some(mv),
                });
                ControlFlow::Continue(())
            }
            Err(e) => ControlFlow::Break(Stop::Bad(e)),
        }
    })?;
    let truncated = match walked {
        Some(Stop::Bad(e)) => return Err(e),
        Some(Stop::Full) => true,
        None => false,
    };
    Ok(Line::new(frames, result, truncated))
}

/// The pattern as a per-game test of the shared scan.
struct PatternMatcher<'a> {
    pattern: Pattern,
    cancel: &'a CancelToken,
    max_frames: usize,
    examined: AtomicU64,
    unknown: AtomicU64,
    folded: AtomicU64,
}

impl GameMatcher for PatternMatcher<'_> {
    fn prefilter(&self, entry: &IndexEntry) -> Shortcut {
        let scope = &self.pattern.scope;
        let result = result_kind(entry.result);
        let dead = fold_constants(&self.pattern.root, result) == Some(false)
            && (!scope.ignore_colors
                || fold_constants(&self.pattern.root, result.swapped()) == Some(false));
        if dead {
            self.folded.fetch_add(1, Ordering::Relaxed);
            Shortcut::Skip
        } else {
            Shortcut::Examine
        }
    }

    fn evaluate(&self, entry: &IndexEntry, record: &[u8]) -> Result<Option<u8>, RecordError> {
        let hint = usize::from(entry.num_half_moves);
        let line = line_from_record(record, result_kind(entry.result), self.max_frames, hint)?;
        // `line_from_record` fails on a damaged line instead of flagging it.
        debug_assert!(!line.broken);
        let found = match_line(&self.pattern, &line, Some(self.cancel));
        self.examined
            .fetch_add(u64::from(found.examined), Ordering::Relaxed);
        self.unknown
            .fetch_add(u64::from(found.unknown_positions), Ordering::Relaxed);
        Ok(found.ply.map(filter_value))
    }
}

/// Searches the main lines of `db` for `pattern` and combines the result
/// with `filter` as `op` says (`Reset`, `And`, `Or`, as in the other
/// searches). See [`search_pattern_with`] for the limits.
///
/// The value stored in the filter for a matching game is the ply of the first
/// matching position plus one (1 if the ply is 254 or more); a game that is
/// examined and does not match gets 0. Cancelling leaves `filter` as it was
/// and sets [`PatternReport::interrupted`]. The result does not depend on the
/// number of threads.
///
/// # Errors
///
/// [`ScanError::Pattern`] if the pattern does not validate,
/// [`ScanError::Search`] if `filter` does not cover the database.
pub fn search_pattern<D: GameSource + ?Sized>(
    db: &mut D,
    pattern: &Pattern,
    op: FilterOp,
    filter: &mut Filter,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<PatternReport, ScanError> {
    search_pattern_with(
        db,
        pattern,
        op,
        filter,
        &ScanLimits::default(),
        progress,
        cancel,
    )
}

/// Like [`search_pattern`] with explicit [`ScanLimits`].
///
/// # Errors
///
/// See [`search_pattern`].
pub fn search_pattern_with<D: GameSource + ?Sized>(
    db: &mut D,
    pattern: &Pattern,
    op: FilterOp,
    filter: &mut Filter,
    limits: &ScanLimits,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<PatternReport, ScanError> {
    pattern.validate()?;
    let run = Pattern {
        name: pattern.name.clone(),
        description: pattern.description.clone(),
        scope: pattern.scope.clone(),
        root: capped(&pattern.root, limits.mate_nodes_cap.max(1_000)),
    };
    let matcher = PatternMatcher {
        pattern: run,
        cancel,
        max_frames: limits.max_frames,
        examined: AtomicU64::new(0),
        unknown: AtomicU64::new(0),
        folded: AtomicU64::new(0),
    };
    let report = scan_games(db, &matcher, op, filter, progress, cancel)?;
    Ok(PatternReport {
        matched: report.matched,
        start_count: report.start_count,
        elapsed: report.elapsed,
        interrupted: report.interrupted,
        unreadable: report.unreadable,
        unreadable_total: report.unreadable_total,
        positions_examined: matcher.examined.load(Ordering::Relaxed),
        unknown_positions: matcher.unknown.load(Ordering::Relaxed),
        games_folded_out: u32::try_from(matcher.folded.load(Ordering::Relaxed)).unwrap_or(u32::MAX),
        uses_available_mate: has_available_mate(&pattern.root),
    })
}

/// One occurrence of a pattern in a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Plies played before the first position of the occurrence (0 = the
    /// initial position; the board shows the position after `ply` moves of
    /// the main line).
    pub ply: u16,
    /// What to draw on the board at that position (D3275).
    pub highlights: Vec<Highlight>,
}

/// Recomputes the occurrences of `pattern` in the main line of `game`
/// (D3284/D3285 of the spec): the first position of every run of `hold`
/// consecutive matching positions, in ply order, each with its highlights.
/// Used after loading a game of the filter and by "test on this game". It
/// never changes the game and does not look at the filter.
///
/// # Errors
///
/// The validation error if `pattern` is not valid.
///
/// The game is read up to [`MAX_FRAMES`] positions; use
/// [`hits_in_game_with`] to apply the same `max_frames` as a scan.
pub fn hits_in_game(pattern: &Pattern, game: &Game) -> Result<Vec<Hit>, PatternError> {
    hits_in_game_with(pattern, game, &ScanLimits::default())
}

/// [`hits_in_game`] with the `max_frames` of `limits` (clamped to
/// `1..=`[`MAX_FRAMES`]), so that a hit is recomputed over the same part of
/// the game that [`search_pattern_with`] saw. `mate_nodes_cap` is applied
/// too, for the same reason.
///
/// # Errors
///
/// The validation error if `pattern` is not valid.
pub fn hits_in_game_with(
    pattern: &Pattern,
    game: &Game,
    limits: &ScanLimits,
) -> Result<Vec<Hit>, PatternError> {
    pattern.validate()?;
    let line = Line::from_game(game, limits.max_frames.clamp(1, MAX_FRAMES));
    let run = Pattern {
        name: pattern.name.clone(),
        description: pattern.description.clone(),
        scope: pattern.scope.clone(),
        root: capped(&pattern.root, limits.mate_nodes_cap.max(1_000)),
    };
    Ok(hits_in_line(&run, &line))
}

/// [`hits_in_game`] on a line already built.
pub fn hits_in_line(pattern: &Pattern, line: &Line) -> Vec<Hit> {
    matching_plies(pattern, line, None)
        .into_iter()
        .map(|ply| Hit {
            ply,
            highlights: highlights_at(pattern, line, usize::from(ply)),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::model::{Scope, Side};

    fn pattern(root: Condition) -> Pattern {
        Pattern {
            name: "t".into(),
            description: String::new(),
            scope: Scope::default(),
            root,
        }
    }

    #[test]
    fn filter_value_overflows_to_one() {
        assert_eq!(filter_value(0), 1);
        assert_eq!(filter_value(1), 2);
        assert_eq!(filter_value(253), 254);
        assert_eq!(filter_value(254), 1);
        assert_eq!(filter_value(u16::MAX), 1);
    }

    #[test]
    fn cost_only_counts_available_mate() {
        let quiet = pattern(Condition::SideToMove { side: Side::White });
        assert!(!estimate_cost(&quiet, 10_000_000).needs_confirmation);
        let mate = Condition::Mate(super::super::model::MateCond {
            by: super::super::model::Who::White,
            moves_min: 1,
            moves_max: 1,
            mode: MateMode::Available,
            forcing_only: true,
            max_nodes: 20_000,
        });
        let p = pattern(Condition::Not {
            item: Box::new(mate),
        });
        let small = estimate_cost(&p, 1000);
        assert!(small.uses_available_mate && !small.needs_confirmation);
        assert_eq!(small.estimated_positions, 60_000);
        assert!(estimate_cost(&p, 40_000).needs_confirmation);
    }

    #[test]
    fn cap_limits_every_mate_budget() {
        let mate = Condition::Mate(super::super::model::MateCond {
            by: super::super::model::Who::White,
            moves_min: 1,
            moves_max: 1,
            mode: MateMode::Available,
            forcing_only: true,
            max_nodes: 500_000,
        });
        let c = capped(&Condition::All { items: vec![mate] }, 10_000);
        let Condition::All { items } = c else {
            panic!("shape")
        };
        assert!(matches!(&items[0], Condition::Mate(m) if m.max_nodes == 10_000));
    }

    #[test]
    fn line_from_record_rejects_garbage() {
        assert!(line_from_record(&[], ResultKind::Unknown, 10, 0).is_err());
    }
}
