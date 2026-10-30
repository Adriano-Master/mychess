//! [`EcoStatsTable`]: games and results per ECO prefix (D1181 to D1185).
//!
//! Scid keeps these counters in the database statistics and recounts them
//! after every change (`recalcFlagCounts`); the table here is built by one
//! pass over the index and updated by difference.

use mychess_core::{Eco, GameResult};
use thiserror::Error;

use crate::filter::Filter;
use crate::source::GameSource;

/// Codes per basic ECO code (see `mychess_core::Eco`).
const SUBCODES: u32 = 131;
/// Number of nodes per level: all, letter, letter+tens, basic code and
/// basic code + subcode letter.
const LEVEL_SIZES: [usize; 5] = [1, 5, 50, 500, 500 * 26];

/// Which games [`EcoStatsTable::build`] counts.
#[derive(Debug, Clone, Copy)]
pub enum EcoScope<'a> {
    /// Every game of the database, deleted ones included (as Scid does).
    WholeBase,
    /// Only the games of the filter (an extension of MyChess, D1183).
    Filter(&'a Filter),
}

/// The ECO prefix is not valid (D1182).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid ECO prefix")]
pub struct InvalidPrefix;

/// Counters of one ECO prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EcoStats {
    /// Games with a code under the prefix.
    pub count: u32,
    /// White wins.
    pub white: u32,
    /// Draws.
    pub draw: u32,
    /// Black wins.
    pub black: u32,
    /// Games without a result.
    pub none: u32,
}

impl EcoStats {
    /// The score of White in tenths of a percent (Scid prints `666` as
    /// `66.6`): `(2 * wins + draws + no result) * 500 / count`, truncated.
    /// A game without a result counts as half a point. Zero when empty.
    pub fn score_tenths(&self) -> u32 {
        if self.count == 0 {
            return 0;
        }
        let points = 2 * u64::from(self.white) + u64::from(self.draw) + u64::from(self.none);
        u32::try_from(points * 500 / u64::from(self.count)).unwrap_or(u32::MAX)
    }
}

/// A child node of the ECO tree with its counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcoChild {
    /// The prefix of the child, e.g. `B9` for the child `9` of `B`.
    pub prefix: String,
    /// Its counters.
    pub stats: EcoStats,
}

/// Counters `[none, white, black, draw]`.
type Counter = [u32; 4];

fn slot(result: GameResult) -> usize {
    match result {
        GameResult::White => 1,
        GameResult::Black => 2,
        GameResult::Draw => 3,
        // `Unknown` and anything the core may add: no result.
        _ => 0,
    }
}

fn to_stats(c: Counter) -> EcoStats {
    let [none, white, black, draw] = c;
    let count = c.iter().fold(0u32, |acc, &x| acc.saturating_add(x));
    EcoStats {
        count,
        white,
        draw,
        black,
        none,
    }
}

/// Games and results per ECO prefix, at the five levels of D1181.
#[derive(Debug, Clone)]
pub struct EcoStatsTable {
    levels: [Vec<Counter>; 5],
    unreadable: u32,
}

impl Default for EcoStatsTable {
    fn default() -> Self {
        EcoStatsTable::empty()
    }
}

/// Where the counters of a code live, one index per level.
fn indices(eco: Eco) -> [Option<usize>; 5] {
    let code = u32::from(eco.to_scid_code()) - 1;
    let basic = code / SUBCODES;
    let ext = code % SUBCODES;
    let to = |v: u32| usize::try_from(v).ok();
    [
        Some(0),
        to(basic / 100),
        to(basic / 10),
        to(basic),
        // The digit 1-4 of the extended code is not a level of its own.
        if ext > 0 {
            to(basic * 26 + (ext - 1) / 5)
        } else {
            None
        },
    ]
}

/// A prefix after validation: the level and the index in it.
fn parse_prefix(prefix: &str) -> Result<(usize, usize), InvalidPrefix> {
    let b = prefix.as_bytes();
    if b.len() > 5 {
        return Err(InvalidPrefix);
    }
    let mut index = 0usize;
    if let Some(&c) = b.first() {
        if !(b'A'..=b'E').contains(&c) {
            return Err(InvalidPrefix);
        }
        index = usize::from(c - b'A');
    }
    for &c in b.iter().skip(1).take(2) {
        if !c.is_ascii_digit() {
            return Err(InvalidPrefix);
        }
        index = index * 10 + usize::from(c - b'0');
    }
    if let Some(&c) = b.get(3) {
        if !c.is_ascii_lowercase() {
            return Err(InvalidPrefix);
        }
        index = index * 26 + usize::from(c - b'a');
    }
    // The fifth character is accepted and ignored, as in Scid.
    Ok((b.len().min(4), index))
}

impl EcoStatsTable {
    fn empty() -> EcoStatsTable {
        EcoStatsTable {
            levels: LEVEL_SIZES.map(|n| vec![[0; 4]; n]),
            unreadable: 0,
        }
    }

    /// Counts the games of `db` that have an ECO code, by one scan of the
    /// index. Entries that cannot be read are counted in
    /// [`EcoStatsTable::unreadable`].
    pub fn build<D: GameSource + ?Sized>(db: &D, scope: EcoScope<'_>) -> EcoStatsTable {
        let mut table = EcoStatsTable::empty();
        for n in 1..=db.len() {
            if let EcoScope::Filter(filter) = scope
                && !filter.contains(n)
            {
                continue;
            }
            let Ok(entry) = db.entry(n) else {
                table.unreadable = table.unreadable.saturating_add(1);
                continue;
            };
            if let Some(eco) = entry.eco() {
                table.bump(eco, entry.result, true);
            }
        }
        table
    }

    /// How many index entries could not be read while building.
    pub fn unreadable(&self) -> u32 {
        self.unreadable
    }

    fn bump(&mut self, eco: Eco, result: GameResult, add: bool) {
        let s = slot(result);
        for (level, index) in indices(eco).into_iter().enumerate() {
            let counter = index
                .and_then(|i| self.levels.get_mut(level).and_then(|l| l.get_mut(i)))
                .and_then(|c| c.get_mut(s));
            if let Some(c) = counter {
                *c = if add {
                    c.saturating_add(1)
                } else {
                    c.saturating_sub(1)
                };
            }
        }
    }

    /// Updates the table after the code of a game changed from `old` to
    /// `new` (D1185), the same as building it again. A game without a code
    /// is in no node.
    pub fn apply_change(&mut self, old: Option<Eco>, new: Option<Eco>, result: GameResult) {
        if old == new {
            return;
        }
        if let Some(eco) = old {
            self.bump(eco, result, false);
        }
        if let Some(eco) = new {
            self.bump(eco, result, true);
        }
    }

    /// The counters of an ECO prefix: empty (all), `B`, `B9`, `B90`, `B90a`
    /// or `B90a2` (the last two are the same node).
    ///
    /// # Errors
    ///
    /// [`InvalidPrefix`] when the text breaks the rules of D1182.
    pub fn get(&self, prefix: &str) -> Result<EcoStats, InvalidPrefix> {
        let (level, index) = parse_prefix(prefix)?;
        Ok(self
            .levels
            .get(level)
            .and_then(|l| l.get(index))
            .copied()
            .map(to_stats)
            .unwrap_or_default())
    }

    /// The children of a prefix in the ECO tree (D1184): letters under the
    /// empty prefix, digits under a letter or a letter and a digit, and
    /// subcode letters under a basic code. Nothing deeper.
    ///
    /// # Errors
    ///
    /// [`InvalidPrefix`] when the prefix is not valid.
    pub fn children(&self, prefix: &str) -> Result<Vec<EcoChild>, InvalidPrefix> {
        parse_prefix(prefix)?;
        let labels: Vec<char> = match prefix.len() {
            0 => ('A'..='E').collect(),
            1 | 2 => ('0'..='9').collect(),
            3 => ('a'..='z').collect(),
            _ => Vec::new(),
        };
        labels
            .into_iter()
            .map(|label| {
                let child = format!("{prefix}{label}");
                self.get(&child).map(|stats| EcoChild {
                    prefix: child,
                    stats,
                })
            })
            .collect()
    }
}

/// The step of the grid lines of the bar chart for the largest count `max`
/// (`eco.tcl`): 5 by default, growing in the steps of D1184.
pub fn bar_scale(max: u32) -> u32 {
    const STEPS: [(u32, u32); 12] = [
        (100_000, 50_000),
        (50_000, 25_000),
        (20_000, 10_000),
        (10_000, 5_000),
        (5_000, 2_500),
        (2_000, 1_000),
        (1_000, 500),
        (500, 250),
        (200, 100),
        (100, 50),
        (50, 25),
        (20, 10),
    ];
    STEPS
        .iter()
        .find(|&&(above, _)| max > above)
        .map_or(5, |&(_, step)| step)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eco(t: &str) -> Eco {
        t.parse().unwrap()
    }

    fn table(items: &[(Option<&str>, GameResult)]) -> EcoStatsTable {
        let mut t = EcoStatsTable::empty();
        for (code, result) in items {
            if let Some(code) = code {
                t.bump(eco(code), *result, true);
            }
        }
        t
    }

    fn sample() -> EcoStatsTable {
        table(&[
            (Some("A00a"), GameResult::White),
            (Some("B90a"), GameResult::Draw),
            (Some("B90a2"), GameResult::Black),
            (Some("B91"), GameResult::Unknown),
            (None, GameResult::White),
        ])
    }

    #[test]
    fn counts_by_prefix_like_the_spec_example() {
        let t = sample();
        let count = |p: &str| t.get(p).unwrap().count;
        assert_eq!(count(""), 4);
        assert_eq!(count("B"), 3);
        assert_eq!(count("B9"), 3);
        assert_eq!(count("B90"), 2);
        assert_eq!(count("B90a"), 2);
        assert_eq!(t.get("B90a"), t.get("B90a2"));
        assert_eq!(count("B91"), 1);
        assert_eq!(count("B91a"), 0);
    }

    #[test]
    fn rejects_invalid_prefixes() {
        let t = sample();
        for p in [
            "b", "F", "Bx", "B9x", "B90A", "B901", "B90a22x", "B90a2x1", "é",
        ] {
            assert_eq!(t.get(p), Err(InvalidPrefix), "{p:?}");
        }
        // Scid ignores the fifth character.
        assert!(t.get("B90a2").is_ok());
        assert!(t.get("B90az").is_ok());
    }

    #[test]
    fn score_follows_scid() {
        let s = EcoStats {
            count: 3,
            white: 2,
            draw: 1,
            black: 0,
            none: 0,
        };
        assert_eq!(s.score_tenths(), 833);
        let s = EcoStats {
            count: 3,
            white: 2,
            draw: 0,
            black: 1,
            none: 0,
        };
        assert_eq!(s.score_tenths(), 666);
        assert_eq!(EcoStats::default().score_tenths(), 0);
        let s = EcoStats {
            count: 2,
            none: 2,
            ..EcoStats::default()
        };
        assert_eq!(s.score_tenths(), 500);
    }

    #[test]
    fn children_follow_the_tree() {
        let t = sample();
        let labels = |p: &str| -> Vec<String> {
            t.children(p)
                .unwrap()
                .into_iter()
                .map(|c| c.prefix)
                .collect()
        };
        assert_eq!(labels(""), ["A", "B", "C", "D", "E"]);
        assert_eq!(labels("B").len(), 10);
        assert_eq!(labels("B9")[0], "B90");
        assert_eq!(labels("B90").len(), 26);
        assert_eq!(labels("B90a").len(), 0);
        let b90 = t.children("B9").unwrap();
        assert_eq!(b90[0].stats.count, 2);
        assert_eq!(t.children("x"), Err(InvalidPrefix));
    }

    #[test]
    fn bar_scale_steps() {
        assert_eq!(bar_scale(0), 5);
        assert_eq!(bar_scale(20), 5);
        assert_eq!(bar_scale(21), 10);
        assert_eq!(bar_scale(100), 25);
        assert_eq!(bar_scale(101), 50);
        assert_eq!(bar_scale(100_001), 50_000);
        assert_eq!(bar_scale(u32::MAX), 50_000);
    }

    #[test]
    fn apply_change_equals_rebuilding() {
        let mut t = table(&[
            (Some("B90a"), GameResult::White),
            (Some("B91"), GameResult::Draw),
        ]);
        t.apply_change(Some(eco("B91")), Some(eco("C20a")), GameResult::Draw);
        t.apply_change(Some(eco("B90a")), None, GameResult::White);
        t.apply_change(None, Some(eco("A00")), GameResult::Black);
        let rebuilt = table(&[
            (Some("C20a"), GameResult::Draw),
            (Some("A00"), GameResult::Black),
        ]);
        assert_eq!(t.levels, rebuilt.levels);
    }
}
