//! The tournament finder: the form, its checks and the rows (spec M6-3,
//! section 4 and rules 91 to 100).

use std::sync::Arc;

use mychess_core::PartialDate;
use mychess_db::finder::{Standing, TournamentQuery, TournamentRow, TournamentSort};

use super::TourKey;
use crate::error::RequestId;
use crate::panel_calc::Refresh;
use crate::spelling_ui::{
    DEFAULT_LIST_SIZE, DEFAULT_TOURNAMENT_START, Shared, TournamentPrefs, TournamentSortKey,
};

/// The fields of the form of the tournament finder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TournamentField {
    /// "Games", from.
    GamesMin,
    /// "Games", to.
    GamesMax,
    /// "Players", from.
    PlayersMin,
    /// "Players", to.
    PlayersMax,
    /// "Mean Elo", from.
    EloMin,
    /// "Mean Elo", to.
    EloMax,
    /// "Date", from.
    Start,
    /// "Date", to.
    End,
    /// "Event".
    Event,
    /// "Site".
    Site,
    /// "Player".
    Player,
    /// "Country".
    Country,
    /// "List Size".
    Limit,
}

/// The texts of the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentForm {
    /// Fewest games.
    pub games_min: String,
    /// Most games.
    pub games_max: String,
    /// Fewest players.
    pub players_min: String,
    /// Most players.
    pub players_max: String,
    /// Lowest mean Elo.
    pub elo_min: String,
    /// Highest mean Elo.
    pub elo_max: String,
    /// First date (`AAAA.MM.DD` with `?`).
    pub start: String,
    /// Last date; empty = no limit.
    pub end: String,
    /// Part of the event name.
    pub event: String,
    /// Part of the site name.
    pub site: String,
    /// Part of a player's name.
    pub player: String,
    /// Three letters of the country code.
    pub country: String,
    /// List size.
    pub limit: String,
}

const MAX_COUNT: u32 = 1_000_000;

fn number(text: &str, min: u32, max: u32, default: u32) -> u32 {
    text.trim().parse::<u64>().ok().map_or(default, |n| {
        u32::try_from(n).unwrap_or(u32::MAX).clamp(min, max)
    })
}

/// `AAAA` or `AAAA.MM` completed the way `check` of `tourney.tcl` does.
fn complete_date(text: &str, end: bool) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = text.split('.').collect();
    let fill = |tail: &str| format!("{text}{tail}");
    let done = match parts.len() {
        1 => fill(if end { ".12.31" } else { ".??.??" }),
        2 => fill(if end { ".31" } else { ".??" }),
        _ => text.to_owned(),
    };
    if !end && done.ends_with(".01.01") {
        return format!("{}.??.??", done.get(..4).unwrap_or(""));
    }
    done
}

impl TournamentForm {
    /// The defaults of the window (`defaults`, rule 111 of the spec).
    pub fn defaults() -> TournamentForm {
        TournamentForm::from_prefs(&TournamentPrefs::default())
    }

    /// The form with the last values of the preferences.
    pub fn from_prefs(prefs: &TournamentPrefs) -> TournamentForm {
        TournamentForm {
            games_min: prefs.min_games.to_string(),
            games_max: "9999".to_owned(),
            players_min: prefs.min_players.to_string(),
            players_max: "1999".to_owned(),
            elo_min: "0".to_owned(),
            elo_max: mychess_core::MAX_ELO.to_string(),
            start: prefs.start.clone(),
            end: String::new(),
            event: String::new(),
            site: String::new(),
            player: String::new(),
            country: String::new(),
            limit: prefs.limit.to_string(),
        }
    }

    /// The form of "Open &lt;player&gt;" (rule 4): no lower date, the player
    /// filled in, one player and one game are enough.
    pub fn for_player(name: &str, prefs: &TournamentPrefs) -> TournamentForm {
        TournamentForm {
            start: "1800.??.??".to_owned(),
            player: name.to_owned(),
            players_min: "1".to_owned(),
            games_min: "1".to_owned(),
            ..TournamentForm::from_prefs(prefs)
        }
    }

    pub(crate) fn set(&mut self, field: TournamentField, text: String) {
        let slot = match field {
            TournamentField::GamesMin => &mut self.games_min,
            TournamentField::GamesMax => &mut self.games_max,
            TournamentField::PlayersMin => &mut self.players_min,
            TournamentField::PlayersMax => &mut self.players_max,
            TournamentField::EloMin => &mut self.elo_min,
            TournamentField::EloMax => &mut self.elo_max,
            TournamentField::Start => &mut self.start,
            TournamentField::End => &mut self.end,
            TournamentField::Event => &mut self.event,
            TournamentField::Site => &mut self.site,
            TournamentField::Player => &mut self.player,
            TournamentField::Country => &mut self.country,
            TournamentField::Limit => &mut self.limit,
        };
        *slot = text;
    }

    /// The checks of `check` (rule 3): dates completed, at least two players
    /// at the least, a maximum never below the minimum, the country in
    /// capitals with three letters (`---` is none). A date that is not a
    /// date is replaced by the default.
    pub(crate) fn normalize(&mut self) {
        self.start = complete_date(&self.start, false);
        self.end = complete_date(&self.end, true);
        if !PartialDate::is_valid_str(&self.start) {
            DEFAULT_TOURNAMENT_START.clone_into(&mut self.start);
        }
        if !PartialDate::is_valid_str(&self.end) {
            self.end.clear();
        }
        let games_min = number(&self.games_min, 0, MAX_COUNT, 0);
        let games_max = number(&self.games_max, 1, MAX_COUNT, 9999).max(games_min);
        let players_min = number(&self.players_min, 1, 1999, 4);
        let players_max = number(&self.players_max, 1, 1999, 1999).max(players_min);
        let top = u32::from(mychess_core::MAX_ELO);
        let elo_min = number(&self.elo_min, 0, top, 0);
        let elo_max = number(&self.elo_max, 0, top, top).max(elo_min);
        self.games_min = games_min.to_string();
        self.games_max = games_max.to_string();
        self.players_min = players_min.to_string();
        self.players_max = players_max.to_string();
        self.elo_min = elo_min.to_string();
        self.elo_max = elo_max.to_string();
        self.limit = number(&self.limit, 0, 100_000, DEFAULT_LIST_SIZE).to_string();
        let country: String = self
            .country
            .trim()
            .chars()
            .filter(char::is_ascii_alphabetic)
            .take(3)
            .collect::<String>()
            .to_ascii_uppercase();
        self.country = if country.len() == 3 {
            country
        } else {
            String::new()
        };
    }

    /// The query of a normalized form.
    pub(crate) fn query(&self, sort: TournamentSortKey) -> TournamentQuery {
        let top = u32::from(mychess_core::MAX_ELO);
        let country = <[u8; 3]>::try_from(self.country.as_bytes()).ok();
        TournamentQuery {
            start: PartialDate::parse_lenient(&self.start),
            end: PartialDate::parse_lenient(&self.end),
            country,
            site: self.site.clone(),
            event: self.event.clone(),
            player: self.player.clone(),
            games: number(&self.games_min, 0, MAX_COUNT, 0)
                ..=number(&self.games_max, 1, MAX_COUNT, 9999),
            players: number(&self.players_min, 1, 1999, 4)
                ..=number(&self.players_max, 1, 1999, 1999),
            mean_elo: u16::try_from(number(&self.elo_min, 0, top, 0)).unwrap_or(0)
                ..=u16::try_from(number(&self.elo_max, 0, top, top))
                    .unwrap_or(mychess_core::MAX_ELO),
            sort: match sort {
                TournamentSortKey::Date => TournamentSort::Date,
                TournamentSortKey::Players => TournamentSort::Players,
                TournamentSortKey::Games => TournamentSort::Games,
                TournamentSortKey::Elo => TournamentSort::Elo,
                TournamentSortKey::Event => TournamentSort::Event,
                TournamentSortKey::Site => TournamentSort::Site,
                TournamentSortKey::Winner => TournamentSort::Winner,
            },
            limit: usize::try_from(number(&self.limit, 0, 100_000, DEFAULT_LIST_SIZE)).unwrap_or(0),
        }
    }
}

/// The first or the second of a tournament.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandingView {
    /// The name.
    pub name: String,
    /// The text before the first comma (what the list shows).
    pub surname: String,
    /// The Elo; `None` when 0.
    pub elo: Option<u16>,
    /// The points as text (`3.5`).
    pub points: String,
}

impl StandingView {
    fn of(standing: &Standing) -> StandingView {
        let name = standing.name.to_string();
        StandingView {
            surname: name.split(',').next().unwrap_or("").trim().to_owned(),
            name,
            elo: Some(standing.elo).filter(|e| *e != 0),
            points: standing.points_text(),
        }
    }
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentRowView {
    /// The place in the list, from 1.
    pub rank: u32,
    /// The date of the first game.
    pub date: PartialDate,
    /// The site.
    pub site: String,
    /// The event.
    pub event: String,
    /// The players.
    pub players: u32,
    /// The games.
    pub games: u32,
    /// The mean Elo; `None` when no game has one.
    pub mean_elo: Option<u16>,
    /// The number of the first game, from 1.
    pub first_game: u32,
    /// The winner.
    pub winner: Option<StandingView>,
    /// The second.
    pub runner_up: Option<StandingView>,
    /// The first two have the same points (`1=` and `1=`).
    pub tie: bool,
}

impl TournamentRowView {
    pub(crate) fn of(index: usize, row: &TournamentRow) -> TournamentRowView {
        TournamentRowView {
            rank: u32::try_from(index + 1).unwrap_or(u32::MAX),
            date: row.date,
            site: row.site.to_string(),
            event: row.event.to_string(),
            players: row.players,
            games: row.games,
            mean_elo: Some(row.mean_elo).filter(|e| *e != 0),
            first_game: row.first_game,
            winner: row.winner.as_ref().map(StandingView::of),
            runner_up: row.runner_up.as_ref().map(StandingView::of),
            tie: matches!((&row.winner, &row.runner_up), (Some(a), Some(b)) if a.points2 == b.points2),
        }
    }
}

/// What a click on a row does (D3315, D3317).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TournamentAction {
    /// Shows the table of the tournament (the crosstable panel).
    OpenCrosstable,
    /// The filter of the database holds the games of the tournament.
    ShowInFilter,
    /// Loads the first game into the editor.
    LoadFirstGame,
}

/// The rows of an answer, with the games of each tournament.
#[derive(Debug)]
pub(crate) struct Found {
    pub(crate) key: TourKey,
    pub(crate) rows: Shared<Vec<TournamentRowView>>,
    /// The numbers of the games of every row, parallel to `rows`.
    pub(crate) games: Vec<Arc<[u32]>>,
    /// The first game of every row, parallel to `rows`.
    pub(crate) first: Vec<u32>,
}

/// The state of the tournament finder window.
#[derive(Debug)]
pub(crate) struct TournamentFinderUi {
    pub(crate) open: bool,
    pub(crate) form: TournamentForm,
    pub(crate) started: bool,
    pub(crate) sort: TournamentSortKey,
    /// The query asked for with "Update".
    pub(crate) applied: Option<TournamentQuery>,
    pub(crate) refresh: Refresh<TourKey>,
    pub(crate) found: Option<Found>,
    pub(crate) running_request: Option<RequestId>,
}

impl Default for TournamentFinderUi {
    fn default() -> TournamentFinderUi {
        TournamentFinderUi {
            open: false,
            form: TournamentForm::defaults(),
            started: false,
            sort: TournamentSortKey::Date,
            applied: None,
            refresh: Refresh::default(),
            found: None,
            running_request: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_completed_as_check_does() {
        assert_eq!(complete_date("1990", false), "1990.??.??");
        assert_eq!(complete_date("1990", true), "1990.12.31");
        assert_eq!(complete_date("1990.05", false), "1990.05.??");
        assert_eq!(complete_date("1990.05", true), "1990.05.31");
        assert_eq!(complete_date("1990.01.01", false), "1990.??.??");
        assert_eq!(complete_date("1990.01.01", true), "1990.01.01");
        assert_eq!(complete_date("  ", true), "");
    }

    #[test]
    fn the_checks_fix_players_countries_and_bad_dates() {
        let mut f = TournamentForm::defaults();
        f.players_min = "0".to_owned();
        f.players_max = "1".to_owned();
        f.country = "br".to_owned();
        f.start = "oops".to_owned();
        f.end = "2020".to_owned();
        f.normalize();
        assert_eq!(f.players_min, "1", "a player is the least");
        assert_eq!(f.players_max, "1");
        assert_eq!(f.country, "", "two letters are no code");
        assert_eq!(f.start, DEFAULT_TOURNAMENT_START);
        assert_eq!(f.end, "2020.12.31");
        f.country = "---".to_owned();
        f.normalize();
        assert_eq!(f.country, "");
        f.country = "bra".to_owned();
        f.normalize();
        assert_eq!(f.country, "BRA");
        assert_eq!(f.query(TournamentSortKey::Date).country, Some(*b"BRA"));
    }

    #[test]
    fn the_defaults_and_the_player_form_follow_the_spec() {
        let f = TournamentForm::defaults();
        let q = f.query(TournamentSortKey::Date);
        assert_eq!(q.start, PartialDate::parse_lenient("1960.??.??"));
        assert_eq!((*q.players.start(), *q.players.end()), (4, 1999));
        assert_eq!((*q.games.start(), *q.games.end()), (10, 9999));
        assert_eq!(q.limit, 1000);
        let p = TournamentForm::for_player("Alfa, A", &TournamentPrefs::default());
        let q = p.query(TournamentSortKey::Date);
        assert_eq!(q.player, "Alfa, A");
        assert_eq!(q.start, PartialDate::parse_lenient("1800.??.??"));
        assert_eq!((*q.players.start(), *q.games.start()), (1, 1));
        assert!(q.event.is_empty() && q.site.is_empty());
    }
}
