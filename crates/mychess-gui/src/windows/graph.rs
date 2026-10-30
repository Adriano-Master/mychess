//! The score and time graph panel (spec M4-7, D488, D489, D873-D875):
//! bars of the scores or the move times of the main line, the running time
//! totals of each side, the grid and the bar of the current move, drawn on
//! a `canvas`.
//!
//! The application prepares everything ([`ScoreGraphView`], the bar of the
//! current move, the click-to-move mapping); this module only converts graph
//! coordinates to pixels and draws. The grid, the bars and the lines are one
//! cached layer, rebuilt when the application builds a new view (its stamp
//! changes), when the size changes (the cache does that itself) or when the
//! theme changes. The current-move bar and the hover marker are drawn over
//! it every frame: they are a few rectangles.
// Pixel math on bounded values (the axes have at most 20 000 ticks).
#![allow(clippy::cast_precision_loss)]

use std::cell::Cell;

use fluent_bundle::FluentArgs;
use iced::widget::canvas::{self, Action, Cache, Event, Fill, Frame, LineDash, Path, Stroke, Text};
use iced::widget::{checkbox, column, container, pick_list, row, stack, text, tooltip};
use iced::{Color, Element, Length, Pixels, Point, Rectangle, Size, mouse};
use mychess_app::Message as AppMessage;
use mychess_app::graphs::{
    Axis, CurveSegment, GraphKind, GraphMessage, GraphStyle, LineGraphView, ScoreGraphView,
    grid_lines, move_ticks, nearest_ply, ply_at_x, x_of_ply,
};

use super::header_dialog::Labeled;
use super::panes;
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::graph_colors::GraphColors;
use crate::theme::tokens::Tokens;
use crate::theme::{self};

/// Room left of the plot for the y labels, in logical pixels.
const MARGIN_LEFT: f32 = 34.0;
/// Room right of the plot.
const MARGIN_RIGHT: f32 = 10.0;
/// Room above the plot.
const MARGIN_TOP: f32 = 8.0;
/// Room under the plot for the x labels.
const MARGIN_BOTTOM: f32 = 18.0;
/// The plot is not drawn (nor clickable) below this size.
const MIN_PLOT: f32 = 24.0;
/// A bar takes this share of the 0.5 wide slot of its ply (Scid `-barwidth
/// .7`, in graph units of a move; the plies are half a unit apart).
const BAR_SHARE: f32 = 0.7;
/// Size of the axis labels.
const LABEL_SIZE: f32 = 10.0;
/// Labels drawn per axis, at most (the application bounds the ticks too).
const MAX_LABELS: usize = 64;

/// The visual state of the panel: the cache of the static layer.
#[derive(Debug, Default)]
pub struct GraphUi {
    cache: Cache,
    seen_stamp: Cell<u64>,
    /// The colors the cache was drawn with (a change of palette redraws).
    palette: Cell<Option<GraphColors>>,
}

impl GraphUi {
    /// Drops the drawing when the application built another view.
    pub fn sync(&mut self, stamp: u64) {
        if self.seen_stamp.get() != stamp {
            self.seen_stamp.set(stamp);
            self.cache.clear();
        }
    }
}

// ---- geometry (pure) ----

/// The rectangle of the plot inside a canvas of `size`; `None` when it is
/// too small to be useful.
pub(crate) fn plot_rect(size: Size) -> Option<Rectangle> {
    let width = size.width - MARGIN_LEFT - MARGIN_RIGHT;
    let height = size.height - MARGIN_TOP - MARGIN_BOTTOM;
    (width.is_finite() && height.is_finite() && width >= MIN_PLOT && height >= MIN_PLOT).then(
        || {
            Rectangle::new(
                Point::new(MARGIN_LEFT, MARGIN_TOP),
                Size::new(width, height),
            )
        },
    )
}

/// Pixel `x` (in the canvas) of the graph coordinate `x`.
pub(crate) fn px_x(axis: &Axis, x: f32, plot: Rectangle) -> f32 {
    plot.x + (x - axis.min) / (axis.max - axis.min) * plot.width
}

/// Pixel `y` of the graph coordinate `y` (up is positive).
pub(crate) fn px_y(axis: &Axis, y: f32, plot: Rectangle) -> f32 {
    plot.y + plot.height - (y - axis.min) / (axis.max - axis.min) * plot.height
}

/// The graph coordinate under the pixel `px` of the canvas.
pub(crate) fn x_at_px(axis: &Axis, px: f32, plot: Rectangle) -> f32 {
    axis.min + (px - plot.x) / plot.width * (axis.max - axis.min)
}

/// The rectangle `(origin, size)` of the bar `(x, y)`: from the zero line
/// (or the lower edge of an axis that starts above zero) to the value,
/// limited to the plot. `None` for a value that is not finite or a bar
/// entirely outside the axis.
pub(crate) fn bar_rect(
    x_axis: &Axis,
    y_axis: &Axis,
    bar: (f32, f32),
    plot: Rectangle,
) -> Option<(Point, Size)> {
    let (x, y) = bar;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let half = 0.25 * BAR_SHARE;
    let left = px_x(x_axis, x - half, plot).max(plot.x);
    let right = px_x(x_axis, x + half, plot).min(plot.x + plot.width);
    let base = px_y(y_axis, 0.0f32.clamp(y_axis.min, y_axis.max), plot);
    let top = px_y(y_axis, y.clamp(y_axis.min, y_axis.max), plot);
    let width = (right - left).max(1.0);
    let height = (base - top).abs();
    (right > left && height > 0.0)
        .then(|| (Point::new(left, base.min(top)), Size::new(width, height)))
}

/// The number of a move and the dots of the side: ply 1 is `1.`, ply 2 `1...`.
pub(crate) fn move_label(ply: usize) -> String {
    let number = ply.div_ceil(2);
    if ply % 2 == 1 {
        format!("{number}.")
    } else {
        format!("{number}...")
    }
}

/// A tick value without a useless fraction.
fn tick_text(value: f32) -> String {
    if (value - value.round()).abs() < 1e-3 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

/// The value of the bar of `ply`, if it has one.
pub(crate) fn bar_at_ply(bars: &[(f32, f32)], ply: usize) -> Option<f32> {
    let x = x_of_ply(ply);
    let at = bars.partition_point(|b| b.0 < x - 1e-3);
    bars.get(at).filter(|b| (b.0 - x).abs() < 1e-3).map(|b| b.1)
}

// ---- the line style (M4-9) ----

/// Pixel `x` of a ply (a float, to follow the curve) in the line style: the
/// axis runs from 0 to `plies` (at least 1).
pub(crate) fn line_px_x(ply: f32, plies: usize, plot: Rectangle) -> f32 {
    plot.x + ply / plies.max(1) as f32 * plot.width
}

/// Pixel `y` of a squashed value in `-1.0..=1.0` (up is positive).
pub(crate) fn line_px_y(y: f32, plot: Rectangle) -> f32 {
    plot.y + plot.height * (1.0 - y.clamp(-1.0, 1.0)) / 2.0
}

/// The ply under the pixel `px` in the line style, from 1 up.
pub(crate) fn line_ply_at_px(px: f32, plies: usize, plot: Rectangle) -> Option<usize> {
    line_click_ply_at_px(px, plies, plot).filter(|p| *p >= 1)
}

/// Like [`line_ply_at_px`] but ply 0 (the start position) counts: a click or
/// a drag can go back to it (D1556).
pub(crate) fn line_click_ply_at_px(px: f32, plies: usize, plot: Rectangle) -> Option<usize> {
    let x = (px - plot.x) / plot.width * plies as f32;
    nearest_ply(x, plies)
}

/// The curve of a segment as points in pixels (the Bezier sampled).
fn sampled(segment: &CurveSegment, plies: usize, plot: Rectangle) -> Vec<Point> {
    const STEPS: usize = 12;
    (0..=STEPS)
        .map(|k| {
            let (x, y) = segment.at(k as f32 / STEPS as f32);
            Point::new(line_px_x(x, plies, plot), line_px_y(y, plot))
        })
        .collect()
}

// ---- the canvas ----

struct GraphCanvas<'a> {
    view: &'a ScoreGraphView,
    current: Option<f32>,
    ui: &'a GraphUi,
    i18n: &'a I18n,
    /// The canvas of the report panel: a click goes through the report.
    report: bool,
}

impl GraphCanvas<'_> {
    fn is_score(&self) -> bool {
        matches!(self.view.kind, GraphKind::Score | GraphKind::ScoreCombo)
    }

    fn line(&self) -> Option<&LineGraphView> {
        self.view.line.as_ref()
    }

    /// The ply of the current move in the line style.
    fn current_ply(&self) -> Option<usize> {
        let x = self.current?;
        // Inverse of `x_of_ply`; the bar x of a ply is `ply / 2 + 0.5`.
        let ply = ((x - 0.5) * 2.0).round();
        // Checked `>= 1` and capped: the cast cannot truncate or change sign.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let index = ply.min(1.0e7) as usize;
        (ply >= 1.0).then_some(index)
    }

    /// The plies that the mouse is over, in the plot only.
    fn hovered_ply(&self, bounds: Rectangle, cursor: mouse::Cursor) -> Option<usize> {
        let plot = plot_rect(bounds.size())?;
        let at = cursor.position_in(bounds)?;
        if !plot.contains(at) {
            return None;
        }
        if let Some(line) = self.line() {
            return line_ply_at_px(at.x, line.plies, plot);
        }
        let x = x_at_px(&self.view.x_axis, at.x, plot);
        ply_at_x(&self.view.x_axis, x, self.view.plies)
    }

    fn draw_labels(&self, frame: &mut Frame, plot: Rectangle, c: &GraphColors) {
        let view = self.view;
        for x in view.x_axis.label_ticks().take(MAX_LABELS) {
            frame.fill_text(Text {
                content: tick_text(x),
                position: Point::new(px_x(&view.x_axis, x, plot), plot.y + plot.height + 3.0),
                color: c.label,
                size: Pixels(LABEL_SIZE),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Top,
                ..Text::default()
            });
        }
        for y in view.y_axis.label_ticks().take(MAX_LABELS) {
            frame.fill_text(Text {
                content: tick_text(y),
                position: Point::new(plot.x - 4.0, px_y(&view.y_axis, y, plot)),
                color: c.label,
                size: Pixels(LABEL_SIZE),
                align_x: iced::alignment::Horizontal::Right.into(),
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            });
        }
    }

    /// The line style: grid, filled areas, curve, gaps and markers (D1401-D1404,
    /// D1408). The sides are told apart by the side of the zero line and by
    /// the two fills (light above, dark below), not by hue alone (D1411).
    #[allow(clippy::unused_self)] // kept with its sibling `draw_static`
    fn draw_line_static(
        &self,
        frame: &mut Frame,
        plot: Rectangle,
        line: &LineGraphView,
        c: &GraphColors,
    ) {
        let plies = line.plies;
        frame.fill_rectangle(plot.position(), plot.size(), c.background);
        let zero_y = line_px_y(0.0, plot);
        for (eval, y) in grid_lines() {
            let py = line_px_y(y, plot);
            let color = if eval == 0.0 { c.zero } else { c.minor };
            let width = if eval == 0.0 { 0.5 } else { 1.0 };
            frame.stroke(
                &Path::line(Point::new(plot.x, py), Point::new(plot.x + plot.width, py)),
                Stroke::default().with_width(width).with_color(color),
            );
            frame.fill_text(Text {
                content: tick_text(eval),
                position: Point::new(plot.x - 4.0, py),
                color: c.label,
                size: Pixels(LABEL_SIZE),
                align_x: iced::alignment::Horizontal::Right.into(),
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            });
        }
        for ply in move_ticks(plies, MAX_LABELS / 4) {
            let px = line_px_x(ply as f32, plies, plot);
            frame.fill_text(Text {
                content: (ply / 2).to_string(),
                position: Point::new(px, plot.y + plot.height + 3.0),
                color: c.label,
                size: Pixels(LABEL_SIZE),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Top,
                ..Text::default()
            });
        }
        // The areas between the curve and the zero line.
        for segment in &line.segments {
            let points = sampled(segment, plies, plot);
            for (above, color) in [(true, c.white_line), (false, c.black_line)] {
                let path = Path::new(|b| {
                    let (first, last) = (points[0], points[points.len() - 1]);
                    b.move_to(Point::new(first.x, zero_y));
                    for p in &points {
                        let y = if above {
                            p.y.min(zero_y)
                        } else {
                            p.y.max(zero_y)
                        };
                        b.line_to(Point::new(p.x, y));
                    }
                    b.line_to(Point::new(last.x, zero_y));
                    b.close();
                });
                let alpha = if segment.gap { 0.2 } else { 0.5 };
                frame.fill(&path, Fill::from(color.scale_alpha(alpha)));
            }
        }
        // The curve: solid, dashed and faint across gaps (D1404).
        for segment in &line.segments {
            let points = sampled(segment, plies, plot);
            let path = Path::new(|b| {
                for (i, p) in points.iter().enumerate() {
                    if i == 0 {
                        b.move_to(*p);
                    } else {
                        b.line_to(*p);
                    }
                }
            });
            let mut stroke = Stroke::default().with_width(2.0).with_color(c.bar);
            if segment.gap {
                stroke.line_dash = LineDash {
                    segments: &[4.0, 4.0],
                    offset: 0,
                };
                stroke = stroke.with_color(c.bar.scale_alpha(0.6));
            }
            frame.stroke(&path, stroke);
        }
        // A lone point has no segment: show it.
        if let [only] = line.points.as_slice() {
            let at = Point::new(
                line_px_x(only.ply as f32, plies, plot),
                line_px_y(only.y, plot),
            );
            frame.fill(&Path::circle(at, 3.0), c.bar);
        }
        for marker in &line.markers {
            if let Some(point) = line.point_at(marker.ply) {
                let at = Point::new(
                    line_px_x(marker.ply as f32, plies, plot),
                    line_px_y(point.y, plot),
                );
                frame.fill(&Path::circle(at, 4.5), c.marker(marker.kind));
                frame.stroke(
                    &Path::circle(at, 4.5),
                    Stroke::default().with_width(1.0).with_color(c.background),
                );
            }
        }
        frame.stroke_rectangle(
            plot.position(),
            plot.size(),
            Stroke::default().with_width(1.0).with_color(c.label),
        );
    }

    fn draw_static(&self, frame: &mut Frame, plot: Rectangle, c: &GraphColors) {
        if let Some(line) = self.line() {
            return self.draw_line_static(frame, plot, line, c);
        }
        let view = self.view;
        frame.fill_rectangle(plot.position(), plot.size(), c.background);
        let line = |frame: &mut Frame, from: Point, to: Point, color: Color| {
            frame.stroke(
                &Path::line(from, to),
                Stroke::default().with_width(1.0).with_color(color),
            );
        };
        // The grid: light lines, then the highlighted ones. Each set is one
        // path with a sub-path per line (up to `MAX_TICKS` of them), so the
        // canvas gets two strokes instead of thousands.
        let grid = |frame: &mut Frame,
                    xs: &mut dyn Iterator<Item = f32>,
                    ys: &mut dyn Iterator<Item = f32>,
                    color: Color| {
            let path = Path::new(|b| {
                for x in xs {
                    let px = px_x(&view.x_axis, x, plot);
                    b.move_to(Point::new(px, plot.y));
                    b.line_to(Point::new(px, plot.y + plot.height));
                }
                for y in ys {
                    let py = px_y(&view.y_axis, y, plot);
                    b.move_to(Point::new(plot.x, py));
                    b.line_to(Point::new(plot.x + plot.width, py));
                }
            });
            frame.stroke(&path, Stroke::default().with_width(1.0).with_color(color));
        };
        grid(
            frame,
            &mut view.x_axis.minor_ticks(),
            &mut view.y_axis.minor_ticks(),
            c.minor,
        );
        grid(
            frame,
            &mut view.x_axis.major_ticks(),
            &mut view.y_axis.major_ticks(),
            c.major,
        );
        self.draw_labels(frame, plot, c);
        // The bars, and the zero line over them.
        for bar in &view.bars {
            if let Some((origin, size)) = bar_rect(&view.x_axis, &view.y_axis, *bar, plot) {
                frame.fill_rectangle(origin, size, c.bar);
            }
        }
        if (view.y_axis.min..=view.y_axis.max).contains(&0.0) {
            let py = px_y(&view.y_axis, 0.0, plot);
            line(
                frame,
                Point::new(plot.x, py),
                Point::new(plot.x + plot.width, py),
                c.zero,
            );
        }
        // The running totals of the clocks.
        for (points, color) in [
            (&view.white_line, c.white_line),
            (&view.black_line, c.black_line),
        ] {
            if points.len() < 2 {
                continue;
            }
            let path = Path::new(|b| {
                for (i, (x, y)) in points.iter().enumerate() {
                    let at = Point::new(
                        px_x(&view.x_axis, *x, plot),
                        px_y(
                            &view.y_axis,
                            y.clamp(view.y_axis.min, view.y_axis.max),
                            plot,
                        ),
                    );
                    if i == 0 {
                        b.move_to(at);
                    } else {
                        b.line_to(at);
                    }
                }
            });
            frame.stroke(&path, Stroke::default().with_width(2.0).with_color(color));
        }
        frame.stroke_rectangle(
            plot.position(),
            plot.size(),
            Stroke::default().with_width(1.0).with_color(c.label),
        );
    }

    /// The text of the hovered ply: "12.: +0.35" or "12...: 8 s".
    fn hover_text(&self, ply: usize) -> Option<String> {
        if let Some(line) = self.line() {
            let point = line.point_at(ply)?;
            let mut args = FluentArgs::new();
            args.set("move", move_label(ply));
            args.set("value", format!("{:+.2}", point.eval));
            return Some(self.i18n.tr_args("graph-hover-score", &args));
        }
        let value = bar_at_ply(&self.view.bars, ply)?;
        let mut args = FluentArgs::new();
        args.set("move", move_label(ply));
        if self.is_score() {
            args.set("value", format!("{value:+.2}"));
            Some(self.i18n.tr_args("graph-hover-score", &args))
        } else {
            args.set("value", tick_text(value));
            Some(self.i18n.tr_args("graph-hover-time", &args))
        }
    }
}

impl canvas::Program<Message> for GraphCanvas<'_> {
    /// Whether the left button is held over the plot (dragging, D1407).
    type State = bool;

    fn update(
        &self,
        dragging: &mut bool,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        // The click of a ply, in the bar x the application understands.
        let goto = |ply: usize| {
            if self.report {
                return Action::publish(Message::App(AppMessage::Report(
                    mychess_app::report::ReportMessage::GoTo(ply),
                )));
            }
            Action::publish(Message::App(AppMessage::Graph(GraphMessage::GoToPly(ply))))
        };
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let plot = plot_rect(bounds.size())?;
                let at = cursor.position_in(bounds).filter(|p| plot.contains(*p))?;
                if let Some(line) = self.line() {
                    *dragging = true;
                    let ply = line_click_ply_at_px(at.x, line.plies, plot)?;
                    return Some(goto(ply).and_capture());
                }
                let x = x_at_px(&self.view.x_axis, at.x, plot);
                Some(
                    Action::publish(Message::App(AppMessage::Graph(GraphMessage::Click(x))))
                        .and_capture(),
                )
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                *dragging = false;
                None
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if *dragging => {
                let plot = plot_rect(bounds.size())?;
                let at = cursor.position_in(bounds).filter(|p| plot.contains(*p))?;
                let ply = match self.line() {
                    Some(line) => line_click_ply_at_px(at.x, line.plies, plot)?,
                    None => self.hovered_ply(bounds, cursor)?,
                };
                Some(goto(ply).and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                Some(Action::request_redraw())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &bool,
        renderer: &iced::Renderer,
        theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let Some(plot) = plot_rect(bounds.size()) else {
            return Vec::new();
        };
        let tokens = Tokens::of(theme);
        let colors = GraphColors::of(&tokens);
        if self.ui.palette.replace(Some(colors)) != Some(colors) {
            self.ui.cache.clear();
        }
        let base = self.ui.cache.draw(renderer, bounds.size(), |frame| {
            self.draw_static(frame, plot, &colors);
        });
        let mut over = Frame::new(renderer, bounds.size());
        let view = self.view;
        if let Some(line) = self.line() {
            if let Some(ply) = self.current_ply() {
                let px = line_px_x(ply as f32, line.plies, plot);
                over.stroke(
                    &Path::line(Point::new(px, plot.y), Point::new(px, plot.y + plot.height)),
                    Stroke::default().with_width(1.5).with_color(colors.current),
                );
                if let Some(point) = line.point_at(ply) {
                    let at = Point::new(px, line_px_y(point.y, plot));
                    over.fill(&Path::circle(at, 4.0), colors.current);
                }
            }
        } else if let Some(x) = self.current {
            let half = 0.25 * BAR_SHARE;
            let left = px_x(&view.x_axis, x - half, plot).max(plot.x);
            let right = px_x(&view.x_axis, x + half, plot).min(plot.x + plot.width);
            if right > left {
                over.fill_rectangle(
                    Point::new(left, plot.y),
                    Size::new((right - left).max(2.0), plot.height),
                    colors.current.scale_alpha(0.35),
                );
            }
        }
        if let Some(ply) = self.hovered_ply(bounds, cursor) {
            let px = match self.line() {
                Some(line) => line_px_x(ply as f32, line.plies, plot),
                None => px_x(&view.x_axis, x_of_ply(ply), plot),
            };
            over.stroke(
                &Path::line(Point::new(px, plot.y), Point::new(px, plot.y + plot.height)),
                Stroke::default().with_width(1.0).with_color(colors.current),
            );
            if let Some(content) = self.hover_text(ply) {
                over.fill_text(Text {
                    content,
                    position: Point::new(plot.x + plot.width - 4.0, plot.y + 4.0),
                    color: tokens.text,
                    size: Pixels(12.0),
                    align_x: iced::alignment::Horizontal::Right.into(),
                    align_y: iced::alignment::Vertical::Top,
                    ..Text::default()
                });
            }
        }
        vec![base, over.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &bool,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.hovered_ply(bounds, cursor).is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// The curve of the game report (spec M4-10b): the canvas of the graph
/// panel over the report's own view, with its own cache. `current` is the
/// ply of the cursor on the main line; a click goes to a ply through the
/// report.
pub(crate) fn report_curve<'a>(
    view: &'a ScoreGraphView,
    ui: &'a GraphUi,
    current: Option<usize>,
    i18n: &'a I18n,
) -> Element<'a, Message> {
    canvas::Canvas::new(GraphCanvas {
        view,
        current: current.map(x_of_ply),
        ui,
        i18n,
        report: true,
    })
    .width(Length::Fill)
    .height(Length::Fixed(150.0))
    .into()
}

// ---- the panel ----

fn kind_key(kind: GraphKind) -> &'static str {
    match kind {
        GraphKind::Auto => "graph-kind-auto",
        GraphKind::Score => "graph-kind-score",
        GraphKind::Time => "graph-kind-time",
        GraphKind::ScoreCombo => "graph-kind-score-combo",
        GraphKind::TimeCombo => "graph-kind-time-combo",
    }
}

fn option_box<'a>(
    i18n: &I18n,
    label_key: &str,
    tip_key: &str,
    on: bool,
    message: GraphMessage,
) -> Element<'a, Message> {
    let body = checkbox(on)
        .label(i18n.tr(label_key))
        .size(14)
        .text_size(12)
        .on_toggle(move |_| Message::App(AppMessage::Graph(message)));
    tooltip(
        body,
        container(text(i18n.tr(tip_key)).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Bottom,
    )
    .into()
}

fn toolbar<'a>(shell: &Shell) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let prefs = shell.app.prefs.graph;
    let options: Vec<Labeled<GraphKind>> = GraphKind::ALL
        .into_iter()
        .map(|value| Labeled {
            value,
            label: i18n.tr(kind_key(value)),
        })
        .collect();
    let current = Labeled {
        value: prefs.kind,
        label: i18n.tr(kind_key(prefs.kind)),
    };
    row![
        pick_list(options, Some(current), |choice: Labeled<GraphKind>| {
            Message::App(AppMessage::Graph(GraphMessage::SetKind(choice.value)))
        })
        .text_size(12)
        .padding(3),
        option_box(
            i18n,
            "graph-style-line",
            "graph-style-line-tip",
            prefs.style == GraphStyle::Line,
            GraphMessage::SetStyle(match prefs.style {
                GraphStyle::Line => GraphStyle::Bars,
                GraphStyle::Bars => GraphStyle::Line,
            }),
        ),
        option_box(
            i18n,
            "graph-highlight",
            "graph-highlight-tip",
            prefs.highlight_current,
            GraphMessage::ToggleHighlight,
        ),
        option_box(
            i18n,
            "graph-invert-white",
            "graph-invert-white-tip",
            prefs.invert.white,
            GraphMessage::ToggleInvertWhite,
        ),
        option_box(
            i18n,
            "graph-invert-black",
            "graph-invert-black-tip",
            prefs.invert.black,
            GraphMessage::ToggleInvertBlack,
        ),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center)
    .wrap()
    .into()
}

/// The graph panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let i18n = &shell.i18n;
    let state = shell.app.graph();
    let Some(graph) = state.view() else {
        return panes::body(text(""), "graph-pane");
    };
    let plot = canvas::Canvas::new(GraphCanvas {
        view: graph,
        current: state.current_x(),
        ui: &shell.ui.graph,
        i18n,
        report: false,
    })
    .width(Length::Fill)
    .height(Length::Fill);
    let mut body = stack![plot];
    if graph.empty {
        body = body.push(
            container(
                text(i18n.tr("graph-empty"))
                    .size(13)
                    .style(theme::muted_text),
            )
            .center(Length::Fill),
        );
    }
    panes::body(
        column![
            toolbar(shell),
            text(graph.title.clone()).size(13),
            body.width(Length::Fill).height(Length::Fill),
        ]
        .spacing(6)
        .padding(8),
        "graph-pane",
    )
}

#[cfg(test)]
mod tests;
