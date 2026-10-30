// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_tree_search`,
// `sortTreeMoves`) and src/crosstab.cpp (`Performance`). Copyright (c)
// 1999-2002 Shane Hudson; Scid vs PC contributors. Scid is distributed under
// the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The arithmetic of the opening tree: scores, averages, performance and the
//! sort of the rows (`docs/specs/m5-1-motor-da-arvore.md`, rules 7 to 13).
//!
//! All divisions are integer divisions that truncate, as in the Scid; sums are
//! widened to `u64` so no overflow is possible.

use mychess_core::{Color, EcoBook, Position};

use super::{Sort, TreeNode, TreeRow, TreeTotal};
use crate::si4::RawMove;

/// Expected rating difference for a score of 50% to 100% (Scid:
/// `perf_elodiff` in `crosstab.cpp`).
pub(crate) const PERF_ELODIFF: [u32; 51] = [
    0, 7, 14, 21, 29, 36, 43, 50, 57, 65, 72, 80, 87, 95, 102, 110, 117, 125, 133, 141, 149, 158,
    166, 175, 184, 193, 202, 211, 220, 230, 240, 251, 262, 273, 284, 296, 309, 322, 336, 351, 366,
    383, 401, 422, 444, 470, 501, 538, 589, 677, 800,
];

/// Averages need at least this many games with a rating (rules 8 and 9).
const MIN_RATED: u32 = 10;

/// The performance rating for an average opponent rating `opp_avg` and a
/// score of `percent` per cent (`Crosstable::Performance`).
///
/// `percent` above 100 counts as 100. Unlike the Scid, which subtracts
/// without a sign, a result below zero is limited to 0 (decision D947).
///
/// ```
/// use mychess_db::tree::performance;
///
/// assert_eq!(performance(2500, 50), 2500);
/// assert_eq!(performance(2500, 75), 2693);
/// assert_eq!(performance(2500, 100), 3300);
/// assert_eq!(performance(2500, 0), 1700);
/// assert_eq!(performance(500, 0), 0);
/// ```
pub fn performance(opp_avg: u32, percent: u32) -> u32 {
    let percent = percent.min(100);
    if percent < 50 {
        let diff = PERF_ELODIFF
            .get((50 - percent) as usize)
            .copied()
            .unwrap_or(0);
        opp_avg.saturating_sub(diff)
    } else {
        let diff = PERF_ELODIFF
            .get((percent - 50) as usize)
            .copied()
            .unwrap_or(0);
        opp_avg.saturating_add(diff)
    }
}

/// `(freq[W]*2 + freq[D] + freq[None]) * 500 / total`, truncated (rule 7).
/// `total` 0 gives 0.
pub(crate) fn score_permille(freq: [u32; 4], total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    let [white, _black, draw, none] = freq.map(u64::from);
    let points = white * 2 + draw + none;
    // At most 1000: points <= 2 * total.
    u32::try_from(points * 500 / total).unwrap_or(1000)
}

/// `sum / count` when `count >= 10` (rule 8).
fn average_if_enough(sum: u64, count: u64) -> Option<u32> {
    if count >= u64::from(MIN_RATED) {
        u32::try_from(sum / count).ok()
    } else {
        None
    }
}

/// The performance of rule 9 from the opponent average and the score.
fn node_performance(sum: u64, count: u64, score: u32, turn: Color) -> Option<u32> {
    let opp_avg = average_if_enough(sum, count)?;
    let mut percent = (score + 5) / 10;
    if turn == Color::Black {
        percent = 100u32.saturating_sub(percent);
    }
    Some(performance(opp_avg, percent))
}

/// `(sum + count/2) / count`, rounded (rule 10).
fn average_year(sum: u64, count: u64) -> Option<u32> {
    if count == 0 {
        return None;
    }
    u32::try_from((sum + count / 2) / count).ok()
}

/// Builds the (unsorted) row of one node.
pub(super) fn build_row(
    pos: &Position,
    node: &TreeNode,
    eco_book: Option<&EcoBook>,
    hide_moves: bool,
) -> TreeRow {
    let score = score_permille(node.freq, u64::from(node.total));
    let (san, uci, after) = match node.mv {
        None => ("[end]".to_owned(), None, Some(pos.clone())),
        Some(mv) => describe_move(pos, mv),
    };
    let eco = eco_book
        .zip(after.as_ref())
        .and_then(|(book, p)| book.lookup(p))
        .map(mychess_core::EcoEntry::eco);
    TreeRow {
        san,
        uci,
        mv: node.mv,
        total: node.total,
        freq: node.freq,
        score_permille: score,
        avg_elo: average_if_enough(node.elo_sum, u64::from(node.elo_count)),
        perf: node_performance(node.perf_sum, u64::from(node.perf_count), score, pos.turn()),
        avg_year: average_year(node.year_sum, u64::from(node.year_count)),
        eco,
        hidden: hide_moves,
    }
}

/// The SAN, the UCI text and the position after the move. A move that the
/// core cannot reproduce (the record has a move that is not legal) is shown
/// as its squares and has no position after it.
fn describe_move(pos: &Position, mv: RawMove) -> (String, Option<String>, Option<Position>) {
    let core_move = if mv.is_null() {
        Some(mychess_core::Move::Null)
    } else {
        pos.find_move(mv.from, mv.to, mv.promotion)
    };
    let Some(core_move) = core_move else {
        return (format!("{}{}", mv.from, mv.to), None, None);
    };
    let san = pos
        .san(core_move)
        .unwrap_or_else(|_| format!("{}{}", mv.from, mv.to));
    let uci = pos.uci(core_move);
    (san, Some(uci), pos.after(core_move).ok())
}

/// The totals line (rule 13): `None` without nodes.
pub(super) fn build_total(nodes: &[TreeNode], turn: Color) -> Option<TreeTotal> {
    if nodes.is_empty() {
        return None;
    }
    let mut freq = [0u32; 4];
    let (mut elo_sum, mut elo_count) = (0u64, 0u64);
    let (mut perf_sum, mut perf_count) = (0u64, 0u64);
    let (mut year_sum, mut year_count) = (0u64, 0u64);
    let mut total = 0u64;
    for node in nodes {
        for (acc, add) in freq.iter_mut().zip(node.freq) {
            *acc = acc.saturating_add(add);
        }
        total += u64::from(node.total);
        elo_sum += node.elo_sum;
        elo_count += u64::from(node.elo_count);
        perf_sum += node.perf_sum;
        perf_count += u64::from(node.perf_count);
        year_sum += node.year_sum;
        year_count += u64::from(node.year_count);
    }
    let score = score_permille(freq, total);
    Some(TreeTotal {
        total: u32::try_from(total).unwrap_or(u32::MAX),
        freq,
        score_permille: score,
        avg_elo: average_if_enough(elo_sum, elo_count),
        perf: node_performance(perf_sum, perf_count, score, turn),
        avg_year: average_year(year_sum, year_count),
    })
}

/// Sorts the rows with the pair-swap algorithm of `sortTreeMoves` (decision
/// D944). It is not stable: equal rows may change places, exactly as in the
/// Scid, which keeps golden comparisons possible.
pub(super) fn sort_rows(rows: &mut [TreeRow], sort: Sort, turn: Color) {
    let count = rows.len();
    for outer in 0..count.saturating_sub(1) {
        for inner in outer + 1..count {
            let (Some(a), Some(b)) = (rows.get(outer), rows.get(inner)) else {
                continue;
            };
            let result: i64 = match sort {
                Sort::Frequency => i64::from(a.total) - i64::from(b.total),
                // Scid: strCompare(inner, outer), a byte-wise, case-sensitive
                // comparison; a negative result swaps.
                Sort::Alpha => match b.san.as_bytes().cmp(a.san.as_bytes()) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                },
                Sort::Eco => eco_code(a) - eco_code(b),
                Sort::Score => {
                    let diff = i64::from(a.score_permille) - i64::from(b.score_permille);
                    if turn == Color::Black { -diff } else { diff }
                }
            };
            if result < 0 {
                rows.swap(outer, inner);
            }
        }
    }
}

fn eco_code(row: &TreeRow) -> i64 {
    row.eco.map_or(0, |e| i64::from(e.to_scid_code()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn performance_matches_the_spec_examples() {
        assert_eq!(performance(2500, 50), 2500);
        assert_eq!(performance(2500, 75), 2693);
        assert_eq!(performance(2500, 100), 3300);
        assert_eq!(performance(2500, 0), 1700);
        assert_eq!(performance(2500, 400), 3300);
        assert_eq!(performance(100, 0), 0);
    }

    #[test]
    fn score_truncates() {
        // (1*2 + 0 + 0) * 500 / 3 = 333
        assert_eq!(score_permille([1, 1, 0, 1], 3), 500);
        assert_eq!(score_permille([1, 2, 0, 0], 3), 333);
        assert_eq!(score_permille([0, 0, 0, 0], 0), 0);
    }

    #[test]
    fn year_is_rounded() {
        assert_eq!(average_year(2000 + 2001, 2), Some(2001));
        assert_eq!(average_year(0, 0), None);
    }
}
