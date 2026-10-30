// Scid: src/optable.cpp `PercentScore`/`TheoryPercent`/`PercentFreq` 617-666,
// `BestGames` 1440-1550, `TopPlayers` 1576-1779, `TopEcoCodes` 1787-1888,
// `AvgLength`/`AvgElo` 1891-1941; src/tkscid.cpp `avgGameLength` 14264 and
// `sc_report` 14280-14504. Copyright (c) 2001-2003 Shane Hudson. Scid is
// distributed under the GNU General Public License (see its COPYING); this
// file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The figures of a report, computed from [`ReportCore`] with the integer
//! arithmetic and the rounding of the Scid (decisions D3605 and D3611).

use std::collections::HashMap;

use mychess_core::{Color, GameResult};

use super::model::{ReportCore, ReportLine, result_points};
use crate::crosstable::ResultCounts;
use crate::tree::performance;

fn slot_of(result: GameResult) -> usize {
    match result {
        GameResult::White => 0,
        GameResult::Black => 1,
        GameResult::Draw => 2,
        _ => 3,
    }
}

fn count_of(counts: &ResultCounts, result: GameResult) -> u32 {
    match result {
        GameResult::White => counts.white,
        GameResult::Black => counts.black,
        GameResult::Draw => counts.draws,
        _ => counts.unknown,
    }
}

/// Games of `counts`, `*` included.
pub(crate) fn all_games(counts: &ResultCounts) -> u32 {
    counts
        .white
        .saturating_add(counts.black)
        .saturating_add(counts.draws)
        .saturating_add(counts.unknown)
}

/// The score of White in tenths of a per cent:
/// `(2 * wins + draws) * 500 / (wins + draws + losses)`; `*` is left out; 0
/// without games.
pub(crate) fn score_tenths(counts: &ResultCounts) -> u32 {
    let total = u64::from(counts.white) + u64::from(counts.black) + u64::from(counts.draws);
    if total == 0 {
        return 0;
    }
    let points = u64::from(counts.white) * 2 + u64::from(counts.draws);
    u32::try_from(points * 500 / total).unwrap_or(u32::MAX)
}

fn ratio_tenths(n: u32, total: u64) -> u32 {
    (u64::from(n) * 1000)
        .checked_div(total)
        .map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX))
}

/// Average rating of one colour, with the score of the opponents against it
/// (Scid: `AvgElo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EloReport {
    /// The average rating, rounded; 0 when no game has a rating.
    pub average: u32,
    /// Games with a rating of this colour.
    pub games: u32,
    /// The score, in per cent, of the **opponent** of this colour against it.
    pub opponent_percent: u32,
    /// The performance of the opponent, from `average` and the percentage.
    pub opponent_performance: u32,
}

/// A player of the "most frequent players" table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRow {
    /// Position in the list, from 1.
    pub rank: u32,
    /// The name as in the name base.
    pub name: String,
    /// Games in the report.
    pub games: u32,
    /// The first year in which the player played the line; 0 if unknown.
    pub first_year: u16,
    /// The last year; 0 if unknown.
    pub last_year: u16,
    /// The score in per cent: `(50 * points + games / 2) / games`.
    pub score_percent: u32,
    /// The highest rating in the games; 0 if none.
    pub max_elo: u16,
    /// The first notes of the theory table with a game of the player.
    pub notes: Vec<u32>,
    /// Whether more notes than [`PlayerRow::notes`] exist.
    pub more_notes: bool,
}

/// Notes listed per player (Scid: `PLAYERFREQ_MAXNOTES`).
pub const PLAYER_MAX_NOTES: usize = 8;

/// A list of games chosen from the lines (Scid: `BestGames`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BestKind {
    /// The shortest wins of a colour.
    Shortest(Color),
    /// The highest average rating.
    HighRating,
    /// The oldest games (an unknown date never wins).
    Oldest,
    /// The newest games.
    Newest,
}

/// A group of ECO codes (`A00-A09`...), most frequent first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcoGroup {
    /// The first code of the group, such as `C40`.
    pub first: String,
    /// The last code, such as `C49`.
    pub last: String,
    /// Games in the group.
    pub games: u32,
    /// The score of White in per cent.
    pub score_percent: u32,
}

impl ReportCore {
    /// Games of the report (`*` included): the Scid's `FilterCount`.
    pub fn report_games(&self) -> u32 {
        all_games(&self.total)
    }

    /// Games of the report except those of the excluded move (`TheoryCount`).
    pub fn theory_games(&self) -> u32 {
        all_games(&self.theory)
    }

    /// The score of White in the report, in tenths of a per cent.
    pub fn score_tenths(&self) -> u32 {
        score_tenths(&self.total)
    }

    /// The score of White without the excluded move, in tenths.
    pub fn theory_score_tenths(&self) -> u32 {
        score_tenths(&self.theory)
    }

    /// Half points of White without the excluded move (`TheoryScore`).
    pub fn theory_points2(&self) -> u32 {
        self.theory
            .white
            .saturating_mul(2)
            .saturating_add(self.theory.draws)
    }

    /// The frequency of a result among the report games, in tenths of a per
    /// cent: `n * 1000 / games` (`*` games count in the divisor).
    pub fn freq_tenths(&self, result: GameResult) -> u32 {
        ratio_tenths(
            count_of(&self.total, result),
            u64::from(self.report_games()),
        )
    }

    /// The frequency of a result among all the games of the database; 0 for
    /// an empty database (the Scid divides by zero).
    pub fn all_freq_tenths(&self, result: GameResult) -> u32 {
        ratio_tenths(
            count_of(&self.database.results, result),
            u64::from(all_games(&self.database.results)),
        )
    }

    /// The score of White among all the games of the database, in tenths.
    pub fn all_score_tenths(&self) -> u32 {
        score_tenths(&self.database.results)
    }

    /// The average length, in full moves, of the games with `result` among
    /// the lines of the report: `sum / count`, truncated; 0 without games.
    pub fn avg_length(&self, result: GameResult) -> u32 {
        let (mut n, mut sum) = (0u64, 0u64);
        for l in self.lines.iter().filter(|l| l.result == result) {
            n += 1;
            sum += u64::from(l.full_moves());
        }
        sum.checked_div(n)
            .map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX))
    }

    /// The average length of the games with `result` in the whole database,
    /// rounded (`avgGameLength`); 0 without games.
    pub fn all_avg_length(&self, result: GameResult) -> u32 {
        let i = slot_of(result);
        let n = u64::from(self.database.length_count.get(i).copied().unwrap_or(0));
        let sum = self.database.length_sum.get(i).copied().unwrap_or(0);
        (sum + n / 2)
            .checked_div(n)
            .map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX))
    }

    /// The average rating of `color` and the performance against it.
    pub fn elo_report(&self, color: Color) -> EloReport {
        let (mut n, mut sum, mut score) = (0u64, 0u64, 0u64);
        for l in &self.lines {
            let elo = match color {
                Color::White => l.white_elo,
                Color::Black => l.black_elo,
            };
            if elo == 0 {
                continue;
            }
            n += 1;
            sum += u64::from(elo);
            // The opponent of `color`: a win of the other side is 2 points.
            let theirs = match (color, l.result) {
                (Color::White, GameResult::Black) | (Color::Black, GameResult::White) => 2,
                (_, GameResult::White | GameResult::Black) => 0,
                _ => 1,
            };
            score += theirs;
        }
        if n == 0 {
            return EloReport::default();
        }
        let average = u32::try_from((sum + n / 2) / n).unwrap_or(u32::MAX);
        let percent = u32::try_from(((score * 50 + n / 2) / n).min(100)).unwrap_or(100);
        EloReport {
            average,
            games: u32::try_from(n).unwrap_or(u32::MAX),
            opponent_percent: percent,
            opponent_performance: performance(average, percent),
        }
    }

    /// The `count` most frequent players of `color` in the lines, with the
    /// score of that colour.
    pub fn top_players(&self, color: Color, count: usize) -> Vec<PlayerRow> {
        self.top_players_scored(color, color, count)
    }

    /// Like [`ReportCore::top_players`] but the score of each row is the one
    /// of the colour `score_of` (the player report lists the opponents and
    /// shows the score of the player against each).
    pub fn top_players_scored(
        &self,
        color: Color,
        score_of: Color,
        count: usize,
    ) -> Vec<PlayerRow> {
        #[derive(Default)]
        struct Acc {
            name: String,
            games: u32,
            points: u32,
            max_elo: u16,
            first_year: u16,
            last_year: u16,
            notes: Vec<u32>,
            more_notes: bool,
        }
        let mut by_id: HashMap<u32, Acc> = HashMap::new();
        let mut visit = |l: &ReportLine, with_stats: bool| {
            let (id, name, elo) = match color {
                Color::White => (l.white_id, &l.white, l.white_elo),
                Color::Black => (l.black_id, &l.black, l.black_elo),
            };
            let points = points_of(l, score_of);
            let acc = by_id.entry(id).or_default();
            if with_stats {
                acc.name.clone_from(name);
                acc.games = acc.games.saturating_add(1);
                acc.points = acc.points.saturating_add(points);
                acc.max_elo = acc.max_elo.max(elo);
                if let Some(y) = l.date.year().filter(|y| *y != 0) {
                    acc.first_year = if acc.first_year == 0 {
                        y
                    } else {
                        acc.first_year.min(y)
                    };
                    acc.last_year = acc.last_year.max(y);
                }
            } else if l.note != 0 && !acc.notes.contains(&l.note) {
                if acc.notes.len() < PLAYER_MAX_NOTES {
                    acc.notes.push(l.note);
                } else {
                    acc.more_notes = true;
                }
            }
        };
        for l in &self.lines {
            visit(l, true);
        }
        // The notes come in table order.
        for &i in &self.table_order {
            if let Some(l) = self.lines.get(i) {
                visit(l, false);
            }
        }
        let mut ranked: Vec<(u32, Acc)> = by_id.into_iter().filter(|(_, a)| a.games > 0).collect();
        ranked.sort_by(|(ia, a), (ib, b)| {
            b.games
                .cmp(&a.games)
                .then(b.max_elo.cmp(&a.max_elo))
                .then(ia.cmp(ib))
        });
        ranked
            .into_iter()
            .take(count)
            .enumerate()
            .map(|(i, (_, a))| PlayerRow {
                rank: u32::try_from(i + 1).unwrap_or(u32::MAX),
                score_percent: (50 * a.points + a.games / 2) / a.games.max(1),
                name: a.name,
                games: a.games,
                first_year: a.first_year,
                last_year: a.last_year,
                max_elo: a.max_elo,
                notes: a.notes,
                more_notes: a.more_notes,
            })
            .collect()
    }

    /// The `count` best games of `kind` among the lines. Ties go to the
    /// higher game number (the Scid's `v >= bestValue`).
    pub fn best(&self, kind: BestKind, count: usize) -> Vec<&ReportLine> {
        let mut ranked: Vec<(u64, &ReportLine)> = self
            .lines
            .iter()
            .filter_map(|l| best_value(kind, l).map(|v| (v, l)))
            .collect();
        ranked.sort_by(|(va, a), (vb, b)| vb.cmp(va).then(b.game.cmp(&a.game)));
        ranked.into_iter().take(count).map(|(_, l)| l).collect()
    }

    /// The `count` most frequent groups of ECO codes of the lines, with the
    /// score of White.
    pub fn top_eco_codes(&self, count: usize) -> Vec<EcoGroup> {
        self.top_eco_codes_scored(count, Color::White)
    }

    /// Like [`ReportCore::top_eco_codes`] with the score of `score_of`.
    pub fn top_eco_codes_scored(&self, count: usize, score_of: Color) -> Vec<EcoGroup> {
        let mut groups: HashMap<String, (u32, u32)> = HashMap::new();
        for l in &self.lines {
            let Some(eco) = l.eco else { continue };
            let basic = eco.basic_string();
            let Some(prefix) = basic.get(..2) else {
                continue;
            };
            let e = groups.entry(prefix.to_owned()).or_default();
            e.0 += 1;
            e.1 += points_of(l, score_of);
        }
        let mut ranked: Vec<(String, (u32, u32))> = groups.into_iter().collect();
        // Most frequent first; the lowest class wins a tie (the Scid scans
        // the classes in order with a strict comparison).
        ranked.sort_by(|(ka, (na, _)), (kb, (nb, _))| nb.cmp(na).then(ka.cmp(kb)));
        ranked
            .into_iter()
            .take(count)
            .map(|(k, (n, pts))| EcoGroup {
                first: format!("{k}0"),
                last: format!("{k}9"),
                games: n,
                score_percent: (50 * pts + n / 2) / n.max(1),
            })
            .collect()
    }

    /// The share of the report games that show `theme`, in per cent:
    /// `(100 * n + total / 2) / total`.
    pub fn theme_percent(&self, theme: usize) -> u32 {
        let total = u64::from(self.report_games());
        let n = u64::from(self.themes.get(theme).copied().unwrap_or(0));
        percent_rounded(n, total)
    }

    /// The share of the games of the report (`all = false`) or of the whole
    /// database (`all = true`) in an endgame class, in per cent.
    pub fn endgame_percent(&self, class: usize, all: bool) -> u32 {
        let counts = if all {
            &self.endgames.all
        } else {
            &self.endgames.report
        };
        let sum: u64 = counts.iter().map(|&n| u64::from(n)).sum();
        percent_rounded(u64::from(counts.get(class).copied().unwrap_or(0)), sum)
    }
}

/// The points (2, 1 or 0) that `color` took in the game of `line`.
fn points_of(line: &ReportLine, color: Color) -> u32 {
    match color {
        Color::White => result_points(line.result),
        Color::Black => 2 - result_points(line.result),
    }
}

/// `counts` seen from `view`: for Black the wins and losses trade places.
pub(crate) fn oriented(counts: &ResultCounts, view: Color) -> ResultCounts {
    match view {
        Color::White => *counts,
        Color::Black => ResultCounts {
            white: counts.black,
            black: counts.white,
            ..*counts
        },
    }
}

fn percent_rounded(n: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    u32::try_from((100 * n + total / 2) / total).unwrap_or(u32::MAX)
}

/// The key of a game in a [`BestKind`] list; `None` when it cannot be in it.
fn best_value(kind: BestKind, l: &ReportLine) -> Option<u64> {
    match kind {
        BestKind::Shortest(color) => {
            let wanted = match color {
                Color::White => GameResult::White,
                Color::Black => GameResult::Black,
            };
            if l.result != wanted {
                return None;
            }
            let left = 1000u64.saturating_sub(u64::from(l.full_moves()));
            Some((left << 12) + u64::from(l.avg_elo))
        }
        BestKind::HighRating => Some(u64::from(l.avg_elo)),
        BestKind::Oldest => {
            let date = u64::from(l.date.to_scid_u32());
            if date == 0 {
                None
            } else {
                let latest = (u64::from(mychess_core::MAX_YEAR) << 9) | (12 << 5) | 31;
                Some(latest.saturating_sub(date))
            }
        }
        BestKind::Newest => Some(u64::from(l.date.to_scid_u32())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(w: u32, d: u32, b: u32, u: u32) -> ResultCounts {
        ResultCounts {
            white: w,
            black: b,
            draws: d,
            unknown: u,
        }
    }

    #[test]
    fn score_is_in_tenths_and_leaves_unfinished_games_out() {
        // (2*3 + 1) * 500 / 5 = 700.
        assert_eq!(score_tenths(&counts(3, 1, 1, 0)), 700);
        assert_eq!(score_tenths(&counts(3, 1, 1, 9)), 700);
        assert_eq!(score_tenths(&counts(0, 0, 0, 4)), 0);
    }

    #[test]
    fn frequency_counts_unfinished_games_in_the_divisor() {
        let core = ReportCore {
            total: counts(3, 1, 1, 0),
            ..ReportCore::default()
        };
        assert_eq!(core.freq_tenths(GameResult::White), 600);
        assert_eq!(core.all_freq_tenths(GameResult::White), 0, "empty database");
        let core = ReportCore {
            total: counts(1, 0, 0, 1),
            ..ReportCore::default()
        };
        assert_eq!(core.freq_tenths(GameResult::White), 500);
    }

    #[test]
    fn percent_rounds_half_up() {
        assert_eq!(percent_rounded(1, 3), 33);
        assert_eq!(percent_rounded(2, 3), 67);
        assert_eq!(percent_rounded(1, 0), 0);
    }
}
