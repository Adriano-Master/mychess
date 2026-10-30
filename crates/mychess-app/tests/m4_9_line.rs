//! M4-9 contract tests of the continuous-line graph (spec
//! `m4-9-grafico-de-linha.md`, D1400-D1413, D1527-D1529): the pure geometry,
//! the view, the preference and the message.
#![allow(
    clippy::unwrap_used,
    clippy::assert_is_empty,
    clippy::nonminimal_bool,
    clippy::redundant_closure_for_method_calls,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::type_complexity
)]

mod m4_7_common;

use m4_7_common::{game_with, shuffle_game};
use mychess_app::graphs::{
    GraphKind, GraphMessage, GraphPrefs, GraphStyle, Invert, LinePoint, Marker, MarkerKind,
    grid_lines, line_graph_view, monotone_segments, move_ticks, nearest_ply, score_graph_view,
    score_series, squash,
};
use mychess_app::{AppState, Message, Preferences, load_preferences, render_preferences, update};
use proptest::prelude::*;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn point(ply: usize, eval: f32) -> LinePoint {
    LinePoint {
        ply,
        eval,
        y: squash(eval),
    }
}

// ---- squash (D1403) ----

#[test]
fn squash_maps_zero_to_zero_and_follows_the_formula() {
    assert_eq!(squash(0.0), 0.0);
    let want = 2.0 / (1.0 + (-0.368f32).exp()) - 1.0;
    assert!(close(squash(1.0), want), "{} vs {want}", squash(1.0));
    // 5 pawns is the logistic of the win probability scale.
    let five = 2.0 / (1.0 + (-0.368f32 * 5.0).exp()) - 1.0;
    assert!(close(squash(5.0), five));
}

#[test]
fn squash_stays_strictly_inside_the_rectangle_at_the_limit() {
    for e in [10.0f32, -10.0, 1000.0, -1000.0] {
        let y = squash(e);
        assert!(y.abs() < 1.0 && y.abs() > 0.9, "squash({e}) = {y}");
    }
    assert_eq!(squash(1000.0), squash(10.0), "saturates at the score limit");
}

proptest! {
    #[test]
    fn squash_is_odd_increasing_and_bounded(a in -50.0f32..50.0, b in -50.0f32..50.0) {
        prop_assert!((squash(a) + squash(-a)).abs() < 1e-5);
        prop_assert!(squash(a).abs() <= 1.0);
        if a.clamp(-10.0, 10.0) < b.clamp(-10.0, 10.0) {
            prop_assert!(squash(a) < squash(b));
        }
    }
}

#[test]
fn squash_of_nan_is_zero_not_a_panic() {
    assert_eq!(squash(f32::NAN), 0.0);
}

// ---- grid ----

#[test]
fn the_grid_has_zero_and_plus_minus_one_three_five() {
    let lines = grid_lines();
    assert_eq!(lines.len(), 7);
    assert!(lines.contains(&(0.0, 0.0)));
    for e in [1.0f32, 3.0, 5.0] {
        let up = lines.iter().find(|l| l.0 == e).expect("positive line");
        let down = lines.iter().find(|l| l.0 == -e).expect("negative line");
        assert!(close(up.1, squash(e)) && close(down.1, -squash(e)));
    }
}

// ---- monotone curve (D1401) ----

#[test]
fn the_curve_passes_through_the_points_and_has_one_piece_per_pair() {
    let points = [point(1, 0.3), point(2, -1.0), point(3, 4.0), point(5, 4.0)];
    let segs = monotone_segments(&points);
    assert_eq!(segs.len(), 3);
    for (seg, pair) in segs.iter().zip(points.windows(2)) {
        assert_eq!(seg.from, (pair[0].ply as f32, pair[0].y));
        assert_eq!(seg.to, (pair[1].ply as f32, pair[1].y));
        assert_eq!(seg.at(0.0), seg.from);
        let end = seg.at(1.0);
        assert!(close(end.0, seg.to.0) && close(end.1, seg.to.1));
    }
}

#[test]
fn fewer_than_two_points_give_no_pieces() {
    assert!(monotone_segments(&[]).is_empty());
    assert!(monotone_segments(&[point(3, 1.0)]).is_empty());
}

#[test]
fn a_piece_that_skips_unscored_plies_is_a_gap() {
    let segs = monotone_segments(&[point(1, 0.0), point(2, 1.0), point(5, 2.0)]);
    assert!(!segs[0].gap, "neighbours: continuous");
    assert!(segs[1].gap, "plies 3 and 4 have no score: dashed");
}

#[test]
fn a_flat_stretch_stays_flat_and_a_peak_is_not_overshot() {
    let points = [point(1, 0.0), point(2, 5.0), point(3, 0.0)];
    let top = squash(5.0);
    for seg in monotone_segments(&points) {
        for i in 0..=100u8 {
            let (_, y) = seg.at(f32::from(i) / 100.0);
            assert!(y <= top + 1e-5 && y >= -1e-5, "y = {y}");
        }
    }
    let flat = monotone_segments(&[point(1, 2.0), point(2, 2.0), point(3, 2.0)]);
    for seg in flat {
        for i in 0..=10u8 {
            assert!(close(seg.at(f32::from(i) / 10.0).1, squash(2.0)));
        }
    }
}

proptest! {
    #[test]
    fn the_curve_never_leaves_the_range_of_its_two_end_values(
        evals in proptest::collection::vec(-10.0f32..10.0, 2..40),
        gaps in proptest::collection::vec(1usize..4, 40),
    ) {
        let mut ply = 0;
        let points: Vec<LinePoint> = evals
            .iter()
            .zip(&gaps)
            .map(|(e, g)| {
                ply += g;
                point(ply, *e)
            })
            .collect();
        let segs = monotone_segments(&points);
        prop_assert_eq!(segs.len(), points.len() - 1);
        for (seg, pair) in segs.iter().zip(points.windows(2)) {
            let (lo, hi) = (pair[0].y.min(pair[1].y), pair[0].y.max(pair[1].y));
            let mut last_x = seg.from.0;
            for i in 0..=50u8 {
                let (x, y) = seg.at(f32::from(i) / 50.0);
                prop_assert!(y >= lo - 1e-4 && y <= hi + 1e-4, "y {y} outside [{lo}, {hi}]");
                prop_assert!(x >= last_x - 1e-4, "x goes back");
                last_x = x;
            }
        }
    }
}

// ---- nearest_ply (D1407) ----

#[test]
fn nearest_ply_rounds_and_clamps() {
    assert_eq!(nearest_ply(2.4, 10), Some(2));
    assert_eq!(nearest_ply(2.6, 10), Some(3));
    assert_eq!(nearest_ply(0.0, 10), Some(0));
    assert_eq!(nearest_ply(-5.0, 10), Some(0));
    assert_eq!(nearest_ply(99.0, 10), Some(10));
    assert_eq!(nearest_ply(10.0, 10), Some(10));
}

#[test]
fn nearest_ply_refuses_hostile_input() {
    assert_eq!(nearest_ply(f32::NAN, 10), None);
    assert_eq!(nearest_ply(f32::INFINITY, 10), None);
    assert_eq!(nearest_ply(f32::NEG_INFINITY, 10), None);
    assert_eq!(nearest_ply(3.0, 0), None, "a game without moves");
}

proptest! {
    #[test]
    fn nearest_ply_is_within_the_game(x in -1e6f32..1e6, plies in 1usize..2000) {
        let ply = nearest_ply(x, plies).unwrap();
        prop_assert!(ply <= plies);
    }
}

// ---- move_ticks (D1409) ----

#[test]
fn ticks_are_in_range_increasing_and_limited() {
    for plies in [0usize, 1, 2, 9, 40, 120, 500, 1000] {
        for max in [0usize, 1, 3, 10, 50] {
            let ticks = move_ticks(plies, max);
            assert!(ticks.len() <= max.max(1), "{plies}/{max}: {ticks:?}");
            assert!(ticks.iter().all(|t| *t >= 1 && *t <= plies), "{ticks:?}");
            assert!(ticks.windows(2).all(|w| w[0] < w[1]), "{ticks:?}");
        }
    }
}

#[test]
fn a_game_without_moves_has_no_ticks() {
    assert!(move_ticks(0, 10).is_empty());
}

#[test]
fn a_long_game_gets_sparse_round_labels() {
    let ticks = move_ticks(500, 10);
    assert!(!ticks.is_empty());
    // Labels are on whole moves (even plies) and evenly spaced.
    assert!(ticks.iter().all(|t| t % 2 == 0));
    let step = ticks[0];
    assert!(ticks.iter().enumerate().all(|(i, t)| *t == step * (i + 1)));
}

// ---- line_graph_view (D1405, D1412) ----

#[test]
fn points_follow_the_eval_comments_of_the_main_line() {
    let game = game_with(&["[%eval 0.35]", "", "[%eval -1.2]", "[%eval 2]"]);
    let view = line_graph_view(&game, Invert::default(), &[]);
    assert_eq!(view.plies, 4);
    let plies: Vec<usize> = view.points.iter().map(|p| p.ply).collect();
    assert_eq!(plies, vec![1, 3, 4], "ply 2 has no score: a gap");
    assert!(close(view.points[1].eval, -1.2));
    assert!(close(view.points[1].y, squash(-1.2)));
    assert_eq!(view.segments.len(), 2);
    assert!(view.segments[0].gap && !view.segments[1].gap);
    assert_eq!(view.point_at(3).map(|p| p.ply), Some(3));
    assert!(view.point_at(2).is_none());
}

#[test]
fn ply_zero_only_exists_when_the_root_has_an_eval() {
    let mut game = game_with(&["[%eval 0.2]", "[%eval 0.3]"]);
    let without = line_graph_view(&game, Invert::default(), &[]);
    assert_eq!(without.points.first().map(|p| p.ply), Some(1));
    let root = game.root();
    game.set_comment(root, "[%eval 0.15]").unwrap();
    let with = line_graph_view(&game, Invert::default(), &[]);
    assert_eq!(with.points.first().map(|p| p.ply), Some(0));
    assert!(close(with.points[0].eval, 0.15));
    assert_eq!(with.plies, 2, "the x axis is still 0..=plies");
}

#[test]
fn a_game_without_evaluations_has_no_curve() {
    let game = game_with(&["", "", ""]);
    let view = line_graph_view(&game, Invert::default(), &[]);
    assert!(view.points.is_empty() && view.segments.is_empty());
    assert_eq!(view.plies, 3);
}

#[test]
fn a_game_without_moves_has_zero_plies_and_no_points() {
    let game = game_with(&[]);
    let view = line_graph_view(&game, Invert::default(), &[]);
    assert_eq!(view.plies, 0);
    assert!(view.points.is_empty());
}

#[test]
fn a_single_scored_move_is_a_point_without_pieces() {
    let game = game_with(&["", "[%eval 1.0]", ""]);
    let view = line_graph_view(&game, Invert::default(), &[]);
    assert_eq!(view.points.len(), 1);
    assert!(view.segments.is_empty());
}

#[test]
fn mates_saturate_inside_the_rectangle() {
    let game = game_with(&["[%eval #3]", "[%eval #-2]", "[%eval 400]"]);
    let view = line_graph_view(&game, Invert::default(), &[]);
    assert_eq!(view.points.len(), 3);
    assert!(close(view.points[0].eval, 10.0), "{:?}", view.points[0]);
    assert!(close(view.points[1].eval, -10.0));
    assert!(close(view.points[2].eval, 10.0));
    for p in &view.points {
        assert!(p.y.abs() < 1.0 && p.y.abs() > 0.9, "{p:?}");
    }
}

#[test]
fn a_null_score_is_a_real_point_at_the_zero_line() {
    let game = game_with(&["[%eval 0.0]", "[%eval 0.00]", "[%eval -0.0]"]);
    let view = line_graph_view(&game, Invert::default(), &[]);
    assert_eq!(view.points.len(), 3);
    assert!(view.points.iter().all(|p| p.y.abs() < 1e-6));
    assert!(view.segments.iter().all(|s| !s.gap));
}

#[test]
fn the_points_are_the_same_series_as_the_bars_with_any_inversion() {
    let game = game_with(&[
        "[%eval 0.5]",
        "[%eval -0.2]",
        "[%eval #4]",
        "",
        "[%eval 3.3]",
        "[%eval -7]",
    ]);
    for (white, black) in [(false, false), (true, false), (false, true), (true, true)] {
        let invert = Invert { white, black };
        let view = line_graph_view(&game, invert, &[]);
        let series = score_series(&game, invert);
        assert_eq!(view.points.len(), series.len(), "{invert:?}");
        for (p, s) in view.points.iter().zip(&series) {
            assert!(close(p.eval, s.y), "{invert:?}: {} vs {}", p.eval, s.y);
            // Bars: x = ply * 0.5 + 0.5 (D483).
            assert!(close(s.x, p.ply as f32 * 0.5 + 0.5));
        }
    }
}

#[test]
fn markers_sit_on_the_curve_without_changing_it() {
    let game = game_with(&["[%eval 0.5]", "[%eval -3]", "[%eval 0.1]"]);
    let plain = line_graph_view(&game, Invert::default(), &[]);
    let markers = [
        Marker {
            ply: 2,
            kind: MarkerKind::Blunder,
        },
        Marker {
            ply: 3,
            kind: MarkerKind::Brilliant,
        },
        Marker {
            ply: 99,
            kind: MarkerKind::Miss,
        },
    ];
    let marked = line_graph_view(&game, Invert::default(), &markers);
    assert_eq!(marked.points, plain.points);
    assert_eq!(marked.segments, plain.segments);
    assert_eq!(marked.markers.len(), 2, "a marker past the game is dropped");
    assert!(marked.markers.iter().all(|m| m.ply <= marked.plies));
}

#[test]
fn a_500_ply_game_builds_every_point() {
    let game = shuffle_game(500, |i| Some(format!("[%eval {}]", (i % 7) as f32 - 3.0)));
    let view = line_graph_view(&game, Invert::default(), &[]);
    assert_eq!(view.plies, 500);
    assert_eq!(view.points.len(), 500);
    assert_eq!(view.segments.len(), 499);
    assert!(view.segments.iter().all(|s| !s.gap));
}

// ---- the style (D1400) and the view (D1527) ----

#[test]
fn the_default_style_is_line_and_names_round_trip() {
    assert_eq!(GraphStyle::default(), GraphStyle::Line);
    assert_eq!(GraphPrefs::default().style, GraphStyle::Line);
    for style in GraphStyle::ALL {
        assert_eq!(GraphStyle::from_name(style.name()), Some(style));
    }
    assert_eq!(GraphStyle::Line.name(), "line");
    assert_eq!(GraphStyle::Bars.name(), "bars");
    assert_eq!(GraphStyle::from_name("Line"), None);
    assert_eq!(GraphStyle::from_name(""), None);
}

fn evals() -> mychess_core::Game {
    game_with(&[
        "[%eval 0.17]",
        "[%eval 0.25]",
        "[%eval 0.3]",
        "[%eval -0.4]",
        "[%eval 0.5]",
        "[%eval -4]",
    ])
}

#[test]
fn the_line_style_adds_a_curve_and_keeps_the_bars() {
    let game = evals();
    let bars = score_graph_view(
        &game,
        GraphPrefs {
            style: GraphStyle::Bars,
            ..GraphPrefs::default()
        },
    );
    let line = score_graph_view(&game, GraphPrefs::default());
    assert!(bars.line.is_none(), "Bars is the M4-7 graph, unchanged");
    let curve = line.line.as_ref().expect("the default style draws a line");
    assert_eq!(curve.points.len(), 6);
    assert_eq!(line.bars, bars.bars, "the bars stay in the view");
    assert_eq!(line.y_axis, bars.y_axis);
    assert_eq!(line.title, bars.title);
}

#[test]
fn time_kinds_stay_in_bars_whatever_the_style() {
    let game = evals();
    for kind in [GraphKind::Time, GraphKind::TimeCombo] {
        let view = score_graph_view(
            &game,
            GraphPrefs {
                kind,
                style: GraphStyle::Line,
                ..GraphPrefs::default()
            },
        );
        assert!(view.line.is_none(), "{kind:?}");
    }
}

#[test]
fn a_game_without_scores_has_no_curve_and_is_empty() {
    let game = game_with(&["", "", ""]);
    let view = score_graph_view(&game, GraphPrefs::default());
    assert!(view.line.as_ref().is_none_or(|l| l.points.is_empty()));
    assert!(view.empty);
}

// ---- preference (D1400, D1527) ----

#[test]
fn style_is_written_and_read_back() {
    for style in GraphStyle::ALL {
        let prefs = Preferences {
            graph: GraphPrefs {
                style,
                ..GraphPrefs::default()
            },
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(
            text.contains(&format!("style = \"{}\"", style.name())),
            "{text}"
        );
        let loaded = load_preferences(&text);
        assert_eq!(loaded.prefs.graph.style, style);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }
}

#[test]
fn an_old_file_without_style_gets_the_line() {
    let loaded = load_preferences("[graph]\nkind = \"score\"\n");
    assert_eq!(loaded.prefs.graph.style, GraphStyle::Line);
    assert_eq!(loaded.prefs.graph.kind, GraphKind::Score);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn a_hostile_style_falls_back_without_spoiling_the_other_fields() {
    for bad in [
        "style = \"curve\"",
        "style = \"BARS\"",
        "style = 3",
        "style = true",
        "style = []",
    ] {
        let loaded = load_preferences(&format!("[graph]\nkind = \"time\"\n{bad}\n"));
        assert_eq!(loaded.prefs.graph.style, GraphStyle::Line, "{bad}");
        assert_eq!(loaded.prefs.graph.kind, GraphKind::Time, "{bad}");
    }
    let loaded = load_preferences("[graph]\nstyle = \"bars\"\n");
    assert_eq!(loaded.prefs.graph.style, GraphStyle::Bars);
}

// ---- message and state (D1400, D1528) ----

#[test]
fn set_style_changes_the_view_and_persists() {
    let mut state = AppState::default();
    state.session.replace_game(evals());
    drop(update(&mut state, GraphMessage::SetOpen(true).into()));
    assert!(state.graph().view().unwrap().line.is_some());
    drop(update(
        &mut state,
        Message::Graph(GraphMessage::SetStyle(GraphStyle::Bars)),
    ));
    assert_eq!(state.prefs.graph.style, GraphStyle::Bars);
    let view = state.graph().view().expect("view");
    assert!(view.line.is_none());
    assert_eq!(view.bars.len(), 6);
    let loaded = load_preferences(&render_preferences(&state.prefs));
    assert_eq!(loaded.prefs.graph.style, GraphStyle::Bars);
    drop(update(
        &mut state,
        Message::Graph(GraphMessage::SetStyle(GraphStyle::Line)),
    ));
    assert!(state.graph().view().unwrap().line.is_some());
}

#[test]
fn set_style_keeps_the_other_preferences() {
    let mut state = AppState::default();
    drop(update(
        &mut state,
        Message::Graph(GraphMessage::SetKind(GraphKind::Score)),
    ));
    drop(update(
        &mut state,
        Message::Graph(GraphMessage::ToggleInvertBlack),
    ));
    drop(update(
        &mut state,
        Message::Graph(GraphMessage::SetStyle(GraphStyle::Bars)),
    ));
    let g = state.prefs.graph;
    assert_eq!(g.kind, GraphKind::Score);
    assert!(g.invert.black && !g.invert.white);
    assert!(g.highlight_current);
    assert_eq!(g.style, GraphStyle::Bars);
}

// ---- GoToPly (D1556) ----

#[test]
fn go_to_ply_reaches_plies_without_a_score_and_ply_zero() {
    // 60 plies, scores only on the first 20.
    let game = shuffle_game(60, |i| (i < 20).then(|| "[%eval 0.30]".to_owned()));
    let target = m4_7_common::node_at(&game, 40);
    let mut state = AppState::default();
    state.session.replace_game(game);
    drop(update(&mut state, GraphMessage::SetOpen(true).into()));
    drop(update(&mut state, GraphMessage::GoToPly(40).into()));
    assert_eq!(state.session.cursor(), target, "ply 40 has no score");
    drop(update(&mut state, GraphMessage::GoToPly(0).into()));
    assert_eq!(state.session.cursor(), state.session.game().root());
    // Past the end is clamped to the last ply of the view.
    drop(update(&mut state, GraphMessage::GoToPly(9_999).into()));
    let last = state.graph().view().expect("view").plies;
    assert_eq!(
        state.session.cursor(),
        m4_7_common::node_at(state.session.game(), last)
    );
}
