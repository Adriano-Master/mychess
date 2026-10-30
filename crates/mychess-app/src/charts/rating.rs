//! The rating graph: the `[rating_graph]` preferences, the palette and the
//! ready-to-draw series (spec M6-4, slice b, D3375, D3404).

use mychess_db::{MAX_RATING_PLAYERS, RatingSeries};
use serde::Serialize;

/// The colours of the players, in order, cyclic (`graphs.tcl` 849-852:
/// steelblue, seagreen, rosybrown, violet, sandybrown, skyblue, indianred,
/// slateblue, orange).
pub const RATING_PALETTE: [[u8; 3]; MAX_RATING_PLAYERS] = [
    [70, 130, 180],
    [46, 139, 87],
    [188, 143, 143],
    [238, 130, 238],
    [244, 164, 96],
    [135, 206, 235],
    [205, 92, 92],
    [106, 90, 205],
    [255, 165, 0],
];

/// The "Start year" menu.
pub const START_YEARS: [u16; 10] = [1900, 1980, 1985, 1990, 1995, 2000, 2005, 2010, 2015, 2020];
/// The "Start rating" menu.
pub const START_ELOS: [u16; 11] = [0, 500, 1000, 1500, 1800, 2000, 2200, 2300, 2400, 2500, 2600];

/// Highest year or rating the preferences keep.
const MAX_YEAR: u16 = 2100;
const MAX_ELO: u16 = 4000;

/// The options of the graph, in `[rating_graph]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RatingPrefs {
    /// Games before this year are dropped.
    pub start_year: u16,
    /// Ratings below this value are dropped.
    pub start_elo: u16,
    /// Count only the games in the filter.
    pub only_filter: bool,
    /// "Show dots".
    pub show_dots: bool,
}

impl Default for RatingPrefs {
    fn default() -> RatingPrefs {
        RatingPrefs {
            start_year: 1900,
            start_elo: 0,
            only_filter: false,
            show_dots: true,
        }
    }
}

impl RatingPrefs {
    /// The preferences with every number inside its range.
    #[must_use]
    pub fn sanitized(mut self) -> RatingPrefs {
        self.start_year = self.start_year.min(MAX_YEAR);
        self.start_elo = self.start_elo.min(MAX_ELO);
        self
    }

    pub(crate) fn options(self) -> mychess_db::RatingOptions {
        mychess_db::RatingOptions {
            start_year: self.start_year,
            start_elo: self.start_elo,
            only_filter: self.only_filter,
        }
    }
}

/// One player's line.
#[derive(Debug, Clone, PartialEq)]
pub struct RatingLineView {
    /// The name (the legend).
    pub name: String,
    /// The colour, from [`RATING_PALETTE`].
    pub color: [u8; 3],
    /// `(x, rating)`: x is `year + (month - 1) * 100 / 12 / 100`.
    pub points: Vec<(f32, u16)>,
    /// Index entries of the database that could not be read and are in no
    /// point (the same number for every player).
    pub skipped: u32,
}

/// The axes of the graph (rule of `graphs.tcl` 849-1009): y lines every 25
/// (blue every 100), x lines every year (blue every 5; labelled every 5 years
/// when the range passes 10).
#[derive(Debug, Clone, PartialEq)]
pub struct RatingAxes {
    /// Left end of x (a whole year).
    pub x_min: f32,
    /// Right end of x (a whole year).
    pub x_max: f32,
    /// Bottom of y (a multiple of 50).
    pub y_min: u16,
    /// Top of y (a multiple of 50).
    pub y_max: u16,
    /// The labelled x lines.
    pub x_labels: Vec<f32>,
    /// The labelled y lines (every 50).
    pub y_labels: Vec<u16>,
    /// Distance between the light x lines.
    pub x_minor: f32,
    /// Distance between the light y lines.
    pub y_minor: u16,
    /// Distance between the blue x lines.
    pub x_major: f32,
    /// Distance between the blue y lines.
    pub y_major: u16,
}

/// The graph, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct RatingGraphView {
    /// One line per player, in the order of the list.
    pub lines: Vec<RatingLineView>,
    /// No player has a point.
    pub empty: bool,
    /// Index entries that could not be read (a warning when above 0).
    pub skipped_entries: u32,
    /// The axes (of the data, or of a default range when empty).
    pub axes: RatingAxes,
}

/// The axes for points between `x` and `y` extremes.
pub(crate) fn axes(points: &[(f32, u16)]) -> RatingAxes {
    let (mut x_lo, mut x_hi, mut y_lo, mut y_hi) = (f32::MAX, f32::MIN, u16::MAX, u16::MIN);
    for &(x, y) in points {
        x_lo = x_lo.min(x);
        x_hi = x_hi.max(x);
        y_lo = y_lo.min(y);
        y_hi = y_hi.max(y);
    }
    if points.is_empty() {
        x_lo = 2000.0;
        x_hi = 2020.0;
        y_lo = 2000;
        y_hi = 2800;
    }
    let x_min = x_lo.floor();
    let x_max = (x_hi.ceil()).max(x_min + 1.0);
    let y_min = (y_lo / 50) * 50;
    let y_max = (y_hi / 50 + 1) * 50;
    let span = x_max - x_min;
    let label_step = if span > 10.0 { 5.0 } else { 1.0 };
    let mut x_labels = Vec::new();
    let mut x = (x_min / label_step).ceil() * label_step;
    while x <= x_max && x_labels.len() < 100 {
        x_labels.push(x);
        x += label_step;
    }
    let y_labels = (y_min..=y_max).step_by(50).collect();
    RatingAxes {
        x_min,
        x_max,
        y_min,
        y_max,
        x_labels,
        y_labels,
        x_minor: 1.0,
        y_minor: 25,
        x_major: 5.0,
        y_major: 100,
    }
}

/// The lines of `players` with their series.
pub(crate) fn graph_view(players: &[String], series: &[RatingSeries]) -> RatingGraphView {
    let lines: Vec<RatingLineView> = players
        .iter()
        .zip(series)
        .enumerate()
        .map(|(i, (name, s))| RatingLineView {
            name: name.clone(),
            color: RATING_PALETTE[i % RATING_PALETTE.len()],
            points: s.xy(),
            skipped: s.skipped,
        })
        .collect();
    let all: Vec<(f32, u16)> = lines
        .iter()
        .flat_map(|l| l.points.iter().copied())
        .collect();
    RatingGraphView {
        empty: all.is_empty(),
        skipped_entries: lines.iter().map(|l| l.skipped).max().unwrap_or(0),
        axes: axes(&all),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_round_to_years_and_fifties() {
        let a = axes(&[(2001.25, 2433), (2010.5, 2611)]);
        assert_eq!((a.x_min, a.x_max), (2001.0, 2011.0));
        assert_eq!((a.y_min, a.y_max), (2400, 2650));
        // 10 years or fewer: a label every year.
        assert_eq!(a.x_labels.len(), 11);
        assert_eq!(a.y_labels.first(), Some(&2400));
    }

    #[test]
    fn a_long_range_labels_every_five_years() {
        let a = axes(&[(1990.0, 2000), (2020.0, 2700)]);
        assert_eq!(
            a.x_labels,
            vec![1990.0, 1995.0, 2000.0, 2005.0, 2010.0, 2015.0, 2020.0]
        );
    }

    #[test]
    fn an_empty_graph_has_default_axes() {
        let v = graph_view(&["A".to_owned()], &[RatingSeries::default()]);
        assert!(v.empty);
        assert_eq!(v.lines.len(), 1);
        assert!(v.axes.x_max > v.axes.x_min);
    }

    #[test]
    fn the_nine_colours_cycle() {
        let names: Vec<String> = (0..9).map(|i| format!("P{i}")).collect();
        let series = vec![RatingSeries::default(); 9];
        let v = graph_view(&names, &series);
        assert_eq!(v.lines[8].color, RATING_PALETTE[8]);
        assert_ne!(v.lines[0].color, v.lines[1].color);
    }

    #[test]
    fn prefs_are_limited() {
        let p = RatingPrefs {
            start_year: 9999,
            start_elo: 9999,
            ..RatingPrefs::default()
        }
        .sanitized();
        assert_eq!((p.start_year, p.start_elo), (2100, 4000));
    }
}
