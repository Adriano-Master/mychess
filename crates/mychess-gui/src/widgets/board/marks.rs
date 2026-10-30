//! Pure geometry of board marks and the color choice of mark gestures.
//!
//! Scid: `GetBox` and `GetArrowCoords` in `tcl/board.tcl:2327-2410`.

use iced::{Point, Rectangle, Size};
use mychess_core::{MarkKind, Square};

use super::geometry::{BoardLayout, Orientation};
use super::interaction::Mods;

/// Default shrink of an arrow at both ends (Scid `GetArrowCoords`).
pub const DEFAULT_SHRINK: f32 = 0.6;
/// Longest shrink allowed for arrows between adjacent squares.
const MAX_SHRINK_ADJACENT: f32 = 0.667;
/// Arrow shaft width, as a fraction of the square side.
const SHAFT_WIDTH: f32 = 0.14;
/// Arrow head length, as a fraction of the square side.
const HEAD_LENGTH: f32 = 0.38;
/// Arrow head width, as a fraction of the square side.
const HEAD_WIDTH: f32 = 0.40;

/// Box proportions of the square marks (`GetBox` portions in the Scid).
pub const CIRCLE_PORTION: f32 = 0.85;
/// Portion of the square used by a text mark.
pub const TEXT_PORTION: f32 = 0.80;

/// The mark the user paints with `Shift`+click and `Ctrl`+click (Scid
/// `commenteditor::State`, `tcl/windows/comment.tcl:25-30`). Session only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkBrush {
    /// The kind painted by `Shift`+click.
    pub kind: MarkKind,
    /// The color name (Tk name or `#rrggbb`).
    pub color: String,
}

impl Default for MarkBrush {
    fn default() -> MarkBrush {
        MarkBrush {
            kind: MarkKind::Circle,
            color: "skyBlue".to_owned(),
        }
    }
}

/// A computed arrow: a shaft from `tail` to `base` and a triangular head.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrow {
    /// Start of the shaft.
    pub tail: Point,
    /// The head's vertex.
    pub tip: Point,
    /// Where the shaft ends (the base of the head).
    pub base: Point,
    /// The head triangle: the tip and the two base corners.
    pub head: [Point; 3],
    /// Width of the shaft.
    pub shaft_width: f32,
}

/// The concentric box of side `portion * rect.width` (Scid `GetBox`).
pub fn inner_box(rect: Rectangle, portion: f32) -> Rectangle {
    let side = rect.width * portion;
    let c = rect.center();
    Rectangle::new(
        Point::new(c.x - side / 2.0, c.y - side / 2.0),
        Size::new(side, side),
    )
}

/// The arrow from `from` to `to`; `None` when they are the same square or
/// the geometry degenerates. `shrink` is clamped to `0.0..=1.0`.
pub fn arrow_geometry(
    layout: &BoardLayout,
    from: Square,
    to: Square,
    o: Orientation,
    shrink: f32,
) -> Option<Arrow> {
    if from == to {
        return None;
    }
    let from_rect = layout.square_rect(from, o);
    let c0 = from_rect.center();
    let c1 = layout.square_center(to, o);
    let (dx, dy) = (c1.x - c0.x, c1.y - c0.y);
    let mut shrink = if shrink.is_finite() {
        shrink.clamp(0.0, 1.0)
    } else {
        DEFAULT_SHRINK
    };
    let lambda = if dx.abs() > dy.abs() {
        let edge = if dx > 0.0 {
            from_rect.x + from_rect.width
        } else {
            from_rect.x
        };
        (edge - c0.x) / dx
    } else {
        let edge = if dy > 0.0 {
            from_rect.y + from_rect.height
        } else {
            from_rect.y
        };
        (edge - c0.y) / dy
    };
    let file_delta = i32::from(u8::from(to.file())) - i32::from(u8::from(from.file()));
    let rank_delta = i32::from(u8::from(to.rank())) - i32::from(u8::from(from.rank()));
    if shrink > MAX_SHRINK_ADJACENT && file_delta.abs() <= 1 && rank_delta.abs() <= 1 {
        shrink = MAX_SHRINK_ADJACENT;
    }
    let k = shrink * lambda;
    let tail = Point::new(c0.x + k * dx, c0.y + k * dy);
    let tip = Point::new(c1.x - k * dx, c1.y - k * dy);
    let (vx, vy) = (tip.x - tail.x, tip.y - tail.y);
    let length = vx.hypot(vy);
    if !length.is_finite() || length <= f32::EPSILON {
        return None;
    }
    let (ux, uy) = (vx / length, vy / length);
    let head_len = (HEAD_LENGTH * layout.s).min(length);
    let base = Point::new(tip.x - ux * head_len, tip.y - uy * head_len);
    let half = HEAD_WIDTH * layout.s / 2.0;
    let head = [
        tip,
        Point::new(base.x - uy * half, base.y + ux * half),
        Point::new(base.x + uy * half, base.y - ux * half),
    ];
    Some(Arrow {
        tail,
        tip,
        base,
        head,
        shaft_width: SHAFT_WIDTH * layout.s,
    })
}

/// An engine arrow (spec M4-5, D459): [`arrow_geometry`] with a shaft of
/// `shaft` times the side of a square, so that secondary and sequence
/// arrows are thinner than the best one.
pub fn engine_arrow_geometry(
    layout: &BoardLayout,
    from: Square,
    to: Square,
    o: Orientation,
    shaft: f32,
) -> Option<Arrow> {
    let mut arrow = arrow_geometry(layout, from, to, o, DEFAULT_SHRINK)?;
    if shaft.is_finite() && shaft > 0.0 {
        arrow.shaft_width = shaft * layout.s;
    }
    Some(arrow)
}

/// Extra reach of the clickable tip around the head triangle, in logical
/// pixels (the head is ~68 px² at 30 px squares).
pub const HEAD_HIT_MARGIN: f32 = 4.0;

impl Arrow {
    /// The outline of the whole arrow (shaft and head) as a polygon, in
    /// drawing order: left of the tail, left of the base, left corner of
    /// the head, the tip, and back along the right side.
    pub fn outline(&self) -> [Point; 7] {
        let [tip, left, right] = self.head;
        // Unit normal to the shaft, from the head's base corners (always
        // wide, even when the shaft degenerates to a point).
        let (nx, ny) = (left.x - right.x, left.y - right.y);
        let len = nx.hypot(ny);
        let (nx, ny) = if len > f32::EPSILON {
            (nx / len, ny / len)
        } else {
            (0.0, 0.0)
        };
        let h = self.shaft_width / 2.0;
        let off = |p: Point, k: f32| Point::new(p.x + nx * k, p.y + ny * k);
        [
            off(self.tail, h),
            off(self.base, h),
            left,
            tip,
            right,
            off(self.base, -h),
            off(self.tail, -h),
        ]
    }

    /// [`outline`](Self::outline) moved `d` pixels inwards, with mitered
    /// corners (a stroke of width `w` centred on it covers the band from
    /// `d - w/2` to `d + w/2` inside the outline). `None` when the arrow is too thin for it
    /// (the shaft is not wider than `2 d`) or `d` is not positive.
    pub fn outline_inset(&self, d: f32) -> Option<Vec<Point>> {
        if !(d.is_finite() && d > 0.0) || self.shaft_width <= 2.0 * d {
            return None;
        }
        inset_polygon(&self.outline(), d)
    }

    /// Distance from `p` to the head triangle (0 inside); `INFINITY` for a
    /// point that is not finite.
    pub fn head_distance(&self, p: Point) -> f32 {
        if !(p.x.is_finite() && p.y.is_finite()) {
            return f32::INFINITY;
        }
        if self.head_contains(p) {
            return 0.0;
        }
        let [tip, left, right] = self.head;
        let segment = |from: Point, to: Point| {
            let (dx, dy) = (to.x - from.x, to.y - from.y);
            let len2 = dx * dx + dy * dy;
            let along = if len2 > 0.0 {
                (((p.x - from.x) * dx + (p.y - from.y) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (p.x - (from.x + along * dx)).hypot(p.y - (from.y + along * dy))
        };
        segment(tip, left)
            .min(segment(left, right))
            .min(segment(right, tip))
    }

    /// Whether `p` is inside the head triangle (the clickable tip).
    pub fn head_contains(&self, p: Point) -> bool {
        fn side(a: Point, b: Point, p: Point) -> f32 {
            (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
        }
        let [a, b, c] = self.head;
        let (d1, d2, d3) = (side(a, b, p), side(b, c, p), side(c, a, p));
        let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        p.x.is_finite() && p.y.is_finite() && !(negative && positive)
    }
}

/// A simple polygon moved `d` pixels inwards (mitered corners); `None`
/// when it has fewer than 3 vertices, is degenerate or a vertex is not
/// finite.
pub fn inset_polygon(points: &[Point], d: f32) -> Option<Vec<Point>> {
    let n = points.len();
    if n < 3 || !d.is_finite() {
        return None;
    }
    let area2: f32 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    if !area2.is_finite() || area2.abs() <= f32::EPSILON {
        return None;
    }
    // The inward normal of an edge: to the left of a counter-clockwise
    // polygon (y down: the sign of the area flips it).
    let sign = if area2 > 0.0 { 1.0 } else { -1.0 };
    let normal = |a: Point, b: Point| -> Option<(f32, f32)> {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = dx.hypot(dy);
        (len > f32::EPSILON).then(|| (-dy / len * sign, dx / len * sign))
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (prev, cur, next) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        let (n1, n2) = (normal(prev, cur)?, normal(cur, next)?);
        let dot = n1.0 * n2.0 + n1.1 * n2.1;
        // The miter of a very sharp corner is capped.
        let k = d / (1.0 + dot).max(0.25);
        out.push(Point::new(
            cur.x + (n1.0 + n2.0) * k,
            cur.y + (n1.1 + n2.1) * k,
        ));
    }
    Some(out)
}

/// The mark color chosen by the modifiers of a right-button gesture.
///
/// `Shift` > `Command` (Ctrl on Linux and Windows) > `Alt`; none is green.
/// Names match the `ChessBase` letters `G R Y B` that `toggle_mark` writes.
pub fn color_for_modifiers(mods: Mods) -> &'static str {
    if mods.shift {
        "firebrick"
    } else if mods.command {
        "yellow2"
    } else if mods.alt {
        "blue"
    } else {
        "green"
    }
}

#[cfg(test)]
mod tests {
    use iced::Size;

    use super::*;
    use crate::widgets::board::geometry::{CoordsMode, LayoutOptions};

    #[test]
    fn inset_polygon_moves_a_square_inwards_and_rejects_degenerates() {
        let square = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        for orientation in [false, true] {
            let mut points = square.to_vec();
            if orientation {
                points.reverse();
            }
            let inner = inset_polygon(&points, 1.0).expect("inset");
            for p in inner {
                assert!((1.0 - 1e-3..=9.0 + 1e-3).contains(&p.x), "{p:?}");
                assert!((1.0 - 1e-3..=9.0 + 1e-3).contains(&p.y), "{p:?}");
            }
        }
        assert!(inset_polygon(&square[..2], 1.0).is_none());
        assert!(inset_polygon(&[Point::ORIGIN; 4], 1.0).is_none());
        assert!(inset_polygon(&square, f32::NAN).is_none());
    }

    #[test]
    fn the_arrow_outline_inset_stays_inside_and_needs_a_wide_enough_shaft() {
        let l = layout();
        let a = engine_arrow_geometry(
            &l,
            "e2".parse().expect("sq"),
            "e4".parse().expect("sq"),
            Orientation::White,
            0.14,
        )
        .expect("arrow");
        let inner = a.outline_inset(2.0).expect("inset");
        let outer = a.outline();
        let (min_x, max_x) = outer.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p.x), hi.max(p.x))
        });
        assert!(inner.iter().all(|p| p.x >= min_x && p.x <= max_x));
        assert!(a.outline_inset(a.shaft_width).is_none(), "too thin");
        assert!(a.outline_inset(0.0).is_none());
    }

    #[test]
    fn engine_arrow_shaft_is_a_fraction_of_the_square() {
        let l = layout();
        let from: Square = "e2".parse().expect("sq");
        let to: Square = "e4".parse().expect("sq");
        let thin = engine_arrow_geometry(&l, from, to, Orientation::White, 0.07).expect("arrow");
        assert!((thin.shaft_width - 0.07 * l.s).abs() < 1e-4);
        let normal =
            engine_arrow_geometry(&l, from, to, Orientation::White, f32::NAN).expect("arrow");
        assert!((normal.shaft_width - SHAFT_WIDTH * l.s).abs() < 1e-4);
    }

    #[test]
    fn head_contains_the_tip_but_not_the_tail_or_garbage() {
        let l = layout();
        let from: Square = "e2".parse().expect("sq");
        let to: Square = "e4".parse().expect("sq");
        let arrow =
            arrow_geometry(&l, from, to, Orientation::White, DEFAULT_SHRINK).expect("arrow");
        let inside = Point::new(
            (arrow.base.x + arrow.tip.x * 2.0) / 3.0,
            (arrow.base.y + arrow.tip.y * 2.0) / 3.0,
        );
        assert!(arrow.head_contains(inside));
        assert!(!arrow.head_contains(arrow.tail));
        assert!(!arrow.head_contains(Point::new(f32::NAN, 1.0)));
        assert!(!arrow.head_contains(Point::new(arrow.tip.x + 50.0, arrow.tip.y)));
    }

    fn layout() -> BoardLayout {
        BoardLayout::compute(
            Size::new(800.0, 800.0),
            LayoutOptions {
                coords: CoordsMode::None,
            },
        )
        .expect("layout fits")
    }

    fn sq(name: &str) -> Square {
        name.parse().expect("square")
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn e2_e4_arrow_is_shrunk_thirty_percent_of_a_square_at_each_end() {
        let layout = layout();
        let o = Orientation::White;
        let a = arrow_geometry(&layout, sq("e2"), sq("e4"), o, 0.6).expect("arrow");
        let c0 = layout.square_center(sq("e2"), o);
        let c1 = layout.square_center(sq("e4"), o);
        // s = 100: 30 px in front of e2's center and 30 px before e4's.
        assert!(close(a.tail.x, c0.x) && close(c0.y - a.tail.y, 30.0));
        assert!(close(c1.y - a.tip.y, -30.0) && close(a.tip.x, c1.x));
    }

    #[test]
    fn knight_arrow_uses_a_quarter_lambda() {
        let layout = layout();
        let o = Orientation::White;
        let a = arrow_geometry(&layout, sq("b1"), sq("c3"), o, 0.6).expect("arrow");
        let c0 = layout.square_center(sq("b1"), o);
        let c1 = layout.square_center(sq("c3"), o);
        let k = 0.6 * 0.25;
        assert!(close(a.tail.x, c0.x + k * (c1.x - c0.x)));
        assert!(close(a.tail.y, c0.y + k * (c1.y - c0.y)));
    }

    #[test]
    fn same_square_has_no_arrow() {
        assert_eq!(
            arrow_geometry(&layout(), sq("e4"), sq("e4"), Orientation::White, 0.6),
            None
        );
    }

    #[test]
    fn adjacent_squares_clamp_the_shrink() {
        let layout = layout();
        let o = Orientation::White;
        let a = arrow_geometry(&layout, sq("e2"), sq("e3"), o, 1.0).expect("arrow");
        let c0 = layout.square_center(sq("e2"), o);
        // lambda = 0.5, shrink clamped to 0.667: 33.35 px.
        assert!(close(c0.y - a.tail.y, 0.667 * 0.5 * 100.0));
    }

    #[test]
    fn diagonal_arrow_head_is_symmetric_about_the_shaft() {
        let layout = layout();
        let a =
            arrow_geometry(&layout, sq("a1"), sq("h8"), Orientation::White, 0.6).expect("arrow");
        let mid = Point::new(
            f32::midpoint(a.head[1].x, a.head[2].x),
            f32::midpoint(a.head[1].y, a.head[2].y),
        );
        assert!(close(mid.x, a.base.x) && close(mid.y, a.base.y));
        let width = (a.head[1].x - a.head[2].x).hypot(a.head[1].y - a.head[2].y);
        assert!(close(width, 40.0));
        assert!(close(a.shaft_width, 14.0));
    }

    #[test]
    fn inner_box_is_concentric() {
        let rect = Rectangle::new(Point::new(100.0, 200.0), Size::new(100.0, 100.0));
        let b = inner_box(rect, 0.85);
        assert!(close(b.width, 85.0));
        assert!(close(b.x, 107.5) && close(b.y, 207.5));
    }

    #[test]
    fn modifiers_choose_the_color_with_shift_first() {
        let none = Mods::default();
        assert_eq!(color_for_modifiers(none), "green");
        let m = |shift, command, alt| Mods {
            shift,
            command,
            alt,
        };
        assert_eq!(color_for_modifiers(m(true, true, true)), "firebrick");
        assert_eq!(color_for_modifiers(m(false, true, true)), "yellow2");
        assert_eq!(color_for_modifiers(m(false, false, true)), "blue");
    }
}
