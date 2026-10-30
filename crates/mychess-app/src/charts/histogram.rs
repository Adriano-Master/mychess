//! The graphs of the filter: the `[filter_graph]` preferences, the axis and
//! the ready-to-draw data (spec M6-4, slice b, D3372, D3374, D3376, D3402).
//! Independent of `graphs/` (D3351).

use mychess_db::{FilterGraphConfig, GraphKind, Histogram, HistogramView, MAX_GRAPH_FIELD};
use serde::{Serialize, Serializer};

use crate::panel_calc::named_enum;

named_enum! {
    /// Which graph (the "type" buttons).
    pub enum FilterGraphKind {
        /// Eleven fixed decades.
        Decade => "decade",
        /// One bin per `year_step` years.
        #[default]
        Year => "year",
        /// One bin per `elo_step` rating points.
        Elo => "elo",
        /// One bin per `moves_step` full moves.
        Moves => "moves",
    }
}

named_enum! {
    /// What the statistics panel shows.
    pub enum StatsTab {
        /// The table of the statistics.
        #[default]
        Table => "table",
        /// The relative graph: games per 1000 of the base in each bin.
        Relative => "relative",
        /// The absolute graph: games of the filter in each bin.
        Absolute => "absolute",
    }
}

impl FilterGraphKind {
    pub(crate) fn engine(self) -> GraphKind {
        match self {
            FilterGraphKind::Decade => GraphKind::Decade,
            FilterGraphKind::Year => GraphKind::Year,
            FilterGraphKind::Elo => GraphKind::Elo,
            FilterGraphKind::Moves => GraphKind::Moves,
        }
    }
}

/// Which of the three fields of a graph a message changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphField {
    /// The first value.
    Min,
    /// The last value.
    Max,
    /// The width of a bin.
    Step,
}

/// The graph of the filter on show and its configuration, in
/// `[filter_graph]`. The fields hold what the entries of the window hold; they
/// are limited to four digits and normalised (rule G6) when used.
// One flag per independent option.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct FilterGraphPrefs {
    /// The tab on show.
    pub tab: StatsTab,
    /// The kind of graph.
    pub kind: FilterGraphKind,
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
    /// Points of rating per bin.
    pub elo_step: i32,
    /// First move of the Moves graph.
    pub moves_min: i32,
    /// Last move.
    pub moves_max: i32,
    /// Moves per bin.
    pub moves_step: i32,
    /// "Estimate": the rating of a game with one rating missing.
    pub estimate: bool,
    /// "Show dots".
    pub show_dots: bool,
}

impl Default for FilterGraphPrefs {
    /// The defaults of the Scid; the last year is 0, which normalising turns
    /// into the current year.
    fn default() -> FilterGraphPrefs {
        FilterGraphPrefs::new(0)
    }
}

impl FilterGraphPrefs {
    /// The defaults of the Scid for `current_year`.
    pub fn new(current_year: u16) -> FilterGraphPrefs {
        FilterGraphPrefs::from_config(
            StatsTab::Table,
            FilterGraphKind::Year,
            FilterGraphConfig::new(current_year),
            true,
        )
    }

    fn from_config(
        tab: StatsTab,
        kind: FilterGraphKind,
        c: FilterGraphConfig,
        show_dots: bool,
    ) -> FilterGraphPrefs {
        FilterGraphPrefs {
            tab,
            kind,
            year_min: c.year_min,
            year_max: c.year_max,
            year_step: c.year_step,
            elo_min: c.elo_min,
            elo_max: c.elo_max,
            elo_step: c.elo_step,
            moves_min: c.moves_min,
            moves_max: c.moves_max,
            moves_step: c.moves_step,
            estimate: c.estimate,
            show_dots,
        }
    }

    /// The preferences with every number inside the four digits of the
    /// entries of the window (and not negative).
    #[must_use]
    pub fn sanitized(mut self) -> FilterGraphPrefs {
        for v in [
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
            *v = (*v).clamp(0, MAX_GRAPH_FIELD);
        }
        self
    }

    /// The configuration of the engine, normalised (rule G6).
    pub(crate) fn config(&self, current_year: u16) -> FilterGraphConfig {
        FilterGraphConfig {
            year_min: self.year_min,
            year_max: self.year_max,
            year_step: self.year_step,
            elo_min: self.elo_min,
            elo_max: self.elo_max,
            elo_step: self.elo_step,
            moves_min: self.moves_min,
            moves_max: self.moves_max,
            moves_step: self.moves_step,
            estimate: self.estimate,
        }
        .normalized(current_year)
    }

    /// Sets a field of the axis of `kind` (the Decade graph has none).
    pub(crate) fn set_field(&mut self, kind: FilterGraphKind, field: GraphField, value: i32) {
        let slot = match (kind, field) {
            (FilterGraphKind::Year, GraphField::Min) => &mut self.year_min,
            (FilterGraphKind::Year, GraphField::Max) => &mut self.year_max,
            (FilterGraphKind::Year, GraphField::Step) => &mut self.year_step,
            (FilterGraphKind::Elo, GraphField::Min) => &mut self.elo_min,
            (FilterGraphKind::Elo, GraphField::Max) => &mut self.elo_max,
            (FilterGraphKind::Elo, GraphField::Step) => &mut self.elo_step,
            (FilterGraphKind::Moves, GraphField::Min) => &mut self.moves_min,
            (FilterGraphKind::Moves, GraphField::Max) => &mut self.moves_max,
            (FilterGraphKind::Moves, GraphField::Step) => &mut self.moves_step,
            (FilterGraphKind::Decade, _) => return,
        };
        *slot = value;
    }

    /// The axis back to the defaults (the "Defaults" button), keeping the
    /// tab, the kind and the dots.
    pub(crate) fn reset_axes(&self, current_year: u16) -> FilterGraphPrefs {
        FilterGraphPrefs::from_config(
            self.tab,
            self.kind,
            FilterGraphConfig::new(current_year),
            self.show_dots,
        )
    }
}

#[derive(Serialize)]
struct Table {
    tab: &'static str,
    kind: &'static str,
    year_min: i32,
    year_max: i32,
    year_step: i32,
    elo_min: i32,
    elo_max: i32,
    elo_step: i32,
    moves_min: i32,
    moves_max: i32,
    moves_step: i32,
    estimate: bool,
    show_dots: bool,
}

impl Serialize for FilterGraphPrefs {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Table {
            tab: self.tab.name(),
            kind: self.kind.name(),
            year_min: self.year_min,
            year_max: self.year_max,
            year_step: self.year_step,
            elo_min: self.elo_min,
            elo_max: self.elo_max,
            elo_step: self.elo_step,
            moves_min: self.moves_min,
            moves_max: self.moves_max,
            moves_step: self.moves_step,
            estimate: self.estimate,
            show_dots: self.show_dots,
        }
        .serialize(serializer)
    }
}

/// Most divisions of an axis.
pub const MAX_TICKS: usize = 20;

/// A y axis from 0 with a "nice" step (1, 2 or 5 times a power of ten) of at
/// most about ten divisions (rule D3376).
#[derive(Debug, Clone, PartialEq)]
pub struct YAxis {
    /// The distance between lines.
    pub step: f32,
    /// The top of the axis: a multiple of `step` not below the data.
    pub max: f32,
    /// The lines, from 0 to `max`.
    pub ticks: Vec<f32>,
}

impl YAxis {
    /// The axis for data up to `data_max`.
    pub fn nice(data_max: f32) -> YAxis {
        let data_max = if data_max.is_finite() {
            data_max.max(0.0)
        } else {
            0.0
        };
        let mut step = 0.1_f32;
        'search: loop {
            for factor in [1.0_f32, 2.0, 5.0] {
                let candidate = step * factor;
                if data_max / candidate <= 10.0 {
                    step = candidate;
                    break 'search;
                }
            }
            step *= 10.0;
            if step > 1.0e9 {
                break;
            }
        }
        // `data_max / step` is at most about 10 by the search above, so the
        // count is a small positive whole number.
        let count = (0..MAX_TICKS)
            .find(|i| step * f32::from(u8::try_from(*i).unwrap_or(u8::MAX)) >= data_max)
            .unwrap_or(MAX_TICKS)
            .max(1);
        let at = |i: usize| step * f32::from(u8::try_from(i).unwrap_or(u8::MAX));
        let ticks: Vec<f32> = (0..=count).map(at).collect();
        YAxis {
            step,
            max: at(count),
            ticks,
        }
    }
}

/// A point of a graph.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphPoint {
    /// The label under the axis (empty: none).
    pub label: String,
    /// The height.
    pub value: f32,
}

/// A graph of the filter, ready to draw: points at x = 1..N.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphView {
    /// The kind of graph.
    pub kind: FilterGraphKind,
    /// The relative graph (games per 1000 of the base) or the absolute one.
    pub relative: bool,
    /// One point per bin.
    pub points: Vec<GraphPoint>,
    /// The y axis.
    pub axis: YAxis,
    /// The height of the "mean" line; `None` for an empty base.
    pub mean: Option<f32>,
    /// Games of the filter inside the bins.
    pub total: u32,
    /// Games in the filter.
    pub filter_total: u32,
    /// Games in the database.
    pub all_total: u32,
    /// Index entries that could not be read.
    pub unreadable: u32,
    /// Blue vertical lines go before these points (the decade graph, before
    /// 1950 and 2000; the relative graph only).
    pub blue_lines: Vec<usize>,
    /// The Fluent key of the title: `graph-title-relative` or
    /// `graph-title-absolute`.
    pub title_key: &'static str,
}

fn convert(view: &HistogramView) -> Vec<GraphPoint> {
    view.bins
        .iter()
        .map(|p| GraphPoint {
            label: p.label.clone(),
            value: p.value,
        })
        .collect()
}

/// The graph of `histogram`.
pub(crate) fn graph_view(histogram: &Histogram, relative: bool) -> GraphView {
    let data = if relative {
        histogram.relative()
    } else {
        histogram.absolute()
    };
    let kind = match histogram.kind {
        GraphKind::Decade => FilterGraphKind::Decade,
        GraphKind::Year => FilterGraphKind::Year,
        GraphKind::Elo => FilterGraphKind::Elo,
        GraphKind::Moves => FilterGraphKind::Moves,
    };
    GraphView {
        kind,
        relative,
        points: convert(&data),
        axis: YAxis::nice(data.max.max(data.mean.unwrap_or(0.0))),
        mean: data.mean,
        total: data.total,
        filter_total: histogram.filter_total,
        all_total: histogram.all_total,
        unreadable: histogram.unreadable,
        blue_lines: if relative && kind == FilterGraphKind::Decade {
            vec![4, 9]
        } else {
            Vec::new()
        },
        title_key: if relative {
            "graph-title-relative"
        } else {
            "graph-title-absolute"
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axis_uses_steps_of_1_2_5() {
        let a = YAxis::nice(7.0);
        assert!((a.step - 1.0).abs() < 1e-6);
        assert!((a.max - 7.0).abs() < 1e-6);
        let a = YAxis::nice(13.0);
        assert!((a.step - 2.0).abs() < 1e-6);
        assert!((a.max - 14.0).abs() < 1e-6);
        let a = YAxis::nice(0.0);
        assert_eq!(a.ticks.len(), 2);
        let a = YAxis::nice(430.0);
        assert!((a.step - 50.0).abs() < 1e-3);
        assert!(a.ticks.len() <= MAX_TICKS + 1);
        let a = YAxis::nice(f32::NAN);
        assert!(a.max > 0.0);
    }

    #[test]
    fn prefs_are_limited_to_four_digits() {
        let mut p = FilterGraphPrefs::new(2020);
        p.year_min = -5;
        p.elo_max = 99_999;
        let p = p.sanitized();
        assert_eq!((p.year_min, p.elo_max), (0, MAX_GRAPH_FIELD));
        // Normalising turns the illegal year back into the default.
        assert_eq!(p.config(2020).year_min, 1995);
    }

    #[test]
    fn set_field_ignores_the_decade_graph() {
        let mut p = FilterGraphPrefs::new(2020);
        let before = p.clone();
        p.set_field(FilterGraphKind::Decade, GraphField::Min, 3);
        assert_eq!(p, before);
        p.set_field(FilterGraphKind::Moves, GraphField::Step, 2);
        assert_eq!(p.moves_step, 2);
    }

    #[test]
    fn the_defaults_are_those_of_scid() {
        let p = FilterGraphPrefs::new(2020);
        assert_eq!((p.year_min, p.year_max, p.year_step), (1995, 2020, 1));
        assert_eq!((p.elo_min, p.elo_max, p.elo_step), (2100, 2800, 100));
        assert!(p.estimate && p.show_dots);
    }
}
