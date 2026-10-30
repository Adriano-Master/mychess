//! Review fixes of the M6-4 GUI slice (D3450, D3454): the static layer of
//! the three canvases is cached and only the hover is drawn per frame; the
//! export dialog remembers the table it proposed a name for.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::sync::{Arc, Mutex, PoisonError};

use iced::widget::canvas::Program;
use iced::{Point, Rectangle, Size, Theme, mouse};
use iced_test::core::Font;
use iced_test::core::Pixels;
use iced_test::core::renderer::Headless;
use iced_test::renderer::Renderer;
use mychess_app::charts::{
    FilterGraphKind, GraphPoint, GraphView, RatingAxes, RatingGraphView, RatingLineView, YAxis,
};
use mychess_app::piece_tracker::TrackerCell;
use mychess_core::Square;
use mychess_gui::widgets::chart::{ArcKey, StaticLayer};
use mychess_gui::windows::filter_stats_panel::{FilterGraph, FilterState};
use mychess_gui::windows::ptracker_panel::{HeatMap, HeatState};
use mychess_gui::windows::rating_panel::{RatingChart, RatingState};

/// One headless renderer at a time (the graphics backend dislikes several
/// at once).
static LOCK: Mutex<()> = Mutex::new(());

fn renderer() -> Renderer {
    iced_test::futures::futures::executor::block_on(Renderer::new(
        Font::DEFAULT,
        Pixels(16.0),
        std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
    ))
    .expect("a headless renderer")
}

fn bounds() -> Rectangle {
    Rectangle::new(Point::ORIGIN, Size::new(520.0, 320.0))
}

fn cursor(x: f32, y: f32) -> mouse::Cursor {
    mouse::Cursor::Available(Point::new(x, y))
}

fn graph(n: usize) -> Arc<GraphView> {
    let points = (0..n)
        .map(|i| GraphPoint {
            label: if i % 10 == 0 {
                i.to_string()
            } else {
                String::new()
            },
            value: f32::from(u8::try_from(i % 7).unwrap_or(0)),
        })
        .collect();
    Arc::new(GraphView {
        kind: FilterGraphKind::Year,
        relative: false,
        points,
        axis: YAxis {
            step: 2.0,
            max: 8.0,
            ticks: vec![0.0, 2.0, 4.0, 6.0, 8.0],
        },
        mean: Some(3.0),
        total: 10,
        filter_total: 10,
        all_total: 10,
        unreadable: 0,
        blue_lines: Vec::new(),
        title_key: "graph-title-absolute",
    })
}

fn rating() -> Arc<RatingGraphView> {
    Arc::new(RatingGraphView {
        lines: vec![RatingLineView {
            name: "A".to_owned(),
            color: [255, 0, 0],
            points: vec![(2000.0, 2400), (2001.5, 2450), (2003.0, 2500)],
            skipped: 0,
        }],
        empty: false,
        skipped_entries: 0,
        axes: RatingAxes {
            x_min: 2000.0,
            x_max: 2004.0,
            y_min: 2350,
            y_max: 2550,
            x_labels: vec![2000.0, 2002.0, 2004.0],
            y_labels: vec![2400, 2500],
            x_minor: 1.0,
            y_minor: 25,
            x_major: 5.0,
            y_major: 100,
        },
    })
}

#[test]
fn the_filter_graph_repaints_its_static_layer_only_when_the_data_or_theme_change() {
    let _guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let renderer = renderer();
    let data = graph(10_000);
    let program = FilterGraph::new(Arc::clone(&data), false, false);
    let state = FilterState::default();
    let light = Theme::Light;
    let draw = |p: &FilterGraph, theme: &Theme, at| {
        p.draw(&state, &renderer, theme, bounds(), at);
    };
    draw(&program, &light, mouse::Cursor::Unavailable);
    assert_eq!(state.redraws(), 1);
    // Only the cursor moves: the layer stays.
    for x in [60.0, 61.0, 200.0, 333.0] {
        draw(&program, &light, cursor(x, 100.0));
    }
    draw(&program, &light, mouse::Cursor::Unavailable);
    assert_eq!(state.redraws(), 1, "mouse moves repaint only the hover");
    // The theme changes.
    draw(&program, &Theme::Dark, cursor(100.0, 100.0));
    assert_eq!(state.redraws(), 2);
    // Other data (a new Arc, even with equal content).
    let other = FilterGraph::new(graph(10_000), false, false);
    draw(&other, &Theme::Dark, cursor(100.0, 100.0));
    assert_eq!(state.redraws(), 3);
    // The dots option.
    let dots = FilterGraph::new(graph(10_000), true, false);
    draw(&dots, &Theme::Dark, cursor(100.0, 100.0));
    assert_eq!(state.redraws(), 4);
}

#[test]
fn the_rating_graph_repaints_its_static_layer_only_when_the_data_or_theme_change() {
    let _guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let renderer = renderer();
    let data = rating();
    let program = RatingChart::new(Arc::clone(&data), false);
    let state = RatingState::default();
    program.draw(
        &state,
        &renderer,
        &Theme::Light,
        bounds(),
        cursor(10.0, 10.0),
    );
    for x in [100.0, 101.0, 250.0] {
        program.draw(&state, &renderer, &Theme::Light, bounds(), cursor(x, 120.0));
    }
    assert_eq!(state.redraws(), 1);
    program.draw(
        &state,
        &renderer,
        &Theme::Dark,
        bounds(),
        cursor(100.0, 120.0),
    );
    assert_eq!(state.redraws(), 2);
    let other = RatingChart::new(rating(), false);
    other.draw(
        &state,
        &renderer,
        &Theme::Dark,
        bounds(),
        cursor(100.0, 120.0),
    );
    assert_eq!(state.redraws(), 3);
}

fn cells(count: u32) -> Vec<TrackerCell> {
    (0..64u32)
        .map(|i| TrackerCell {
            square: Square::new(i),
            count,
            percent: 1.5,
            fraction: 0.5,
            rgb: [200, 100, 50],
        })
        .collect()
}

#[test]
fn the_heat_map_repaints_its_squares_only_when_they_change() {
    let _guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let renderer = renderer();
    let state = HeatState::default();
    let map = HeatMap::new(cells(3), vec![Square::new(8)], false);
    let big = Rectangle::new(Point::ORIGIN, Size::new(500.0, 500.0));
    for x in [30.0, 31.0, 200.0] {
        map.draw(&state, &renderer, &Theme::Light, big, cursor(x, 100.0));
    }
    assert_eq!(state.redraws(), 1);
    // Same content in a new view: still cached.
    let same = HeatMap::new(cells(3), vec![Square::new(8)], false);
    same.draw(&state, &renderer, &Theme::Light, big, cursor(40.0, 100.0));
    assert_eq!(state.redraws(), 1);
    // Other counts, or stale, or another theme, repaint.
    HeatMap::new(cells(4), vec![Square::new(8)], false).draw(
        &state,
        &renderer,
        &Theme::Light,
        big,
        cursor(40.0, 100.0),
    );
    assert_eq!(state.redraws(), 2);
    HeatMap::new(cells(4), vec![Square::new(8)], true).draw(
        &state,
        &renderer,
        &Theme::Light,
        big,
        cursor(40.0, 100.0),
    );
    assert_eq!(state.redraws(), 3);
    HeatMap::new(cells(4), vec![Square::new(8)], true).draw(
        &state,
        &renderer,
        &Theme::Dark,
        big,
        cursor(40.0, 100.0),
    );
    assert_eq!(state.redraws(), 4);
}

#[test]
fn an_arc_key_is_the_identity_of_the_allocation() {
    let a = Arc::new(5);
    let b = Arc::new(5);
    assert_eq!(ArcKey::of(&a), ArcKey::of(&Arc::clone(&a)));
    assert_ne!(ArcKey::of(&a), ArcKey::of(&b));
    let _layer: StaticLayer<ArcKey<i32>> = StaticLayer::default();
}
