//! The evaluation bar and the MultiPV traffic lights next to the board
//! (spec M4-5, D454, D458, D459; Scid `board.tcl` `update_evalbar`,
//! `update_mpvtls`).
//!
//! Both are sibling canvases of the board, as tall as the 8 squares. They
//! only draw the views the application prepared ([`EvalBarView`],
//! [`TrafficLight`]); no score is interpreted here.
// Pixel math on bounded values (a few dozen ticks, sizes are capped).
#![allow(clippy::cast_precision_loss)]

use std::cell::RefCell;
use std::sync::OnceLock;

use iced::advanced::graphics::text::Paragraph as ShapedText;
use iced::advanced::text::{Alignment, LineHeight, Paragraph, Wrapping};
use iced::widget::canvas::{self, Cache, Frame, Path, Stroke, Text};
use iced::{Color, Element, Length, Pixels, Point, Rectangle, Size, mouse};

use crate::app_api::{EvalBarView, TrafficLight};
use crate::fonts::NOTO_SANS_BOLD;
use crate::theme::engine_overlay as eo;

/// Size of the label text.
const LABEL_SIZE: f32 = 9.0;
/// Room left around the label inside the bar, both sides together.
const LABEL_PADDING: f32 = 4.0;
/// The widest labels the application can write for a position of a game
/// (a bound of two digits of pawns, a mate in two digits); the bar is as
/// wide as the widest of them (D858).
const WIDEST_LABELS: [&str; 5] = [
    "\u{2265}+88.88",
    "\u{2264}-88.88",
    "\u{2265}-10.00",
    "-M88",
    "+100.0",
];

/// Width of the bar or of the traffic lights next to a board of squares of
/// side `s`: `ceil(s / 10) + 2` as in the Scid (`board.tcl:1399`).
pub fn strip_width(s: f32) -> f32 {
    if s.is_finite() && s > 0.0 {
        (s / 10.0).ceil() + 2.0
    } else {
        2.0
    }
}

/// The width of `content` in the font and size of the label.
fn measure_label(content: &str) -> f32 {
    let text = iced::advanced::text::Text {
        content,
        bounds: Size::new(1.0e6, 1.0e6),
        size: Pixels(LABEL_SIZE),
        line_height: LineHeight::default(),
        font: NOTO_SANS_BOLD,
        align_x: Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: Text::default().shaping,
        wrapping: Wrapping::None,
    };
    let width = ShapedText::with_text(text).min_bounds().width;
    if width.is_finite() { width } else { 0.0 }
}

/// The width of the widest possible label, measured with the font of the
/// label once (the fonts are registered before the first view).
pub fn widest_label_width() -> f32 {
    static WIDTH: OnceLock<f32> = OnceLock::new();
    *WIDTH.get_or_init(|| {
        WIDEST_LABELS
            .iter()
            .map(|label| measure_label(label))
            .fold(0.0, f32::max)
    })
}

/// Width of the bar. It depends only on the preference of showing the
/// label (`with_label`), never on whether there is a value right now, so
/// the board does not move while the user navigates: with the label the
/// bar is wide enough for the widest possible text, else `ceil(s/10)+2`
/// (D851, D858).
pub fn bar_width(s: f32, with_label: bool) -> f32 {
    let base = strip_width(s);
    if with_label {
        base.max((widest_label_width() + LABEL_PADDING).ceil())
    } else {
        base
    }
}

/// The white part of a bar of `height`: `(top, length)` measured from the
/// top edge. White is at the bottom unless the board is turned.
pub fn white_span(fill: f32, flipped: bool, height: f32) -> (f32, f32) {
    let fill = if fill.is_finite() {
        fill.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let length = fill * height;
    if flipped {
        (0.0, length)
    } else {
        (height - length, length)
    }
}

/// The `y` of a tick at `position` (`0.0..=1.0`, as the fill).
pub fn tick_y(position: f32, flipped: bool, height: f32) -> f32 {
    let (top, length) = white_span(position, flipped, height);
    if flipped { length } else { top }
}

/// The bands of `count` traffic lights in `height`: `(top, length)`, the
/// first at the top, each `height / count` tall (`board.tcl:3371-3398`).
pub fn light_bands(count: usize, height: f32) -> Vec<(f32, f32)> {
    if count == 0 {
        return Vec::new();
    }
    let each = height / count as f32;
    (0..count).map(|i| (i as f32 * each, each)).collect()
}

/// A drawing cache keyed by what it was drawn from: the value is compared
/// on every draw and the cache cleared when it differs.
#[derive(Debug, Default)]
struct Keyed<T> {
    cache: Cache,
    seen: RefCell<Option<T>>,
}

impl<T: PartialEq + Clone> Keyed<T> {
    fn cache_for(&self, key: &T) -> &Cache {
        let mut seen = self.seen.borrow_mut();
        if seen.as_ref() != Some(key) {
            self.cache.clear();
            *seen = Some(key.clone());
        }
        &self.cache
    }
}

/// The thickness of the outline: one logical pixel, so it follows the
/// scale factor of the window.
const OUTLINE: f32 = 1.0;

/// Draws the outline as four rectangles inside `size` (no half-pixel
/// offsets, which blur at fractional scale factors).
fn draw_outline(frame: &mut Frame, size: Size, color: Color) {
    let t = OUTLINE.min(size.width / 2.0).min(size.height / 2.0);
    frame.fill_rectangle(Point::ORIGIN, Size::new(size.width, t), color);
    frame.fill_rectangle(
        Point::new(0.0, size.height - t),
        Size::new(size.width, t),
        color,
    );
    frame.fill_rectangle(
        Point::new(0.0, t),
        Size::new(t, size.height - 2.0 * t),
        color,
    );
    frame.fill_rectangle(
        Point::new(size.width - t, t),
        Size::new(t, size.height - 2.0 * t),
        color,
    );
}

struct EvalBarCanvas<'a> {
    view: &'a EvalBarView,
}

impl<M> canvas::Program<M> for EvalBarCanvas<'_> {
    type State = Keyed<EvalBarView>;

    fn draw(
        &self,
        state: &Keyed<EvalBarView>,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let cache = state.cache_for(self.view);
        vec![cache.draw(renderer, bounds.size(), |frame| {
            draw_bar(frame, bounds.size(), self.view);
        })]
    }
}

fn draw_bar(frame: &mut Frame, size: Size, view: &EvalBarView) {
    let alpha = if view.stale { eo::STALE_ALPHA } else { 1.0 };
    let bar_width = size.width.max(2.0);
    let x = 0.0;
    let bar = Size::new(bar_width, size.height);
    if let Some(fill) = view.fill {
        frame.fill_rectangle(Point::new(x, 0.0), bar, eo::BAR_BLACK.scale_alpha(alpha));
        let (top, length) = white_span(fill, view.flipped, size.height);
        frame.fill_rectangle(
            Point::new(x, top),
            Size::new(bar_width, length),
            eo::BAR_WHITE.scale_alpha(alpha),
        );
        for tick in &view.ticks {
            let y = tick_y(*tick, view.flipped, size.height);
            frame.stroke(
                &Path::line(Point::new(x, y), Point::new(x + bar_width, y)),
                Stroke::default()
                    .with_width(1.0)
                    .with_color(eo::BAR_OUTLINE.scale_alpha(alpha)),
            );
        }
    }
    // The empty bar is only the outline (Scid state `clear`).
    draw_outline(frame, bar, eo::BAR_OUTLINE);
    if let (Some(label), Some(_)) = (&view.label, view.fill) {
        // The label sits at the end of the side that is ahead.
        let at_bottom = view.label_on_white != view.flipped;
        let y = if at_bottom {
            size.height - LABEL_SIZE
        } else {
            LABEL_SIZE
        };
        let color: Color = if view.label_on_white {
            eo::BAR_TEXT_ON_WHITE
        } else {
            eo::BAR_TEXT_ON_BLACK
        };
        frame.fill_text(Text {
            content: label.clone(),
            position: Point::new(size.width / 2.0, y),
            color: color.scale_alpha(alpha),
            size: Pixels(LABEL_SIZE),
            font: NOTO_SANS_BOLD,
            align_x: iced::alignment::Horizontal::Center.into(),
            align_y: iced::alignment::Vertical::Center,
            ..Text::default()
        });
    }
}

/// The evaluation bar as an element of `width` by `height`.
pub fn eval_bar<'a, M: 'a>(view: &'a EvalBarView, width: f32, height: f32) -> Element<'a, M> {
    canvas::Canvas::new(EvalBarCanvas { view })
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .into()
}

struct LightsCanvas<'a> {
    lights: &'a [TrafficLight],
}

impl<M> canvas::Program<M> for LightsCanvas<'_> {
    type State = Keyed<Vec<TrafficLight>>;

    fn draw(
        &self,
        state: &Keyed<Vec<TrafficLight>>,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let cache = state.cache_for(&self.lights.to_vec());
        vec![cache.draw(renderer, bounds.size(), |frame| {
            self.paint(frame, bounds);
        })]
    }
}

impl LightsCanvas<'_> {
    fn paint(&self, frame: &mut Frame, bounds: Rectangle) {
        for (light, (top, length)) in self
            .lights
            .iter()
            .zip(light_bands(self.lights.len(), bounds.height))
        {
            frame.fill_rectangle(
                Point::new(0.0, top),
                Size::new(bounds.width, length),
                eo::light(light.color.r, light.color.g, light.color.b),
            );
        }
        draw_outline(frame, bounds.size(), eo::BAR_OUTLINE);
    }
}

/// The traffic lights as an element of `width` by `height`.
pub fn traffic_lights<'a, M: 'a>(
    lights: &'a [TrafficLight],
    width: f32,
    height: f32,
) -> Element<'a, M> {
    canvas::Canvas::new(LightsCanvas { lights })
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_width_follows_the_scid_formula() {
        assert!((strip_width(80.0) - 10.0).abs() < f32::EPSILON);
        assert!((strip_width(81.0) - 11.0).abs() < f32::EPSILON);
        assert!((strip_width(f32::NAN) - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn white_is_measured_from_the_bottom_unless_flipped() {
        assert_eq!(white_span(0.75, false, 400.0), (100.0, 300.0));
        assert_eq!(white_span(0.75, true, 400.0), (0.0, 300.0));
        assert_eq!(white_span(0.5, false, 400.0), (200.0, 200.0));
    }

    #[test]
    fn white_span_defends_against_bad_fills() {
        assert_eq!(white_span(f32::NAN, false, 100.0), (50.0, 50.0));
        assert_eq!(white_span(2.0, false, 100.0), (0.0, 100.0));
        assert_eq!(white_span(-1.0, true, 100.0), (0.0, 0.0));
    }

    #[test]
    fn ticks_follow_the_fill_axis() {
        assert!((tick_y(0.25, false, 400.0) - 300.0).abs() < 1e-3);
        assert!((tick_y(0.25, true, 400.0) - 100.0).abs() < 1e-3);
    }

    #[test]
    fn traffic_light_bands_split_the_height_evenly() {
        assert_eq!(light_bands(0, 100.0), []);
        assert_eq!(light_bands(1, 80.0), [(0.0, 80.0)]);
        assert_eq!(
            light_bands(4, 80.0),
            [(0.0, 20.0), (20.0, 20.0), (40.0, 20.0), (60.0, 20.0)]
        );
    }

    #[test]
    fn the_width_depends_only_on_the_label_preference() {
        assert!((bar_width(80.0, false) - 10.0).abs() < f32::EPSILON);
        assert!(bar_width(80.0, true) > bar_width(80.0, false));
        // The same for a small and a large board: nothing about a value.
        assert!(bar_width(20.0, true) >= strip_width(20.0));
    }

    #[test]
    fn the_widest_possible_label_fits_inside_the_bar() {
        for s in [20.0, 80.0, 200.0, 290.0] {
            let width = bar_width(s, true);
            for label in WIDEST_LABELS {
                let text = measure_label(label);
                assert!(
                    text + LABEL_PADDING <= width + f32::EPSILON,
                    "{label:?}: {text} + padding in {width} (s = {s})"
                );
            }
        }
        // The outer labels of the finding: seven characters.
        for label in ["\u{2265}-10.00", "\u{2265}+10.50"] {
            assert!(measure_label(label) + LABEL_PADDING <= bar_width(80.0, true));
        }
    }

    #[test]
    fn the_keyed_cache_clears_only_when_the_key_changes() {
        let keyed: Keyed<u8> = Keyed::default();
        keyed.cache_for(&1);
        keyed.cache_for(&1);
        assert_eq!(keyed.seen.borrow().as_ref(), Some(&1));
        keyed.cache_for(&2);
        assert_eq!(keyed.seen.borrow().as_ref(), Some(&2));
    }
}
