// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_name_plist`,
// `SortNamebaseNodes`, `recalcEstimatedRatings`).
// Copyright notices of src/tkscid.cpp: Copyright (c) 1999-2004 Shane Hudson;
// Copyright (c) 2006-2007 Pascal Georges; Copyright (c) 2009-2013 Stevenaaus.
// Scid is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The table behind the player finder (rules 75 to 80 and 87).

use std::ops::RangeInclusive;
use std::sync::{Arc, Mutex};

use mychess_core::{MAX_ELO, PartialDate};

use super::{Cancelled, compare_names, fold};
use crate::player_stats::PlayerStats;
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, SEARCH_CHECK_EVERY, Ticker};
use crate::si4::{NameKind, bytes_to_string};
use crate::source::GameSource;
use crate::spelling::check::is_surname_only;
use crate::spelling::{PlayerComment, SpellChecker};

/// What the country column shows for a player the spelling file does not
/// know.
pub const UNKNOWN_COUNTRY: &str = "~~~";

/// One player of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRow {
    /// The ID of the name in the name base.
    pub id: u32,
    /// The name.
    pub name: Arc<str>,
    /// Games as White or Black (a game against oneself counts twice).
    pub games: u32,
    /// Date of the oldest game with a known year.
    pub first: PartialDate,
    /// Date of the newest game with a known year.
    pub last: PartialDate,
    /// The highest Elo in the games or, when 0, the peak rating of the
    /// spelling file (rule 87).
    pub max_elo: u16,
}

/// The fingerprint of a checker and the countries computed with it.
type CountryCache = (u64, Arc<[Box<str>]>);

/// The players of a database with their statistics, sorted by name.
///
/// Built by one pass over the index ([`PlayerTable::build`]); filters and
/// orders then only look at the table. Players without games are not
/// listed.
#[derive(Debug)]
pub struct PlayerTable {
    rows: Vec<PlayerRow>,
    estimated: bool,
    /// The countries of the rows and the fingerprint of the checker they
    /// were computed with (0 = no checker).
    countries: Mutex<Option<CountryCache>>,
}

/// Which countries to show and keep.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CountryFilter {
    /// No country column and no lookup in the spelling file.
    #[default]
    No,
    /// The column, every player.
    All,
    /// The column and only the players whose country code starts with this
    /// text (any case).
    Prefix(String),
}

/// The columns the table can be ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerSort {
    /// By name (default).
    #[default]
    Name,
    /// Most games first.
    Games,
    /// Oldest first game first; players without dates last.
    Oldest,
    /// Newest last game first; players without dates last.
    Newest,
    /// Highest Elo first.
    Elo,
    /// By country code.
    Country,
}

impl PlayerSort {
    /// Whether the natural direction of the column is descending.
    fn default_descending(self) -> bool {
        matches!(
            self,
            PlayerSort::Games | PlayerSort::Newest | PlayerSort::Elo
        )
    }
}

/// A query on a [`PlayerTable`] (rule 76).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerQuery {
    /// The name starts with this text (ignoring case, not spaces).
    pub prefix: String,
    /// Games, inclusive.
    pub games: RangeInclusive<u32>,
    /// Elo, inclusive.
    pub elo: RangeInclusive<u16>,
    /// The country column and filter.
    pub country: CountryFilter,
    /// The order.
    pub sort: PlayerSort,
    /// `None` = the natural direction of the column; `Some(true)` =
    /// descending, `Some(false)` = ascending.
    pub descending: Option<bool>,
    /// The most rows returned; 0 = no limit.
    pub limit: usize,
}

impl Default for PlayerQuery {
    /// The defaults of the window (D3312): games 1 to 100 000, Elo 0 to
    /// 4000, 1000 rows ordered by name, no country.
    fn default() -> PlayerQuery {
        PlayerQuery {
            prefix: String::new(),
            games: 1..=100_000,
            elo: 0..=MAX_ELO,
            country: CountryFilter::No,
            sort: PlayerSort::Name,
            descending: None,
            limit: 1000,
        }
    }
}

/// A row of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerFound {
    /// The ID of the name.
    pub id: u32,
    /// The name.
    pub name: Arc<str>,
    /// The number of games.
    pub games: u32,
    /// Date of the oldest game.
    pub first: PartialDate,
    /// Date of the newest game.
    pub last: PartialDate,
    /// The Elo of the column.
    pub max_elo: u16,
    /// The country code, `~~~` when unknown; `None` without the column.
    pub country: Option<String>,
}

#[derive(Clone, Copy, Default)]
struct Acc {
    games: u32,
    first: Option<PartialDate>,
    last: Option<PartialDate>,
    elo: u16,
}

/// The peak rating of a player, from the spelling file, limited to 4000.
pub(crate) fn estimated_peak(checker: &SpellChecker, name: &str) -> u16 {
    checker
        .comment_exact(name)
        .map_or(0, |c| PlayerComment::parse(c).peak())
        .min(MAX_ELO)
}

impl PlayerTable {
    /// One pass over the index, cancellable (rule 75). With a `spelling`
    /// checker for players, a player with Elo 0 in all games, with games and
    /// whose name is not just a surname gets the peak rating of the file.
    ///
    /// # Errors
    ///
    /// [`Cancelled`] if `cancel` is set during the pass.
    pub fn build<D: GameSource + ?Sized>(
        db: &D,
        spelling: Option<&SpellChecker>,
        cancel: &CancelToken,
        progress: &mut dyn ProgressSink,
    ) -> Result<PlayerTable, Cancelled> {
        let names = db.names();
        let size = usize::try_from(names.num_names(NameKind::Player)).unwrap_or(0);
        let mut acc = vec![Acc::default(); size];
        let games = db.len();
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
            for (id, elo) in [(e.white_id, e.white_elo), (e.black_id, e.black_elo)] {
                let Some(a) = usize::try_from(id).ok().and_then(|i| acc.get_mut(i)) else {
                    continue;
                };
                a.games = a.games.saturating_add(1);
                a.elo = a.elo.max(elo);
                if e.date.year().is_some() {
                    if a.first.is_none_or(|f| e.date < f) {
                        a.first = Some(e.date);
                    }
                    if a.last.is_none_or(|l| e.date > l) {
                        a.last = Some(e.date);
                    }
                }
            }
        }
        let checker = spelling.filter(|s| s.kind() == NameKind::Player);
        let mut estimated = false;
        let mut rows = Vec::new();
        for (index, a) in acc.iter().enumerate() {
            if a.games == 0 {
                continue;
            }
            let Ok(id) = u32::try_from(index) else {
                continue;
            };
            let Some(raw) = names.get(NameKind::Player, id) else {
                continue;
            };
            let name = bytes_to_string(raw);
            let mut max_elo = a.elo;
            if max_elo == 0
                && let Some(c) = checker
                && !is_surname_only(&name)
            {
                max_elo = estimated_peak(c, &name);
                estimated |= max_elo != 0;
            }
            rows.push(PlayerRow {
                id,
                name: Arc::from(&*name),
                games: a.games,
                first: a.first.unwrap_or(PartialDate::UNKNOWN),
                last: a.last.unwrap_or(PartialDate::UNKNOWN),
                max_elo,
            });
        }
        // Stable sort by the folded name over an order by the plain name.
        rows.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        rows.sort_by_cached_key(|r| fold(&r.name));
        progress.progress(u64::from(games), u64::from(games));
        Ok(PlayerTable {
            rows,
            estimated,
            countries: Mutex::new(None),
        })
    }

    /// The number of players listed.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no player has games.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Whether some Elo comes from the spelling file instead of the games.
    pub fn has_estimates(&self) -> bool {
        self.estimated
    }

    /// The rows, in name order.
    pub fn rows(&self) -> &[PlayerRow] {
        &self.rows
    }

    /// The country of every row (parallel to `rows`): the last country of
    /// the loose comment, empty if the comment has none, [`UNKNOWN_COUNTRY`]
    /// without checker or comment. The result is kept with the fingerprint of
    /// the checker (0 without one) and recomputed when another checker, or
    /// none, is asked for (D3441).
    fn countries(&self, spelling: Option<&SpellChecker>) -> Arc<[Box<str>]> {
        let checker = spelling.filter(|s| s.kind() == NameKind::Player);
        let key = checker.map_or(0, SpellChecker::fingerprint);
        let mut cache = self
            .countries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((k, list)) = cache.as_ref()
            && *k == key
        {
            return Arc::clone(list);
        }
        let list: Arc<[Box<str>]> = self
            .rows
            .iter()
            .map(|r| {
                checker
                    .and_then(|c| c.comment_loose(&r.name))
                    .map_or(UNKNOWN_COUNTRY, |c| PlayerComment::parse(c).last_country())
                    .into()
            })
            .collect();
        *cache = Some((key, Arc::clone(&list)));
        list
    }

    /// Filters, orders and cuts the table (rules 76 to 78). The name prefix
    /// is found by binary search on the sorted names.
    pub fn query(&self, q: &PlayerQuery, spelling: Option<&SpellChecker>) -> Vec<PlayerFound> {
        let prefix = fold(&q.prefix);
        let start = self
            .rows
            .partition_point(|r| fold(&r.name).as_str() < prefix.as_str());
        let window = self.rows.get(start..).unwrap_or(&[]);
        let with_country = q.country != CountryFilter::No;
        let countries: Arc<[Box<str>]> = if with_country {
            self.countries(spelling)
        } else {
            Arc::from([])
        };
        let wanted = match &q.country {
            CountryFilter::Prefix(p) => Some(fold(p)),
            _ => None,
        };
        let mut found: Vec<(usize, &PlayerRow)> = Vec::new();
        for (offset, row) in window.iter().enumerate() {
            if !prefix.is_empty() && !fold(&row.name).starts_with(prefix.as_str()) {
                break;
            }
            if !q.games.contains(&row.games) || !q.elo.contains(&row.max_elo) {
                continue;
            }
            let index = start + offset;
            if let Some(w) = &wanted {
                let country = countries.get(index).map_or("", AsRef::as_ref);
                if !fold(country).starts_with(w.as_str()) {
                    continue;
                }
            }
            found.push((index, row));
        }
        let descending = q.descending.unwrap_or_else(|| q.sort.default_descending());
        found.sort_by(|(ia, a), (ib, b)| {
            let primary = match q.sort {
                PlayerSort::Name => compare_names(&a.name, &b.name),
                PlayerSort::Games => a.games.cmp(&b.games),
                PlayerSort::Elo => a.max_elo.cmp(&b.max_elo),
                PlayerSort::Oldest => {
                    (a.first.is_unknown(), a.first).cmp(&(b.first.is_unknown(), b.first))
                }
                PlayerSort::Newest => a.last.cmp(&b.last),
                PlayerSort::Country => {
                    let c = |i: &usize| countries.get(*i).map_or("", AsRef::as_ref);
                    c(ia).cmp(c(ib))
                }
            };
            let primary = if descending {
                primary.reverse()
            } else {
                primary
            };
            primary.then_with(|| compare_names(&a.name, &b.name))
        });
        let limit = if q.limit == 0 { found.len() } else { q.limit };
        found
            .into_iter()
            .take(limit)
            .map(|(index, row)| PlayerFound {
                id: row.id,
                name: Arc::clone(&row.name),
                games: row.games,
                first: row.first,
                last: row.last,
                max_elo: row.max_elo,
                country: with_country.then(|| {
                    countries
                        .get(index)
                        .map_or_else(String::new, ToString::to_string)
                }),
            })
            .collect()
    }
}

impl PlayerStats {
    /// Raises the Elo of the players that have none in any game to the peak
    /// rating of the spelling file (`recalcEstimatedRatings`, rule 87):
    /// players with games whose name is not just a surname. `checker` must be
    /// the one for players.
    pub fn apply_spelling_estimates<D: GameSource + ?Sized>(
        &mut self,
        db: &D,
        checker: &SpellChecker,
    ) {
        if checker.kind() != NameKind::Player {
            return;
        }
        let size = usize::try_from(db.names().num_names(NameKind::Player)).unwrap_or(0);
        let mut used = vec![false; size];
        for n in 1..=db.len() {
            let Ok(e) = db.entry(n) else {
                continue;
            };
            for id in [e.white_id, e.black_id] {
                if let Some(slot) = usize::try_from(id).ok().and_then(|i| used.get_mut(i)) {
                    *slot = true;
                }
            }
        }
        for (index, _) in used.iter().enumerate().filter(|(_, u)| **u) {
            let Ok(id) = u32::try_from(index) else {
                continue;
            };
            if self.max_elo(id) != 0 {
                continue;
            }
            let Some(raw) = db.names().get(NameKind::Player, id) else {
                continue;
            };
            let name = bytes_to_string(raw);
            if !is_surname_only(&name) {
                self.raise(id, estimated_peak(checker, &name));
            }
        }
    }
}
