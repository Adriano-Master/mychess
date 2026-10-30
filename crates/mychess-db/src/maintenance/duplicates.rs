// Derived from Scid vs PC 4.27, src/tkscid.cpp (`hashName` 2263,
// `gamesHaveSameMoves` 2277-2337, `checkDuplicate` 2339-2418,
// `sc_base_duplicates` 2420-2671) and tcl/file/maint.tcl (`markTwins`
// 511-683, `doMarkDups` 722-757). Copyright (c) 1999-2004 Shane Hudson;
// Scid vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Twin games (decisions D3511 and D3524 and rules 16 to 22 of
//! `docs/specs/m7-1-manutencao.md`).
//!
//! The algorithm is the one of the Scid (`checkDuplicate` and
//! `gamesHaveSameMoves`) with these differences, all on purpose:
//!
//! - the order is deterministic: games are grouped by the unordered pair of
//!   players and, inside a group, the pairs `(i, j)` with `i < j` are visited
//!   in ascending order of game number. With "shorter", a tie deletes the game
//!   with the **lower** number (rule 17). A group is first split by the values
//!   that every enabled criterion needs equal (site, round, year...), which
//!   cannot change the result and keeps a big group from being cut;
//! - "first four letters" compares four Unicode characters, case-sensitive
//!   like the Scid's byte hash but without collisions (rule 16);
//! - the "place-holder" rule needs both games to have a White name and an
//!   event other than `?` (rule 18);
//! - the home-pawn signature is not used: it only speeds up the Scid and the
//!   result cannot depend on it (rule 19);
//! - [`find_duplicates`] does not write: it returns the games to delete, and
//!   the application sets the flag with `Si4Writer::set_flags_batch` (and the
//!   ratings with `Si4Writer::set_elo_batch`).
//!
//! The records are read in order and decoded in parallel with `rayon`; the
//! result does not depend on the number of threads.

use std::collections::HashMap;
use std::ops::ControlFlow;

use mychess_core::{Eco, GameResult, PartialDate};
use rayon::prelude::*;

use super::{MaintError, Scope};
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, SEARCH_CHECK_EVERY, Ticker};
use crate::si4::{NameKind, RawMove, SideRating, bytes_to_string};
use crate::source::GameSource;

/// The moves compared, at most (`MAX_SAME_MOVES`): 60 moves each side.
const MAX_SAME_MOVES: usize = 120;

/// Fewest half moves of a game when "skip short games" is on.
const SHORT_GAME_HALF_MOVES: u16 = 10;

/// Most pairs examined inside one group of games that can be twins of each
/// other (same players and the same value of every criterion that must be
/// equal, so a larger player group is first split by those values). A larger
/// group is cut here (see [`DuplicateReport::groups_truncated`]) so a database
/// full of the same two names, site, round and month cannot make the scan
/// quadratic.
pub const MAX_PAIRS_PER_GROUP: u64 = 100_000;

/// Records read before the parallel decoding of a chunk.
const CHUNK: usize = 256;

/// Which game of a pair is deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DeleteWhich {
    /// The one with fewer half moves (a tie deletes the lower number).
    #[default]
    Shorter,
    /// The one with the lower game number (`DELETE_OLDER`).
    LowerNumber,
    /// The one with the higher game number (`DELETE_NEWER`).
    HigherNumber,
}

/// The criteria of the "Delete twin games" dialog. [`Default`] holds the values
/// of `start.tcl` 773-791 (decision D3524 aside: undeleting first is the
/// application's option and is off by default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateCriteria {
    /// Player names compared whole; off: the first four characters.
    pub exact_names: bool,
    /// White and Black must be the same; off: the colors may be swapped.
    pub same_colors: bool,
    /// Same event.
    pub same_event: bool,
    /// Same site.
    pub same_site: bool,
    /// Same round.
    pub same_round: bool,
    /// Same year.
    pub same_year: bool,
    /// Same month.
    pub same_month: bool,
    /// Same day.
    pub same_day: bool,
    /// Same result.
    pub same_result: bool,
    /// Same basic ECO code (three characters).
    pub same_eco: bool,
    /// The shorter game's moves are the start of the other's.
    pub same_moves: bool,
    /// Ignore games with fewer than 10 half moves.
    pub skip_short: bool,
    /// Never delete a game that has comments.
    pub keep_commented: bool,
    /// Never delete a game that has variations.
    pub keep_with_variations: bool,
    /// The game that stays and has no Elo takes the Elo of the deleted one.
    pub copy_ratings: bool,
    /// Which game of a pair is deleted.
    pub delete: DeleteWhich,
}

impl Default for DuplicateCriteria {
    fn default() -> DuplicateCriteria {
        DuplicateCriteria {
            exact_names: true,
            same_colors: true,
            same_event: false,
            same_site: true,
            same_round: true,
            same_year: true,
            same_month: true,
            same_day: false,
            same_result: false,
            same_eco: false,
            same_moves: true,
            skip_short: true,
            keep_commented: true,
            keep_with_variations: true,
            copy_ratings: false,
            delete: DeleteWhich::Shorter,
        }
    }
}

/// Two games found to be twins, `first < second` (base 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TwinPair {
    /// The lower game number.
    pub first: u32,
    /// The higher game number.
    pub second: u32,
}

/// Counters of a run (rule 22).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DuplicateReport {
    /// Candidate games examined (not deleted, in scope, long enough).
    pub examined: u32,
    /// Pairs found (both games still unmarked when the pair was examined).
    pub pairs_found: u32,
    /// Games to delete.
    pub marked: u32,
    /// Pairs whose game to delete was kept by the comment or variation
    /// protection.
    pub protected: u32,
    /// Games undeleted before the run: filled by the application, which does
    /// that write (option "undelete all first"); always 0 here.
    pub undeleted_before: u32,
    /// Groups cut at [`MAX_PAIRS_PER_GROUP`] pairs: some pairs of these groups
    /// were not examined.
    pub groups_truncated: u32,
    /// Entries that could not be read from the index and were skipped.
    pub unreadable_entries: u32,
    /// The run was cancelled: the result covers the groups done.
    pub interrupted: bool,
}

/// The result of [`find_duplicates`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DuplicateScan {
    /// The pairs found, in ascending order. The twin checker walks them.
    pub pairs: Vec<TwinPair>,
    /// Games to delete, ascending, without repeats.
    pub to_delete: Vec<u32>,
    /// With `copy_ratings`: the kept games that take an Elo, ready for
    /// `Si4Writer::set_elo_batch`, ascending by game.
    pub rating_copies: Vec<(u32, SideRating, SideRating)>,
    /// The counters.
    pub report: DuplicateReport,
}

/// A game that can be a twin: only the fields the decision uses.
struct Cand {
    number: u32,
    /// White and Black as tokens that are equal exactly when the players
    /// match under the criteria.
    white: u32,
    black: u32,
    white_known: bool,
    event_known: bool,
    event_id: u32,
    site_id: u32,
    round_id: u32,
    date: PartialDate,
    result: GameResult,
    eco_basic: Option<Eco>,
    half_moves: u16,
    comments: u8,
    variations: u8,
    ratings: [(u16, u8); 2],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Link {
    No,
    Yes,
    NeedMoves,
}

fn is_known(name: Option<&[u8]>) -> bool {
    !matches!(name, None | Some(b"" | b"?"))
}

/// The token of the first four characters of a name (rule 16): equal exactly
/// when the prefixes are equal. A prefix already seen costs no allocation.
fn prefix4_token(name: Option<&[u8]>, interner: &mut HashMap<String, u32>) -> u32 {
    let text = name.map(bytes_to_string);
    let text = text.as_deref().unwrap_or("");
    let end = text.char_indices().nth(4).map_or(text.len(), |(at, _)| at);
    let prefix = text.get(..end).unwrap_or(text);
    if let Some(&token) = interner.get(prefix) {
        return token;
    }
    let token = u32::try_from(interner.len()).unwrap_or(u32::MAX);
    interner.insert(prefix.to_owned(), token);
    token
}

/// The values two games must share to be twins, for the criteria that are on
/// (every one of them makes [`link`] answer `No` when it differs). Games of
/// one player group with different keys can never be a pair, so the quadratic
/// walk runs inside each key only.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SubKey {
    ints: [u32; 8],
    eco: Option<Eco>,
}

fn sub_key(crit: &DuplicateCriteria, c: &Cand) -> SubKey {
    let plain = |on: bool, value: u32| if on { value } else { 0 };
    let part = |on: bool, value: Option<u32>| if on { value.map_or(0, |v| v + 1) } else { 0 };
    let result = match c.result {
        GameResult::White => 1,
        GameResult::Black => 2,
        GameResult::Draw => 3,
        _ => 0,
    };
    SubKey {
        ints: [
            plain(crit.same_colors, c.white),
            plain(crit.same_event, c.event_id),
            plain(crit.same_site, c.site_id),
            plain(crit.same_round, c.round_id),
            part(crit.same_year, c.date.year().map(u32::from)),
            part(crit.same_month, c.date.month().map(u32::from)),
            part(crit.same_day, c.date.day().map(u32::from)),
            plain(crit.same_result, result),
        ],
        eco: if crit.same_eco { c.eco_basic } else { None },
    }
}

/// The part of `checkDuplicate` that needs only the index entries.
fn link(crit: &DuplicateCriteria, a: &Cand, b: &Cand) -> Link {
    let straight = a.white == b.white && a.black == b.black;
    let swapped = a.white == b.black && a.black == b.white;
    if !(straight || (!crit.same_colors && swapped)) {
        return Link::No;
    }
    let checks = [
        (crit.same_event, a.event_id == b.event_id),
        (crit.same_site, a.site_id == b.site_id),
        (crit.same_round, a.round_id == b.round_id),
        (crit.same_year, a.date.year() == b.date.year()),
        (crit.same_month, a.date.month() == b.date.month()),
        (crit.same_day, a.date.day() == b.date.day()),
        (crit.same_result, a.result == b.result),
        (crit.same_eco, a.eco_basic == b.eco_basic),
    ];
    if checks.iter().any(|&(on, equal)| on && !equal) {
        return Link::No;
    }
    // Place-holder games: one with at most one move replaced by the full one.
    if (a.half_moves <= 2 || b.half_moves <= 2)
        && a.white_known
        && b.white_known
        && a.event_known
        && b.event_known
        && a.date.year() == b.date.year()
        && a.site_id == b.site_id
        && a.round_id == b.round_id
    {
        return Link::Yes;
    }
    if crit.same_moves {
        Link::NeedMoves
    } else {
        Link::Yes
    }
}

/// The first 120 moves of the main line; `None` if the record cannot be read.
fn mainline_prefix(record: &[u8]) -> Option<Vec<RawMove>> {
    let mut moves = Vec::new();
    let walked = crate::si4::walk_mainline(record, |_, mv| {
        if let Some(mv) = mv {
            moves.push(mv);
            if moves.len() >= MAX_SAME_MOVES {
                return ControlFlow::Break(());
            }
        }
        ControlFlow::<()>::Continue(())
    });
    walked.ok().map(|_| moves)
}

/// Whether the shorter line is the start of the longer one.
fn same_moves(a: &[RawMove], b: &[RawMove]) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    long.get(..short.len()) == Some(short)
}

/// How the player groups are walked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Split {
    /// Split each player group by [`SubKey`] first (the normal mode).
    ByKey,
    /// Walk the whole player group, as the Scid does: the reference the
    /// split must equal.
    Whole,
}

/// Finds the twin games of `scope` under the criteria `c` (`sc_base
/// duplicates`) without writing anything.
///
/// Progress runs from 0 to `len + candidates` and never goes back; `cancel` is
/// looked at every 5000 games or 100 ms and at each group. A cancelled run
/// returns the pairs and games found so far with `interrupted` set. A game
/// whose record cannot be decoded is never a twin when moves are compared.
///
/// The result is exact, like the Scid's all-pairs walk: games with the same
/// players are first split by the values every enabled criterion needs equal
/// (site, round, year...), because games with different values can never be a
/// pair. Only a split group of more than about 447 games that agree on all of
/// those values can reach [`MAX_PAIRS_PER_GROUP`] (see
/// [`DuplicateReport::groups_truncated`]).
///
/// # Errors
///
/// [`MaintError::FilterSize`] if the scope filter does not cover the database.
pub fn find_duplicates<D: GameSource + ?Sized>(
    db: &mut D,
    scope: Scope<'_>,
    c: &DuplicateCriteria,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<DuplicateScan, MaintError> {
    find_duplicates_with_limit(db, scope, c, MAX_PAIRS_PER_GROUP, progress, cancel)
}

/// Like [`find_duplicates`] with another cap on the pairs examined inside one
/// group (the default is [`MAX_PAIRS_PER_GROUP`]); a group that needs more is
/// counted in [`DuplicateReport::groups_truncated`].
///
/// # Errors
///
/// [`MaintError::FilterSize`] if the scope filter does not cover the database.
pub fn find_duplicates_with_limit<D: GameSource + ?Sized>(
    db: &mut D,
    scope: Scope<'_>,
    c: &DuplicateCriteria,
    max_pairs_per_group: u64,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<DuplicateScan, MaintError> {
    run(
        db,
        scope,
        c,
        max_pairs_per_group,
        Split::ByKey,
        progress,
        cancel,
    )
}

/// The reference walk of the whole player group, quadratic: only to prove
/// that the split of [`find_duplicates`] changes nothing.
#[doc(hidden)]
pub fn find_duplicates_whole_groups<D: GameSource + ?Sized>(
    db: &mut D,
    scope: Scope<'_>,
    c: &DuplicateCriteria,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<DuplicateScan, MaintError> {
    run(
        db,
        scope,
        c,
        MAX_PAIRS_PER_GROUP,
        Split::Whole,
        progress,
        cancel,
    )
}

#[allow(clippy::too_many_lines)]
fn run<D: GameSource + ?Sized>(
    db: &mut D,
    scope: Scope<'_>,
    c: &DuplicateCriteria,
    limit: u64,
    split: Split,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<DuplicateScan, MaintError> {
    let games = db.len();
    scope.validate(games)?;
    let mut scan = DuplicateScan::default();

    // Phase 1: the candidates, from the index only.
    let mut cands: Vec<Cand> = Vec::new();
    let mut interner: HashMap<String, u32> = HashMap::new();
    // Token of each player ID under "first four letters" (u32::MAX: not yet).
    let mut player_tokens: Vec<u32> = Vec::new();
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    for n in 1..=games {
        if ticker.tick() {
            progress.progress(u64::from(n - 1), u64::from(games));
            if cancel.is_cancelled() {
                scan.report.interrupted = true;
                return Ok(scan);
            }
        }
        if !scope.contains(n) {
            continue;
        }
        let Ok(entry) = db.entry(n) else {
            scan.report.unreadable_entries = scan.report.unreadable_entries.saturating_add(1);
            continue;
        };
        if entry.flags.contains(crate::si4::GameFlags::DELETE)
            || (c.skip_short && entry.num_half_moves < SHORT_GAME_HALF_MOVES)
        {
            continue;
        }
        let names = db.names();
        let white_name = names.get(NameKind::Player, entry.white_id);
        let black_name = names.get(NameKind::Player, entry.black_id);
        let (white, black) = if c.exact_names {
            (entry.white_id, entry.black_id)
        } else {
            if player_tokens.is_empty() {
                let count = usize::try_from(names.num_names(NameKind::Player)).unwrap_or(0);
                player_tokens = vec![u32::MAX; count];
            }
            let mut token = |id: u32, name: Option<&[u8]>| {
                let cached = usize::try_from(id)
                    .ok()
                    .and_then(|at| player_tokens.get_mut(at));
                match cached {
                    Some(slot) if *slot != u32::MAX => *slot,
                    Some(slot) => {
                        *slot = prefix4_token(name, &mut interner);
                        *slot
                    }
                    None => prefix4_token(name, &mut interner),
                }
            };
            (
                token(entry.white_id, white_name),
                token(entry.black_id, black_name),
            )
        };
        cands.push(Cand {
            number: n,
            white_known: is_known(white_name),
            event_known: is_known(names.get(NameKind::Event, entry.event_id)),
            white,
            black,
            event_id: entry.event_id,
            site_id: entry.site_id,
            round_id: entry.round_id,
            date: entry.date,
            result: entry.result,
            eco_basic: entry.eco().map(Eco::basic),
            half_moves: entry.num_half_moves,
            comments: entry.comment_count,
            variations: entry.variation_count,
            ratings: [
                (entry.white_elo, entry.white_rating_type),
                (entry.black_elo, entry.black_rating_type),
            ],
        });
    }
    scan.report.examined = u32::try_from(cands.len()).unwrap_or(u32::MAX);
    let total = u64::from(games) + cands.len() as u64;
    progress.progress(u64::from(games), total);

    // Groups: the same unordered pair of players. Sorting by (key, index)
    // keeps the members in ascending game order.
    let mut keyed: Vec<((u32, u32), usize)> = cands
        .iter()
        .enumerate()
        .map(|(index, cand)| {
            (
                (cand.white.min(cand.black), cand.white.max(cand.black)),
                index,
            )
        })
        .collect();
    keyed.sort_unstable();
    let mut groups: Vec<Vec<usize>> = keyed
        .chunk_by(|x, y| x.0 == y.0)
        .filter(|run| run.len() > 1)
        .map(|run| run.iter().map(|&(_, index)| index).collect())
        .collect();
    drop(keyed);
    // The groups follow their first member.
    groups.sort_unstable_by_key(|g| g.first().copied());

    // Phase 2: the groups.
    let mut done = 0u64;
    let mut parts: Vec<(SubKey, usize)> = Vec::new();
    let mut part: Vec<usize> = Vec::new();
    for group in &groups {
        if cancel.is_cancelled() {
            scan.report.interrupted = true;
            break;
        }
        let ok = if split == Split::Whole {
            scan_group(db, c, &cands, group, limit, &mut scan, cancel)
        } else {
            parts.clear();
            parts.extend(
                group
                    .iter()
                    .filter_map(|&i| cands.get(i).map(|cand| (sub_key(c, cand), i))),
            );
            parts.sort_unstable();
            let mut all_done = true;
            for run in parts.chunk_by(|x, y| x.0 == y.0) {
                if run.len() < 2 {
                    continue;
                }
                part.clear();
                part.extend(run.iter().map(|&(_, i)| i));
                if !scan_group(db, c, &cands, &part, limit, &mut scan, cancel) {
                    all_done = false;
                    break;
                }
            }
            all_done
        };
        if !ok {
            scan.report.interrupted = true;
            break;
        }
        done += group.len() as u64;
        progress.progress(u64::from(games) + done, total);
    }
    // Groups that no pair touched still count as done.
    if !scan.report.interrupted {
        progress.progress(total, total);
    }
    scan.pairs.sort_unstable();
    scan.to_delete.sort_unstable();
    scan.to_delete.dedup();
    scan.rating_copies.sort_unstable_by_key(|(n, _, _)| *n);
    scan.report.pairs_found = u32::try_from(scan.pairs.len()).unwrap_or(u32::MAX);
    scan.report.marked = u32::try_from(scan.to_delete.len()).unwrap_or(u32::MAX);
    Ok(scan)
}

/// Reads and decodes the moves of the members that some pair will compare:
/// a superset of what the decision needs, since it ignores the games marked
/// meanwhile. `None` if cancelled.
fn load_lines<D: GameSource + ?Sized>(
    db: &mut D,
    crit: &DuplicateCriteria,
    members: &[&Cand],
    limit: u64,
    cancel: &CancelToken,
) -> Option<Vec<Option<Vec<RawMove>>>> {
    let count = members.len();
    let mut lines: Vec<Option<Vec<RawMove>>> = vec![None; count];
    if !crit.same_moves {
        return Some(lines);
    }
    let mut need = vec![false; count];
    let mut pairs = 0u64;
    'outer: for i in 0..count {
        for j in i + 1..count {
            pairs += 1;
            if pairs > limit {
                break 'outer;
            }
            if link(crit, members[i], members[j]) == Link::NeedMoves {
                need[i] = true;
                need[j] = true;
            }
        }
    }
    let wanted: Vec<usize> = (0..count).filter(|&k| need[k]).collect();
    for chunk in wanted.chunks(CHUNK) {
        if cancel.is_cancelled() {
            return None;
        }
        let records: Vec<(usize, Option<Vec<u8>>)> = chunk
            .iter()
            .map(|&k| {
                let number = members[k].number;
                (k, db.record_bytes(number).ok().map(<[u8]>::to_vec))
            })
            .collect();
        let decoded: Vec<(usize, Option<Vec<RawMove>>)> = records
            .par_iter()
            .map(|(k, record)| (*k, record.as_deref().and_then(mainline_prefix)))
            .collect();
        for (k, line) in decoded {
            lines[k] = line;
        }
    }
    Some(lines)
}

/// Visits the pairs `(i, j)`, `i < j`, of one group, the same way twice: first
/// to learn whose moves are needed, then to decide. Returns `false` if
/// cancelled.
fn scan_group<D: GameSource + ?Sized>(
    db: &mut D,
    crit: &DuplicateCriteria,
    cands: &[Cand],
    group: &[usize],
    limit: u64,
    scan: &mut DuplicateScan,
    cancel: &CancelToken,
) -> bool {
    let members: Vec<&Cand> = group.iter().filter_map(|&i| cands.get(i)).collect();
    let count = members.len();

    let Some(lines) = load_lines(db, crit, &members, limit, cancel) else {
        return false;
    };
    let mut marked = vec![false; count];
    let mut ratings: Vec<[(u16, u8); 2]> = members.iter().map(|member| member.ratings).collect();
    let mut rating_changed = vec![[false; 2]; count];
    let mut pairs = 0u64;
    let mut truncated = false;
    'rows: for i in 0..count {
        for j in i + 1..count {
            if marked[i] {
                break;
            }
            pairs += 1;
            if pairs > limit {
                truncated = true;
                break 'rows;
            }
            if pairs.is_multiple_of(4096) && cancel.is_cancelled() {
                return false;
            }
            if marked[j] {
                continue;
            }
            let (left, right) = (members[i], members[j]);
            let twins = match link(crit, left, right) {
                Link::No => false,
                Link::Yes => true,
                Link::NeedMoves => match (lines[i].as_deref(), lines[j].as_deref()) {
                    (Some(xs), Some(ys)) => same_moves(xs, ys),
                    _ => false,
                },
            };
            if !twins {
                continue;
            }
            scan.pairs.push(TwinPair {
                first: left.number,
                second: right.number,
            });
            let delete_first = match crit.delete {
                DeleteWhich::Shorter => left.half_moves <= right.half_moves,
                DeleteWhich::LowerNumber => true,
                DeleteWhich::HigherNumber => false,
            };
            let (gone, kept) = if delete_first { (i, j) } else { (j, i) };
            let doomed = members[gone];
            let immune = (crit.keep_commented && doomed.comments > 0)
                || (crit.keep_with_variations && doomed.variations > 0);
            if immune {
                scan.report.protected = scan.report.protected.saturating_add(1);
                continue;
            }
            marked[gone] = true;
            scan.to_delete.push(members[gone].number);
            if crit.copy_ratings {
                for side in 0..2 {
                    let from = ratings[gone][side];
                    if ratings[kept][side].0 == 0 && from.0 != 0 {
                        ratings[kept][side] = from;
                        rating_changed[kept][side] = true;
                    }
                }
            }
        }
    }
    if truncated {
        scan.report.groups_truncated = scan.report.groups_truncated.saturating_add(1);
    }
    for (k, changed) in rating_changed.iter().enumerate() {
        if changed[0] || changed[1] {
            let side = |s: usize| changed[s].then_some(ratings[k][s]);
            scan.rating_copies
                .push((members[k].number, side(0), side(1)));
        }
    }
    true
}
