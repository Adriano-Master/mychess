//! Pure board geometry: orientation, layout and pixel/square conversions.
//!
//! No renderer here: only the geometric types of iced and the squares of
//! `mychess-core`. Spec: `docs/specs/m2-3-casca-e-tabuleiro.md`, section B.
//! Scid: `getSquare` and `resize2` in `tcl/board.tcl`.
// The casts below convert between small bounded integers (0..=63, cell
// indices, pixel sizes capped at `MAX_SQUARE_SIZE`) and `f32`.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use iced::{Point, Rectangle, Size};
use mychess_core::{File, Rank, Square};

/// Margin reserved for coordinates, as a fraction of the square side.
const COORD_MARGIN: f32 = 0.30;
/// Largest square side the layout produces (keeps `f32` arithmetic exact).
const MAX_SQUARE_SIZE: f32 = 2048.0;

/// Which side is at the bottom of the board.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// White at the bottom (the default).
    #[default]
    White,
    /// Black at the bottom.
    Black,
}

impl Orientation {
    /// The opposite orientation.
    #[must_use]
    pub fn flipped(self) -> Orientation {
        match self {
            Orientation::White => Orientation::Black,
            Orientation::Black => Orientation::White,
        }
    }
}

/// Where the coordinate labels are drawn (Scid `boardCoords`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum CoordsMode {
    /// No labels.
    None,
    /// Files below and ranks on the left.
    #[default]
    Outside,
    /// Labels on all four sides.
    OutsideBothSides,
    /// Small labels inside the corner squares.
    Inside,
}

impl CoordsMode {
    /// The next mode in the cycle `None > Outside > Both > Inside > None`.
    #[must_use]
    pub fn next(self) -> CoordsMode {
        match self {
            CoordsMode::None => CoordsMode::Outside,
            CoordsMode::Outside => CoordsMode::OutsideBothSides,
            CoordsMode::OutsideBothSides => CoordsMode::Inside,
            CoordsMode::Inside => CoordsMode::None,
        }
    }

    fn reserves_margin(self) -> bool {
        matches!(self, CoordsMode::Outside | CoordsMode::OutsideBothSides)
    }
}

/// Display cell of a square: `col` and `row` in `0..=7`, row 0 at the top.
pub fn display_cell(sq: Square, o: Orientation) -> (u8, u8) {
    let file = u8::from(sq.file());
    let rank = u8::from(sq.rank());
    match o {
        Orientation::White => (file, 7 - rank),
        Orientation::Black => (7 - file, rank),
    }
}

/// Inverse of [`display_cell`]; `None` outside `0..=7`.
pub fn square_from_cell(col: u8, row: u8, o: Orientation) -> Option<Square> {
    if col > 7 || row > 7 {
        return None;
    }
    let (file, rank) = match o {
        Orientation::White => (col, 7 - row),
        Orientation::Black => (7 - col, row),
    };
    Some(Square::from_coords(
        File::new(u32::from(file)),
        Rank::new(u32::from(rank)),
    ))
}

/// The file letters and rank digits in display order (left to right, top to
/// bottom): `a-h` and `8-1` for White at the bottom, reversed for Black.
pub fn coordinate_labels(o: Orientation) -> ([char; 8], [char; 8]) {
    let files = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
    let ranks = ['8', '7', '6', '5', '4', '3', '2', '1'];
    match o {
        Orientation::White => (files, ranks),
        Orientation::Black => {
            let mut f = files;
            let mut r = ranks;
            f.reverse();
            r.reverse();
            (f, r)
        }
    }
}

/// What the layout must make room for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutOptions {
    /// The coordinate mode (decides the outside margins).
    pub coords: CoordsMode,
}

/// The computed placement of the board inside a canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoardLayout {
    /// Side of one square in logical pixels (an integer value).
    pub s: f32,
    /// The 8x8 board area.
    pub board: Rectangle,
    /// Everything (margins included).
    pub bounds: Rectangle,
}

/// The largest whole square side `s` whose canvas side `8s + 2 round(cm s)`
/// fits in `limit`; `None` when not even one pixel fits.
///
/// The canvas side grows by at least 8 per step of `s`, so the answer is
/// monotone in `limit` and a canvas of exactly that side gives back the same
/// `s` (the property the player strips rely on).
fn largest_square(limit: f32, cm: f32) -> Option<f32> {
    if !(limit.is_finite() && limit > 0.0) {
        return None;
    }
    // The rounding of the margin can make the true answer one more than the
    // floor of the real-valued fit, so start one above.
    let mut s = ((limit / 2.0f32.mul_add(cm, 8.0)).floor() + 1.0).min(MAX_SQUARE_SIZE);
    while s >= 1.0 {
        let side = 8.0f32.mul_add(s, 2.0 * (cm * s).round());
        if side <= limit {
            return Some(s);
        }
        s -= 1.0;
    }
    None
}

fn margin_fraction(coords: CoordsMode) -> f32 {
    if coords.reserves_margin() {
        COORD_MARGIN
    } else {
        0.0
    }
}

impl BoardLayout {
    /// Fits the board and its margins in `size`, centered.
    ///
    /// Returns `None` when `size` is empty, negative, not finite or too small
    /// for a one pixel square.
    pub fn compute(size: Size, opts: LayoutOptions) -> Option<BoardLayout> {
        let (w, h) = (size.width, size.height);
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            return None;
        }
        let cm = margin_fraction(opts.coords);
        let s = largest_square(w.min(h), cm)?;
        let margin = (cm * s).round();
        let total = 8.0f32.mul_add(s, 2.0 * margin);
        let x0 = ((w - total) / 2.0).round();
        let y0 = ((h - total) / 2.0).round();
        Some(BoardLayout {
            s,
            board: Rectangle::new(
                Point::new(x0 + margin, y0 + margin),
                Size::new(8.0 * s, 8.0 * s),
            ),
            bounds: Rectangle::new(Point::new(x0, y0), Size::new(total, total)),
        })
    }

    /// The rectangle of a square.
    pub fn square_rect(&self, sq: Square, o: Orientation) -> Rectangle {
        let (col, row) = display_cell(sq, o);
        self.cell_rect(col, row)
    }

    /// The rectangle of a display cell.
    pub fn cell_rect(&self, col: u8, row: u8) -> Rectangle {
        Rectangle::new(
            Point::new(
                self.board.x + f32::from(col) * self.s,
                self.board.y + f32::from(row) * self.s,
            ),
            Size::new(self.s, self.s),
        )
    }

    /// The center of a square.
    pub fn square_center(&self, sq: Square, o: Orientation) -> Point {
        self.square_rect(sq, o).center()
    }

    /// The square under `p`. Half-open: the right and bottom edges of the
    /// board belong to no square. Not finite points give `None`.
    pub fn square_at(&self, p: Point, o: Orientation) -> Option<Square> {
        if !(p.x.is_finite() && p.y.is_finite()) {
            return None;
        }
        let dx = p.x - self.board.x;
        let dy = p.y - self.board.y;
        let side = 8.0 * self.s;
        if dx < 0.0 || dy < 0.0 || dx >= side || dy >= side {
            return None;
        }
        let col = (dx / self.s).floor().min(7.0) as u8;
        let row = (dy / self.s).floor().min(7.0) as u8;
        square_from_cell(col, row, o)
    }
}

// ---- the column of the board: strip, canvas, strip (spec M2-5, D237) ----

/// Height of a full player strip.
pub const STRIP_FULL_HEIGHT: f32 = 44.0;
/// Height of a compact (one line) player strip.
pub const STRIP_COMPACT_HEIGHT: f32 = 28.0;
/// Space between a strip and the canvas of the board.
pub const STRIP_GAP: f32 = 4.0;
/// Smallest board side (`8s`) that keeps the full strips.
pub const FULL_MIN_BOARD: f32 = 360.0;
/// Smallest board side (`8s`) that keeps the compact strips.
pub const COMPACT_MIN_BOARD: f32 = 240.0;

/// How tall the player strips are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StripMode {
    /// Two lines with avatar (height 44).
    Full,
    /// One line without avatar (height 28).
    Compact,
    /// No strips: the board takes the height.
    Hidden,
}

impl StripMode {
    /// The height of one strip of this mode.
    pub fn height(self) -> f32 {
        match self {
            StripMode::Full => STRIP_FULL_HEIGHT,
            StripMode::Compact => STRIP_COMPACT_HEIGHT,
            StripMode::Hidden => 0.0,
        }
    }

    fn gap(self) -> f32 {
        if self == StripMode::Hidden {
            0.0
        } else {
            STRIP_GAP
        }
    }
}

/// What the user asks for; the space available decides the final
/// [`StripMode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StripPolicy {
    /// No strips at all.
    Off,
    /// Full strips when the board stays at least 360 px, else compact when
    /// it stays at least 240 px, else none.
    Auto,
    /// Compact strips when the board stays at least 240 px, else none.
    PreferCompact,
}

/// The measures of the board column for one available size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnMetrics {
    /// The strip mode that fits.
    pub mode: StripMode,
    /// Side of a square.
    pub s: f32,
    /// The exact size to give the canvas (`8s` plus the margins).
    pub canvas: Size,
    /// Width of each strip: exactly the width of the board (`8s`).
    pub strip_width: f32,
    /// Distance from the left of the canvas to the first square (and to the
    /// left of the strips).
    pub strip_inset: f32,
    /// Height of each strip (`0` when hidden).
    pub strip_height: f32,
    /// Space between a strip and the canvas (`0` when hidden).
    pub gap: f32,
    /// Size of the whole column: strip, canvas, strip.
    pub total: Size,
    /// Where the column starts inside the available area (centered).
    pub origin: Point,
}

fn fit_column(avail: Size, coords: CoordsMode, mode: StripMode) -> Option<ColumnMetrics> {
    let cm = margin_fraction(coords);
    let (height, gap) = (mode.height(), mode.gap());
    let usable_h = avail.height - 2.0 * height - 2.0 * gap;
    let s = largest_square(avail.width.min(usable_h), cm)?;
    let margin = (cm * s).round();
    let side = 8.0f32.mul_add(s, 2.0 * margin);
    let total = Size::new(side, side + 2.0 * height + 2.0 * gap);
    Some(ColumnMetrics {
        mode,
        s,
        canvas: Size::new(side, side),
        strip_width: 8.0 * s,
        strip_inset: margin,
        strip_height: height,
        gap,
        total,
        origin: Point::new(
            ((avail.width - total.width) / 2.0).round(),
            ((avail.height - total.height) / 2.0).round(),
        ),
    })
}

/// Measures the column of the board inside `avail`.
///
/// The strip mode follows from the space, without circularity: full strips
/// if the board still has `8s >= 360`, else compact ones if `8s >= 240`,
/// else none (spec rule 8). `None` for a size that is not finite, not
/// positive or too small for a one pixel square.
pub fn column_metrics(
    avail: Size,
    coords: CoordsMode,
    policy: StripPolicy,
) -> Option<ColumnMetrics> {
    if !(avail.width.is_finite()
        && avail.height.is_finite()
        && avail.width > 0.0
        && avail.height > 0.0)
    {
        return None;
    }
    let candidates: &[(StripMode, f32)] = match policy {
        StripPolicy::Off => &[(StripMode::Hidden, 0.0)],
        StripPolicy::Auto => &[
            (StripMode::Full, FULL_MIN_BOARD),
            (StripMode::Compact, COMPACT_MIN_BOARD),
            (StripMode::Hidden, 0.0),
        ],
        StripPolicy::PreferCompact => &[
            (StripMode::Compact, COMPACT_MIN_BOARD),
            (StripMode::Hidden, 0.0),
        ],
    };
    candidates.iter().find_map(|&(mode, min_board)| {
        fit_column(avail, coords, mode).filter(|m| 8.0 * m.s >= min_board)
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn plain(size: f32) -> BoardLayout {
        BoardLayout::compute(
            Size::new(size, size),
            LayoutOptions {
                coords: CoordsMode::None,
            },
        )
        .expect("layout fits")
    }

    #[rstest]
    #[case((50.0, 750.0), false, "a1")]
    #[case((750.0, 50.0), false, "h8")]
    #[case((799.99, 799.99), false, "h1")]
    #[case((0.0, 0.0), false, "a8")]
    #[case((50.0, 750.0), true, "h8")]
    #[case((750.0, 50.0), true, "a1")]
    fn square_at_matches_the_spec_table(
        #[case] p: (f32, f32),
        #[case] flipped: bool,
        #[case] expected: &str,
    ) {
        let layout = plain(800.0);
        let o = if flipped {
            Orientation::Black
        } else {
            Orientation::White
        };
        let got = layout.square_at(Point::new(p.0, p.1), o);
        assert_eq!(got, expected.parse::<Square>().ok());
    }

    #[test]
    fn square_at_excludes_right_and_bottom_edges() {
        let layout = plain(800.0);
        assert_eq!(
            layout.square_at(Point::new(800.0, 400.0), Orientation::White),
            None
        );
        assert_eq!(
            layout.square_at(Point::new(400.0, 800.0), Orientation::White),
            None
        );
        assert_eq!(
            layout.square_at(Point::new(-0.1, 10.0), Orientation::White),
            None
        );
    }

    #[test]
    fn square_at_rejects_non_finite_points() {
        let layout = plain(800.0);
        for p in [
            Point::new(f32::NAN, 10.0),
            Point::new(10.0, f32::INFINITY),
            Point::new(f32::NEG_INFINITY, f32::NAN),
        ] {
            assert_eq!(layout.square_at(p, Orientation::White), None);
        }
    }

    #[test]
    fn center_of_every_square_maps_back_in_both_orientations() {
        let layout = plain(437.0);
        for o in [Orientation::White, Orientation::Black] {
            for sq in Square::ALL {
                let c = layout.square_center(sq, o);
                assert_eq!(layout.square_at(c, o), Some(sq));
                assert!(layout.square_rect(sq, o).contains(c));
            }
        }
    }

    #[test]
    fn black_orientation_is_white_rotated_180_degrees() {
        let layout = plain(640.0);
        for sq in Square::ALL {
            let rotated = Square::ALL[63 - usize::from(sq)];
            assert_eq!(
                layout.square_rect(sq, Orientation::Black),
                layout.square_rect(rotated, Orientation::White)
            );
        }
    }

    #[test]
    fn display_cell_and_square_from_cell_are_inverse() {
        for o in [Orientation::White, Orientation::Black] {
            for sq in Square::ALL {
                let (c, r) = display_cell(sq, o);
                assert_eq!(square_from_cell(c, r, o), Some(sq));
            }
        }
        assert_eq!(square_from_cell(8, 0, Orientation::White), None);
        assert_eq!(square_from_cell(0, 8, Orientation::Black), None);
    }

    #[test]
    fn degenerate_sizes_give_no_layout() {
        let opts = LayoutOptions {
            coords: CoordsMode::Outside,
        };
        for size in [
            Size::new(0.0, 100.0),
            Size::new(-5.0, 100.0),
            Size::new(f32::NAN, 100.0),
            Size::new(100.0, f32::INFINITY),
            Size::new(5.0, 5.0),
        ] {
            assert_eq!(BoardLayout::compute(size, opts), None, "{size:?}");
        }
    }

    #[test]
    fn layout_is_integral_centered_and_inside_the_canvas() {
        let opts = LayoutOptions {
            coords: CoordsMode::Outside,
        };
        let size = Size::new(900.0, 700.0);
        let layout = BoardLayout::compute(size, opts).expect("layout fits");
        assert_eq!(layout.s.fract(), 0.0);
        assert!(layout.bounds.x >= 0.0 && layout.bounds.y >= 0.0);
        assert!(layout.bounds.x + layout.bounds.width <= size.width);
        assert!(layout.bounds.y + layout.bounds.height <= size.height);
    }

    #[test]
    fn outside_and_both_sides_share_the_same_board() {
        let size = Size::new(800.0, 700.0);
        let layout =
            |coords| BoardLayout::compute(size, LayoutOptions { coords }).expect("layout fits");
        assert_eq!(
            layout(CoordsMode::Outside).board,
            layout(CoordsMode::OutsideBothSides).board
        );
    }

    #[test]
    fn coordinate_labels_follow_the_orientation() {
        let (files, ranks) = coordinate_labels(Orientation::White);
        assert_eq!(
            (files[0], files[7], ranks[0], ranks[7]),
            ('a', 'h', '8', '1')
        );
        let (files, ranks) = coordinate_labels(Orientation::Black);
        assert_eq!(
            (files[0], files[7], ranks[0], ranks[7]),
            ('h', 'a', '1', '8')
        );
    }

    #[test]
    fn coords_mode_cycles_through_all_four_modes() {
        let mut mode = CoordsMode::None;
        let mut seen = Vec::new();
        for _ in 0..4 {
            mode = mode.next();
            seen.push(mode);
        }
        assert_eq!(
            seen,
            [
                CoordsMode::Outside,
                CoordsMode::OutsideBothSides,
                CoordsMode::Inside,
                CoordsMode::None
            ]
        );
    }
    fn metrics(w: f32, h: f32, coords: CoordsMode, policy: StripPolicy) -> ColumnMetrics {
        column_metrics(Size::new(w, h), coords, policy).expect("fits")
    }

    #[rstest]
    #[case(1000.0, 800.0, StripPolicy::Auto, StripMode::Full, 88.0)]
    #[case(700.0, 420.0, StripPolicy::Auto, StripMode::Compact, 44.0)]
    #[case(300.0, 200.0, StripPolicy::Auto, StripMode::Hidden, 25.0)]
    #[case(1000.0, 800.0, StripPolicy::Off, StripMode::Hidden, 100.0)]
    #[case(1000.0, 800.0, StripPolicy::PreferCompact, StripMode::Compact, 92.0)]
    fn column_metrics_matches_the_spec_table(
        #[case] w: f32,
        #[case] h: f32,
        #[case] policy: StripPolicy,
        #[case] mode: StripMode,
        #[case] s: f32,
    ) {
        let m = metrics(w, h, CoordsMode::Inside, policy);
        assert_eq!((m.mode, m.s), (mode, s));
    }

    #[test]
    fn strips_are_as_wide_as_the_board_and_aligned_with_it() {
        for coords in [CoordsMode::Outside, CoordsMode::Inside, CoordsMode::None] {
            let m = metrics(900.0, 700.0, coords, StripPolicy::Auto);
            let layout = BoardLayout::compute(m.canvas, LayoutOptions { coords }).expect("canvas");
            assert_eq!(m.strip_width, layout.board.width);
            assert_eq!(m.strip_inset, layout.board.x);
            assert_eq!(layout.s, m.s);
            assert_eq!(layout.bounds.size(), m.canvas);
        }
    }

    #[test]
    fn the_column_is_centered_in_the_available_area() {
        let m = metrics(1000.0, 800.0, CoordsMode::Inside, StripPolicy::Auto);
        assert_eq!(m.total, Size::new(704.0, 704.0 + 88.0 + 8.0));
        assert_eq!(m.origin, Point::new(148.0, 0.0));
    }

    #[test]
    fn degenerate_available_sizes_give_none() {
        for (w, h) in [
            (0.0, 100.0),
            (100.0, -1.0),
            (f32::NAN, 100.0),
            (100.0, f32::INFINITY),
            (7.0, 700.0),
        ] {
            for policy in [
                StripPolicy::Auto,
                StripPolicy::Off,
                StripPolicy::PreferCompact,
            ] {
                assert_eq!(
                    column_metrics(Size::new(w, h), CoordsMode::Inside, policy),
                    None,
                    "{w}x{h}"
                );
            }
        }
    }

    fn coords_strategy() -> impl proptest::strategy::Strategy<Value = CoordsMode> {
        proptest::prop_oneof![
            proptest::strategy::Just(CoordsMode::None),
            proptest::strategy::Just(CoordsMode::Outside),
            proptest::strategy::Just(CoordsMode::OutsideBothSides),
            proptest::strategy::Just(CoordsMode::Inside),
        ]
    }

    fn policy_strategy() -> impl proptest::strategy::Strategy<Value = StripPolicy> {
        proptest::prop_oneof![
            proptest::strategy::Just(StripPolicy::Off),
            proptest::strategy::Just(StripPolicy::Auto),
            proptest::strategy::Just(StripPolicy::PreferCompact),
        ]
    }

    fn rank(mode: StripMode) -> u8 {
        match mode {
            StripMode::Hidden => 0,
            StripMode::Compact => 1,
            StripMode::Full => 2,
        }
    }

    proptest::proptest! {
        // Rule 11 (a) and (b).
        #[test]
        #[allow(clippy::float_cmp)]
        fn strip_width_equals_the_board_width_and_everything_fits(
            w in 1.0f32..4000.0, h in 1.0f32..4000.0,
            coords in coords_strategy(), policy in policy_strategy(),
        ) {
            if let Some(m) = column_metrics(Size::new(w, h), coords, policy) {
                let layout = BoardLayout::compute(m.canvas, LayoutOptions { coords })
                    .expect("the canvas of a metrics fits");
                proptest::prop_assert_eq!(m.strip_width, layout.board.width);
                proptest::prop_assert_eq!(m.strip_width, 8.0 * m.s);
                proptest::prop_assert_eq!(layout.s, m.s);
                proptest::prop_assert!(m.total.width <= w && m.total.height <= h);
                proptest::prop_assert!(m.origin.x >= 0.0 && m.origin.y >= 0.0);
            }
        }

        // Rule 11 (c): growing the area never shrinks the board nor worsens
        // the strip mode.
        #[test]
        fn growing_never_shrinks_the_board_nor_worsens_the_mode(
            w in 1.0f32..3000.0, h in 1.0f32..3000.0,
            dw in 0.0f32..500.0, dh in 0.0f32..500.0,
            coords in coords_strategy(), policy in policy_strategy(),
        ) {
            let small = column_metrics(Size::new(w, h), coords, policy);
            let big = column_metrics(Size::new(w + dw, h + dh), coords, policy);
            if let Some(small) = small {
                let big = big.expect("a bigger area fits too");
                proptest::prop_assert!(rank(big.mode) >= rank(small.mode));
                if big.mode == small.mode {
                    proptest::prop_assert!(big.s >= small.s);
                }
            }
        }

        // Rule 11 (d).
        #[test]
        fn outside_and_both_sides_agree(
            w in 1.0f32..4000.0, h in 1.0f32..4000.0, policy in policy_strategy(),
        ) {
            let a = column_metrics(Size::new(w, h), CoordsMode::Outside, policy);
            let b = column_metrics(Size::new(w, h), CoordsMode::OutsideBothSides, policy);
            proptest::prop_assert_eq!(a, b);
        }

        // Rule 11 (e): hidden strips give the board of a plain layout.
        #[test]
        fn hidden_strips_give_the_plain_board(
            w in 1.0f32..4000.0, h in 1.0f32..4000.0, coords in coords_strategy(),
        ) {
            let m = column_metrics(Size::new(w, h), coords, StripPolicy::Off);
            let plain = BoardLayout::compute(Size::new(w, h), LayoutOptions { coords });
            proptest::prop_assert_eq!(m.map(|m| m.s), plain.map(|l| l.s));
        }

        // Rule 11 (f): any float, no panic.
        #[test]
        fn any_float_never_panics(
            w in proptest::num::f32::ANY, h in proptest::num::f32::ANY,
            coords in coords_strategy(), policy in policy_strategy(),
        ) {
            let _ = column_metrics(Size::new(w, h), coords, policy);
        }
    }
}
