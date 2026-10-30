//! The continuous-line score graph (spec M4-9, D1400-D1413): a single curve
//! through the scores of the main line, on a non-linear scale.
//!
//! Everything here is pure geometry in "graph units": x is the **ply** (0 is
//! the start position, D1405, D1409) and y is the squashed score in
//! `-1.0..=1.0` (D1403). The GUI maps both to pixels and draws.

use mychess_core::{Game, eval_of};

use super::series::{Invert, SCORE_LIMIT};

/// Steepness of the squash: `y = 2 / (1 + e^(-k * eval)) - 1` (D1403).
pub const SQUASH_K: f32 = 0.368;

/// The evaluations (pawns) that get a grid line, besides zero (D1403).
pub const GRID_EVALS: [f32; 3] = [1.0, 3.0, 5.0];

/// How the scores are drawn (D1400).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphStyle {
    /// One continuous curve with the area filled (the default).
    #[default]
    Line,
    /// One bar per move, as the Scid draws it.
    Bars,
}

impl GraphStyle {
    /// Every style, in menu order.
    pub const ALL: [GraphStyle; 2] = [GraphStyle::Line, GraphStyle::Bars];

    /// The spelling used in the preferences file.
    pub fn name(self) -> &'static str {
        match self {
            GraphStyle::Line => "line",
            GraphStyle::Bars => "bars",
        }
    }

    /// Reads the spelling used in the preferences file.
    pub fn from_name(text: &str) -> Option<GraphStyle> {
        GraphStyle::ALL.into_iter().find(|s| s.name() == text)
    }
}

/// Maps a score in pawns to `-1.0..=1.0`. Zero maps to zero, the scale is
/// odd and increasing, and a mate (clamped to the score limit upstream) lands
/// near the border. A value that is not finite maps to zero.
pub fn squash(eval: f32) -> f32 {
    if !eval.is_finite() {
        return 0.0;
    }
    let eval = eval.clamp(-SCORE_LIMIT, SCORE_LIMIT);
    2.0 / (1.0 + (-SQUASH_K * eval).exp()) - 1.0
}

/// A class of key moment drawn over the curve (D1408). The report (M4-10)
/// supplies these; without a report there are none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    /// A brilliant move.
    Brilliant,
    /// A great (only) move.
    Great,
    /// An inaccuracy.
    Inaccuracy,
    /// A mistake.
    Mistake,
    /// A missed chance.
    Miss,
    /// A blunder.
    Blunder,
}

/// A marker on the curve; it never changes the curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    /// The ply the marker sits on (1 is White's first move).
    pub ply: usize,
    /// What it marks.
    pub kind: MarkerKind,
}

/// One point of the curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinePoint {
    /// The ply (the x coordinate).
    pub ply: usize,
    /// The score in pawns, clamped to the score limit.
    pub eval: f32,
    /// The squashed score (the y coordinate).
    pub y: f32,
}

/// A cubic Bezier piece between two consecutive points of the curve, in
/// `(ply, y)` units. `gap` is set when moves without a score lie between the
/// ends: the GUI draws it dashed and faint (D1404).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveSegment {
    /// First end `(ply, y)`.
    pub from: (f32, f32),
    /// First control point.
    pub c1: (f32, f32),
    /// Second control point.
    pub c2: (f32, f32),
    /// Second end `(ply, y)`.
    pub to: (f32, f32),
    /// Whether the segment crosses moves without a score.
    pub gap: bool,
}

impl CurveSegment {
    /// The point of the curve at `t` in `0.0..=1.0`.
    #[allow(clippy::many_single_char_names)] // the Bernstein weights
    pub fn at(&self, t: f32) -> (f32, f32) {
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        (
            a * self.from.0 + b * self.c1.0 + c * self.c2.0 + d * self.to.0,
            a * self.from.1 + b * self.c1.1 + c * self.c2.1 + d * self.to.1,
        )
    }
}

/// The slopes of the monotone cubic through `(x, y)` (Fritsch-Carlson):
/// the interpolant never leaves the range of its two end values. `x` must be
/// strictly increasing; the result has one slope per point.
#[allow(clippy::many_single_char_names)] // the notation of the method
fn tangents(xs: &[f32], ys: &[f32]) -> Vec<f32> {
    let n = xs.len();
    if n < 2 {
        return vec![0.0; n];
    }
    let d: Vec<f32> = (0..n - 1)
        .map(|i| (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]))
        .collect();
    let mut m = vec![0.0; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        // A change of direction (or a plateau) is a flat extremum.
        m[i] = if d[i - 1] * d[i] <= 0.0 {
            0.0
        } else {
            f32::midpoint(d[i - 1], d[i])
        };
    }
    for i in 0..n - 1 {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
            continue;
        }
        let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
        let norm = a * a + b * b;
        if norm > 9.0 {
            let tau = 3.0 / norm.sqrt();
            m[i] = tau * a * d[i];
            m[i + 1] = tau * b * d[i];
        }
    }
    m
}

/// The monotone Bezier pieces through `points` (D1401): they pass exactly
/// through every point and never exceed the values of their two ends.
pub fn monotone_segments(points: &[LinePoint]) -> Vec<CurveSegment> {
    #[allow(clippy::cast_precision_loss)] // plies are far below 2^24
    let xs: Vec<f32> = points.iter().map(|p| p.ply as f32).collect();
    let ys: Vec<f32> = points.iter().map(|p| p.y).collect();
    let m = tangents(&xs, &ys);
    points
        .windows(2)
        .enumerate()
        .map(|(i, pair)| {
            let h = xs[i + 1] - xs[i];
            CurveSegment {
                from: (xs[i], ys[i]),
                c1: (xs[i] + h / 3.0, ys[i] + m[i] * h / 3.0),
                c2: (xs[i + 1] - h / 3.0, ys[i + 1] - m[i + 1] * h / 3.0),
                to: (xs[i + 1], ys[i + 1]),
                gap: pair[1].ply > pair[0].ply + 1,
            }
        })
        .collect()
}

/// The ply nearest to the graph coordinate `x`, for a click or a hover
/// (D1407): `0..=plies`, or `None` when `x` is not finite or the game has no
/// moves.
pub fn nearest_ply(x: f32, plies: usize) -> Option<usize> {
    if !x.is_finite() || plies == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let top = plies as f32;
    // `x` is clamped to `0..=plies`: the cast cannot truncate or change sign.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ply = x.clamp(0.0, top).round() as usize;
    Some(ply)
}

/// The plies that get a move-number label: multiples of a "nice" number of
/// moves (1, 2, 5, 10, 20, 50, 100...) chosen so there are at most
/// `max_labels` of them (D1409).
pub fn move_ticks(plies: usize, max_labels: usize) -> Vec<usize> {
    let max_labels = max_labels.max(1);
    let moves = plies / 2;
    let mut step = 1usize;
    for nice in [1usize, 2, 5].into_iter().cycle().scan(1usize, |scale, n| {
        let value = n.saturating_mul(*scale);
        if n == 5 {
            *scale = scale.saturating_mul(10);
        }
        Some(value)
    }) {
        step = nice;
        if moves / nice <= max_labels || nice >= moves.max(1) {
            break;
        }
    }
    // Bounded by `plies`: the step is at least 2.
    #[allow(clippy::maybe_infinite_iter)]
    let ticks = (1..)
        .map(|k| 2 * step * k)
        .take_while(|ply| *ply <= plies)
        .collect();
    ticks
}

/// The grid lines of the y axis: `(score in pawns, y)` for 0 and the
/// positive and negative values of [`GRID_EVALS`].
pub fn grid_lines() -> Vec<(f32, f32)> {
    let mut lines = vec![(0.0, 0.0)];
    for e in GRID_EVALS {
        lines.push((e, squash(e)));
        lines.push((-e, squash(-e)));
    }
    lines
}

/// Everything the GUI needs to draw the line graph.
#[derive(Debug, Clone, PartialEq)]
pub struct LineGraphView {
    /// The scored plies in order (ply 0, the start position, only when it
    /// has a score, D1405).
    pub points: Vec<LinePoint>,
    /// The curve between consecutive points.
    pub segments: Vec<CurveSegment>,
    /// Key moments over the curve.
    pub markers: Vec<Marker>,
    /// Number of main-line plies; the x axis is `0..=plies`.
    pub plies: usize,
}

impl LineGraphView {
    /// The point of `ply`, if it has a score.
    pub fn point_at(&self, ply: usize) -> Option<&LinePoint> {
        let at = self.points.binary_search_by_key(&ply, |p| p.ply).ok()?;
        self.points.get(at)
    }
}

/// Builds the line graph of the main line of `game`. Reads the same
/// `[%eval]` comments as the bars (D1412) and applies the same inversion.
/// **`O(plies)`**: it belongs to the cached view, not to `view`.
pub fn line_graph_view(game: &Game, invert: Invert, markers: &[Marker]) -> LineGraphView {
    let mut points = Vec::new();
    let mut push = |ply: usize, raw: f32, flip: bool| {
        if !raw.is_finite() {
            return;
        }
        let eval = if flip { -raw } else { raw }.clamp(-SCORE_LIMIT, SCORE_LIMIT);
        points.push(LinePoint {
            ply,
            eval,
            y: squash(eval),
        });
    };
    // The start position: White is to move there, like after a Black move.
    if let Some(raw) = game.comment(game.root()).ok().flatten().and_then(eval_of) {
        push(0, raw, invert.white);
    }
    let mut plies = 0;
    for (i, id) in game.mainline().enumerate() {
        let ply = i + 1;
        plies = ply;
        if let Some(raw) = game.comment(id).ok().flatten().and_then(eval_of) {
            // Odd ply: White just moved, so Black is to move there.
            push(
                ply,
                raw,
                if ply % 2 == 1 {
                    invert.black
                } else {
                    invert.white
                },
            );
        }
    }
    let segments = monotone_segments(&points);
    LineGraphView {
        points,
        segments,
        markers: markers.iter().copied().filter(|m| m.ply <= plies).collect(),
        plies,
    }
}

/// Builds the line graph from scores that do not live in the game's comments
/// (the game report keeps its own evaluations, D1447). `scores[ply]` is the
/// evaluation in pawns from White's point of view after `ply` moves (0 is
/// the start position); `None` leaves a gap. Values are clamped like
/// [`line_graph_view`]'s.
pub fn line_view_from_scores(scores: &[Option<f32>], markers: &[Marker]) -> LineGraphView {
    let mut points = Vec::new();
    for (ply, score) in scores.iter().enumerate() {
        if let Some(raw) = score.filter(|v| v.is_finite()) {
            let eval = raw.clamp(-SCORE_LIMIT, SCORE_LIMIT);
            points.push(LinePoint {
                ply,
                eval,
                y: squash(eval),
            });
        }
    }
    let plies = scores.len().saturating_sub(1);
    let segments = monotone_segments(&points);
    LineGraphView {
        points,
        segments,
        markers: markers.iter().copied().filter(|m| m.ply <= plies).collect(),
        plies,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{TWELVE, game_with};
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn squash_is_odd_increasing_and_bounded() {
        assert_eq!(squash(0.0), 0.0);
        assert!((squash(2.0) + squash(-2.0)).abs() < 1e-6);
        assert!(squash(1.0) < squash(2.0));
        assert!(squash(10.0) < 1.0 && squash(-10.0) > -1.0);
        assert_eq!(squash(f32::NAN), 0.0);
        assert!(squash(f32::INFINITY) <= 1.0);
    }

    #[test]
    fn small_advantages_are_not_flattened_by_big_ones() {
        // +1 keeps a visible share of the range even though +9 exists.
        assert!(squash(1.0) > 0.15 * squash(9.0));
    }

    #[test]
    fn a_line_view_from_scores_skips_gaps_and_foreign_markers() {
        let scores = [Some(0.2), None, Some(-3.0), Some(f32::NAN), Some(500.0)];
        let markers = [
            Marker {
                ply: 2,
                kind: MarkerKind::Mistake,
            },
            Marker {
                ply: 9,
                kind: MarkerKind::Blunder,
            },
        ];
        let view = line_view_from_scores(&scores, &markers);
        assert_eq!(view.plies, 4);
        assert_eq!(view.points.len(), 3);
        assert_eq!(view.markers.len(), 1);
        assert!(view.points.iter().all(|p| p.y.abs() <= 1.0));
        assert!(view.segments.iter().any(|s| s.gap));
    }

    #[test]
    fn style_names_round_trip() {
        for style in GraphStyle::ALL {
            assert_eq!(GraphStyle::from_name(style.name()), Some(style));
        }
        assert_eq!(GraphStyle::from_name("Line"), None);
        assert_eq!(GraphStyle::default(), GraphStyle::Line);
    }

    #[test]
    fn nearest_ply_rounds_and_clamps() {
        assert_eq!(nearest_ply(2.4, 10), Some(2));
        assert_eq!(nearest_ply(2.6, 10), Some(3));
        assert_eq!(nearest_ply(-5.0, 10), Some(0));
        assert_eq!(nearest_ply(99.0, 10), Some(10));
        assert_eq!(nearest_ply(f32::NAN, 10), None);
        assert_eq!(nearest_ply(1.0, 0), None);
    }

    #[test]
    fn ticks_stay_within_the_label_budget() {
        assert_eq!(move_ticks(0, 8), Vec::<usize>::new());
        assert_eq!(move_ticks(10, 8), vec![2, 4, 6, 8, 10]);
        let ticks = move_ticks(300, 8);
        assert!(ticks.len() <= 8 && !ticks.is_empty());
        assert!(ticks.iter().all(|p| p % 2 == 0 && *p <= 300));
    }

    #[test]
    fn grid_lines_are_symmetric_around_zero() {
        let lines = grid_lines();
        assert_eq!(lines.len(), 7);
        assert_eq!(lines[0], (0.0, 0.0));
        assert!(lines.contains(&(3.0, squash(3.0))));
        assert!(lines.contains(&(-3.0, squash(-3.0))));
    }

    #[test]
    fn the_curve_passes_through_the_points_and_marks_gaps() {
        // Scores on plies 1, 2 and 5 only.
        let comments: Vec<&str> = vec!["[%eval 0.5]", "[%eval 1.0]", "", "", "[%eval -2.0]"];
        let game = game_with(&TWELVE[..5], &comments);
        let view = line_graph_view(&game, Invert::default(), &[]);
        let plies: Vec<usize> = view.points.iter().map(|p| p.ply).collect();
        assert_eq!(plies, vec![1, 2, 5]);
        assert_eq!(view.segments.len(), 2);
        assert!(!view.segments[0].gap);
        assert!(view.segments[1].gap, "plies 3 and 4 have no score");
        assert_eq!(view.segments[1].from.0, 2.0);
        assert_eq!(view.segments[1].to, (5.0, squash(-2.0)));
        assert_eq!(view.plies, 5);
    }

    #[test]
    fn inversion_negates_by_side_and_markers_outside_are_dropped() {
        let game = game_with(&TWELVE[..2], &["[%eval 1.0]", "[%eval 1.0]"]);
        let invert = Invert {
            white: false,
            black: true,
        };
        let markers = [
            Marker {
                ply: 1,
                kind: MarkerKind::Blunder,
            },
            Marker {
                ply: 9,
                kind: MarkerKind::Miss,
            },
        ];
        let view = line_graph_view(&game, invert, &markers);
        assert_eq!(view.point_at(1).map(|p| p.eval), Some(-1.0));
        assert_eq!(view.point_at(2).map(|p| p.eval), Some(1.0));
        assert_eq!(view.markers.len(), 1);
    }

    #[test]
    fn the_view_carries_the_curve_only_in_line_style() {
        use super::super::view::{GraphKind, GraphPrefs, score_graph_view};
        let game = game_with(&TWELVE[..3], &["[%eval 0.5]", "[%eval 1.0]", "[%eval 2.0]"]);
        let line = GraphPrefs {
            kind: GraphKind::Score,
            ..GraphPrefs::default()
        };
        let view = score_graph_view(&game, line);
        assert_eq!(view.line.as_ref().map(|l| l.points.len()), Some(3));
        assert_eq!(view.bars.len(), 3, "the bars stay for the other style");
        let bars = GraphPrefs {
            style: GraphStyle::Bars,
            ..line
        };
        assert!(score_graph_view(&game, bars).line.is_none());
        let time = GraphPrefs {
            kind: GraphKind::Time,
            ..line
        };
        assert!(score_graph_view(&game, time).line.is_none());
    }

    #[test]
    fn an_unscored_game_has_no_curve() {
        let game = game_with(&TWELVE[..4], &[]);
        let view = line_graph_view(&game, Invert::default(), &[]);
        assert!(view.points.is_empty() && view.segments.is_empty());
    }

    proptest! {
        #[test]
        fn the_curve_never_leaves_the_values_of_its_ends(
            values in proptest::collection::vec(-12.0f32..12.0, 2..30),
            gaps in proptest::collection::vec(1usize..4, 30),
        ) {
            let mut ply = 0;
            let points: Vec<LinePoint> = values
                .iter()
                .zip(&gaps)
                .map(|(v, g)| {
                    ply += g;
                    let eval = v.clamp(-SCORE_LIMIT, SCORE_LIMIT);
                    LinePoint { ply, eval, y: squash(eval) }
                })
                .collect();
            for segment in monotone_segments(&points) {
                let (lo, hi) = (
                    segment.from.1.min(segment.to.1) - 1e-4,
                    segment.from.1.max(segment.to.1) + 1e-4,
                );
                for k in 0..=20 {
                    #[allow(clippy::cast_precision_loss)]
                    let (x, y) = segment.at(k as f32 / 20.0);
                    prop_assert!(y >= lo && y <= hi, "y {y} outside {lo}..{hi}");
                    prop_assert!(x >= segment.from.0 - 1e-4 && x <= segment.to.0 + 1e-4);
                }
            }
        }
    }
}
