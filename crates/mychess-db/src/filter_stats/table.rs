//! The lines of the statistics window (`::windows::stats::Refresh` of
//! `tcl/windows/stats.tcl`, spec 2.2, rules S1 to S4).

use std::collections::BTreeMap;

use super::{FilterStatsRow, StatsError, StatsQuery, filter_stats_rows};
use crate::filter::Filter;
use crate::progress::{CancelToken, ProgressSink};
use crate::source::GameSource;

/// Rating thresholds of the "both rated" lines the Scid offers.
const ELO_CHOICES: [(u16, bool); 7] = [
    (2600, true),
    (2500, false),
    (2400, true),
    (2300, false),
    (2200, true),
    (2000, true),
    (1800, false),
];

/// Years of the "since" and interval lines the Scid offers (besides 2011 up
/// to the current year, which are all offered and checked).
const YEAR_CHOICES: [(u16, bool); 9] = [
    (1900, false),
    (1950, false),
    (1960, false),
    (1970, false),
    (1980, false),
    (1990, false),
    (2000, true),
    (2005, true),
    (2010, true),
];

/// Which lines the statistics window shows: the rating thresholds and the
/// years that are checked (`::windows::stats::display`). The preference is
/// kept by the application (`[stats]` of `prefs.toml`); a threshold or a year
/// that is not in the maps is not offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsConfig {
    /// Rating threshold to checked.
    pub elo: BTreeMap<u16, bool>,
    /// Year to checked.
    pub years: BTreeMap<u16, bool>,
}

impl StatsConfig {
    /// The defaults of Scid (`start.tcl` 649-674): thresholds 2600, 2400,
    /// 2200 and 2000 checked (2500, 2300 and 1800 offered); the years 2000,
    /// 2005, 2010 and **every year from 2011 to `current_year`** checked
    /// (1900, 1950, 1960, 1970, 1980 and 1990 offered) (rule S4).
    pub fn new(current_year: u16) -> StatsConfig {
        let mut years: BTreeMap<u16, bool> = YEAR_CHOICES.into_iter().collect();
        for year in 2011..=current_year {
            years.insert(year, true);
        }
        StatsConfig {
            elo: ELO_CHOICES.into_iter().collect(),
            years,
        }
    }

    /// The criteria in the order of the window: all games; the checked
    /// thresholds from the highest down; the interval of each checked year up
    /// to the next checked year (the last one is open); the "since" line of
    /// each checked year.
    pub fn queries(&self) -> Vec<StatsQuery> {
        let mut queries = vec![StatsQuery::All];
        let mut elos: Vec<u16> = self
            .elo
            .iter()
            .filter_map(|(&elo, &on)| on.then_some(elo))
            .collect();
        elos.sort_unstable_by(|a, b| b.cmp(a));
        queries.extend(elos.into_iter().map(StatsQuery::BothRated));
        let years: Vec<u16> = self
            .years
            .iter()
            .filter_map(|(&year, &on)| on.then_some(year))
            .collect();
        for (i, &from) in years.iter().enumerate() {
            queries.push(StatsQuery::Years {
                from,
                to: years.get(i + 1).copied(),
            });
        }
        queries.extend(years.into_iter().map(StatsQuery::Since));
        queries
    }
}

/// A line of the table: the criterion and its counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatsLine {
    /// The criterion (what the label says).
    pub query: StatsQuery,
    /// The counts.
    pub row: FilterStatsRow,
}

/// The whole table of the window for `config`, in one pass over the index.
///
/// # Errors
///
/// [`StatsError::FilterSize`] if `filter` does not cover `db`;
/// [`StatsError::Cancelled`] if `cancel` is set.
pub fn filter_stats_table<D: GameSource + ?Sized>(
    db: &D,
    filter: &Filter,
    config: &StatsConfig,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<Vec<StatsLine>, StatsError> {
    let queries = config.queries();
    let rows = filter_stats_rows(db, filter, &queries, progress, cancel)?;
    Ok(queries
        .into_iter()
        .zip(rows)
        .map(|(query, row)| StatsLine { query, row })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_those_of_scid() {
        let config = StatsConfig::new(2014);
        let on = |map: &BTreeMap<u16, bool>| -> Vec<u16> {
            map.iter().filter(|(_, on)| **on).map(|(&k, _)| k).collect()
        };
        assert_eq!(on(&config.elo), [2000, 2200, 2400, 2600]);
        assert_eq!(
            on(&config.years),
            [2000, 2005, 2010, 2011, 2012, 2013, 2014]
        );
        assert_eq!(config.elo.len(), 7);
        assert_eq!(config.years.get(&1990), Some(&false));
    }

    #[test]
    fn queries_follow_the_order_of_the_window() {
        let mut config = StatsConfig::new(2011);
        config.elo.retain(|&elo, _| elo == 2400 || elo == 2600);
        config
            .years
            .retain(|&y, _| y == 2000 || y == 2010 || y == 2011);
        assert_eq!(
            config.queries(),
            [
                StatsQuery::All,
                StatsQuery::BothRated(2600),
                StatsQuery::BothRated(2400),
                StatsQuery::Years {
                    from: 2000,
                    to: Some(2010)
                },
                StatsQuery::Years {
                    from: 2010,
                    to: Some(2011)
                },
                StatsQuery::Years {
                    from: 2011,
                    to: None
                },
                StatsQuery::Since(2000),
                StatsQuery::Since(2010),
                StatsQuery::Since(2011),
            ]
        );
    }

    #[test]
    fn an_unchecked_year_widens_the_interval_before_it() {
        let mut config = StatsConfig::new(2010);
        config.years.insert(2005, false);
        let intervals: Vec<_> = config
            .queries()
            .into_iter()
            .filter(|q| matches!(q, StatsQuery::Years { .. }))
            .collect();
        assert_eq!(
            intervals,
            [
                StatsQuery::Years {
                    from: 2000,
                    to: Some(2010)
                },
                StatsQuery::Years {
                    from: 2010,
                    to: None
                }
            ]
        );
    }
}
