//! The table of statistics of the filter and its `[stats]` preferences
//! (spec M6-4, slice b, D3370, D3371, D3403).

use std::collections::BTreeMap;

use mychess_db::{FilterStatsRow, StatsConfig, StatsLine, StatsQuery};
use serde::{Serialize, Serializer};

/// The criterion of a line, for the front-end to word (`All games`, `Both
/// rated 2400+`, `2005 - 2010`, `2010 - `, `Since 2010.01.01`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatsLabel {
    /// Every game of the filter.
    All,
    /// Both players rated at least this.
    BothRated(u16),
    /// A range of years, `from <= year < to`; `None` is open.
    Years {
        /// First year.
        from: u16,
        /// First year left out.
        to: Option<u16>,
    },
    /// Since this year.
    Since(u16),
}

impl From<StatsQuery> for StatsLabel {
    fn from(query: StatsQuery) -> StatsLabel {
        match query {
            StatsQuery::All => StatsLabel::All,
            StatsQuery::BothRated(n) => StatsLabel::BothRated(n),
            StatsQuery::Years { from, to } => StatsLabel::Years { from, to },
            StatsQuery::Since(y) => StatsLabel::Since(y),
        }
    }
}

/// A line of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatsLineView {
    /// The criterion.
    pub label: StatsLabel,
    /// Games.
    pub total: u32,
    /// White wins.
    pub white: u32,
    /// Draws.
    pub draw: u32,
    /// Black wins.
    pub black: u32,
    /// Games without result (they count half a point).
    pub none: u32,
    /// The score in tenths of a percent.
    pub score_tenths: u32,
    /// Index entries that could not be read and are in no count (0 for a
    /// healthy database).
    pub skipped_entries: u32,
}

impl StatsLineView {
    /// The score as `52.3%` (`%3u.%u%%` of the Scid, no padding).
    pub fn score_text(&self) -> String {
        format!("{}.{}%", self.score_tenths / 10, self.score_tenths % 10)
    }
}

impl From<&StatsLine> for StatsLineView {
    fn from(line: &StatsLine) -> StatsLineView {
        let FilterStatsRow {
            total,
            white,
            draw,
            black,
            none,
            skipped_entries,
        } = line.row;
        StatsLineView {
            label: line.query.into(),
            total,
            white,
            draw,
            black,
            none,
            score_tenths: line.row.score_tenths(),
            skipped_entries,
        }
    }
}

/// The choices of the "Options" menu of the statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsOptionsView {
    /// Rating thresholds offered and whether each is checked.
    pub elo: Vec<(u16, bool)>,
    /// Years offered and whether each is checked.
    pub years: Vec<(u16, bool)>,
}

/// Highest rating threshold or year the preferences keep.
const MAX_CHOICE: u16 = 3000;
/// Most thresholds or years the preferences keep.
const MAX_CHOICES: usize = 200;

/// The lines of the statistics table that the user checked, in `[stats]`.
/// `None` is the Scid's default (thresholds 2600, 2400, 2200, 2000; years
/// 2000, 2005, 2010 and 2011 up to the current year, D3371).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatsPrefs {
    /// The checked rating thresholds, if the user chose.
    pub elo_checked: Option<Vec<u16>>,
    /// The checked years, if the user chose.
    pub years_checked: Option<Vec<u16>>,
    /// The current year when the user last chose the years: the years up to
    /// it were decided (`years_checked`), later ones are new and start
    /// checked, as in the Scid (D3426). `None` (an older file): no year is
    /// added to what the user chose.
    pub years_known: Option<u16>,
}

fn tidy(list: &[u16]) -> Vec<u16> {
    let mut list: Vec<u16> = list.iter().copied().filter(|n| *n <= MAX_CHOICE).collect();
    list.sort_unstable();
    list.dedup();
    list.truncate(MAX_CHOICES);
    list
}

impl StatsPrefs {
    /// The preferences with sorted, bounded lists.
    #[must_use]
    pub fn sanitized(self) -> StatsPrefs {
        StatsPrefs {
            elo_checked: self.elo_checked.map(|l| tidy(&l)),
            years_checked: self.years_checked.map(|l| tidy(&l)),
            years_known: self.years_known.filter(|y| *y <= MAX_CHOICE),
        }
    }

    /// The configuration for `current_year`: the defaults, then the choices
    /// of the user (a threshold or a year that is not checked is off).
    pub(crate) fn config(&self, current_year: u16) -> StatsConfig {
        let mut config = StatsConfig::new(current_year);
        let apply = |map: &mut BTreeMap<u16, bool>, checked: &Option<Vec<u16>>| {
            if let Some(list) = checked {
                for on in map.values_mut() {
                    *on = false;
                }
                for n in list {
                    map.insert(*n, true);
                }
            }
        };
        apply(&mut config.elo, &self.elo_checked);
        if let Some(list) = &self.years_checked {
            // Scid: tcl/start.tcl, a year after the ones the user chose
            // starts checked.
            let known = self.years_known.unwrap_or(u16::MAX);
            for (year, on) in &mut config.years {
                if *year <= known {
                    *on = false;
                }
            }
            for n in list {
                config.years.insert(*n, true);
            }
        }
        config
    }

    /// The preferences after the user checked or unchecked a threshold.
    pub(crate) fn with_elo(&self, current_year: u16, value: u16, on: bool) -> StatsPrefs {
        let config = self.config(current_year);
        StatsPrefs {
            elo_checked: Some(checked(&config.elo, value, on)),
            years_checked: self.years_checked.clone(),
            years_known: self.years_known,
        }
        .sanitized()
    }

    /// The preferences after the user checked or unchecked a year.
    pub(crate) fn with_year(&self, current_year: u16, value: u16, on: bool) -> StatsPrefs {
        let config = self.config(current_year);
        StatsPrefs {
            elo_checked: self.elo_checked.clone(),
            years_checked: Some(checked(&config.years, value, on)),
            years_known: Some(current_year),
        }
        .sanitized()
    }

    /// The menu for `current_year`.
    pub(crate) fn options_view(&self, current_year: u16) -> StatsOptionsView {
        let config = self.config(current_year);
        StatsOptionsView {
            elo: config.elo.into_iter().collect(),
            years: config.years.into_iter().collect(),
        }
    }
}

fn checked(map: &BTreeMap<u16, bool>, value: u16, on: bool) -> Vec<u16> {
    let mut list: Vec<u16> = map
        .iter()
        .filter(|(k, v)| **v && **k != value)
        .map(|(k, _)| *k)
        .collect();
    if on {
        list.push(value);
    }
    list
}

#[derive(Serialize)]
struct Table<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    elo_checked: &'a Option<Vec<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    years_checked: &'a Option<Vec<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    years_known: &'a Option<u16>,
}

impl Serialize for StatsPrefs {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Table {
            elo_checked: &self.elo_checked,
            years_checked: &self.years_checked,
            years_known: &self.years_known,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_prefs_give_the_defaults_of_scid() {
        let config = StatsPrefs::default().config(2013);
        assert_eq!(config, StatsConfig::new(2013));
    }

    #[test]
    fn an_old_file_without_years_known_adds_no_year() {
        let loaded = crate::load_preferences("[stats]\nyears_checked = [2000, 2011]\n");
        assert_eq!(loaded.prefs.stats.years_known, None);
        let config = loaded.prefs.stats.config(2030);
        let on: Vec<u16> = config
            .years
            .iter()
            .filter(|(_, on)| **on)
            .map(|(y, _)| *y)
            .collect();
        assert_eq!(on, [2000, 2011], "later years do not start checked");
        // With the year the user last chose, a later one starts checked.
        let known = StatsPrefs {
            years_known: Some(2013),
            ..loaded.prefs.stats.as_ref().clone()
        };
        assert_eq!(known.config(2014).years.get(&2014), Some(&true));
    }

    #[test]
    fn a_choice_replaces_the_defaults() {
        let prefs = StatsPrefs::default().with_elo(2013, 2400, false);
        let config = prefs.config(2013);
        assert_eq!(config.elo.get(&2400), Some(&false));
        assert_eq!(config.elo.get(&2600), Some(&true));
        // The years were not touched.
        assert_eq!(config.years, StatsConfig::new(2013).years);
        let prefs = prefs.with_year(2013, 2012, false);
        assert_eq!(prefs.config(2013).years.get(&2012), Some(&false));
        assert_eq!(prefs.config(2013).years.get(&2013), Some(&true));
    }

    #[test]
    fn a_year_after_the_ones_chosen_starts_checked() {
        let prefs = StatsPrefs::default().with_year(2013, 2012, false);
        let later = prefs.config(2015);
        assert_eq!(later.years.get(&2012), Some(&false));
        for y in [2011, 2013, 2014, 2015] {
            assert_eq!(later.years.get(&y), Some(&true), "{y}");
        }
        // An older file has no horizon: nothing is added to the choice.
        let old = StatsPrefs {
            years_known: None,
            ..prefs
        };
        assert_eq!(old.config(2015).years.get(&2015), Some(&false));
    }

    #[test]
    fn lists_are_sorted_bounded_and_unique() {
        let p = StatsPrefs {
            elo_checked: Some(vec![2400, 2400, 9999, 2000]),
            years_checked: None,
            years_known: None,
        }
        .sanitized();
        assert_eq!(p.elo_checked, Some(vec![2000, 2400]));
    }

    #[test]
    fn score_text_has_one_decimal() {
        let line = StatsLineView {
            label: StatsLabel::All,
            total: 3,
            white: 1,
            draw: 1,
            black: 1,
            none: 0,
            score_tenths: 505,
            skipped_entries: 0,
        };
        assert_eq!(line.score_text(), "50.5%");
    }
}
