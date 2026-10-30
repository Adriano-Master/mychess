// Derived from Scid vs PC 4.27, src/tkscid.cpp (`Tourney`, `SortTourney`,
// `sc_base_tournaments`) and src/date.cpp (`date_AddMonths`). Copyright (c)
// 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is distributed under
// the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The tournament finder (rules 91 to 100): the games of a database grouped
//! into tournaments by event, site and date.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::sync::Arc;

use mychess_core::{GameResult, MAX_ELO, PartialDate};

use super::{Cancelled, alpha_contains, compare_names};
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, SEARCH_CHECK_EVERY, Ticker};
use crate::si4::{NameKind, bytes_to_string};
use crate::source::GameSource;

/// The order of the tournaments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TournamentSort {
    /// Newest first (default).
    #[default]
    Date,
    /// Most players first.
    Players,
    /// Most games first.
    Games,
    /// Highest mean Elo first.
    Elo,
    /// Event name, ascending.
    Event,
    /// Site name, ascending.
    Site,
    /// Name of the winner, then of the runner-up, ascending.
    Winner,
}

/// A query for [`find_tournaments`] (`sc_base tournaments`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentQuery {
    /// Games on or after this date (unknown parts count as 0, so
    /// `1960.??.??` accepts any date of 1960); unknown = no limit, and then
    /// games without a date are included.
    pub start: PartialDate,
    /// Games on or before this date; unknown = no limit.
    pub end: PartialDate,
    /// Only sites whose name ends with these three letters (exact case).
    pub country: Option<[u8; 3]>,
    /// The site contains this text (ignoring case and spaces); empty = any.
    pub site: String,
    /// The event contains this text; empty = any.
    pub event: String,
    /// Some player contains this text; empty = any.
    pub player: String,
    /// Games of the tournament, inclusive.
    pub games: RangeInclusive<u32>,
    /// Players of the tournament, inclusive.
    pub players: RangeInclusive<u32>,
    /// Mean Elo, inclusive.
    pub mean_elo: RangeInclusive<u16>,
    /// The order.
    pub sort: TournamentSort,
    /// The most tournaments returned; 0 = no limit.
    pub limit: usize,
}

impl Default for TournamentQuery {
    /// The defaults of `sc_base tournaments`: no dates, games 1 to 9999,
    /// players 2 to 999, Elo 0 to 4000, 100 tournaments, newest first.
    fn default() -> TournamentQuery {
        TournamentQuery {
            start: PartialDate::UNKNOWN,
            end: PartialDate::UNKNOWN,
            country: None,
            site: String::new(),
            event: String::new(),
            player: String::new(),
            games: 1..=9999,
            players: 2..=999,
            mean_elo: 0..=MAX_ELO,
            sort: TournamentSort::Date,
            limit: 100,
        }
    }
}

impl TournamentQuery {
    /// The defaults of the window (rule 92): from `1960.??.??` to
    /// `<year>.12.31`, 4 to 1999 players, 10 to 9999 games, 1000
    /// tournaments.
    pub fn window_defaults(year: u16) -> TournamentQuery {
        TournamentQuery {
            start: PartialDate::new(Some(1960), None, None).unwrap_or(PartialDate::UNKNOWN),
            end: PartialDate::new(Some(year), Some(12), Some(31)).unwrap_or(PartialDate::UNKNOWN),
            players: 4..=1999,
            games: 10..=9999,
            limit: 1000,
            ..TournamentQuery::default()
        }
    }
}

/// A player's place in a tournament.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    /// The player.
    pub name: Arc<str>,
    /// The highest Elo of the player in the tournament.
    pub elo: u16,
    /// Twice the points (2 per win, 1 per draw or game without result).
    pub points2: u32,
}

impl Standing {
    /// The points as text: `3.5`, `2.0`.
    pub fn points_text(&self) -> String {
        format!(
            "{}.{}",
            self.points2 / 2,
            if self.points2 % 2 == 1 { '5' } else { '0' }
        )
    }
}

/// A tournament of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentRow {
    /// The date of the earliest game (unknown parts as in the games).
    pub date: PartialDate,
    /// The site.
    pub site: Arc<str>,
    /// The event.
    pub event: Arc<str>,
    /// Distinct players.
    pub players: u32,
    /// Games.
    pub games: u32,
    /// Mean of all the ratings of the games (one per side), 0 if none.
    pub mean_elo: u16,
    /// Number (1-based) of the game with the earliest date.
    pub first_game: u32,
    /// The winner.
    pub winner: Option<Standing>,
    /// The runner-up.
    pub runner_up: Option<Standing>,
    /// Numbers (1-based) of the games of the tournament, ascending.
    pub game_numbers: Arc<[u32]>,
}

/// `date_AddMonths` on a packed date, with the same wrapping of the year
/// below 1 as the original (an unknown year never falls in a window).
fn add_months(bits: u32, months: i32) -> u32 {
    let mut year = bits >> 9;
    let mut month = (bits >> 5) & 0xF;
    let day = bits & 0x1F;
    let mut left = months;
    while left < 0 {
        if month <= 1 {
            year = year.wrapping_sub(1);
            month = 12;
        } else {
            month -= 1;
        }
        left += 1;
    }
    while left > 0 {
        month += 1;
        if month > 12 {
            year = year.wrapping_add(1);
            month = 1;
        }
        left -= 1;
    }
    year.wrapping_mul(512) | (month << 5) | day
}

struct Player {
    id: u32,
    elo: u32,
    score: u32,
}

#[derive(Default)]
struct Book {
    list: Vec<Player>,
    map: Option<HashMap<u32, usize>>,
}

/// Players beyond which a lookup table is built.
const LINEAR_LIMIT: usize = 24;

impl Book {
    fn add(&mut self, id: u32, elo: u32, score: u32) {
        let at = match &self.map {
            Some(map) => map.get(&id).copied(),
            None => self.list.iter().position(|p| p.id == id),
        };
        if let Some(p) = at.and_then(|i| self.list.get_mut(i)) {
            p.elo = p.elo.max(elo);
            p.score += score;
            return;
        }
        self.list.push(Player { id, elo, score });
        let index = self.list.len() - 1;
        if let Some(map) = &mut self.map {
            map.insert(id, index);
        } else if self.list.len() > LINEAR_LIMIT {
            self.map = Some(
                self.list
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (p.id, i))
                    .collect(),
            );
        }
    }

    /// The best two (rule 95): the most recently added player wins ties.
    fn best(&self) -> [Option<usize>; 2] {
        let mut out = [None, None];
        for slot in 0..2 {
            let mut best: Option<usize> = None;
            let mut best_score = -1i64;
            let mut best_elo = 0u32;
            for (i, p) in self.list.iter().enumerate().rev() {
                if slot == 1 && out[0] == Some(i) {
                    continue;
                }
                let score = i64::from(p.score);
                if score > best_score || (score == best_score && p.elo > best_elo) {
                    best = Some(i);
                    best_score = score;
                    best_elo = p.elo;
                }
            }
            out[slot] = best;
        }
        out
    }
}

struct Tourney {
    site: u32,
    event: u32,
    start: u32,
    min_date: u32,
    max_date: u32,
    event_date: u32,
    games: Vec<u32>,
    book: Book,
    elo_sum: u64,
    elo_count: u64,
    first_game: u32,
    best: [Option<usize>; 2],
}

impl Tourney {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn mean_elo(&self) -> u32 {
        if self.elo_count == 0 {
            return 0;
        }
        // Single precision, as the original.
        ((self.elo_sum as f32) / (self.elo_count as f32) + 0.5) as u32
    }
}

/// Groups the games into tournaments and answers the query (rules 91 to
/// 99). A game belongs to the first tournament with the same event and
/// site whose `EventDate` equals its own (when known) or whose window of
/// plus or minus three months around the date of the tournament's first
/// game contains the date of the game. Ties of the order are broken by the
/// number of the earliest game.
///
/// # Errors
///
/// [`Cancelled`] if `cancel` is set during the pass.
#[allow(clippy::too_many_lines)]
pub fn find_tournaments<D: GameSource + ?Sized>(
    db: &D,
    q: &TournamentQuery,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<Vec<TournamentRow>, Cancelled> {
    let names = db.names();
    let min_date = q.start.to_scid_u32();
    let max_date = if q.end.is_unknown() {
        u32::MAX
    } else {
        q.end.to_scid_u32()
    };
    let use_site: Vec<bool> = (0..names.num_names(NameKind::Site))
        .map(|id| {
            let raw = names.get(NameKind::Site, id).unwrap_or(&[]);
            if let Some(code) = q.country {
                let tail = raw.len().checked_sub(3).and_then(|s| raw.get(s..));
                let same = match tail {
                    Some(t) if raw.len() > 3 => t == code,
                    _ => raw == code,
                };
                if !same {
                    return false;
                }
            }
            q.site.trim().is_empty() || alpha_contains(&bytes_to_string(raw), &q.site)
        })
        .collect();
    let use_event: Option<Vec<bool>> = (!q.event.trim().is_empty()).then(|| {
        (0..names.num_names(NameKind::Event))
            .map(|id| {
                let raw = names.get(NameKind::Event, id).unwrap_or(&[]);
                alpha_contains(&bytes_to_string(raw), &q.event)
            })
            .collect()
    });

    let games = db.len();
    let mut tourneys: Vec<Tourney> = Vec::new();
    let mut groups: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    for n in 1..=games {
        if ticker.tick() {
            progress.progress(u64::from(n - 1), u64::from(games));
            if cancel.is_cancelled() {
                return Err(Cancelled);
            }
        }
        let Ok(e) = db.entry(n) else {
            continue;
        };
        let date = e.date.to_scid_u32();
        if date < min_date || date > max_date {
            continue;
        }
        let site_ok = usize::try_from(e.site_id)
            .ok()
            .and_then(|i| use_site.get(i))
            .copied()
            .unwrap_or(false);
        if !site_ok {
            continue;
        }
        if let Some(ev) = &use_event
            && !usize::try_from(e.event_id)
                .ok()
                .and_then(|i| ev.get(i))
                .copied()
                .unwrap_or(false)
        {
            continue;
        }
        let event_date = e.event_date.to_scid_u32();
        let list = groups.entry((e.event_id, e.site_id)).or_default();
        let found = list.iter().copied().find(|&t| {
            tourneys.get(t).is_some_and(|t| {
                (event_date != 0 && event_date == t.event_date)
                    || (date >= t.min_date && date <= t.max_date)
            })
        });
        let index = found.unwrap_or_else(|| {
            tourneys.push(Tourney {
                site: e.site_id,
                event: e.event_id,
                start: date,
                min_date: add_months(date, -3),
                max_date: add_months(date, 3),
                event_date,
                games: Vec::new(),
                book: Book::default(),
                elo_sum: 0,
                elo_count: 0,
                first_game: n,
                best: [None, None],
            });
            let index = tourneys.len() - 1;
            list.push(index);
            index
        });
        let Some(t) = tourneys.get_mut(index) else {
            continue;
        };
        let (ws, bs) = match e.result {
            GameResult::White => (2, 0),
            GameResult::Black => (0, 2),
            _ => (1, 1),
        };
        t.book.add(e.white_id, u32::from(e.white_elo), ws);
        t.book.add(e.black_id, u32::from(e.black_elo), bs);
        for elo in [e.white_elo, e.black_elo] {
            if elo != 0 {
                t.elo_sum += u64::from(elo);
                t.elo_count += 1;
            }
        }
        if t.games.is_empty() || date < t.start {
            t.start = date;
            t.first_game = n;
        }
        t.games.push(n);
    }
    progress.progress(u64::from(games), u64::from(games));

    for t in &mut tourneys {
        t.best = t.book.best();
    }
    let player_name = |id: u32| -> Arc<str> {
        names
            .get(NameKind::Player, id)
            .map_or_else(|| Arc::from(""), |raw| Arc::from(&*bytes_to_string(raw)))
    };
    let mut kept: Vec<&Tourney> = tourneys
        .iter()
        .filter(|t| {
            let mean = t.mean_elo();
            let players = u32::try_from(t.book.list.len()).unwrap_or(u32::MAX);
            let count = u32::try_from(t.games.len()).unwrap_or(u32::MAX);
            u16::try_from(mean).is_ok_and(|m| q.mean_elo.contains(&m))
                && q.players.contains(&players)
                && q.games.contains(&count)
                && (q.player.trim().is_empty()
                    || t.book
                        .list
                        .iter()
                        .any(|p| alpha_contains(&player_name(p.id), &q.player)))
        })
        .collect();
    let site_name = |t: &Tourney| {
        bytes_to_string(names.get(NameKind::Site, t.site).unwrap_or(&[])).into_owned()
    };
    let event_name = |t: &Tourney| {
        bytes_to_string(names.get(NameKind::Event, t.event).unwrap_or(&[])).into_owned()
    };
    let winner_name = |t: &Tourney, i: usize| {
        t.best
            .get(i)
            .copied()
            .flatten()
            .and_then(|p| t.book.list.get(p))
            .map(|p| player_name(p.id))
    };
    kept.sort_by(|a, b| {
        let primary = match q.sort {
            TournamentSort::Date => b.start.cmp(&a.start),
            TournamentSort::Players => b.book.list.len().cmp(&a.book.list.len()),
            TournamentSort::Games => b.games.len().cmp(&a.games.len()),
            TournamentSort::Elo => b.mean_elo().cmp(&a.mean_elo()),
            TournamentSort::Event => compare_names(&event_name(a), &event_name(b)),
            TournamentSort::Site => compare_names(&site_name(a), &site_name(b)),
            TournamentSort::Winner => (0..2)
                .map(|i| match (winner_name(a, i), winner_name(b, i)) {
                    (Some(x), Some(y)) => compare_names(&x, &y),
                    _ => Ordering::Equal,
                })
                .find(|o| o.is_ne())
                .unwrap_or(Ordering::Equal),
        };
        primary.then(a.first_game.cmp(&b.first_game))
    });
    let limit = if q.limit == 0 { kept.len() } else { q.limit };
    let standing = |t: &Tourney, i: usize| {
        let p = t
            .best
            .get(i)
            .copied()
            .flatten()
            .and_then(|p| t.book.list.get(p))?;
        Some(Standing {
            name: player_name(p.id),
            elo: u16::try_from(p.elo).unwrap_or(MAX_ELO),
            points2: p.score,
        })
    };
    Ok(kept
        .into_iter()
        .take(limit)
        .map(|t| TournamentRow {
            date: PartialDate::from_scid_u32(t.start).unwrap_or(PartialDate::UNKNOWN),
            site: Arc::from(site_name(t)),
            event: Arc::from(event_name(t)),
            players: u32::try_from(t.book.list.len()).unwrap_or(u32::MAX),
            games: u32::try_from(t.games.len()).unwrap_or(u32::MAX),
            mean_elo: u16::try_from(t.mean_elo()).unwrap_or(MAX_ELO),
            first_game: t.first_game,
            winner: standing(t, 0),
            runner_up: standing(t, 1),
            game_numbers: t.games.as_slice().into(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(text: &str) -> u32 {
        PartialDate::parse_lenient(text).to_scid_u32()
    }

    #[test]
    fn add_months_follows_the_original() {
        assert_eq!(add_months(bits("2020.01.10"), -3), bits("2019.10.10"));
        assert_eq!(add_months(bits("2020.01.10"), 3), bits("2020.04.10"));
        assert_eq!(add_months(bits("2020.11.10"), 3), bits("2021.02.10"));
        // Unknown month: -3 goes to month 10 of the previous year, +3 to 3.
        assert_eq!(add_months(bits("2020.??.??"), -3), bits("2019.10.??"));
        assert_eq!(add_months(bits("2020.??.??"), 3), bits("2020.03.??"));
        // No year: the window is empty.
        assert!(add_months(0, -3) > add_months(0, 3));
    }

    #[test]
    fn best_two_prefer_the_player_added_last_on_ties() {
        let mut b = Book::default();
        b.add(1, 2500, 2);
        b.add(2, 2500, 2);
        b.add(3, 2400, 4);
        let [first, second] = b.best();
        assert_eq!(first, Some(2));
        assert_eq!(second, Some(1), "same score and Elo: the later one");
    }

    #[test]
    fn points_text() {
        let s = Standing {
            name: Arc::from("x"),
            elo: 0,
            points2: 7,
        };
        assert_eq!(s.points_text(), "3.5");
    }

    #[test]
    fn book_switches_to_a_map_for_big_tournaments() {
        let mut b = Book::default();
        for id in 0..100 {
            b.add(id, 0, 1);
        }
        b.add(50, 10, 1);
        assert_eq!(b.list.len(), 100);
        assert_eq!(b.list[50].score, 2);
        assert_eq!(b.list[50].elo, 10);
    }
}
