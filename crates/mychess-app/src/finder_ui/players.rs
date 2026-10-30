//! The player finder: the form, its rules and the rows (spec M6-3, section
//! 2 and rules 75 to 80).

use mychess_db::finder::{CountryFilter, PlayerFound, PlayerQuery, PlayerSort};

use super::{TableKey, TableShown};
use crate::error::RequestId;
use crate::panel_calc::Refresh;
use crate::spelling_ui::{FinderPrefs, PlayerSortKey, Shared};

/// The fields of the form of the player finder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerField {
    /// "Player": the start of the name.
    Prefix,
    /// "Games", from.
    GamesMin,
    /// "Games", to.
    GamesMax,
    /// "Elo", from.
    EloMin,
    /// "Elo", to.
    EloMax,
    /// "List Size".
    Limit,
    /// "Country": `no`, `yes` or the start of a code.
    Country,
}

/// The texts of the form, as the user typed them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerForm {
    /// The start of the name.
    pub prefix: String,
    /// Fewest games.
    pub games_min: String,
    /// Most games.
    pub games_max: String,
    /// Lowest Elo.
    pub elo_min: String,
    /// Highest Elo.
    pub elo_max: String,
    /// List size.
    pub limit: String,
    /// `no`, `yes` or a country code.
    pub country: String,
}

/// The Scid's limits (`pfinder.tcl` 23-26, 123).
const GAMES_MIN_MAX: u32 = 100_000;
const GAMES_MAX_MAX: u32 = 1_000_000;
const LIMIT_MAX: u32 = 100_000;

fn number(text: &str, min: u32, max: u32, default: u32) -> u32 {
    text.trim().parse::<u64>().ok().map_or(default, |n| {
        u32::try_from(n).unwrap_or(u32::MAX).clamp(min, max)
    })
}

impl PlayerForm {
    /// The form with the last values of the preferences.
    pub fn from_prefs(prefs: &FinderPrefs) -> PlayerForm {
        PlayerForm {
            prefix: String::new(),
            games_min: prefs.games_min.to_string(),
            games_max: prefs.games_max.to_string(),
            elo_min: prefs.elo_min.to_string(),
            elo_max: prefs.elo_max.to_string(),
            limit: prefs.limit.to_string(),
            country: "no".to_owned(),
        }
    }

    /// The form changes one field.
    pub(crate) fn set(&mut self, field: PlayerField, text: String) {
        match field {
            PlayerField::Prefix => self.prefix = text,
            PlayerField::GamesMin => self.games_min = text,
            PlayerField::GamesMax => self.games_max = text,
            PlayerField::EloMin => self.elo_min = text,
            PlayerField::EloMax => self.elo_max = text,
            PlayerField::Limit => self.limit = text,
            PlayerField::Country => self.country = text,
        }
    }

    /// Applies the checks of the Scid (`check`, 265-272): numbers inside
    /// their ranges, a maximum never below the minimum, the country as
    /// `no`, `yes` or up to three capital letters. The texts are rewritten
    /// as the checked values.
    pub(crate) fn normalize(&mut self, prefs: &FinderPrefs) {
        let games_min = number(&self.games_min, 0, GAMES_MIN_MAX, prefs.games_min);
        let games_max = number(&self.games_max, 1, GAMES_MAX_MAX, prefs.games_max).max(games_min);
        let top = u32::from(mychess_core::MAX_ELO);
        let elo_min = number(&self.elo_min, 0, top, u32::from(prefs.elo_min));
        let elo_max = number(&self.elo_max, 0, top, u32::from(prefs.elo_max)).max(elo_min);
        self.games_min = games_min.to_string();
        self.games_max = games_max.to_string();
        self.elo_min = elo_min.to_string();
        self.elo_max = elo_max.to_string();
        self.limit = number(&self.limit, 0, LIMIT_MAX, prefs.limit).to_string();
        let country = self.country.trim();
        self.country = match country.to_ascii_lowercase().as_str() {
            "" | "no" => "no".to_owned(),
            "yes" => "yes".to_owned(),
            _ => country
                .chars()
                .filter(char::is_ascii_alphabetic)
                .take(3)
                .collect::<String>()
                .to_ascii_uppercase(),
        };
        if self.country.is_empty() {
            "no".clone_into(&mut self.country);
        }
    }

    /// The query of a normalized form.
    pub(crate) fn query(&self, sort: PlayerSortKey, descending: Option<bool>) -> PlayerQuery {
        let defaults = FinderPrefs::default();
        let top = u32::from(mychess_core::MAX_ELO);
        let country = match self.country.as_str() {
            "no" => CountryFilter::No,
            "yes" => CountryFilter::All,
            code => CountryFilter::Prefix(code.to_owned()),
        };
        PlayerQuery {
            prefix: self.prefix.clone(),
            games: number(&self.games_min, 0, GAMES_MIN_MAX, defaults.games_min)
                ..=number(&self.games_max, 1, GAMES_MAX_MAX, defaults.games_max),
            elo: u16::try_from(number(&self.elo_min, 0, top, 0)).unwrap_or(0)
                ..=u16::try_from(number(&self.elo_max, 0, top, top))
                    .unwrap_or(mychess_core::MAX_ELO),
            country,
            sort: match sort {
                PlayerSortKey::Name => PlayerSort::Name,
                PlayerSortKey::Games => PlayerSort::Games,
                PlayerSortKey::Oldest => PlayerSort::Oldest,
                PlayerSortKey::Newest => PlayerSort::Newest,
                PlayerSortKey::Elo => PlayerSort::Elo,
                PlayerSortKey::Country => PlayerSort::Country,
            },
            descending,
            limit: usize::try_from(number(&self.limit, 0, LIMIT_MAX, defaults.limit)).unwrap_or(0),
        }
    }
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRowView {
    /// The place in the list, from 1.
    pub rank: u32,
    /// The name.
    pub name: String,
    /// The games.
    pub games: u32,
    /// The year of the oldest game.
    pub first_year: Option<u16>,
    /// The year of the newest game.
    pub last_year: Option<u16>,
    /// The country code (`~~~` when unknown); `None` without the column.
    pub country: Option<String>,
    /// The highest Elo; `None` when 0.
    pub elo: Option<u16>,
}

impl PlayerRowView {
    pub(crate) fn of(rank: usize, found: &PlayerFound) -> PlayerRowView {
        PlayerRowView {
            rank: u32::try_from(rank + 1).unwrap_or(u32::MAX),
            name: found.name.to_string(),
            games: found.games,
            first_year: found.first.year(),
            last_year: found.last.year(),
            country: found.country.clone(),
            elo: Some(found.max_elo).filter(|e| *e != 0),
        }
    }
}

/// The state of the player finder window.
#[derive(Debug)]
pub(crate) struct PlayerFinderUi {
    pub(crate) open: bool,
    pub(crate) form: PlayerForm,
    /// The form was filled from the preferences.
    pub(crate) started: bool,
    pub(crate) sort: PlayerSortKey,
    pub(crate) descending: Option<bool>,
    /// The query asked for with "Update"; the rows answer it.
    pub(crate) applied: Option<PlayerQuery>,
    pub(crate) refresh: Refresh<TableKey>,
    pub(crate) table: Option<TableShown>,
    /// The query in flight.
    pub(crate) querying: Option<RequestId>,
    pub(crate) rows: Shared<Vec<PlayerRowView>>,
    /// The country column is part of `rows`.
    pub(crate) with_country: bool,
}

impl Default for PlayerFinderUi {
    fn default() -> PlayerFinderUi {
        PlayerFinderUi {
            open: false,
            form: PlayerForm::from_prefs(&FinderPrefs::default()),
            started: false,
            sort: PlayerSortKey::Name,
            descending: None,
            applied: None,
            refresh: Refresh::default(),
            table: None,
            querying: None,
            rows: Shared::new(Vec::new()),
            with_country: false,
        }
    }
}

/// The rows for the answer of a query.
pub(crate) fn rows_of(found: &[PlayerFound]) -> Vec<PlayerRowView> {
    found
        .iter()
        .enumerate()
        .map(|(i, f)| PlayerRowView::of(i, f))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> PlayerForm {
        PlayerForm::from_prefs(&FinderPrefs::default())
    }

    #[test]
    fn the_defaults_are_the_ones_of_the_spec() {
        let mut f = form();
        f.normalize(&FinderPrefs::default());
        let q = f.query(PlayerSortKey::Name, None);
        assert_eq!(q, PlayerQuery::default());
    }

    #[test]
    fn a_maximum_below_the_minimum_is_raised_to_it() {
        let mut f = form();
        f.games_min = "50".to_owned();
        f.games_max = "10".to_owned();
        f.elo_min = "2500".to_owned();
        f.elo_max = "2000".to_owned();
        f.normalize(&FinderPrefs::default());
        assert_eq!((f.games_min.as_str(), f.games_max.as_str()), ("50", "50"));
        assert_eq!((f.elo_min.as_str(), f.elo_max.as_str()), ("2500", "2500"));
    }

    #[test]
    fn numbers_are_limited_and_junk_falls_back() {
        let mut f = form();
        f.elo_max = "99999".to_owned();
        f.games_max = "abc".to_owned();
        f.limit = "-5".to_owned();
        f.normalize(&FinderPrefs::default());
        assert_eq!(f.elo_max, "4000");
        assert_eq!(f.games_max, "100000", "the preference");
        assert_eq!(f.limit, "1000", "junk takes the preference");
        f.limit = "9999999".to_owned();
        f.normalize(&FinderPrefs::default());
        assert_eq!(f.limit, "100000");
    }

    #[test]
    fn the_country_is_no_yes_or_a_capital_code() {
        let mut f = form();
        for (typed, shown) in [
            ("", "no"),
            ("NO", "no"),
            ("Yes", "yes"),
            ("bra", "BRA"),
            ("b1r2a3zil", "BRA"),
            ("123", "no"),
        ] {
            f.country = typed.to_owned();
            f.normalize(&FinderPrefs::default());
            assert_eq!(f.country, shown, "{typed}");
        }
        f.country = "BR".to_owned();
        f.normalize(&FinderPrefs::default());
        assert_eq!(
            f.query(PlayerSortKey::Name, None).country,
            CountryFilter::Prefix("BR".to_owned())
        );
    }
}
