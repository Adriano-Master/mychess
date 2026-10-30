//! Plot geometry and text helpers shared by the canvases of the charts of
//! the filter and of the rating graph (spec M6-4d, D3431). Everything is in
//! logical pixels, so a fractional `HiDPI` scale needs no special case.

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Weak};

use iced::alignment::{Horizontal, Vertical};
use iced::widget::canvas::{self, Frame, Text};
use iced::{Color, Pixels, Point, Rectangle, Size};

/// Identity of an `Arc` that does not keep the data alive but keeps the
/// allocation, so the address is never reused while the key lives: two keys
/// are equal only for the very same `Arc`.
#[derive(Debug, Clone)]
pub struct ArcKey<T>(Weak<T>);

impl<T> ArcKey<T> {
    /// The key of `arc`.
    pub fn of(arc: &Arc<T>) -> Self {
        Self(Arc::downgrade(arc))
    }
}

impl<T> PartialEq for ArcKey<T> {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.0, &other.0)
    }
}

/// The static layer of a canvas (grid, labels, bars), kept in a
/// [`canvas::Cache`] and rebuilt only when the `key` (the data, the theme
/// colors, the options) or the size of the canvas changes; the hover
/// highlight is drawn apart, in a second geometry, so moving the pointer
/// never repaints this layer (D3450).
pub struct StaticLayer<K> {
    cache: canvas::Cache,
    key: RefCell<Option<K>>,
    redraws: Cell<u32>,
}

impl<K> Default for StaticLayer<K> {
    fn default() -> Self {
        Self {
            cache: canvas::Cache::new(),
            key: RefCell::new(None),
            redraws: Cell::new(0),
        }
    }
}

impl<K: PartialEq> StaticLayer<K> {
    /// The geometry of the layer: `paint` runs only when `key` differs from
    /// the one of the last call or the size changed.
    pub fn geometry(
        &self,
        renderer: &iced::Renderer,
        size: Size,
        key: K,
        paint: impl FnOnce(&mut Frame),
    ) -> canvas::Geometry {
        {
            let mut last = self.key.borrow_mut();
            if last.as_ref() != Some(&key) {
                self.cache.clear();
                *last = Some(key);
            }
        }
        self.cache.draw(renderer, size, |frame| {
            self.redraws.set(self.redraws.get().saturating_add(1));
            paint(frame);
        })
    }

    /// How many times the layer was painted (for tests).
    pub fn redraws(&self) -> u32 {
        self.redraws.get()
    }
}

/// Room around the plot, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margins {
    /// Left, for the labels of the y axis.
    pub left: f32,
    /// Right.
    pub right: f32,
    /// Top.
    pub top: f32,
    /// Bottom, for the labels of the x axis.
    pub bottom: f32,
}

/// The margins of both charts.
pub const MARGINS: Margins = Margins {
    left: 46.0,
    right: 14.0,
    top: 10.0,
    bottom: 24.0,
};

/// The plot is not drawn below this width or height.
pub const MIN_PLOT: f32 = 40.0;

/// Size of the axis labels.
pub const LABEL_SIZE: f32 = 11.0;

/// The rectangle of the plot inside a canvas of `size`; `None` when it is too
/// small (or the size is not finite).
pub fn plot_rect(size: Size, m: Margins) -> Option<Rectangle> {
    let width = size.width - m.left - m.right;
    let height = size.height - m.top - m.bottom;
    (width.is_finite() && height.is_finite() && width >= MIN_PLOT && height >= MIN_PLOT)
        .then(|| Rectangle::new(Point::new(m.left, m.top), Size::new(width, height)))
}

/// Maps `value` from `[lo, hi]` to `[from, to]`. A flat range maps to
/// `from`.
pub fn map(value: f32, lo: f32, hi: f32, from: f32, to: f32) -> f32 {
    let span = hi - lo;
    if !span.is_finite() || span.abs() < f32::EPSILON {
        return from;
    }
    from + (value - lo) / span * (to - from)
}

/// The pixel `x` of `value` on an axis `[lo, hi]` that spans the plot.
pub fn px_x(value: f32, lo: f32, hi: f32, plot: Rectangle) -> f32 {
    map(value, lo, hi, plot.x, plot.x + plot.width)
}

/// The pixel `y` of `value` on an axis `[lo, hi]` (up is positive).
pub fn px_y(value: f32, lo: f32, hi: f32, plot: Rectangle) -> f32 {
    map(value, lo, hi, plot.y + plot.height, plot.y)
}

/// The index of the value closest to `target`; `None` for no values.
pub fn nearest(values: impl IntoIterator<Item = f32>, target: f32) -> Option<usize> {
    values
        .into_iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .min_by(|a, b| (a.1 - target).abs().total_cmp(&(b.1 - target).abs()))
        .map(|(i, _)| i)
}

/// Draws `content` at `at`.
pub fn label(
    frame: &mut Frame,
    content: String,
    at: Point,
    align: (Horizontal, Vertical),
    color: Color,
) {
    frame.fill_text(Text {
        content,
        position: at,
        color,
        size: Pixels(LABEL_SIZE),
        align_x: align.0.into(),
        align_y: align.1,
        ..Text::default()
    });
}

/// A number for an axis: whole when the step is whole, else with one
/// decimal.
pub fn axis_number(value: f32, step: f32) -> String {
    if step >= 1.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plot_needs_room() {
        assert!(plot_rect(Size::new(400.0, 200.0), MARGINS).is_some());
        assert!(plot_rect(Size::new(60.0, 200.0), MARGINS).is_none());
        assert!(plot_rect(Size::new(f32::NAN, 200.0), MARGINS).is_none());
    }

    #[test]
    fn the_axes_map_up_and_across() {
        let plot = plot_rect(Size::new(400.0, 200.0), MARGINS).unwrap();
        assert!((px_x(0.0, 0.0, 10.0, plot) - plot.x).abs() < 1e-3);
        assert!((px_x(10.0, 0.0, 10.0, plot) - (plot.x + plot.width)).abs() < 1e-3);
        assert!((px_y(0.0, 0.0, 10.0, plot) - (plot.y + plot.height)).abs() < 1e-3);
        assert!((px_y(10.0, 0.0, 10.0, plot) - plot.y).abs() < 1e-3);
        // A flat axis does not divide by zero.
        assert!(px_x(5.0, 3.0, 3.0, plot).is_finite());
    }

    #[test]
    fn the_nearest_value_is_found() {
        assert_eq!(nearest([1.0, 4.0, 9.0], 5.0), Some(1));
        assert_eq!(nearest([], 5.0), None);
        assert_eq!(nearest([f32::NAN, 2.0], 0.0), Some(1));
    }

    #[test]
    fn labels_follow_the_step() {
        assert_eq!(axis_number(50.0, 10.0), "50");
        assert_eq!(axis_number(0.5, 0.1), "0.5");
    }
}
