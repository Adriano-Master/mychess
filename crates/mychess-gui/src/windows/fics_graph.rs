//! The seek graph of FICS (spec M8-9c, D2626): one dot per advertisement,
//! time on the x axis (logarithmic) and rating on the y axis.
//!
//! The geometry is pure ([`SeekPlot`]) so that it can be tested without a
//! window; the canvas only draws what it computes and turns a click into
//! the selection of the nearest dot.

use iced::widget::canvas::{self, Action, Event, Frame, Path, Stroke, Text};
use iced::{Color, Pixels, Point, Rectangle, Size, mouse};
use mychess_app::Message as AppMessage;
use mychess_app::fics::{FicsMessage, Seek, estimated_minutes, is_playable};

use crate::shell::Message;
use crate::windows::fics_dialog::FicsUiMessage;

/// Highest rating of the y axis.
const MAX_RATING: f32 = 3000.0;
/// Longest estimated game of the x axis, in minutes.
const MAX_MINUTES: f32 = 120.0;
/// Margin around the plot, for the labels.
const MARGIN: f32 = 28.0;
/// How near a click must be to a dot, in pixels.
const HIT_RADIUS: f32 = 10.0;
/// Radius of a dot.
const DOT: f32 = 4.0;

/// How a dot is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// `lightning`.
    Lightning,
    /// `blitz`.
    Blitz,
    /// `standard`.
    Standard,
    /// Anything else (`untimed`, the variants).
    Other,
}

impl Class {
    fn of(kind: &str) -> Class {
        match kind {
            "lightning" => Class::Lightning,
            "blitz" => Class::Blitz,
            "standard" => Class::Standard,
            _ => Class::Other,
        }
    }

    fn color(self) -> Color {
        crate::theme::seek_dot(match self {
            Class::Lightning => 0,
            Class::Blitz => 1,
            Class::Standard => 2,
            Class::Other => 3,
        })
    }
}

/// One advertisement placed in the unit square (`0..=1` on both axes; y
/// grows upwards).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dot {
    /// The number to `play`.
    pub index: u32,
    /// Horizontal place.
    pub x: f32,
    /// Vertical place.
    pub y: f32,
    /// The colour class.
    pub class: Class,
    /// Rated game (a circle) or not (a square).
    pub rated: bool,
    /// The app can play it (D2600).
    pub playable: bool,
}

/// The place of an advertisement in the unit square.
#[must_use]
pub fn dot_of(seek: &Seek) -> Dot {
    let minutes = estimated_minutes(seek).clamp(0.0, MAX_MINUTES);
    // A rating of 0 (unrated) sits on the axis.
    #[allow(clippy::cast_precision_loss)] // ratings are below 10 000
    let rating = seek.rating.map_or(0.0, |r| r as f32);
    Dot {
        index: seek.index,
        x: (1.0 + minutes).ln() / (1.0 + MAX_MINUTES).ln(),
        y: (rating / MAX_RATING).clamp(0.0, 1.0),
        class: Class::of(&seek.kind),
        rated: seek.rated,
        playable: is_playable(seek),
    }
}

/// The plot area inside `size`, `None` when too small to draw.
#[must_use]
pub fn plot_rect(size: Size) -> Option<Rectangle> {
    let (w, h) = (size.width - 2.0 * MARGIN, size.height - 2.0 * MARGIN);
    (w > 10.0 && h > 10.0)
        .then(|| Rectangle::new(Point::new(MARGIN, MARGIN * 0.5), Size::new(w, h)))
}

/// The pixel of a dot.
#[must_use]
pub fn pixel(dot: &Dot, plot: Rectangle) -> Point {
    Point::new(
        plot.x + dot.x * plot.width,
        plot.y + (1.0 - dot.y) * plot.height,
    )
}

/// The dot nearest to `at` within the hit radius (the last drawn wins a
/// tie: it is on top).
#[must_use]
pub fn hit(dots: &[Dot], plot: Rectangle, at: Point) -> Option<u32> {
    dots.iter()
        .filter_map(|d| {
            let p = pixel(d, plot);
            let dist = (p.x - at.x).hypot(p.y - at.y);
            (dist <= HIT_RADIUS).then_some((dist, d.index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| index)
}

/// The canvas program of the graph.
#[derive(Debug)]
pub struct SeekPlot {
    /// The dots, oldest first.
    pub dots: Vec<Dot>,
    /// The dot selected.
    pub selected: Option<u32>,
}

impl canvas::Program<Message> for SeekPlot {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return None;
        };
        let plot = plot_rect(bounds.size())?;
        let at = cursor.position_in(bounds)?;
        let index = hit(&self.dots, plot, at)?;
        // The second click on the selected dot plays it (D2626).
        let message = if self.selected == Some(index) {
            Message::App(AppMessage::Fics(FicsMessage::PlaySeek(index)))
        } else {
            Message::Fics(FicsUiMessage::SelectSeek(Some(index)))
        };
        Some(Action::publish(message).and_capture())
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let ink = theme.palette().text;
        let mut frame = Frame::new(renderer, bounds.size());
        let Some(plot) = plot_rect(bounds.size()) else {
            return vec![frame.into_geometry()];
        };
        let faint = Color { a: 0.35, ..ink };
        frame.stroke(
            &Path::rectangle(plot.position(), plot.size()),
            Stroke::default().with_width(1.0).with_color(faint),
        );
        for rating in [1000.0_f32, 2000.0, 3000.0] {
            let y = plot.y + (1.0 - rating / MAX_RATING) * plot.height;
            frame.stroke(
                &Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y)),
                Stroke::default().with_width(0.5).with_color(faint),
            );
            frame.fill_text(Text {
                content: format!("{rating:.0}"),
                position: Point::new(plot.x - 3.0, y),
                color: ink,
                size: Pixels(10.0),
                align_x: iced::alignment::Horizontal::Right.into(),
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            });
        }
        for minutes in [1.0_f32, 5.0, 15.0, 60.0] {
            let x = plot.x + (1.0 + minutes).ln() / (1.0 + MAX_MINUTES).ln() * plot.width;
            frame.fill_text(Text {
                content: format!("{minutes:.0}'"),
                position: Point::new(x, plot.y + plot.height + 3.0),
                color: ink,
                size: Pixels(10.0),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Top,
                ..Text::default()
            });
        }
        for dot in &self.dots {
            let at = pixel(dot, plot);
            let mut color = dot.class.color();
            if !dot.playable {
                color.a = 0.35;
            }
            let shape = if dot.rated {
                Path::circle(at, DOT)
            } else {
                Path::rectangle(
                    Point::new(at.x - DOT, at.y - DOT),
                    Size::new(2.0 * DOT, 2.0 * DOT),
                )
            };
            frame.fill(&shape, color);
            if self.selected == Some(dot.index) {
                frame.stroke(
                    &Path::circle(at, DOT + 3.0),
                    Stroke::default().with_width(2.0).with_color(ink),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_online::fics::{FicsEvent, parse_line};

    fn seek(
        index: u32,
        minutes: u32,
        increment: u32,
        rating: &str,
        kind: &str,
        rated: char,
    ) -> Seek {
        let text = format!(
            "<s> {index} w=bob ti=00 rt={rating} t={minutes} i={increment} r={rated} tp={kind} c=? rr=0-9999 a=t f=f"
        );
        let FicsEvent::SeekAdd(s) = parse_line(&text) else {
            panic!("not a seek: {text}");
        };
        *s
    }

    #[test]
    fn a_longer_game_is_further_right_and_a_better_player_higher() {
        let a = dot_of(&seek(1, 1, 0, "1500E", "lightning", 'u'));
        let b = dot_of(&seek(2, 15, 0, "2200E", "standard", 'r'));
        assert!(b.x > a.x && b.y > a.y);
        assert_eq!((a.class, b.class), (Class::Lightning, Class::Standard));
        assert!(!a.rated && b.rated);
    }

    #[test]
    fn an_unrated_player_sits_on_the_axis_and_huge_values_stay_inside() {
        let a = dot_of(&seek(1, 600, 600, "0", "blitz", 'u'));
        assert!(a.y.abs() < f32::EPSILON);
        assert!((0.0..=1.0).contains(&a.x));
    }

    #[test]
    fn variants_are_dim_and_not_playable() {
        let v = dot_of(&seek(1, 5, 0, "1500E", "crazyhouse", 'u'));
        assert!(!v.playable);
        assert_eq!(v.class, Class::Other);
    }

    #[test]
    fn a_click_picks_the_nearest_dot_within_reach_only() {
        let plot = plot_rect(Size::new(400.0, 300.0)).expect("big enough");
        let dots = vec![
            dot_of(&seek(1, 5, 0, "1500E", "blitz", 'u')),
            dot_of(&seek(2, 5, 0, "1510E", "blitz", 'u')),
        ];
        let at = pixel(&dots[1], plot);
        assert_eq!(hit(&dots, plot, Point::new(at.x + 1.0, at.y)), Some(2));
        assert_eq!(hit(&dots, plot, Point::new(at.x + 200.0, at.y)), None);
    }

    #[test]
    fn a_tiny_canvas_has_no_plot() {
        assert!(plot_rect(Size::new(20.0, 20.0)).is_none());
    }
}
