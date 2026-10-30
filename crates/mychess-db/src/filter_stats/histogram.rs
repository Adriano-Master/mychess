//! The histograms of the filter graphs (`sc_filter freq` and
//! `::tools::graphs::filter::Refresh` / `absfilter::Refresh` of
//! `tcl/tools/graphs.tcl`, spec 2.3, rules G1 to G9 and decision D3372).
//!
//! ## Differences from Scid (D3372, D3395)
//!
//! 1. The last decade, `2010+`, is **open**: every game from 2010 on (the
//!    Scid range ends at 2019.12.31 and loses 2020 and later).
//! 2. A rating bin without "estimate" is **inclusive** at both ends (the Scid
//!    leaves the last value of each bin, such as 2199, out of every bin).
//! 3. The bins of moves use the formula of the absolute graph for both
//!    graphs: `[2i-1, 2(i+step)-2]` half-moves (the relative graph of Scid is
//!    one half-move late).
//! 4. When the first bin of the rating graph starts below 100, it covers the
//!    games from `min` to 1799 (Scid labels it so but reads only
//!    `[min, min+step-1]`).
//! 5. The relative view keeps the per-mille values (1000 games of the bin);
//!    the title is the job of the interface ("per 1000 games"; Scid prints
//!    "100" over per-mille values).
//!
//! (The spec says twelve decades; the Scid list has eleven pairs: `-1919`, the
//! eight decades 20-29 to 90-99, `00-09` and `2010+`.)
//!
//! A game without a date (year 0) falls in the first decade, `-1919`, as in
//! Scid (rule G1).

use super::{StatsError, check_filter, fold_index, year_of};
use crate::filter::Filter;
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::IndexEntry;
use crate::source::GameSource;

/// Most bins a graph has. The fields of the configuration take at most four
/// digits ([`MAX_GRAPH_FIELD`]), so no valid configuration reaches it.
pub const MAX_GRAPH_BINS: usize = 10_000;

/// Largest value of a configuration field (four digits).
pub const MAX_GRAPH_FIELD: i32 = 9999;

/// A rating bin shortens the gap between 0 and "useful" ratings: below this
/// value the first bin runs up to [`ELO_GAP_END`].
const ELO_GAP_START: i64 = 100;

/// Last rating of the first bin when it starts below [`ELO_GAP_START`].
const ELO_GAP_END: i64 = 1799;

/// A lone rating counts at most this much for the other side (rule G2).
const ESTIMATE_CAP: u32 = 2200;

/// Which graph (the "type" buttons of the window).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphKind {
    /// Eleven fixed decades, `-1919` to `2010+`.
    Decade,
    /// One bin per `year_step` years.
    Year,
    /// One bin per `elo_step` rating points.
    Elo,
    /// One bin per `moves_step` full moves.
    Moves,
}

/// The configuration of the X axis (`FilterMinYear` and friends), the same
/// for the relative and the absolute graph. Fields are signed because that is
/// what the entries of the window accept; [`FilterGraphConfig::normalized`]
/// applies `checkConfigFilterGraph`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterGraphConfig {
    /// First year of the Year graph.
    pub year_min: i32,
    /// Last year.
    pub year_max: i32,
    /// Years per bin.
    pub year_step: i32,
    /// First rating of the Rating graph.
    pub elo_min: i32,
    /// Last rating.
    pub elo_max: i32,
    /// Rating points per bin.
    pub elo_step: i32,
    /// First move of the Moves graph.
    pub moves_min: i32,
    /// Last move.
    pub moves_max: i32,
    /// Moves per bin.
    pub moves_step: i32,
    /// "Estimate": the rating of a game is the mean of both sides (rule G2);
    /// otherwise the lower of the two (rule G3).
    pub estimate: bool,
}

impl FilterGraphConfig {
    /// The defaults of Scid (`graphs.tcl` 32-43, `start.tcl` 168-175): years
    /// 1995 to `current_year` by 1, ratings 2100 to 2800 by 100 with
    /// estimate, moves 5 to 80 by 1.
    pub fn new(current_year: u16) -> FilterGraphConfig {
        FilterGraphConfig {
            year_min: 1995,
            year_max: i32::from(current_year),
            year_step: 1,
            elo_min: 2100,
            elo_max: 2800,
            elo_step: 100,
            moves_min: 5,
            moves_max: 80,
            moves_step: 1,
            estimate: true,
        }
    }

    /// Replaces the illegal values by the defaults (rule G6,
    /// `checkConfigFilterGraph`): a step below 1 becomes 1 (years, moves) or
    /// 100 (rating); a minimum below 1 becomes 1 (moves) or 1995 (year), a
    /// rating minimum below 0 becomes 2100; a maximum below 1 becomes 80
    /// (moves), 2800 (rating) or `current_year`. Values above four digits
    /// ([`MAX_GRAPH_FIELD`]) are limited to it.
    #[must_use]
    pub fn normalized(mut self, current_year: u16) -> FilterGraphConfig {
        if self.moves_step < 1 {
            self.moves_step = 1;
        }
        if self.elo_step < 1 {
            self.elo_step = 100;
        }
        if self.year_step < 1 {
            self.year_step = 1;
        }
        if self.moves_min < 1 {
            self.moves_min = 1;
        }
        if self.elo_min < 0 {
            self.elo_min = 2100;
        }
        if self.year_min < 1 {
            self.year_min = 1995;
        }
        if self.moves_max < 1 {
            self.moves_max = 80;
        }
        if self.elo_max < 1 {
            self.elo_max = 2800;
        }
        if self.year_max < 1 {
            self.year_max = i32::from(current_year);
        }
        for field in [
            &mut self.year_min,
            &mut self.year_max,
            &mut self.year_step,
            &mut self.elo_min,
            &mut self.elo_max,
            &mut self.elo_step,
            &mut self.moves_min,
            &mut self.moves_max,
            &mut self.moves_step,
        ] {
            *field = (*field).min(MAX_GRAPH_FIELD);
        }
        self
    }
}

/// A bin: its label and the range of the game key it takes, both ends
/// included. The key is the year (Decade, Year), the number of half-moves
/// (Moves), the lower rating (Rating without estimate) or the sum of both
/// ratings (Rating with estimate: the range is `[2 min, 2 max + 1]`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct BinSpec {
    label: String,
    lo: u32,
    hi: u32,
}

fn to_u32(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

/// The bins of `kind`, in increasing order, disjoint. A range that is empty
/// (maximum below minimum) gives no bins.
fn bin_specs(kind: GraphKind, config: &FilterGraphConfig) -> Vec<BinSpec> {
    let mut bins: Vec<BinSpec> = Vec::new();
    match kind {
        GraphKind::Decade => {
            let decades: [(&str, u32, u32); 11] = [
                ("-1919", 0, 1919),
                ("20-29", 1920, 1929),
                ("30-39", 1930, 1939),
                ("40-49", 1940, 1949),
                ("50-59", 1950, 1959),
                ("60-69", 1960, 1969),
                ("70-79", 1970, 1979),
                ("80-89", 1980, 1989),
                ("90-99", 1990, 1999),
                ("00-09", 2000, 2009),
                // Open: rule 1 of the module doc.
                ("2010+", 2010, u32::MAX),
            ];
            bins.extend(decades.iter().map(|&(label, lo, hi)| BinSpec {
                label: label.to_owned(),
                lo,
                hi,
            }));
        }
        GraphKind::Year => {
            let step = i64::from(config.year_step.max(1));
            let mut i = i64::from(config.year_min.max(0));
            while i <= i64::from(config.year_max) && bins.len() < MAX_GRAPH_BINS {
                bins.push(BinSpec {
                    label: (i + step / 2).to_string(),
                    lo: to_u32(i),
                    hi: to_u32(i + step - 1),
                });
                i += step;
            }
        }
        GraphKind::Elo => {
            let step = i64::from(config.elo_step.max(1));
            let estimate = config.estimate;
            let mut i = i64::from(config.elo_min.max(0));
            while i <= i64::from(config.elo_max) && bins.len() < MAX_GRAPH_BINS {
                let (lo, hi, next) = if i < ELO_GAP_START {
                    (i, ELO_GAP_END, ELO_GAP_END + 1)
                } else {
                    (i, i + step - 1, i + step)
                };
                let (key_lo, key_hi) = if estimate {
                    (2 * lo, 2 * hi + 1)
                } else {
                    (lo, hi)
                };
                bins.push(BinSpec {
                    label: format!("{lo}-{hi}"),
                    lo: to_u32(key_lo),
                    hi: to_u32(key_hi),
                });
                i = next;
            }
        }
        GraphKind::Moves => {
            let step = i64::from(config.moves_step.max(1));
            let mut i = i64::from(config.moves_min.max(1));
            while i <= i64::from(config.moves_max) && bins.len() < MAX_GRAPH_BINS {
                bins.push(BinSpec {
                    label: if i % 5 == 0 {
                        i.to_string()
                    } else {
                        String::new()
                    },
                    lo: to_u32(2 * i - 1),
                    hi: to_u32(2 * (i + step) - 2),
                });
                i += step;
            }
        }
    }
    bins
}

/// The number the bins of `kind` are looked up by (see [`BinSpec`]).
fn key_of(kind: GraphKind, estimate: bool, entry: &IndexEntry) -> u32 {
    match kind {
        GraphKind::Decade | GraphKind::Year => year_of(entry),
        GraphKind::Moves => u32::from(entry.num_half_moves),
        GraphKind::Elo => {
            let (white, black) = (u32::from(entry.white_elo), u32::from(entry.black_elo));
            if estimate {
                // Rule G2 (`sc_filter freq elo ... GuessElo`).
                let mut sum = white + black;
                if white == 0 && black != 0 {
                    sum += black.min(ESTIMATE_CAP);
                } else if black == 0 && white != 0 {
                    sum += white.min(ESTIMATE_CAP);
                }
                sum
            } else {
                white.min(black)
            }
        }
    }
}

/// The index of the bin that holds `key`, if any.
fn bin_of(specs: &[(u32, u32)], key: u32) -> Option<usize> {
    let after = specs.partition_point(|&(lo, _)| lo <= key);
    let index = after.checked_sub(1)?;
    specs
        .get(index)
        .filter(|&&(_, hi)| key <= hi)
        .map(|_| index)
}

/// One bin of a [`Histogram`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistogramBin {
    /// The label under the axis: `20-29`, `1995`, `2100-2199`, `5` (moves
    /// show a label only on multiples of 5; the others are empty).
    pub label: String,
    /// Games of the filter in the bin.
    pub filtered: u32,
    /// Games of the whole database in the bin.
    pub all: u32,
}

/// The counts of a graph: games of the filter and of the whole database in
/// each bin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Histogram {
    /// The graph.
    pub kind: GraphKind,
    /// The bins, left to right.
    pub bins: Vec<HistogramBin>,
    /// Games in the filter (`sc_filter count`), whether or not in a bin.
    pub filter_total: u32,
    /// Games in the database (`sc_base numGames`).
    pub all_total: u32,
    /// Index entries that could not be read (not counted anywhere).
    pub unreadable: u32,
}

/// A point of a [`HistogramView`].
#[derive(Debug, Clone, PartialEq)]
pub struct ViewPoint {
    /// Label under the axis.
    pub label: String,
    /// The height of the point.
    pub value: f32,
}

/// Ready-to-draw data of one graph: one point per bin (x = 1..N), the largest
/// value and the "mean" line.
#[derive(Debug, Clone, PartialEq)]
pub struct HistogramView {
    /// One point per bin.
    pub bins: Vec<ViewPoint>,
    /// The largest value (0 with no bins).
    pub max: f32,
    /// Games of the filter inside the bins.
    pub total: u32,
    /// The height of the horizontal "mean" line; `None` for an empty
    /// database (rule G8).
    pub mean: Option<f32>,
}

impl Histogram {
    /// The absolute graph: the point is the count of the filter in the bin
    /// and the mean line is the mean of the points, in integer division
    /// (rule G7).
    pub fn absolute(&self) -> HistogramView {
        let counts: Vec<u32> = self.bins.iter().map(|b| b.filtered).collect();
        let sum: u64 = counts.iter().map(|&c| u64::from(c)).sum();
        let max = counts.iter().copied().max().unwrap_or(0);
        let mean = if self.all_total > 0 {
            let bins = u64::try_from(counts.len()).unwrap_or(u64::MAX);
            // Scid keeps the previous mean (0) when there are no bins.
            Some(f32_of(sum.checked_div(bins).unwrap_or(0)))
        } else {
            None
        };
        HistogramView {
            bins: self
                .bins
                .iter()
                .map(|b| ViewPoint {
                    label: b.label.clone(),
                    value: f32_of(u64::from(b.filtered)),
                })
                .collect(),
            max: f32_of(u64::from(max)),
            total: u32::try_from(sum).unwrap_or(u32::MAX),
            mean,
        }
    }

    /// The relative graph: games of the filter per 1000 games of the
    /// database in the bin (0 for a bin without games; at most 999.9), and
    /// the mean line `filter * 1000 / database`, also at most 999.9
    /// (rule G7).
    pub fn relative(&self) -> HistogramView {
        let points: Vec<ViewPoint> = self
            .bins
            .iter()
            .map(|b| ViewPoint {
                label: b.label.clone(),
                value: per_mille(u64::from(b.filtered), u64::from(b.all)),
            })
            .collect();
        let max = points.iter().map(|p| p.value).fold(0.0f32, f32::max);
        let mean = (self.all_total > 0)
            .then(|| per_mille(u64::from(self.filter_total), u64::from(self.all_total)));
        let total = self
            .bins
            .iter()
            .fold(0u32, |sum, b| sum.saturating_add(b.filtered));
        HistogramView {
            bins: points,
            max,
            total,
            mean,
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn f32_of(value: u64) -> f32 {
    value as f32
}

/// `filtered * 1000 / all` as the graph shows it: 0 without games, 999.9 at
/// 1000 or more.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn per_mille(filtered: u64, all: u64) -> f32 {
    if all == 0 {
        return 0.0;
    }
    let value = filtered as f64 * 1000.0 / all as f64;
    if value >= 1000.0 { 999.9 } else { value as f32 }
}

/// Counts the games of the database and of the filter in each bin of `kind`,
/// in one pass over the index (rule G9). `config` should come from
/// [`FilterGraphConfig::normalized`]; a field that is not valid is clamped to
/// the closest legal value instead of failing.
///
/// # Errors
///
/// [`StatsError::FilterSize`] if `filter` does not cover `db`;
/// [`StatsError::Cancelled`] if `cancel` is set.
pub fn filter_histogram<D: GameSource + ?Sized>(
    db: &D,
    filter: &Filter,
    kind: GraphKind,
    config: &FilterGraphConfig,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<Histogram, StatsError> {
    check_filter(db, filter)?;
    let specs = bin_specs(kind, config);
    let ranges: Vec<(u32, u32)> = specs.iter().map(|s| (s.lo, s.hi)).collect();
    let estimate = config.estimate;
    let count = specs.len();
    let (counts, unreadable) = fold_index(
        db,
        progress,
        cancel,
        || vec![[0u32; 2]; count],
        |counts: &mut Vec<[u32; 2]>, n, entry| {
            let key = key_of(kind, estimate, entry);
            if let Some(slot) = bin_of(&ranges, key).and_then(|i| counts.get_mut(i)) {
                slot[0] = slot[0].saturating_add(1);
                if filter.contains(n) {
                    slot[1] = slot[1].saturating_add(1);
                }
            }
        },
        |total, part| {
            for (sum, other) in total.iter_mut().zip(part) {
                sum[0] = sum[0].saturating_add(other[0]);
                sum[1] = sum[1].saturating_add(other[1]);
            }
        },
    )?;
    let bins = specs
        .into_iter()
        .zip(counts)
        .map(|(spec, [all, filtered])| HistogramBin {
            label: spec.label,
            filtered,
            all,
        })
        .collect();
    Ok(Histogram {
        kind,
        bins,
        filter_total: filter.count(),
        all_total: db.len(),
        unreadable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(kind: GraphKind, config: &FilterGraphConfig) -> Vec<String> {
        bin_specs(kind, config)
            .into_iter()
            .map(|s| s.label)
            .collect()
    }

    fn ranges(kind: GraphKind, config: &FilterGraphConfig) -> Vec<(u32, u32)> {
        bin_specs(kind, config)
            .into_iter()
            .map(|s| (s.lo, s.hi))
            .collect()
    }

    #[test]
    fn there_are_eleven_decades_and_the_last_is_open() {
        let config = FilterGraphConfig::new(2026);
        let specs = bin_specs(GraphKind::Decade, &config);
        assert_eq!(specs.len(), 11);
        assert_eq!(specs[0].label, "-1919");
        assert_eq!(specs[10].label, "2010+");
        assert_eq!(specs[10].hi, u32::MAX);
    }

    #[test]
    fn year_bins_and_labels() {
        let mut config = FilterGraphConfig::new(2000);
        config.year_min = 1995;
        config.year_step = 2;
        assert_eq!(labels(GraphKind::Year, &config), ["1996", "1998", "2000"]);
        assert_eq!(
            ranges(GraphKind::Year, &config),
            [(1995, 1996), (1997, 1998), (1999, 2000)]
        );
    }

    #[test]
    fn elo_bins_with_estimate_double_the_range() {
        let mut config = FilterGraphConfig::new(2000);
        config.elo_min = 2100;
        config.elo_max = 2300;
        let all = ranges(GraphKind::Elo, &config);
        assert_eq!(all, [(4200, 4399), (4400, 4599), (4600, 4799)]);
        config.estimate = false;
        assert_eq!(
            ranges(GraphKind::Elo, &config),
            [(2100, 2199), (2200, 2299), (2300, 2399)]
        );
        assert_eq!(
            labels(GraphKind::Elo, &config),
            ["2100-2199", "2200-2299", "2300-2399"]
        );
    }

    #[test]
    fn the_first_rating_bin_reaches_1799_when_it_starts_below_100() {
        let mut config = FilterGraphConfig::new(2000);
        config.estimate = false;
        config.elo_min = 0;
        config.elo_max = 2000;
        assert_eq!(
            ranges(GraphKind::Elo, &config),
            [(0, 1799), (1800, 1899), (1900, 1999), (2000, 2099)]
        );
        assert_eq!(labels(GraphKind::Elo, &config)[0], "0-1799");
    }

    #[test]
    fn moves_use_the_formula_of_the_absolute_graph() {
        let mut config = FilterGraphConfig::new(2000);
        config.moves_min = 1;
        config.moves_max = 6;
        config.moves_step = 1;
        assert_eq!(
            ranges(GraphKind::Moves, &config),
            [(1, 2), (3, 4), (5, 6), (7, 8), (9, 10), (11, 12)]
        );
        assert_eq!(labels(GraphKind::Moves, &config), ["", "", "", "", "5", ""]);
        config.moves_step = 3;
        config.moves_max = 4;
        assert_eq!(ranges(GraphKind::Moves, &config), [(1, 6), (7, 12)]);
    }

    #[test]
    fn an_empty_range_gives_no_bins() {
        let mut config = FilterGraphConfig::new(2000);
        config.year_min = 2010;
        config.year_max = 2000;
        assert_eq!(bin_specs(GraphKind::Year, &config).len(), 0);
    }

    #[test]
    fn bin_lookup_takes_the_edges() {
        let r = [(1, 2), (3, 4), (7, 8)];
        assert_eq!(bin_of(&r, 0), None);
        assert_eq!(bin_of(&r, 1), Some(0));
        assert_eq!(bin_of(&r, 2), Some(0));
        assert_eq!(bin_of(&r, 3), Some(1));
        assert_eq!(bin_of(&r, 5), None);
        assert_eq!(bin_of(&r, 8), Some(2));
        assert_eq!(bin_of(&r, 9), None);
        assert_eq!(bin_of(&[], 1), None);
    }

    #[test]
    fn normalization_follows_check_config() {
        let bad = FilterGraphConfig {
            year_min: 0,
            year_max: 0,
            year_step: 0,
            elo_min: -1,
            elo_max: 0,
            elo_step: 0,
            moves_min: 0,
            moves_max: 0,
            moves_step: -3,
            estimate: false,
        };
        let fixed = bad.normalized(2024);
        assert_eq!(
            fixed,
            FilterGraphConfig {
                year_min: 1995,
                year_max: 2024,
                year_step: 1,
                elo_min: 2100,
                elo_max: 2800,
                elo_step: 100,
                moves_min: 1,
                moves_max: 80,
                moves_step: 1,
                estimate: false,
            }
        );
        let big = FilterGraphConfig {
            year_max: 123_456,
            ..FilterGraphConfig::new(2000)
        };
        assert_eq!(big.normalized(2000).year_max, MAX_GRAPH_FIELD);
        assert_eq!(
            FilterGraphConfig::new(2000).normalized(2000),
            FilterGraphConfig::new(2000)
        );
    }

    fn hist(bins: &[(u32, u32)], filter_total: u32, all_total: u32) -> Histogram {
        Histogram {
            kind: GraphKind::Year,
            bins: bins
                .iter()
                .enumerate()
                .map(|(i, &(filtered, all))| HistogramBin {
                    label: i.to_string(),
                    filtered,
                    all,
                })
                .collect(),
            filter_total,
            all_total,
            unreadable: 0,
        }
    }

    #[test]
    fn absolute_view_uses_the_integer_mean_of_the_points() {
        let view = hist(&[(1, 5), (2, 5), (4, 5)], 7, 20).absolute();
        assert_eq!(view.max, 4.0);
        assert_eq!(view.total, 7);
        assert_eq!(view.mean, Some(2.0)); // 7 / 3 truncated
        assert_eq!(view.bins[2].value, 4.0);
    }

    #[test]
    fn relative_view_is_per_mille_capped_at_999_9() {
        let view = hist(&[(1, 4), (0, 0), (10, 10)], 11, 22).relative();
        assert_eq!(view.bins[0].value, 250.0);
        assert_eq!(view.bins[1].value, 0.0);
        assert_eq!(view.bins[2].value, 999.9);
        assert_eq!(view.max, 999.9);
        assert_eq!(view.mean, Some(500.0));
    }

    #[test]
    fn an_empty_database_has_no_mean() {
        let h = hist(&[(0, 0), (0, 0)], 0, 0);
        assert_eq!(h.absolute().mean, None);
        assert_eq!(h.relative().mean, None);
        assert_eq!(h.relative().max, 0.0);
    }
}
