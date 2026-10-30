//! M4-5 (GUI): the evaluation bar and the traffic lights as drawn
//! (spec D453, D454, D458, D459, D850, D851; Scid `board.tcl` `update_evalbar`,
//! `update_mpvtls`). Pure geometry, pixel probes with the software renderer
//! and the layout beside the board. Headless.
#![allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::assertions_on_constants
)]

mod m2_4_common;
mod render_common;

use iced::{Settings, Size, widget};
use iced_test::Simulator;
use m2_4_common::render_lock;
use mychess_app::{EvalBarView, PlayerStrips, PlayersPrefs, Rgb, TrafficLight};
use mychess_core::Position;
use mychess_gui::Message;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::theme::engine_overlay as eo;
use mychess_gui::widgets::board::geometry::{
    BoardLayout, CoordsMode, LayoutOptions, Orientation, column_metrics,
};
use mychess_gui::widgets::board::marks::MarkBrush;
use mychess_gui::widgets::board::pieces::PieceSet;
use mychess_gui::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};
use mychess_gui::widgets::eval_bar::{
    bar_width, eval_bar, light_bands, strip_width, tick_y, traffic_lights, white_span,
};
use mychess_gui::windows::board_column::{
    BoardColumn, CANVAS_ID, EVAL_BAR_ID, LIGHTS_ID, SideBars, policy_of,
};
use proptest::prelude::*;
use render_common::{SCALE, Shot, render};

// ---- pure geometry ----

#[test]
fn strip_width_is_ceil_of_a_tenth_plus_two_for_board_sizes() {
    // Scid `board.tcl:1399`: ceil(s / 10) + 2.
    for (s, want) in [
        (1.0, 3.0),
        (10.0, 3.0),
        (10.5, 4.0),
        (45.0, 7.0),
        (100.0, 12.0),
    ] {
        assert_eq!(strip_width(s), want, "s = {s}");
    }
}

#[test]
fn strip_width_survives_degenerate_sizes() {
    for s in [0.0, -5.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let w = strip_width(s);
        assert!(w.is_finite() && w >= 2.0, "s = {s}: {w}");
    }
}

#[test]
fn the_bar_width_depends_only_on_the_label_preference_and_fits_the_widest_label() {
    // M4-5 fix (D858): the old test tied the width to the presence of a
    // value, which made the board jump while navigating; the width is now
    // a function of the preference alone and holds the widest label.
    for s in [20.0, 80.0, 200.0, 290.0, 400.0] {
        assert_eq!(bar_width(s, false), strip_width(s), "no label, s = {s}");
        assert!(bar_width(s, true) >= strip_width(s), "s = {s}");
        assert!(bar_width(s, true) >= 30.0, "label, s = {s}");
    }
    assert!(bar_width(80.0, true) >= mychess_gui::widgets::eval_bar::widest_label_width());
}

#[test]
fn white_is_measured_from_the_bottom_and_from_the_top_when_flipped() {
    for (fill, height) in [(0.0, 400.0), (0.25, 400.0), (0.5, 320.0), (1.0, 100.0)] {
        let (top, len) = white_span(fill, false, height);
        assert!((len - fill * height).abs() < 1e-3);
        assert!((top + len - height).abs() < 1e-3, "touches the bottom");
        let (top, len) = white_span(fill, true, height);
        assert_eq!(top, 0.0, "touches the top when flipped");
        assert!((len - fill * height).abs() < 1e-3);
    }
}

#[test]
fn white_span_clamps_and_defends_against_non_finite_fills() {
    assert_eq!(white_span(-3.0, false, 80.0), (80.0, 0.0));
    assert_eq!(white_span(7.0, false, 80.0), (0.0, 80.0));
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let (top, len) = white_span(bad, false, 80.0);
        assert_eq!((top, len), (40.0, 40.0), "{bad} falls to the middle");
    }
}

#[test]
fn a_tick_sits_on_the_edge_of_the_white_part() {
    for flipped in [false, true] {
        for p in [0.1_f32, 0.5, 0.9] {
            let (top, len) = white_span(p, flipped, 400.0);
            let edge = if flipped { top + len } else { top };
            assert!((tick_y(p, flipped, 400.0) - edge).abs() < 1e-3);
        }
    }
}

#[test]
fn traffic_light_bands_tile_the_height_without_gaps() {
    for count in 1..=5usize {
        let bands = light_bands(count, 400.0);
        assert_eq!(bands.len(), count);
        let mut next = 0.0;
        for (top, len) in &bands {
            assert!((top - next).abs() < 1e-3, "contiguous");
            assert!((len - 400.0 / count as f32).abs() < 1e-3, "equal height");
            next = top + len;
        }
        assert!((next - 400.0).abs() < 1e-3);
    }
    assert!(light_bands(0, 400.0).is_empty());
}

proptest! {
    #[test]
    fn white_span_always_stays_inside_the_bar(fill in -2.0f32..3.0, h in 1.0f32..2000.0, flipped: bool) {
        let (top, len) = white_span(fill, flipped, h);
        prop_assert!(top >= -1e-3 && len >= -1e-3);
        prop_assert!(top + len <= h + 1e-2);
    }

    #[test]
    fn a_flipped_bar_mirrors_the_white_share(fill in 0.0f32..1.0, h in 10.0f32..1000.0) {
        let (top, len) = white_span(fill, false, h);
        let (ftop, flen) = white_span(fill, true, h);
        prop_assert!((len - flen).abs() < 1e-2);
        prop_assert!((ftop - (h - top - len)).abs() < 1e-2, "mirrored vertically");
    }
}

// ---- pixels ----

fn near(px: [u8; 4], c: iced::Color) -> bool {
    let want = [c.r, c.g, c.b].map(|v| (v * 255.0).round() as i32);
    (0..3).all(|i| (i32::from(px[i]) - want[i]).abs() <= 3)
}

fn dev(v: f32) -> u32 {
    (v * SCALE) as u32
}

/// Renders a bar of 30 x 400 in a window 60 x 400 (the right half is the
/// window background) and returns the shot.
fn bar_shot(view: &EvalBarView) -> Shot {
    render(eval_bar::<()>(view, 30.0, 400.0), Size::new(60.0, 400.0))
}

fn at(shot: &Shot, x: f32, y: f32) -> [u8; 4] {
    shot.pixel(dev(x), dev(y))
}

fn view_with(fill: Option<f32>, flipped: bool) -> EvalBarView {
    EvalBarView {
        fill,
        flipped,
        ..EvalBarView::default()
    }
}

#[test]
fn the_white_part_grows_from_the_bottom_edge() {
    let shot = bar_shot(&view_with(Some(0.75), false));
    // 400 px: white from y = 100.
    assert!(near(at(&shot, 15.0, 60.0), eo::BAR_BLACK), "black above");
    assert!(near(at(&shot, 15.0, 95.0), eo::BAR_BLACK));
    assert!(near(at(&shot, 15.0, 105.0), eo::BAR_WHITE), "white below");
    assert!(near(at(&shot, 15.0, 380.0), eo::BAR_WHITE));
}

#[test]
fn a_flipped_bar_has_white_at_the_top() {
    let shot = bar_shot(&view_with(Some(0.75), true));
    assert!(near(at(&shot, 15.0, 20.0), eo::BAR_WHITE));
    assert!(near(at(&shot, 15.0, 295.0), eo::BAR_WHITE));
    assert!(near(at(&shot, 15.0, 305.0), eo::BAR_BLACK));
    assert!(near(at(&shot, 15.0, 390.0), eo::BAR_BLACK));
}

#[test]
fn a_dead_even_bar_is_split_in_the_middle() {
    let shot = bar_shot(&view_with(Some(0.5), false));
    assert!(near(at(&shot, 15.0, 195.0), eo::BAR_BLACK));
    assert!(near(at(&shot, 15.0, 205.0), eo::BAR_WHITE));
}

#[test]
fn the_extreme_fills_paint_a_single_colour() {
    let all_white = bar_shot(&view_with(Some(1.0), false));
    let all_black = bar_shot(&view_with(Some(0.0), false));
    for y in [10.0, 200.0, 390.0] {
        assert!(near(at(&all_white, 15.0, y), eo::BAR_WHITE), "y = {y}");
        assert!(near(at(&all_black, 15.0, y), eo::BAR_BLACK), "y = {y}");
    }
}

#[test]
fn an_empty_bar_is_only_the_outline() {
    let shot = bar_shot(&EvalBarView::default());
    let background = at(&shot, 45.0, 200.0);
    // Scid state `clear`: no white nor black body.
    assert!(!near(at(&shot, 15.0, 100.0), eo::BAR_WHITE));
    assert!(!near(at(&shot, 15.0, 300.0), eo::BAR_BLACK));
    assert_eq!(at(&shot, 15.0, 100.0), background, "interior is empty");
    // The contour is there: some pixel on the left edge is the outline grey.
    let edge: Vec<[u8; 4]> = (0..2).map(|dx| shot.pixel(dx, dev(200.0))).collect();
    assert!(
        edge.iter().any(|p| near(*p, eo::BAR_OUTLINE)),
        "an outline: {edge:?}"
    );
}

#[test]
fn a_label_without_a_fill_draws_no_text() {
    let with = EvalBarView {
        label: Some("+0.35".to_owned()),
        ..EvalBarView::default()
    };
    let a = bar_shot(&with);
    let b = bar_shot(&EvalBarView::default());
    for y in 0..400u32 {
        for x in 0..60u32 {
            assert_eq!(a.pixel(x, y), b.pixel(x, y), "({x}, {y})");
        }
    }
}

#[test]
fn a_stale_bar_is_faded_but_keeps_its_split() {
    let fresh = bar_shot(&view_with(Some(0.75), false));
    let stale = bar_shot(&EvalBarView {
        stale: true,
        ..view_with(Some(0.75), false)
    });
    let (f, s) = (at(&fresh, 15.0, 50.0), at(&stale, 15.0, 50.0));
    assert_ne!(f, s, "the black part fades");
    assert!(s[0] > f[0], "towards the light background: {f:?} -> {s:?}");
    // Same split: the white edge is still at y = 100.
    assert!(at(&stale, 15.0, 95.0)[0] < at(&stale, 15.0, 105.0)[0]);
}

fn diff_in(a: &Shot, b: &Shot, ys: std::ops::Range<f32>) -> usize {
    let mut n = 0;
    for y in dev(ys.start)..dev(ys.end) {
        for x in 4..dev(30.0) - 4 {
            if a.pixel(x, y) != b.pixel(x, y) {
                n += 1;
            }
        }
    }
    n
}

fn labelled(fill: f32, flipped: bool, on_white: bool) -> (Shot, Shot) {
    let plain = view_with(Some(fill), flipped);
    let label = EvalBarView {
        label: Some("+1.20".to_owned()),
        label_on_white: on_white,
        ..plain.clone()
    };
    (bar_shot(&plain), bar_shot(&label))
}

#[test]
fn the_label_sits_at_the_end_of_the_side_that_is_ahead() {
    // White ahead, board not turned: white is at the bottom.
    let (plain, with) = labelled(0.8, false, true);
    assert!(diff_in(&plain, &with, 340.0..400.0) > 10, "bottom");
    assert_eq!(diff_in(&plain, &with, 0.0..300.0), 0, "nothing elsewhere");
    // Black ahead, not turned: black is at the top.
    let (plain, with) = labelled(0.2, false, false);
    assert!(diff_in(&plain, &with, 0.0..60.0) > 10, "top");
    assert_eq!(diff_in(&plain, &with, 100.0..400.0), 0);
}

#[test]
fn the_label_follows_the_side_when_the_bar_is_flipped() {
    // White ahead, flipped: white is at the top.
    let (plain, with) = labelled(0.8, true, true);
    assert!(diff_in(&plain, &with, 0.0..60.0) > 10, "top");
    assert_eq!(diff_in(&plain, &with, 100.0..400.0), 0);
    // Black ahead, flipped: black is at the bottom.
    let (plain, with) = labelled(0.2, true, false);
    assert!(diff_in(&plain, &with, 340.0..400.0) > 10, "bottom");
    assert_eq!(diff_in(&plain, &with, 0.0..300.0), 0);
}

#[test]
fn the_label_text_contrasts_with_the_part_it_sits_on() {
    // Dark text on white, light text on black: the glyph pixels move away
    // from the background of their side.
    let (plain, with) = labelled(0.8, false, true);
    let mut darkest = 255u8;
    for y in dev(340.0)..dev(400.0) {
        for x in 4..dev(30.0) - 4 {
            if plain.pixel(x, y) != with.pixel(x, y) {
                darkest = darkest.min(with.pixel(x, y)[0]);
            }
        }
    }
    assert!(darkest < 120, "dark glyphs over white: {darkest}");
    let (plain, with) = labelled(0.2, false, false);
    let mut lightest = 0u8;
    for y in 0..dev(60.0) {
        for x in 4..dev(30.0) - 4 {
            if plain.pixel(x, y) != with.pixel(x, y) {
                lightest = lightest.max(with.pixel(x, y)[0]);
            }
        }
    }
    assert!(lightest > 160, "light glyphs over black: {lightest}");
}

#[test]
fn ticks_are_drawn_across_the_bar_only_when_given() {
    let none = bar_shot(&view_with(Some(0.5), false));
    let ticked = bar_shot(&EvalBarView {
        ticks: vec![0.25, 0.75],
        ..view_with(Some(0.5), false)
    });
    // 0.25 of 400 from the bottom is y = 300; 0.75 is y = 100.
    for y in [100.0_f32, 300.0] {
        let row: Vec<_> = (6..24)
            .map(|x| ticked.pixel(dev(x as f32), dev(y)))
            .collect();
        assert!(row.iter().any(|p| near(*p, eo::BAR_OUTLINE)), "tick at {y}");
    }
    let mut different = 0;
    for y in 0..dev(400.0) {
        if none.pixel(dev(15.0), y) != ticked.pixel(dev(15.0), y) {
            different += 1;
        }
    }
    assert!(different >= 2, "ticks add pixels");
}

fn light(rank: u8, r: u8, g: u8, b: u8) -> TrafficLight {
    TrafficLight {
        rank,
        color: Rgb { r, g, b },
    }
}

fn lights_shot(lights: &[TrafficLight], height: f32) -> Shot {
    render(
        traffic_lights::<()>(lights, 12.0, height),
        Size::new(24.0, height),
    )
}

#[test]
fn each_traffic_light_is_one_band_of_its_own_colour_line_1_on_top() {
    let lights = [
        light(1, 0x09, 0xf8, 0x00),
        light(2, 0xf8, 0xf8, 0x00),
        light(3, 0xf8, 0x09, 0x00),
    ];
    let shot = lights_shot(&lights, 300.0);
    for (i, l) in lights.iter().enumerate() {
        let y = 100.0 * i as f32 + 50.0;
        let c = eo::light(l.color.r, l.color.g, l.color.b);
        assert!(near(at(&shot, 6.0, y), c), "band {i} at y = {y}");
    }
}

#[test]
fn five_lines_make_five_equal_bands() {
    let lights: Vec<TrafficLight> = (1..=5u8)
        .map(|r| light(r, 0x30 * r, 0xf8 - 0x20 * r, 0))
        .collect();
    let shot = lights_shot(&lights, 400.0);
    for (i, l) in lights.iter().enumerate() {
        let y = 80.0 * i as f32 + 40.0;
        assert!(
            near(
                at(&shot, 6.0, y),
                eo::light(l.color.r, l.color.g, l.color.b)
            ),
            "band {i}"
        );
    }
}

#[test]
fn a_single_line_is_a_single_strip_of_the_full_height() {
    let shot = lights_shot(&[light(1, 0xf8, 0xf8, 0)], 200.0);
    for y in [10.0, 100.0, 190.0] {
        assert!(near(at(&shot, 6.0, y), eo::light(0xf8, 0xf8, 0)), "y = {y}");
    }
}

#[test]
fn the_default_palette_is_legible_against_both_window_backgrounds() {
    // D850: the colours do not follow the theme; the bar must still stand
    // out of a light and of a dark window (WCAG 3:1 for graphics).
    fn lum(c: iced::Color) -> f32 {
        let f = |v: f32| {
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    }
    fn ratio(a: iced::Color, b: iced::Color) -> f32 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }
    // Inside the bar: the labels.
    assert!(ratio(eo::BAR_TEXT_ON_WHITE, eo::BAR_WHITE) >= 4.5);
    assert!(ratio(eo::BAR_TEXT_ON_BLACK, eo::BAR_BLACK) >= 4.5);
    // The two halves tell apart.
    assert!(ratio(eo::BAR_WHITE, eo::BAR_BLACK) >= 7.0);
    // The outline separates the bar from a light and from a dark window.
    let light_bg = iced::Color::WHITE;
    let dark_bg = iced::Color::from_rgb(0.1, 0.1, 0.1);
    assert!(ratio(eo::BAR_OUTLINE, light_bg) >= 3.0, "outline on light");
    assert!(ratio(eo::BAR_OUTLINE, dark_bg) >= 2.0, "outline on dark");
    // The best arrow is not grey like the others.
    assert!(
        (eo::BEST_ARROW.b - eo::BEST_ARROW.r).abs() > 0.3,
        "the engine arrow is clearly blue"
    );
    assert!((eo::OTHER_ARROW.r - eo::OTHER_ARROW.b).abs() < 0.01, "grey");
    assert!(eo::STALE_ALPHA > 0.0 && eo::STALE_ALPHA < 1.0);
}

// ---- layout beside the board ----

struct Rig {
    position: Position,
    strips: PlayerStrips,
    brush: MarkBrush,
    pieces: PieceSet,
    caches: BoardCaches,
    i18n: I18n,
}

impl Rig {
    fn new() -> Rig {
        Rig {
            position: Position::new(),
            strips: PlayerStrips::default(),
            brush: MarkBrush::default(),
            pieces: PieceSet::default(),
            caches: BoardCaches::default(),
            i18n: I18n::new(Language::EnUs),
        }
    }

    fn element<'a>(
        &'a self,
        size: Size,
        coords: CoordsMode,
        bars: SideBars<'a>,
    ) -> iced::Element<'a, Message> {
        let data = BoardData {
            position: &self.position,
            last_move: None,
            marks: &[],
            orientation: Orientation::White,
            options: BoardOptions {
                coords,
                ..BoardOptions::default()
            },
            accepts_moves: false,
            brush: &self.brush,
            pieces: &self.pieces,
            animation: None,
        };
        BoardColumn {
            board: BoardWidget::new(data, &self.caches),
            strips: &self.strips,
            prefs: PlayersPrefs::default(),
            countdown: true,
            show_side_to_move: true,
            coords,
            orientation: Orientation::White,
            pieces: &self.pieces,
            i18n: &self.i18n,
        }
        .layout_with(size, bars)
    }
}

fn bounds(sim: &mut Simulator<'_, Message>, id: &'static str) -> Option<iced::Rectangle> {
    sim.find(widget::Id::new(id)).ok().map(|t| t.bounds())
}

fn lights3() -> Vec<TrafficLight> {
    (1..=3).map(|r| light(r, 0x20 * r, 0xf8, 0)).collect()
}

#[test]
fn a_labelled_bar_is_30_px_wide_in_the_layout_and_an_unlabelled_one_follows_the_scid() {
    let _guard = render_lock();
    let size = Size::new(900.0, 700.0);
    for label in [None, Some("+0.35".to_owned())] {
        let view = EvalBarView {
            fill: Some(0.6),
            label: label.clone(),
            ..EvalBarView::default()
        };
        let rig = Rig::new();
        let bars = SideBars {
            eval: Some(&view),
            lights: &[],
            eval_label: true,
        };
        let mut sim = Simulator::with_size(
            Settings::default(),
            size,
            rig.element(size, CoordsMode::Outside, bars),
        );
        let canvas = bounds(&mut sim, CANVAS_ID).expect("canvas");
        let bar = bounds(&mut sim, EVAL_BAR_ID).expect("bar");
        let layout = BoardLayout::compute(
            canvas.size(),
            LayoutOptions {
                coords: CoordsMode::Outside,
            },
        )
        .expect("layout");
        // D858: with the label preference on, an empty bar reserves the
        // same room as one with a value.
        assert_eq!(bar.width, bar_width(layout.s, true), "label {label:?}");
        assert!(bar.width >= 30.0);
    }
}

#[test]
fn the_board_shrinks_to_make_room_for_both_columns() {
    let _guard = render_lock();
    let size = Size::new(480.0, 800.0);
    let view = EvalBarView {
        fill: Some(0.6),
        label: Some("+0.35".to_owned()),
        ..EvalBarView::default()
    };
    let strips = lights3();
    let rig = Rig::new();
    let width_of = |bars: SideBars<'_>| {
        let mut sim = Simulator::with_size(
            Settings::default(),
            size,
            rig.element(size, CoordsMode::Inside, bars),
        );
        bounds(&mut sim, CANVAS_ID).expect("canvas").width
    };
    let none = width_of(SideBars::default());
    let one = width_of(SideBars {
        eval: Some(&view),
        lights: &[],
        eval_label: true,
    });
    let both = width_of(SideBars {
        eval: Some(&view),
        lights: &strips,
        eval_label: true,
    });
    let free = column_metrics(size, CoordsMode::Inside, policy_of(PlayersPrefs::default()))
        .expect("fits")
        .canvas
        .width;
    assert_eq!(none, free);
    assert!(one < none, "the bar takes room: {one} vs {none}");
    assert!(both <= one, "the lights take room too: {both} vs {one}");
}

#[test]
fn bars_stay_inside_the_panel_on_narrow_and_tiny_windows_and_line_up_with_the_squares() {
    let _guard = render_lock();
    let view = EvalBarView {
        fill: Some(0.3),
        label: Some("-0.80".to_owned()),
        ..EvalBarView::default()
    };
    let strips = lights3();
    for (w, h) in [
        (300.0, 900.0),
        (420.0, 300.0),
        (360.0, 360.0),
        (240.0, 240.0),
        (1600.0, 400.0),
    ] {
        let rig = Rig::new();
        let size = Size::new(w, h);
        let bars = SideBars {
            eval: Some(&view),
            lights: &strips,
            eval_label: true,
        };
        let mut sim = Simulator::with_size(
            Settings::default(),
            size,
            rig.element(size, CoordsMode::Outside, bars),
        );
        let (Some(canvas), Some(bar)) =
            (bounds(&mut sim, CANVAS_ID), bounds(&mut sim, EVAL_BAR_ID))
        else {
            continue; // too small to draw anything: nothing to misplace
        };
        let layout = BoardLayout::compute(
            canvas.size(),
            LayoutOptions {
                coords: CoordsMode::Outside,
            },
        )
        .expect("layout");
        assert_eq!(bar.height, 8.0 * layout.s, "{w}x{h}");
        assert_eq!(bar.y, canvas.y + layout.board.y, "{w}x{h}");
        assert!(bar.x + bar.width <= w + 0.5, "{w}x{h}: {bar:?}");
        if let Some(l) = bounds(&mut sim, LIGHTS_ID) {
            assert!(bar.x + bar.width <= l.x, "{w}x{h}: bar before lights");
            assert!(l.x + l.width <= w + 0.5, "{w}x{h}: {l:?}");
        }
    }
}

#[test]
fn the_traffic_lights_alone_sit_to_the_right_of_the_board() {
    let _guard = render_lock();
    let strips = lights3();
    let rig = Rig::new();
    let size = Size::new(900.0, 700.0);
    let bars = SideBars {
        eval: None,
        lights: &strips,
        eval_label: true,
    };
    let mut sim = Simulator::with_size(
        Settings::default(),
        size,
        rig.element(size, CoordsMode::Outside, bars),
    );
    let canvas = bounds(&mut sim, CANVAS_ID).expect("canvas");
    assert!(bounds(&mut sim, EVAL_BAR_ID).is_none());
    let l = bounds(&mut sim, LIGHTS_ID).expect("lights");
    assert!(l.x >= canvas.x + canvas.width);
}
