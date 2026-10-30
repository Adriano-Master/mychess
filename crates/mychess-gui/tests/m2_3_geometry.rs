//! M2-3 board geometry, derived from the spec (rules 16-22, "Estrategia de
//! teste" 1): pure functions only, no renderer.

#![allow(clippy::assert_is_empty, clippy::float_cmp, clippy::unnecessary_wraps)]

use iced::{Point, Rectangle, Size};
use mychess_core::Square;
use mychess_gui::widgets::board::geometry::{
    BoardLayout, CoordsMode, LayoutOptions, Orientation, coordinate_labels, display_cell,
    square_from_cell,
};
use proptest::prelude::*;
use rstest::rstest;

const BOTH: [Orientation; 2] = [Orientation::White, Orientation::Black];

fn sq(name: &str) -> Square {
    name.parse().expect("square name")
}

fn opts(coords: CoordsMode) -> LayoutOptions {
    LayoutOptions { coords }
}

fn plain(size: f32) -> BoardLayout {
    BoardLayout::compute(Size::new(size, size), opts(CoordsMode::None))
        .expect("an 800x800 board fits")
}

fn coords_strategy() -> impl Strategy<Value = CoordsMode> {
    prop_oneof![
        Just(CoordsMode::None),
        Just(CoordsMode::Outside),
        Just(CoordsMode::OutsideBothSides),
        Just(CoordsMode::Inside),
    ]
}

fn right(r: Rectangle) -> f32 {
    r.x + r.width
}

fn bottom(r: Rectangle) -> f32 {
    r.y + r.height
}

/// Half-open containment, the convention of the spec (rule 19).
fn half_open(r: Rectangle, p: Point) -> bool {
    p.x >= r.x && p.x < right(r) && p.y >= r.y && p.y < bottom(r)
}

// ---------------------------------------------------------------------------
// Literal table from the spec ("Estrategia de teste" 1).
// ---------------------------------------------------------------------------

#[rstest]
#[case((50.0, 750.0), Orientation::White, Some("a1"))]
#[case((750.0, 50.0), Orientation::White, Some("h8"))]
#[case((799.99, 799.99), Orientation::White, Some("h1"))]
#[case((0.0, 0.0), Orientation::White, Some("a8"))]
#[case((800.0, 400.0), Orientation::White, None)]
#[case((400.0, 800.0), Orientation::White, None)]
#[case((800.0, 800.0), Orientation::White, None)]
#[case((-0.01, 10.0), Orientation::White, None)]
#[case((10.0, -0.01), Orientation::White, None)]
#[case((50.0, 750.0), Orientation::Black, Some("h8"))]
#[case((750.0, 50.0), Orientation::Black, Some("a1"))]
#[case((0.0, 0.0), Orientation::Black, Some("h1"))]
#[case((799.99, 799.99), Orientation::Black, Some("a8"))]
#[case((800.0, 0.0), Orientation::Black, None)]
fn m2_3_square_at_literal_table(
    #[case] p: (f32, f32),
    #[case] o: Orientation,
    #[case] expected: Option<&str>,
) {
    let layout = plain(800.0);
    assert_eq!(layout.s, 100.0);
    assert_eq!(
        layout.square_at(Point::new(p.0, p.1), o),
        expected.map(sq),
        "{p:?} {o:?}"
    );
}

#[rstest]
#[case(99.999, 0.0, Some("a8"))]
#[case(100.0, 0.0, Some("b8"))]
#[case(0.0, 99.999, Some("a8"))]
#[case(0.0, 100.0, Some("a7"))]
fn m2_3_square_at_exact_cell_boundaries_are_half_open(
    #[case] x: f32,
    #[case] y: f32,
    #[case] expected: Option<&str>,
) {
    let layout = plain(800.0);
    assert_eq!(
        layout.square_at(Point::new(x, y), Orientation::White),
        expected.map(sq)
    );
}

#[test]
fn m2_3_board_right_and_bottom_edges_belong_to_no_square() {
    let layout =
        BoardLayout::compute(Size::new(900.0, 700.0), opts(CoordsMode::Outside)).expect("fits");
    let b = layout.board;
    for o in BOTH {
        assert!(layout.square_at(Point::new(b.x, b.y), o).is_some());
        assert!(
            layout
                .square_at(Point::new(right(b) - 0.001, bottom(b) - 0.001), o)
                .is_some()
        );
        assert_eq!(layout.square_at(Point::new(right(b), b.y + 1.0), o), None);
        assert_eq!(layout.square_at(Point::new(b.x + 1.0, bottom(b)), o), None);
        assert_eq!(
            layout.square_at(Point::new(b.x - 0.001, b.y + 1.0), o),
            None
        );
    }
}

// ---------------------------------------------------------------------------
// Layout formula (rule 18) with hand-computed values.
// ---------------------------------------------------------------------------

#[test]
fn m2_3_layout_without_margins_fills_the_square() {
    for coords in [CoordsMode::None, CoordsMode::Inside] {
        let l = BoardLayout::compute(Size::new(800.0, 800.0), opts(coords)).expect("fits");
        assert_eq!(l.s, 100.0);
        assert_eq!(
            l.board,
            Rectangle::new(Point::ORIGIN, Size::new(800.0, 800.0))
        );
        assert_eq!(l.bounds, l.board);
    }
}

// M2-5 (D240): the strips left the canvas, so the formula has no `2 sm`.
#[test]
fn m2_3_layout_outside_matches_the_spec_formula_without_strips() {
    // cm = 0.30: s = floor(800/8.6) = 93; margin = round(27.9) = 28.
    let l = BoardLayout::compute(Size::new(800.0, 800.0), opts(CoordsMode::Outside)).expect("fits");
    assert_eq!(l.s, 93.0);
    assert_eq!(l.board.size(), Size::new(744.0, 744.0));
    assert_eq!(l.bounds.size(), Size::new(800.0, 800.0));
    assert_eq!(l.bounds.position(), Point::ORIGIN);
    assert_eq!(l.board.position(), Point::new(28.0, 28.0));
}

#[test]
fn m2_3_outside_and_both_sides_give_the_same_board() {
    for (w, h) in [
        (800.0, 800.0),
        (1000.0, 640.0),
        (333.0, 777.0),
        (51.0, 90.0),
    ] {
        let a = BoardLayout::compute(Size::new(w, h), opts(CoordsMode::Outside));
        let b = BoardLayout::compute(Size::new(w, h), opts(CoordsMode::OutsideBothSides));
        assert_eq!(a, b, "{w}x{h}");
    }
}

#[test]
fn m2_3_non_square_canvas_is_limited_by_the_short_side_and_centered() {
    let l = BoardLayout::compute(Size::new(1000.0, 400.0), opts(CoordsMode::None)).expect("fits");
    assert_eq!(l.s, 50.0);
    assert_eq!(l.board.position(), Point::new(300.0, 0.0));
}

#[rstest]
#[case(0.0, 0.0)]
#[case(0.0, 500.0)]
#[case(500.0, 0.0)]
#[case(-1.0, 500.0)]
#[case(500.0, -1.0)]
#[case(-500.0, -500.0)]
#[case(f32::NAN, 500.0)]
#[case(500.0, f32::NAN)]
#[case(f32::NAN, f32::NAN)]
#[case(f32::INFINITY, 500.0)]
#[case(500.0, f32::INFINITY)]
#[case(f32::NEG_INFINITY, 500.0)]
#[case(7.99, 7.99)]
#[case(1.0, 1.0)]
#[case(0.5, 100.0)]
#[case(f32::MIN_POSITIVE, f32::MIN_POSITIVE)]
fn m2_3_degenerate_sizes_give_none_without_panic(#[case] w: f32, #[case] h: f32) {
    for coords in [
        CoordsMode::None,
        CoordsMode::Outside,
        CoordsMode::OutsideBothSides,
        CoordsMode::Inside,
    ] {
        assert_eq!(
            BoardLayout::compute(Size::new(w, h), opts(coords)),
            None,
            "{w}x{h} {coords:?}"
        );
    }
}

#[test]
fn m2_3_smallest_possible_board_is_eight_pixels_without_margins() {
    let l = BoardLayout::compute(Size::new(8.0, 8.0), opts(CoordsMode::None))
        .expect("8x8 fits one pixel squares");
    assert_eq!(l.s, 1.0);
    assert_eq!(l.board.size(), Size::new(8.0, 8.0));
}

#[test]
fn m2_3_minimum_window_canvas_has_a_usable_board() {
    // The smallest pane of the board is 240 px (rule 4).
    let l = BoardLayout::compute(Size::new(240.0, 240.0), opts(CoordsMode::Outside)).expect("fits");
    assert!(l.s >= 20.0, "s = {}", l.s);
}

#[test]
fn m2_3_extreme_sizes_do_not_panic_and_stay_inside() {
    for v in [1.0e4_f32, 1.0e6, 1.0e9, f32::MAX] {
        let size = Size::new(v, v);
        for coords in [CoordsMode::None, CoordsMode::Outside] {
            let l = BoardLayout::compute(size, opts(coords)).expect("huge fits");
            assert!(l.s >= 1.0 && l.s.fract() == 0.0);
            assert!(l.bounds.x >= 0.0 && l.bounds.y >= 0.0);
            assert!(right(l.bounds) <= v && bottom(l.bounds) <= v);
            // Hit testing still works while f32 keeps sub-pixel precision.
            if v > 1.0e6 {
                continue;
            }
            let c = l.square_center(sq("e4"), Orientation::White);
            assert_eq!(l.square_at(c, Orientation::White), Some(sq("e4")));
        }
    }
}

// ---------------------------------------------------------------------------
// Cells and labels (rules 17 and 21).
// ---------------------------------------------------------------------------

#[rstest]
#[case("a1", Orientation::White, (0, 7))]
#[case("h8", Orientation::White, (7, 0))]
#[case("e4", Orientation::White, (4, 4))]
#[case("a1", Orientation::Black, (7, 0))]
#[case("h8", Orientation::Black, (0, 7))]
#[case("e4", Orientation::Black, (3, 3))]
fn m2_3_display_cell_literals(#[case] name: &str, #[case] o: Orientation, #[case] cell: (u8, u8)) {
    assert_eq!(display_cell(sq(name), o), cell);
    assert_eq!(square_from_cell(cell.0, cell.1, o), Some(sq(name)));
}

#[test]
fn m2_3_square_from_cell_is_none_outside_the_board() {
    for o in BOTH {
        assert_eq!(square_from_cell(8, 0, o), None);
        assert_eq!(square_from_cell(0, 8, o), None);
        assert_eq!(square_from_cell(255, 255, o), None);
    }
}

#[test]
fn m2_3_display_cell_round_trips_for_every_square_and_orientation() {
    for o in BOTH {
        let mut seen = std::collections::HashSet::new();
        for &s in &Square::ALL {
            let (c, r) = display_cell(s, o);
            assert!(c < 8 && r < 8);
            assert_eq!(square_from_cell(c, r, o), Some(s));
            assert!(seen.insert((c, r)), "two squares share a cell");
        }
        assert_eq!(seen.len(), 64);
    }
}

#[test]
fn m2_3_coordinate_labels_follow_the_orientation() {
    let (files, ranks) = coordinate_labels(Orientation::White);
    assert_eq!(files.iter().collect::<String>(), "abcdefgh");
    assert_eq!(ranks.iter().collect::<String>(), "87654321");
    let (files, ranks) = coordinate_labels(Orientation::Black);
    assert_eq!(files.iter().collect::<String>(), "hgfedcba");
    assert_eq!(ranks.iter().collect::<String>(), "12345678");
}

#[test]
fn m2_3_coordinate_labels_agree_with_the_cells() {
    for o in BOTH {
        let (files, ranks) = coordinate_labels(o);
        for &s in &Square::ALL {
            let (col, row) = display_cell(s, o);
            assert_eq!(files[usize::from(col)], s.file().char());
            assert_eq!(ranks[usize::from(row)], s.rank().char());
        }
    }
}

#[test]
fn m2_3_coords_mode_cycles_through_four_states() {
    let mut m = CoordsMode::None;
    let mut order = Vec::new();
    for _ in 0..4 {
        m = m.next();
        order.push(m);
    }
    assert_eq!(
        order,
        [
            CoordsMode::Outside,
            CoordsMode::OutsideBothSides,
            CoordsMode::Inside,
            CoordsMode::None
        ]
    );
}

#[test]
fn m2_3_orientation_flips_and_defaults_to_white() {
    assert_eq!(Orientation::default(), Orientation::White);
    assert_eq!(Orientation::White.flipped(), Orientation::Black);
    assert_eq!(Orientation::Black.flipped().flipped(), Orientation::Black);
}

// ---------------------------------------------------------------------------
// Properties (a)-(g) of rule 20.
// ---------------------------------------------------------------------------

fn any_layout() -> impl Strategy<Value = (Size, LayoutOptions, BoardLayout)> {
    (1.0f32..4000.0, 1.0f32..4000.0, coords_strategy()).prop_filter_map(
        "layout must fit",
        |(w, h, coords)| {
            let size = Size::new(w, h);
            let o = opts(coords);
            BoardLayout::compute(size, o).map(|l| (size, o, l))
        },
    )
}

proptest! {
    #[test]
    fn m2_3_prop_a_center_hits_its_own_square((_, _, l) in any_layout()) {
        for o in BOTH {
            for &s in &Square::ALL {
                let c = l.square_center(s, o);
                prop_assert_eq!(l.square_at(c, o), Some(s));
                prop_assert!(half_open(l.square_rect(s, o), c));
            }
        }
    }

    #[test]
    fn m2_3_prop_c_squares_are_disjoint_and_cover_the_board(
        (_, _, l) in any_layout(),
        fx in 0.0f32..1.0,
        fy in 0.0f32..1.0,
    ) {
        for o in BOTH {
            let mut area = 0.0f32;
            for &s in &Square::ALL {
                let r = l.square_rect(s, o);
                prop_assert_eq!(r.width, l.s);
                prop_assert_eq!(r.height, l.s);
                prop_assert!(r.x >= l.board.x && right(r) <= right(l.board));
                prop_assert!(r.y >= l.board.y && bottom(r) <= bottom(l.board));
                area += r.width * r.height;
            }
            prop_assert_eq!(area, l.board.width * l.board.height);
            // A random point of the board lies in exactly one half-open square.
            let p = Point::new(l.board.x + fx * l.board.width, l.board.y + fy * l.board.height);
            let hits: Vec<Square> = Square::ALL
                .iter()
                .copied()
                .filter(|&s| half_open(l.square_rect(s, o), p))
                .collect();
            prop_assert_eq!(hits.len(), 1, "point {:?}", p);
            prop_assert_eq!(l.square_at(p, o), Some(hits[0]));
        }
    }

    #[test]
    fn m2_3_prop_d_black_is_white_rotated_180((_, _, l) in any_layout()) {
        for &s in &Square::ALL {
            let rotated = Square::new(63 - u32::from(s));
            prop_assert_eq!(
                l.square_rect(s, Orientation::Black),
                l.square_rect(rotated, Orientation::White)
            );
        }
    }

    #[test]
    fn m2_3_prop_e_bounds_fit_in_size_and_hold_everything(
        w in 1.0f32..4000.0, h in 1.0f32..4000.0,
        coords in coords_strategy(),
    ) {
        if let Some(l) = BoardLayout::compute(Size::new(w, h), opts(coords)) {
            prop_assert!(l.s >= 1.0);
            prop_assert_eq!(l.s.fract(), 0.0);
            prop_assert!(l.bounds.x >= 0.0 && l.bounds.y >= 0.0);
            prop_assert!(right(l.bounds) <= w, "{} > {}", right(l.bounds), w);
            prop_assert!(bottom(l.bounds) <= h, "{} > {}", bottom(l.bounds), h);
            prop_assert!(l.board.x >= l.bounds.x && right(l.board) <= right(l.bounds));
            prop_assert!(l.board.y >= l.bounds.y && bottom(l.board) <= bottom(l.bounds));
            prop_assert_eq!(l.board.width, 8.0 * l.s);
            // All margins and positions are whole pixels.
            for v in [l.board.x, l.board.y, l.bounds.x, l.bounds.y, l.bounds.width, l.bounds.height] {
                prop_assert_eq!(v.fract(), 0.0);
            }
            // Centered to within the rounding pixel.
            let left = l.bounds.x;
            let rightgap = w - right(l.bounds);
            prop_assert!((left - rightgap).abs() <= 1.0, "{left} vs {rightgap}");
            let topgap = l.bounds.y;
            let bottomgap = h - bottom(l.bounds);
            prop_assert!((topgap - bottomgap).abs() <= 1.0, "{topgap} vs {bottomgap}");
        }
    }

    #[test]
    fn m2_3_prop_f_growing_the_canvas_never_shrinks_the_board(
        w in 1.0f32..3000.0, h in 1.0f32..3000.0,
        dw in 0.0f32..500.0, dh in 0.0f32..500.0,
        coords in coords_strategy(),
    ) {
        let o = opts(coords);
        let small = BoardLayout::compute(Size::new(w, h), o).map_or(0.0, |l| l.s);
        let big = BoardLayout::compute(Size::new(w + dw, h + dh), o).map_or(0.0, |l| l.s);
        prop_assert!(big >= small, "{small} -> {big}");
    }

    #[test]
    fn m2_3_prop_g_points_outside_the_board_are_none(
        (_, _, l) in any_layout(),
        x in any::<f32>(), y in any::<f32>(),
    ) {
        for o in BOTH {
            let got = l.square_at(Point::new(x, y), o);
            let inside = x.is_finite() && y.is_finite()
                && x >= l.board.x && x < right(l.board)
                && y >= l.board.y && y < bottom(l.board);
            prop_assert_eq!(got.is_some(), inside, "({}, {})", x, y);
        }
    }

    #[test]
    fn m2_3_prop_g_non_finite_points_are_none((_, _, l) in any_layout()) {
        for o in BOTH {
            for p in [
                Point::new(f32::NAN, 10.0),
                Point::new(10.0, f32::NAN),
                Point::new(f32::INFINITY, f32::INFINITY),
                Point::new(f32::NEG_INFINITY, 0.0),
                Point::new(f32::NAN, f32::NAN),
            ] {
                prop_assert_eq!(l.square_at(p, o), None);
            }
        }
    }

    #[test]
    fn m2_3_prop_compute_never_panics_for_any_float(
        w in any::<f32>(), h in any::<f32>(),
        coords in coords_strategy(),
    ) {
        let r = BoardLayout::compute(Size::new(w, h), opts(coords));
        if let Some(l) = r {
            prop_assert!(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0);
            prop_assert!(right(l.bounds) <= w && bottom(l.bounds) <= h);
        }
    }

    #[test]
    fn m2_3_prop_cells_round_trip(col in 0u8..8, row in 0u8..8) {
        for o in BOTH {
            let s = square_from_cell(col, row, o).expect("inside");
            prop_assert_eq!(display_cell(s, o), (col, row));
        }
    }
}
