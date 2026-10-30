//! The rating graph panel (spec M6-4d, D3437): the rating of one or more
//! players over the years, one line per player in the palette of the Scid
//! (`RATING_PALETTE`), drawn on a `canvas` from the [`RatingPanelView`].
// Pixel math on bounded values (the application bounds the ticks).
#![allow(clippy::cast_precision_loss)]

use std::sync::Arc;

use fluent_bundle::FluentArgs;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::canvas::{self, Action, Frame, Path, Stroke};
use iced::widget::{Id, column, container, pick_list, row, space, text, text_input};
use iced::{Alignment, Element, Length, Point, Rectangle, mouse};
use mychess_app::Message as AppMessage;
use mychess_app::charts::{
    ChartsMessage, RATING_PALETTE, RatingGraphView, RatingPanelView, START_ELOS, START_YEARS,
};

use super::data_panels::{DataPanelsMessage, DataPanelsUi, RATING_PLAYER_ID};
use super::panel_kit::{self, BODY_TEXT, Choice, action, check, choice, danger, flow, muted};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::chart_colors::{ChartColors, series_color};
use crate::theme::{self, Tokens};
use crate::widgets::chart::{ArcKey, MARGINS, StaticLayer, label, plot_rect, px_x, px_y};
use crate::windows::panes;

/// Id of the body of the panel (for tests and automation).
pub const RATING_PANE_ID: &str = "rating-graph-pane";

/// Most lines the canvas draws for a grid (a guard against a bad axis).
const MAX_GRID_LINES: usize = 400;
/// A label is skipped when it would be nearer than this to the last one.
const LABEL_GAP: f32 = 36.0;
/// The pointer picks a point within this distance.
const PICK_RADIUS: f32 = 14.0;

fn charts(message: ChartsMessage) -> Message {
    Message::App(AppMessage::Charts(message))
}

fn ui(message: DataPanelsMessage) -> Message {
    Message::Panels(message)
}

/// What the cached static layer of the chart depends on.
#[derive(PartialEq)]
struct RatingKey {
    graph: ArcKey<RatingGraphView>,
    dots: bool,
    colors: ChartColors,
}

/// State of the canvas: the cached static layer.
#[derive(Default)]
pub struct RatingState {
    layer: StaticLayer<RatingKey>,
}

impl RatingState {
    /// How many times the static layer was painted (for tests).
    pub fn redraws(&self) -> u32 {
        self.layer.redraws()
    }
}

/// The canvas of the graph.
pub struct RatingChart {
    graph: Arc<RatingGraphView>,
    dots: bool,
}

impl RatingChart {
    /// The canvas of `graph`, with a dot on every point when `dots`.
    pub fn new(graph: Arc<RatingGraphView>, dots: bool) -> Self {
        Self { graph, dots }
    }
}

impl RatingChart {
    /// The closest point under `PICK_RADIUS` of the pointer: `(line, point)`.
    fn hovered(&self, bounds: Rectangle, cursor: mouse::Cursor) -> Option<(usize, usize)> {
        let plot = plot_rect(bounds.size(), MARGINS)?;
        let at = cursor.position_in(bounds).filter(|p| plot.contains(*p))?;
        let a = &self.graph.axes;
        let mut best: Option<(f32, usize, usize)> = None;
        for (l, line) in self.graph.lines.iter().enumerate() {
            for (p, (x, elo)) in line.points.iter().enumerate() {
                let px = px_x(*x, a.x_min, a.x_max, plot);
                let py = px_y(
                    f32::from(*elo),
                    f32::from(a.y_min),
                    f32::from(a.y_max),
                    plot,
                );
                let d = (px - at.x).hypot(py - at.y);
                if d <= PICK_RADIUS && best.is_none_or(|(b, _, _)| d < b) {
                    best = Some((d, l, p));
                }
            }
        }
        best.map(|(_, l, p)| (l, p))
    }
}

/// `year.month` of the x of a point (x = year + trunc((month - 1) * 100 / 12) / 100).
fn year_month(x: f32) -> (i32, i32) {
    let year = x.floor();
    let month = ((x - year) * 12.0 + 0.5).floor() + 1.0;
    // Bounded: a year of the graph and a month 1 to 12.
    #[allow(clippy::cast_possible_truncation)]
    (year as i32, (month as i32).clamp(1, 12))
}

impl RatingChart {
    /// The pixel of the point `(x, elo)`.
    fn point(&self, plot: Rectangle, x: f32, elo: u16) -> Point {
        let a = &self.graph.axes;
        Point::new(
            px_x(x, a.x_min, a.x_max, plot),
            px_y(f32::from(elo), f32::from(a.y_min), f32::from(a.y_max), plot),
        )
    }

    /// The layer that does not depend on the pointer: grid, labels, axes,
    /// lines and dots.
    #[allow(clippy::too_many_lines)] // one pass over the layers of the chart
    fn paint_static(
        &self,
        frame: &mut Frame,
        plot: Rectangle,
        tokens: &Tokens,
        colors: &ChartColors,
    ) {
        let a = &self.graph.axes;
        let (ylo, yhi) = (f32::from(a.y_min), f32::from(a.y_max));
        let stroke = |color, width| Stroke::default().with_width(width).with_color(color);
        // Horizontal lines: light every `y_minor`, blue every `y_major`.
        if a.y_minor > 0 {
            let mut last_label = f32::INFINITY;
            let mut v = a.y_min;
            for _ in 0..MAX_GRID_LINES {
                if v > a.y_max {
                    break;
                }
                let y = px_y(f32::from(v), ylo, yhi, plot);
                let major = a.y_major > 0 && v.is_multiple_of(a.y_major);
                frame.stroke(
                    &Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y)),
                    stroke(if major { colors.major } else { colors.minor }, 1.0),
                );
                if (last_label - y).abs() >= 16.0 && v.is_multiple_of(50) {
                    label(
                        frame,
                        v.to_string(),
                        Point::new(plot.x - 6.0, y),
                        (Horizontal::Right, Vertical::Center),
                        colors.label,
                    );
                    last_label = y;
                }
                v = v.saturating_add(a.y_minor);
            }
        }
        // Vertical lines and the labels of the years.
        if a.x_minor > 0.0 {
            let mut v = a.x_min;
            let mut last_label = f32::NEG_INFINITY;
            for _ in 0..MAX_GRID_LINES {
                if v > a.x_max + 1e-3 {
                    break;
                }
                let x = px_x(v, a.x_min, a.x_max, plot);
                let major = a.x_major > 0.0 && ((v - a.x_min) / a.x_major).fract().abs() < 1e-3;
                frame.stroke(
                    &Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                    stroke(if major { colors.major } else { colors.minor }, 1.0),
                );
                if a.x_labels.iter().any(|l| (l - v).abs() < 1e-3) && x - last_label >= LABEL_GAP {
                    label(
                        frame,
                        format!("{v:.0}"),
                        Point::new(x, plot.y + plot.height + 6.0),
                        (Horizontal::Center, Vertical::Top),
                        colors.label,
                    );
                    last_label = x;
                }
                v += a.x_minor;
            }
        }
        frame.stroke(
            &Path::line(
                Point::new(plot.x, plot.y),
                Point::new(plot.x, plot.y + plot.height),
            ),
            stroke(colors.axis, 1.0),
        );
        frame.stroke(
            &Path::line(
                Point::new(plot.x, plot.y + plot.height),
                Point::new(plot.x + plot.width, plot.y + plot.height),
            ),
            stroke(colors.axis, 1.0),
        );
        // The lines.
        let at = |x: f32, elo: u16| self.point(plot, x, elo);
        for line in &self.graph.lines {
            let color = series_color(tokens, line.color);
            if line.points.len() > 1 {
                let path = Path::new(|b| {
                    for (i, (x, elo)) in line.points.iter().enumerate() {
                        if i == 0 {
                            b.move_to(at(*x, *elo));
                        } else {
                            b.line_to(at(*x, *elo));
                        }
                    }
                });
                frame.stroke(&path, stroke(color, 2.0));
            }
            if self.dots || line.points.len() == 1 {
                for (x, elo) in &line.points {
                    frame.fill(&Path::circle(at(*x, *elo), 3.0), color);
                }
            }
        }
    }
}

impl canvas::Program<Message> for RatingChart {
    type State = RatingState;

    fn update(
        &self,
        _state: &mut RatingState,
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        matches!(
            event,
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft)
        )
        .then(Action::request_redraw)
    }

    fn draw(
        &self,
        state: &RatingState,
        renderer: &iced::Renderer,
        theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let Some(plot) = plot_rect(bounds.size(), MARGINS) else {
            return Vec::new();
        };
        let tokens = Tokens::of(theme);
        let colors = ChartColors::of(&tokens);
        let key = RatingKey {
            graph: ArcKey::of(&self.graph),
            dots: self.dots,
            colors,
        };
        // The static layer is cached; only the hover is drawn per frame.
        let layer = state.layer.geometry(renderer, bounds.size(), key, |frame| {
            self.paint_static(frame, plot, &tokens, &colors);
        });
        let mut frame = Frame::new(renderer, bounds.size());
        if let Some((l, p)) = self.hovered(bounds, cursor)
            && let Some(line) = self.graph.lines.get(l)
            && let Some((x, elo)) = line.points.get(p)
        {
            frame.fill(&Path::circle(self.point(plot, *x, *elo), 5.0), colors.hover);
            let (year, month) = year_month(*x);
            label(
                &mut frame,
                format!("{}: {elo} ({year}.{month:02})", line.name),
                Point::new(plot.x + plot.width - 4.0, plot.y + 4.0),
                (Horizontal::Right, Vertical::Top),
                tokens.text,
            );
        }
        vec![layer, frame.into_geometry()]
    }
}

fn swatch<'a>(rgb: [u8; 3]) -> Element<'a, Message> {
    container(space::horizontal())
        .width(Length::Fixed(10.0))
        .height(Length::Fixed(10.0))
        .style(move |theme: &iced::Theme| container::Style {
            background: Some(series_color(&Tokens::of(theme), rgb).into()),
            ..container::Style::default()
        })
        .into()
}

fn players<'a>(view: &RatingPanelView, i18n: &I18n) -> Element<'a, Message> {
    let mut chips = row![].spacing(6).align_y(Alignment::Center);
    for (i, name) in view.players.iter().enumerate() {
        let rgb = view
            .graph
            .as_ref()
            .and_then(|g| g.lines.iter().find(|l| l.name == *name))
            .map_or_else(
                || RATING_PALETTE.get(i).copied().unwrap_or([128, 128, 128]),
                |l| l.color,
            );
        let chip = row![
            swatch(rgb),
            text(name.clone()).size(BODY_TEXT),
            text("\u{d7}").size(BODY_TEXT)
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        chips = chips.push(
            iced::widget::button(chip)
                .padding([3, 8])
                .style(iced::widget::button::secondary)
                .on_press(charts(ChartsMessage::RemovePlayer(name.clone()))),
        );
    }
    if view.players.is_empty() {
        chips = chips.push(muted(i18n.tr("rating-no-players")));
    }
    flow(chips)
}

fn controls<'a>(view: &RatingPanelView, state: &DataPanelsUi, i18n: &I18n) -> Element<'a, Message> {
    let typed = !state.player_text.trim().is_empty();
    let field = text_input(&i18n.tr("rating-add-placeholder"), &state.player_text)
        .id(Id::new(RATING_PLAYER_ID))
        .on_input(|v| ui(DataPanelsMessage::PlayerEdit(v)))
        .on_submit(ui(DataPanelsMessage::AddPlayer))
        .size(BODY_TEXT)
        .padding([3, 6])
        .width(Length::Fixed(190.0));
    let mut r = row![
        field,
        action(
            i18n.tr("rating-add"),
            (typed && view.can_add).then(|| ui(DataPanelsMessage::AddPlayer)),
        ),
        action(
            i18n.tr("rating-game-players"),
            Some(charts(ChartsMessage::UseGamePlayers))
        ),
        action(
            i18n.tr("rating-clear"),
            Some(charts(ChartsMessage::ClearPlayers))
        ),
        action(
            i18n.tr("rating-refresh"),
            Some(charts(ChartsMessage::RefreshRating))
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if view.progress.is_some() {
        r = r.push(action(
            i18n.tr("rating-stop"),
            Some(charts(ChartsMessage::CancelRating)),
        ));
    }
    r = r.push(choice(
        i18n.tr("rating-options"),
        state.rating_options_open,
        Some(ui(DataPanelsMessage::ToggleRatingOptions)),
    ));
    flow(r)
}

fn options<'a>(view: &RatingPanelView, i18n: &I18n) -> Element<'a, Message> {
    let years: Vec<Choice<u16>> = START_YEARS
        .iter()
        .map(|y| Choice {
            value: *y,
            label: y.to_string(),
        })
        .collect();
    let elos: Vec<Choice<u16>> = START_ELOS
        .iter()
        .map(|e| Choice {
            value: *e,
            label: e.to_string(),
        })
        .collect();
    let year = years
        .iter()
        .find(|c| c.value == view.prefs.start_year)
        .cloned();
    let elo = elos
        .iter()
        .find(|c| c.value == view.prefs.start_elo)
        .cloned();
    let r = row![
        muted(i18n.tr("rating-start-year")),
        pick_list(years, year, |c: Choice<u16>| charts(
            ChartsMessage::SetRatingStartYear(c.value)
        ))
        .text_size(BODY_TEXT),
        muted(i18n.tr("rating-start-elo")),
        pick_list(elos, elo, |c: Choice<u16>| charts(
            ChartsMessage::SetRatingStartElo(c.value)
        ))
        .text_size(BODY_TEXT),
        check(
            i18n.tr("rating-only-filter"),
            view.prefs.only_filter,
            Some(|b| charts(ChartsMessage::SetRatingOnlyFilter(b))),
        ),
        check(
            i18n.tr("rating-show-dots"),
            view.prefs.show_dots,
            Some(|b| charts(ChartsMessage::SetRatingDots(b))),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    container(flow(r))
        .padding(8)
        .style(theme::list_header)
        .into()
}

/// The content of the panel for `view`.
pub fn content<'a>(
    view: &RatingPanelView,
    state: &DataPanelsUi,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut page = column![players(view, i18n), controls(view, state, i18n)]
        .spacing(8)
        .padding(8);
    if state.rating_options_open {
        page = page.push(options(view, i18n));
    }
    if let Some((done, total)) = view.progress {
        page = page.push(panel_kit::progress(done, total));
    }
    if view.unavailable {
        return page.push(muted(i18n.tr("rating-no-base"))).into();
    }
    if let Some(issue) = view.issue {
        page = page.push(muted(i18n.tr(issue.message_key())));
    }
    let Some(graph) = &view.graph else {
        return page.into();
    };
    if graph.empty && !view.players.is_empty() {
        page = page.push(muted(i18n.tr("rating-empty")));
    }
    if graph.skipped_entries > 0 {
        let mut args = FluentArgs::new();
        args.set("n", i64::from(graph.skipped_entries));
        page = page.push(danger(i18n.tr_args("rating-skipped", &args)));
    }
    page.push(
        canvas::Canvas::new(RatingChart::new(Arc::clone(graph), view.prefs.show_dots))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .into()
}

/// The panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let view = shell.app.rating_view();
    panes::body(
        content(&view, &shell.ui.dialogs.panels, &shell.i18n),
        RATING_PANE_ID,
    )
}
