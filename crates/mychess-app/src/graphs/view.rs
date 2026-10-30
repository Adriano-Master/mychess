//! The ready-to-draw graph: kind selection, bars, lines, axes and title.

use mychess_core::{Game, GameResult, NodeId, Tags};

use super::axes::{Axis, x_of_ply};
use super::line::{GraphStyle, LineGraphView, Marker, line_graph_view, line_view_from_scores};
use super::series::{
    Invert, ScorePoint, TimeSeries, mainline_plies, score_series_in, time_series_in,
};

/// Fewer scores than this (points) make Auto prefer the times
/// (Scid: `llength < 10` on the flat list of `x y` pairs).
const AUTO_MIN_SCORES: usize = 5;
/// The running-total lines need more than this many points in total
/// (Scid: `llength > 10` on the two flat lists).
const LINES_MIN_POINTS: usize = 5;
/// `Score Combo` with fewer score points than this uses the time y axis
/// (Scid: `llength < 5` on the flat list).
const COMBO_MIN_SCORES: usize = 3;

/// What the graph shows (Scid's Auto, Score, Time, Score Combo, Time Combo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphKind {
    /// Scores, unless there are few of them and more times.
    #[default]
    Auto,
    /// Score bars.
    Score,
    /// Move-time bars (seconds).
    Time,
    /// Score bars plus the running time totals of each side.
    ScoreCombo,
    /// Move-time bars plus the running time totals of each side.
    TimeCombo,
}

/// Graph preferences (`[analysis.graph]` once wired to the state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphPrefs {
    /// Graph kind.
    pub kind: GraphKind,
    /// Score inversions.
    pub invert: Invert,
    /// Whether the view reports the current move bar.
    pub highlight_current: bool,
    /// Line or bars for the scores (M4-9, D1400).
    pub style: GraphStyle,
}

impl Default for GraphPrefs {
    fn default() -> Self {
        GraphPrefs {
            kind: GraphKind::Auto,
            invert: Invert::default(),
            highlight_current: true,
            style: GraphStyle::Line,
        }
    }
}

/// Resolves [`GraphKind::Auto`] to [`GraphKind::Score`] or
/// [`GraphKind::Time`] from the number of score and time points. Auto shows
/// the scores when there are at least 5 of them or when there are no more
/// times than scores (a tie goes to the scores). The other kinds are
/// returned unchanged.
pub fn choose_kind(kind: GraphKind, scores: usize, times: usize) -> GraphKind {
    match kind {
        GraphKind::Auto if scores < AUTO_MIN_SCORES && scores < times => GraphKind::Time,
        GraphKind::Auto => GraphKind::Score,
        other => other,
    }
}

/// Everything the GUI needs to draw the graph.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreGraphView {
    /// What the bars show: [`GraphKind::Score`], [`GraphKind::Time`] or one
    /// of the combos; never `Auto`.
    pub kind: GraphKind,
    /// "White (elo) - Black (elo) (result)".
    pub title: String,
    /// Bars `(x, y)`: pawns for scores, seconds for times. Bar width is 0.7.
    pub bars: Vec<(f32, f32)>,
    /// Running total of White's time, scaled to the y axis (may be empty).
    pub white_line: Vec<(f32, f32)>,
    /// Running total of Black's time, scaled to the y axis (may be empty).
    pub black_line: Vec<(f32, f32)>,
    /// The x axis.
    pub x_axis: Axis,
    /// The y axis.
    pub y_axis: Axis,
    /// No data to draw (no bars and no lines): show the empty axes and a
    /// message.
    pub empty: bool,
    /// Number of main-line plies, the upper bound of [`ply_at_x`](super::ply_at_x).
    pub plies: usize,
    /// The continuous curve of the scores, present when the style is
    /// [`GraphStyle::Line`] and the view shows scores (M4-9). The bars stay
    /// in `bars` either way.
    pub line: Option<LineGraphView>,
}

/// The graph title: `White (elo) - Black (elo) (result)`; a missing rating
/// or an unknown result is left out (`graphs.tcl:659-670`).
pub fn graph_title(tags: &Tags) -> String {
    let elo = |r: &Option<mychess_core::Rating>| match r {
        Some(r) if r.value.get() > 0 => format!(" ({})", r.value.get()),
        _ => String::new(),
    };
    let result = match tags.result {
        GameResult::Unknown => String::new(),
        r => format!(" ({r})"),
    };
    format!(
        "{}{} - {}{}{}",
        tags.white,
        elo(&tags.white_rating),
        tags.black,
        elo(&tags.black_rating),
        result
    )
}

fn max_of(values: impl Iterator<Item = f32>) -> f32 {
    values.fold(f32::NEG_INFINITY, f32::max)
}

/// Scales the running totals; `cap` (the target maximum) absorbs the rounding
/// of `target / max * max`, which could land one ulp above the axis.
fn scaled(total: &[(f32, f32)], scale: f32, cap: f32) -> Vec<(f32, f32)> {
    total
        .iter()
        .map(|&(x, y)| (x, (y * scale).min(cap)))
        .collect()
}

/// x of the current move bar: `Some` when `prefs.highlight_current` is on
/// and `cursor` is a main-line node past the start (the root, a variation
/// node, a stale or foreign node give `None`).
///
/// Cheap: it walks the ancestors of `cursor` once (no allocation and no
/// scan of the comments), so the GUI can call it on every cursor change.
pub fn current_x(game: &Game, cursor: NodeId, prefs: GraphPrefs) -> Option<f32> {
    if !prefs.highlight_current {
        return None;
    }
    let ply = usize::try_from(game.ply(cursor).ok()?).ok()?;
    if ply == 0 {
        return None;
    }
    let mut node = cursor;
    while let Some(parent) = game.parent(node).ok()? {
        if game.children(parent).ok()?.first() != Some(&node) {
            return None;
        }
        node = parent;
    }
    Some(x_of_ply(ply))
}

/// Builds the view of `game`, independent of the cursor (see [`current_x`]).
///
/// **`O(plies)`** with several passes over the comments: it must not run in
/// `view`. The application calls it synchronously at the end of `update`,
/// only while the panel is open, and keeps the result cached by
/// `(GameSession::changes(), kind, invert)` (see `ViewKey`), so navigation
/// and a change of `highlight_current` do not rebuild it (D872).
pub fn score_graph_view(game: &Game, prefs: GraphPrefs) -> ScoreGraphView {
    let plies = mainline_plies(game);
    let scores: Vec<ScorePoint> = score_series_in(game, &plies, prefs.invert);
    // Scid computes the times for every kind except plain Score.
    let times: TimeSeries = if prefs.kind == GraphKind::Score {
        TimeSeries::default()
    } else {
        time_series_in(game, &plies)
    };
    let kind = choose_kind(prefs.kind, scores.len(), times.per_move.len());
    // Auto and the combos draw the running totals.
    let lines_on = matches!(
        prefs.kind,
        GraphKind::Auto | GraphKind::ScoreCombo | GraphKind::TimeCombo
    );

    let show_scores = matches!(kind, GraphKind::Score | GraphKind::ScoreCombo);
    let bars: Vec<(f32, f32)> = if show_scores {
        scores.iter().map(|p| (p.x, p.y)).collect()
    } else {
        times
            .per_move
            .iter()
            .map(|p| (p.x, p.seconds as f32))
            .collect()
    };

    let max_emt = times.per_move.iter().map(|p| p.seconds).max().unwrap_or(0);
    let (mut white_line, mut black_line) = (Vec::new(), Vec::new());
    if lines_on && times.white_total.len() + times.black_total.len() > LINES_MIN_POINTS {
        let max_bar = max_of(bars.iter().map(|b| b.1)).max(1.0);
        let max_lines = [times.white_total.last(), times.black_total.last()]
            .into_iter()
            .flatten()
            .map(|p| p.1)
            .fold(0.0f32, f32::max);
        let target = if prefs.kind == GraphKind::ScoreCombo && scores.len() < COMBO_MIN_SCORES {
            (max_emt as f32).max(1.0)
        } else {
            max_bar
        };
        // Everything at zero (engine games): flat lines, no division.
        let scale = if max_lines > 0.0 {
            target / max_lines
        } else {
            0.0
        };
        white_line = scaled(&times.white_total, scale, target);
        black_line = scaled(&times.black_total, scale, target);
    }

    // The y axis follows the data shown, except for `Score Combo` with
    // almost no scores, which uses the time axis like Scid.
    let time_axis = matches!(kind, GraphKind::Time | GraphKind::TimeCombo)
        || (prefs.kind == GraphKind::ScoreCombo && scores.len() < COMBO_MIN_SCORES);
    let drawn = || {
        bars.iter()
            .map(|b| b.1)
            .chain(white_line.iter().map(|p| p.1))
            .chain(black_line.iter().map(|p| p.1))
    };
    let y_max = max_of(drawn());
    let y_axis = if time_axis {
        Axis::time_fitting(max_emt, -max_of(drawn().map(|v| -v)), y_max)
    } else {
        Axis::score(-max_of(drawn().map(|v| -v)), y_max)
    };
    let max_x = max_of(
        bars.iter()
            .map(|b| b.0)
            .chain(white_line.iter().map(|p| p.0))
            .chain(black_line.iter().map(|p| p.0)),
    );
    let x_axis = Axis::x(max_x);

    let line = (prefs.style == GraphStyle::Line && show_scores)
        .then(|| line_graph_view(game, prefs.invert, &[]));
    ScoreGraphView {
        kind,
        title: graph_title(game.tags()),
        empty: bars.is_empty() && white_line.is_empty() && black_line.is_empty(),
        bars,
        white_line,
        black_line,
        x_axis,
        y_axis,
        plies: plies.len(),
        line,
    }
}

/// The line graph of the game report (M4-10b): the report's own scores
/// (see [`line_view_from_scores`]) wrapped in the view the canvas draws.
pub fn report_graph_view(scores: &[Option<f32>], markers: &[Marker]) -> ScoreGraphView {
    let line = line_view_from_scores(scores, markers);
    ScoreGraphView {
        kind: GraphKind::Score,
        title: String::new(),
        bars: Vec::new(),
        white_line: Vec::new(),
        black_line: Vec::new(),
        x_axis: Axis::x(x_of_ply(line.plies)),
        y_axis: Axis::score(-10.0, 10.0),
        empty: line.points.is_empty(),
        plies: line.plies,
        line: Some(line),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{TWELVE, game_with};
    use super::*;
    use mychess_core::{Elo, Rating, RatingType};
    use rstest::rstest;

    #[rstest]
    #[case(GraphKind::Auto, 4, 5, GraphKind::Time)]
    #[case(GraphKind::Auto, 5, 9, GraphKind::Score)]
    #[case(GraphKind::Auto, 4, 4, GraphKind::Score)]
    #[case(GraphKind::Auto, 4, 3, GraphKind::Score)]
    #[case(GraphKind::Auto, 0, 0, GraphKind::Score)]
    #[case(GraphKind::Auto, 0, 1, GraphKind::Time)]
    #[case(GraphKind::Score, 0, 9, GraphKind::Score)]
    #[case(GraphKind::TimeCombo, 9, 0, GraphKind::TimeCombo)]
    #[case(GraphKind::ScoreCombo, 1, 9, GraphKind::ScoreCombo)]
    fn choose(
        #[case] kind: GraphKind,
        #[case] s: usize,
        #[case] t: usize,
        #[case] want: GraphKind,
    ) {
        assert_eq!(choose_kind(kind, s, t), want);
    }

    fn prefs(kind: GraphKind) -> GraphPrefs {
        GraphPrefs {
            kind,
            ..GraphPrefs::default()
        }
    }

    fn view(g: &Game, kind: GraphKind) -> ScoreGraphView {
        score_graph_view(g, prefs(kind))
    }

    fn last_node(g: &Game) -> NodeId {
        g.mainline().last().unwrap_or(g.root())
    }

    #[test]
    fn empty_game_and_no_data() {
        let g = Game::new();
        let v = view(&g, GraphKind::Auto);
        assert!(v.empty && v.bars.is_empty() && v.plies == 0);
        assert_eq!((v.y_axis.min, v.y_axis.max), (-1.0, 1.0));
        assert_eq!(v.x_axis.max, 5.0);
        let g = game_with(&TWELVE[..4], &["hello"]);
        let v = view(&g, GraphKind::Time);
        assert!(v.empty);
        assert_eq!(v.kind, GraphKind::Time);
        assert_eq!(v.plies, 4);
    }

    #[test]
    fn score_bars_and_axes() {
        let g = game_with(
            &TWELVE[..6],
            &[
                "[%eval 0.5]",
                "[%eval -2.5]",
                "",
                "[%eval 30]",
                "[%eval 1]",
                "[%eval -1]",
            ],
        );
        let v = view(&g, GraphKind::Auto);
        assert_eq!(v.kind, GraphKind::Score);
        assert_eq!(
            v.bars,
            vec![
                (1.0, 0.5),
                (1.5, -2.5),
                (2.5, 10.0),
                (3.0, 1.0),
                (3.5, -1.0)
            ]
        );
        assert_eq!((v.y_axis.min, v.y_axis.max), (-3.0, 10.0));
        assert_eq!(
            current_x(&g, last_node(&g), prefs(GraphKind::Auto)),
            Some(3.5)
        );
        assert!(!v.empty);
        assert!(v.white_line.is_empty());
    }

    #[test]
    fn current_bar_rules() {
        let g = game_with(&TWELVE[..4], &["[%eval 1]"; 4]);
        let p = GraphPrefs::default();
        assert_eq!(current_x(&g, g.root(), p), None);
        let first = g.mainline().next().unwrap();
        assert_eq!(current_x(&g, first, p), Some(1.0));
        let off = GraphPrefs {
            highlight_current: false,
            ..p
        };
        assert_eq!(current_x(&g, first, off), None);
        // A node inside a variation is not on the main line.
        let mut g2 = g.clone();
        let pos = mychess_core::Position::new();
        let mv = pos.parse_uci("d2d4").unwrap();
        let var = g2.add_move(g2.root(), mv).unwrap();
        assert_eq!(current_x(&g2, var, p), None);
    }

    #[test]
    fn invalid_cursor_is_harmless() {
        let g = game_with(&TWELVE[..2], &["[%eval 1]"; 2]);
        let other = game_with(&TWELVE, &[]);
        let foreign = other.mainline().last().unwrap_or(other.root());
        assert_eq!(current_x(&g, foreign, GraphPrefs::default()), None);
    }

    #[test]
    fn combo_with_only_times_is_not_empty() {
        let comments: Vec<String> = (0..12).map(|_| "[%emt 0:30]".to_owned()).collect();
        let refs: Vec<&str> = comments.iter().map(String::as_str).collect();
        let g = game_with(&TWELVE, &refs);
        let v = view(&g, GraphKind::ScoreCombo);
        assert!(v.bars.is_empty() && !v.white_line.is_empty());
        assert!(!v.empty);
    }

    #[test]
    fn huge_times_and_long_games_keep_few_ticks() {
        let g = game_with(&TWELVE[..2], &["[%emt 99999:59:59]", "[%emt 0:01]"]);
        let v = view(&g, GraphKind::Time);
        assert!(v.y_axis.label_ticks().count() <= 50);
        assert!(v.y_axis.minor_ticks().count() <= 200);
        let moves: Vec<&str> = (0..20_000)
            .map(|i| ["g1f3", "g8f6", "f3g1", "f6g8"][i % 4])
            .collect();
        let g = game_with(&moves, &vec!["[%eval 1]"; 20_000]);
        let v = view(&g, GraphKind::Score);
        assert_eq!(v.plies, 20_000);
        assert!(v.x_axis.label_ticks().count() <= 50);
        assert!(v.x_axis.minor_ticks().count() <= 200);
        assert!(v.x_axis.major_ticks().count() <= 50);
    }

    fn timed(n: usize, emt: u32) -> Game {
        let texts: Vec<String> = (0..n).map(|_| format!("[%emt 0:{emt:02}]")).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        game_with(&TWELVE[..n], &refs)
    }

    #[test]
    fn auto_picks_time_when_few_scores() {
        let g = timed(6, 12);
        let v = view(&g, GraphKind::Auto);
        assert_eq!(v.kind, GraphKind::Time);
        assert_eq!(v.bars.len(), 6);
        // 6 totals > 5: Auto draws the lines; each side's total tops the bars.
        assert_eq!(v.white_line.last().map(|p| p.1), Some(12.0));
        assert_eq!(
            (v.y_axis.min, v.y_axis.max, v.y_axis.label_step),
            (0.0, 12.0, 2.0)
        );
    }

    #[test]
    fn time_combo_scales_lines_to_bars() {
        let g = timed(12, 10);
        let v = view(&g, GraphKind::TimeCombo);
        assert_eq!(v.kind, GraphKind::TimeCombo);
        // Totals are 60 each, scaled so the largest equals the tallest bar.
        assert_eq!(v.white_line.last().map(|p| p.1), Some(10.0));
        assert_eq!(v.black_line.last().map(|p| p.1), Some(10.0));
    }

    #[test]
    fn plain_time_has_no_lines_and_few_values_no_lines() {
        let g = timed(12, 10);
        let v = view(&g, GraphKind::Time);
        assert!(v.white_line.is_empty() && v.black_line.is_empty());
        let g = timed(5, 10);
        let v = view(&g, GraphKind::TimeCombo);
        assert!(v.white_line.is_empty());
    }

    #[test]
    fn zero_times_do_not_divide_by_zero() {
        let g = timed(12, 0);
        let v = view(&g, GraphKind::TimeCombo);
        assert!(v.white_line.iter().all(|p| p.1 == 0.0));
        assert!(v.y_axis.max > v.y_axis.min);
    }

    #[test]
    fn score_combo_with_few_scores_uses_time_axis() {
        let mut comments: Vec<String> = (0..12).map(|_| "[%emt 0:30]".to_owned()).collect();
        comments[0] = "[%emt 0:30] [%eval 2]".to_owned();
        let refs: Vec<&str> = comments.iter().map(String::as_str).collect();
        let g = game_with(&TWELVE, &refs);
        let v = view(&g, GraphKind::ScoreCombo);
        assert_eq!(v.kind, GraphKind::ScoreCombo);
        assert_eq!(v.bars, vec![(1.0, 2.0)]);
        assert_eq!(v.y_axis.min, 0.0);
        assert_eq!(v.y_axis.label_step, 5.0);
        // Lines scale to the largest emt (30).
        assert_eq!(v.white_line.last().map(|p| p.1), Some(30.0));
    }

    #[test]
    fn title_variants() {
        let mut t = Tags::default();
        t.white = "Anna".into();
        t.black = "Bob".into();
        assert_eq!(graph_title(&t), "Anna - Bob");
        t.white_rating = Some(Rating::new(Elo::new(2500).unwrap(), RatingType::Elo));
        t.result = GameResult::Draw;
        assert_eq!(graph_title(&t), "Anna (2500) - Bob (1/2-1/2)");
        t.result = GameResult::White;
        t.black_rating = Some(Rating::new(Elo::new(0).unwrap(), RatingType::Elo));
        assert_eq!(graph_title(&t), "Anna (2500) - Bob (1-0)");
    }
}
