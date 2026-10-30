//! The statistics panel of the filter (spec M6-4d, D3435, D3436): a table of
//! the results of the filter and, in two more tabs, the relative and the
//! absolute graphs (by decade, year, rating or moves) drawn on a `canvas`.
//! Everything comes from the [`StatsPanelView`] of the application.
// Pixel math on bounded values (at most 10 000 bins).
#![allow(clippy::cast_precision_loss)]

use std::sync::Arc;

use fluent_bundle::FluentArgs;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::canvas::{self, Action, Frame, Path, Stroke};
use iced::widget::{Id, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Element, Length, Point, Rectangle, mouse};
use mychess_app::Message as AppMessage;
use mychess_app::charts::{
    ChartsMessage, FilterGraphKind, FilterGraphPrefs, GraphField, GraphView, StatsLabel,
    StatsLineView, StatsPanelView, StatsTab,
};

use super::data_panels::{DataPanelsMessage, DataPanelsUi};
use super::panel_kit::{self, BODY_TEXT, action, bold, cell, check, choice, danger, flow, muted};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::chart_colors::ChartColors;
use crate::theme::{self, Tokens};
use crate::widgets::chart::{
    self, ArcKey, MARGINS, StaticLayer, axis_number, label, plot_rect, px_x, px_y,
};
use crate::windows::panes;

/// Id of the body of the panel (for tests and automation).
pub const STATS_PANE_ID: &str = "filter-stats-pane";

const LABEL_W: f32 = 200.0;
const NUMBER_W: f32 = 70.0;
const SCORE_W: f32 = 70.0;

fn charts(message: ChartsMessage) -> Message {
    Message::App(AppMessage::Charts(message))
}

fn ui(message: DataPanelsMessage) -> Message {
    Message::Panels(message)
}

/// The text of the criterion of a line of the table.
pub fn label_text(i18n: &I18n, label: StatsLabel) -> String {
    let mut args = FluentArgs::new();
    match label {
        StatsLabel::BothRated(n) => {
            args.set("n", i64::from(n));
            i18n.tr_args("stats-label-both", &args)
        }
        StatsLabel::Years { from, to: Some(to) } => {
            args.set("from", i64::from(from));
            args.set("to", i64::from(to));
            i18n.tr_args("stats-label-years", &args)
        }
        StatsLabel::Years { from, to: None } => {
            args.set("from", i64::from(from));
            i18n.tr_args("stats-label-years-open", &args)
        }
        StatsLabel::Since(year) => {
            args.set("year", i64::from(year));
            i18n.tr_args("stats-label-since", &args)
        }
        _ => i18n.tr("stats-label-all"),
    }
}

fn line_row<'a>(i18n: &I18n, index: usize, line: &StatsLineView) -> Element<'a, Message> {
    let cells = row![
        cell(label_text(i18n, line.label), LABEL_W, Horizontal::Left),
        cell(line.total.to_string(), NUMBER_W, Horizontal::Right),
        cell(line.white.to_string(), NUMBER_W, Horizontal::Right),
        cell(line.draw.to_string(), NUMBER_W, Horizontal::Right),
        cell(line.black.to_string(), NUMBER_W, Horizontal::Right),
        cell(line.score_text(), SCORE_W, Horizontal::Right),
    ]
    .spacing(8);
    container(cells)
        .padding([2, 8])
        .id(format!("stats-row-{index}"))
        .style(theme::list_row(theme::RowLook {
            zebra: index % 2 == 1,
            selected: false,
            current: false,
            dimmed: false,
        }))
        .into()
}

fn table<'a>(view: &StatsPanelView, i18n: &I18n) -> Element<'a, Message> {
    let header = container(
        row![
            cell(i18n.tr("stats-col-criterion"), LABEL_W, Horizontal::Left),
            cell(i18n.tr("stats-col-games"), NUMBER_W, Horizontal::Right),
            cell(i18n.tr("stats-col-white"), NUMBER_W, Horizontal::Right),
            cell(i18n.tr("stats-col-draw"), NUMBER_W, Horizontal::Right),
            cell(i18n.tr("stats-col-black"), NUMBER_W, Horizontal::Right),
            cell(i18n.tr("stats-col-score"), SCORE_W, Horizontal::Right),
        ]
        .spacing(8),
    )
    .padding([3, 8])
    .style(theme::list_header);
    let mut rows = column![header].spacing(1);
    for (i, line) in view.lines.iter().enumerate() {
        rows = rows.push(line_row(i18n, i, line));
    }
    let skipped = view.lines.first().map_or(0, |l| l.skipped_entries);
    let mut page = column![scrollable(rows).height(Length::Fill)].spacing(6);
    if skipped > 0 {
        let mut args = FluentArgs::new();
        args.set("n", i64::from(skipped));
        page = page.push(danger(i18n.tr_args("stats-skipped", &args)));
    }
    page.into()
}

fn stats_options<'a>(view: &StatsPanelView, i18n: &I18n) -> Element<'a, Message> {
    let Some(options) = &view.options else {
        return space::vertical().into();
    };
    let mut elo = row![muted(i18n.tr("stats-opt-both"))]
        .spacing(10)
        .align_y(Alignment::Center);
    for (n, on) in &options.elo {
        let n = *n;
        elo = elo.push(check(
            format!("{n}+"),
            *on,
            Some(move |b: bool| charts(ChartsMessage::SetStatsElo(n, b))),
        ));
    }
    let mut years = row![muted(i18n.tr("stats-opt-years"))]
        .spacing(10)
        .align_y(Alignment::Center);
    for (y, on) in &options.years {
        let y = *y;
        years = years.push(check(
            y.to_string(),
            *on,
            Some(move |b: bool| charts(ChartsMessage::SetStatsYear(y, b))),
        ));
    }
    container(
        column![
            flow(elo),
            flow(years),
            row![action(
                i18n.tr("stats-defaults"),
                Some(charts(ChartsMessage::ResetStats))
            )]
        ]
        .spacing(8),
    )
    .padding(8)
    .style(theme::list_header)
    .into()
}

// ---- the graph ----

fn axis_value(prefs: &FilterGraphPrefs, kind: FilterGraphKind, field: GraphField) -> i32 {
    match (kind, field) {
        (FilterGraphKind::Year, GraphField::Min) => prefs.year_min,
        (FilterGraphKind::Year, GraphField::Max) => prefs.year_max,
        (FilterGraphKind::Year, _) => prefs.year_step,
        (FilterGraphKind::Elo, GraphField::Min) => prefs.elo_min,
        (FilterGraphKind::Elo, GraphField::Max) => prefs.elo_max,
        (FilterGraphKind::Elo, _) => prefs.elo_step,
        (_, GraphField::Min) => prefs.moves_min,
        (_, GraphField::Max) => prefs.moves_max,
        (_, _) => prefs.moves_step,
    }
}

fn axis_field<'a>(
    view: &StatsPanelView,
    state: &DataPanelsUi,
    i18n: &I18n,
    field: GraphField,
    key: &str,
) -> Element<'a, Message> {
    let kind = view.graph_prefs.kind;
    let typed = DataPanelsUi::axis_index(kind)
        .and_then(|k| state.axis_text[k][DataPanelsUi::field_index(field)].clone());
    let value = typed.unwrap_or_else(|| {
        let v = axis_value(&view.graph_prefs, kind, field);
        // The last year 0 means "the current year" (G6).
        if v == 0 && kind == FilterGraphKind::Year && field == GraphField::Max {
            panel_kit::current_year().to_string()
        } else {
            v.to_string()
        }
    });
    row![
        muted(i18n.tr(key)),
        text_input("", &value)
            .id(Id::new(DataPanelsUi::axis_id(field)))
            .on_input(move |v| ui(DataPanelsMessage::AxisEdit(kind, field, v)))
            .on_submit(ui(DataPanelsMessage::AxisSubmit(kind, field)))
            .size(BODY_TEXT)
            .padding([2, 6])
            .width(Length::Fixed(64.0)),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .into()
}

fn graph_controls<'a>(
    view: &StatsPanelView,
    state: &DataPanelsUi,
    i18n: &I18n,
) -> Element<'a, Message> {
    let prefs = &view.graph_prefs;
    let mut kinds = row![].spacing(4).align_y(Alignment::Center);
    for (kind, key) in [
        (FilterGraphKind::Decade, "stats-kind-decade"),
        (FilterGraphKind::Year, "stats-kind-year"),
        (FilterGraphKind::Elo, "stats-kind-rating"),
        (FilterGraphKind::Moves, "stats-kind-moves"),
    ] {
        kinds = kinds.push(choice(
            i18n.tr(key),
            prefs.kind == kind,
            Some(charts(ChartsMessage::SetGraphKind(kind))),
        ));
    }
    let mut config = row![].spacing(10).align_y(Alignment::Center);
    if prefs.kind != FilterGraphKind::Decade {
        config = config
            .push(axis_field(
                view,
                state,
                i18n,
                GraphField::Min,
                "stats-axis-min",
            ))
            .push(axis_field(
                view,
                state,
                i18n,
                GraphField::Max,
                "stats-axis-max",
            ))
            .push(axis_field(
                view,
                state,
                i18n,
                GraphField::Step,
                "stats-axis-step",
            ));
    }
    if prefs.kind == FilterGraphKind::Elo {
        config = config.push(check(
            i18n.tr("stats-estimate"),
            prefs.estimate,
            Some(|b| charts(ChartsMessage::SetEstimate(b))),
        ));
    }
    config = config
        .push(check(
            i18n.tr("stats-show-dots"),
            prefs.show_dots,
            Some(|b| charts(ChartsMessage::SetGraphDots(b))),
        ))
        .push(action(
            i18n.tr("stats-defaults"),
            Some(ui(DataPanelsMessage::ResetAxes)),
        ));
    column![flow(kinds), flow(config)].spacing(6).into()
}

/// What the cached static layer of the chart depends on.
#[derive(PartialEq)]
struct FilterKey {
    graph: ArcKey<GraphView>,
    dots: bool,
    colors: ChartColors,
}

/// State of the canvas: the cached static layer.
#[derive(Default)]
pub struct FilterState {
    layer: StaticLayer<FilterKey>,
}

impl FilterState {
    /// How many times the static layer was painted (for tests).
    pub fn redraws(&self) -> u32 {
        self.layer.redraws()
    }
}

/// The canvas of a graph of the filter.
pub struct FilterGraph {
    graph: Arc<GraphView>,
    dots: bool,
    /// The language's decimal comma.
    comma: bool,
}

impl FilterGraph {
    /// The canvas of `graph`; `comma` writes decimals with a comma.
    pub fn new(graph: Arc<GraphView>, dots: bool, comma: bool) -> Self {
        Self { graph, dots, comma }
    }
}

impl FilterGraph {
    fn range(&self) -> (f32, f32) {
        (0.5, self.graph.points.len() as f32 + 0.5)
    }

    /// The index of the point under the pointer.
    fn hovered(&self, bounds: Rectangle, cursor: mouse::Cursor) -> Option<usize> {
        let plot = plot_rect(bounds.size(), MARGINS)?;
        let at = cursor.position_in(bounds).filter(|p| plot.contains(*p))?;
        let (lo, hi) = self.range();
        let x = chart::map(at.x, plot.x, plot.x + plot.width, lo, hi);
        chart::nearest((1..=self.graph.points.len()).map(|i| i as f32), x)
    }
}

impl FilterGraph {
    /// The pixel of the point `i` (0-based) with `value`.
    fn point(&self, plot: Rectangle, i: usize, value: f32) -> Point {
        let (lo, hi) = self.range();
        let g = &*self.graph;
        let top = if g.axis.max > 0.0 { g.axis.max } else { 1.0 };
        Point::new(
            px_x(i as f32 + 1.0, lo, hi, plot),
            px_y(value.clamp(0.0, top), 0.0, top, plot),
        )
    }

    /// The layer that does not depend on the pointer: grid, axes, labels,
    /// mean, line and dots.
    #[allow(
        clippy::too_many_lines,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )] // one pass over the layers of the chart
    fn paint_static(&self, frame: &mut Frame, plot: Rectangle, colors: &ChartColors) {
        let g = &*self.graph;
        let n = g.points.len();
        let (lo, hi) = self.range();
        let top = if g.axis.max > 0.0 { g.axis.max } else { 1.0 };
        let line = |from: Point, to: Point, color, width| {
            (
                Path::line(from, to),
                Stroke::default().with_width(width).with_color(color),
            )
        };
        // Horizontal grid and the labels of the y axis.
        for tick in &g.axis.ticks {
            let y = px_y(*tick, 0.0, top, plot);
            let (path, stroke) = line(
                Point::new(plot.x, y),
                Point::new(plot.x + plot.width, y),
                colors.minor,
                1.0,
            );
            frame.stroke(&path, stroke);
            label(
                frame,
                axis_number(*tick, g.axis.step),
                Point::new(plot.x - 6.0, y),
                (Horizontal::Right, Vertical::Center),
                colors.label,
            );
        }
        // Light vertical lines between the bins (thinned when dense).
        let stride = if n > 0 && plot.width / (n as f32) < 6.0 {
            ((6.0 / (plot.width / n as f32)).ceil() as usize).max(1)
        } else {
            1
        };
        for i in (0..=n).step_by(stride) {
            let x = px_x(i as f32 + 0.5, lo, hi, plot);
            let (path, stroke) = line(
                Point::new(x, plot.y),
                Point::new(x, plot.y + plot.height),
                colors.minor,
                1.0,
            );
            frame.stroke(&path, stroke);
        }
        for index in &g.blue_lines {
            let x = px_x(*index as f32 + 0.5, lo, hi, plot);
            let (path, stroke) = line(
                Point::new(x, plot.y),
                Point::new(x, plot.y + plot.height),
                colors.major,
                1.5,
            );
            frame.stroke(&path, stroke);
        }
        // The axes.
        let (path, stroke) = line(
            Point::new(plot.x, plot.y),
            Point::new(plot.x, plot.y + plot.height),
            colors.axis,
            1.0,
        );
        frame.stroke(&path, stroke);
        let (path, stroke) = line(
            Point::new(plot.x, plot.y + plot.height),
            Point::new(plot.x + plot.width, plot.y + plot.height),
            colors.axis,
            1.0,
        );
        frame.stroke(&path, stroke);
        // The mean.
        if let Some(mean) = g.mean {
            let y = px_y(mean, 0.0, top, plot).clamp(plot.y, plot.y + plot.height);
            let (path, stroke) = line(
                Point::new(plot.x, y),
                Point::new(plot.x + plot.width, y),
                colors.mean,
                2.0,
            );
            frame.stroke(&path, stroke);
        }
        // The labels of the x axis, skipped when they would overlap.
        let mut right_edge = f32::NEG_INFINITY;
        for (i, point) in g.points.iter().enumerate() {
            if point.label.is_empty() {
                continue;
            }
            let x = px_x(i as f32 + 1.0, lo, hi, plot);
            let half = point.label.chars().count() as f32 * 3.5;
            if x - half >= right_edge + 4.0 {
                label(
                    frame,
                    point.label.clone(),
                    Point::new(x, plot.y + plot.height + 6.0),
                    (Horizontal::Center, Vertical::Top),
                    colors.label,
                );
                right_edge = x + half;
            }
        }
        // The line and the dots.
        let at = |i: usize, value: f32| self.point(plot, i, value);
        if n > 1 {
            let path = Path::new(|b| {
                for (i, point) in g.points.iter().enumerate() {
                    if i == 0 {
                        b.move_to(at(i, point.value));
                    } else {
                        b.line_to(at(i, point.value));
                    }
                }
            });
            frame.stroke(
                &path,
                Stroke::default().with_width(2.0).with_color(colors.line),
            );
        }
        if self.dots || n == 1 {
            for (i, point) in g.points.iter().enumerate() {
                frame.fill(&Path::circle(at(i, point.value), 3.0), colors.line);
            }
        }
    }
}

impl canvas::Program<Message> for FilterGraph {
    type State = FilterState;

    fn update(
        &self,
        _state: &mut FilterState,
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
        state: &FilterState,
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
        let key = FilterKey {
            graph: ArcKey::of(&self.graph),
            dots: self.dots,
            colors,
        };
        // The static layer is cached; only the hover is drawn per frame.
        let layer = state.layer.geometry(renderer, bounds.size(), key, |frame| {
            self.paint_static(frame, plot, &colors);
        });
        let mut frame = Frame::new(renderer, bounds.size());
        let g = &*self.graph;
        if let Some(i) = self.hovered(bounds, cursor)
            && let Some(point) = g.points.get(i)
        {
            let p = self.point(plot, i, point.value);
            frame.fill(&Path::circle(p, 5.0), colors.hover);
            let value = if g.relative {
                format!("{:.1}", point.value)
            } else {
                format!("{:.0}", point.value)
            };
            let value = if self.comma {
                value.replace('.', ",")
            } else {
                value
            };
            label(
                &mut frame,
                format!("{}: {value}", point.label),
                Point::new(plot.x + plot.width - 4.0, plot.y + 4.0),
                (Horizontal::Right, Vertical::Top),
                tokens.text,
            );
        }
        vec![layer, frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &FilterState,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}

fn graph_body<'a>(view: &StatsPanelView, i18n: &I18n) -> Element<'a, Message> {
    let Some(graph) = &view.graph else {
        return column![muted(i18n.tr("stats-computing"))].into();
    };
    let mut args = FluentArgs::new();
    args.set("n", i64::from(view.filter_games));
    args.set("m", i64::from(view.base_games));
    let mut page = column![
        text(i18n.tr(graph.title_key)).size(14).font(bold()),
        canvas::Canvas::new(FilterGraph::new(
            Arc::clone(graph),
            view.graph_prefs.show_dots,
            i18n.language() == crate::i18n::Language::PtBr,
        ))
        .width(Length::Fill)
        .height(Length::Fill),
        muted(i18n.tr_args("stats-filter-count", &args)),
    ]
    .spacing(4);
    if graph.unreadable > 0 {
        let mut args = FluentArgs::new();
        args.set("n", i64::from(graph.unreadable));
        page = page.push(danger(i18n.tr_args("stats-skipped", &args)));
    }
    page.into()
}

/// The content of the panel for `view`.
pub fn content<'a>(
    view: &StatsPanelView,
    state: &DataPanelsUi,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut tabs = row![].spacing(4).align_y(Alignment::Center);
    for (tab, key) in [
        (StatsTab::Table, "stats-tab-table"),
        (StatsTab::Relative, "stats-tab-relative"),
        (StatsTab::Absolute, "stats-tab-absolute"),
    ] {
        tabs = tabs.push(choice(
            i18n.tr(key),
            view.tab == tab,
            Some(charts(ChartsMessage::SelectTab(tab))),
        ));
    }
    tabs = tabs.push(action(
        i18n.tr("stats-refresh"),
        Some(charts(ChartsMessage::RefreshStats)),
    ));
    if view.progress.is_some() {
        tabs = tabs.push(action(
            i18n.tr("stats-stop"),
            Some(charts(ChartsMessage::CancelStats)),
        ));
    }
    if view.tab == StatsTab::Table {
        tabs = tabs.push(choice(
            i18n.tr("stats-options"),
            state.stats_options_open,
            Some(ui(DataPanelsMessage::ToggleStatsOptions)),
        ));
    }
    let mut page = column![flow(tabs)].spacing(8).padding(8);
    if let Some((done, total)) = view.progress {
        page = page.push(panel_kit::progress(done, total));
    }
    if view.unavailable {
        return page.push(muted(i18n.tr("stats-no-base"))).into();
    }
    if let Some(issue) = view.issue {
        page = page.push(muted(i18n.tr(issue.message_key())));
    }
    if view.tab == StatsTab::Table {
        if state.stats_options_open {
            page = page.push(stats_options(view, i18n));
        }
        page = page.push(table(view, i18n));
    } else {
        page = page.push(graph_controls(view, state, i18n));
        page = page.push(graph_body(view, i18n));
    }
    page.into()
}

/// The panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let view = shell.app.stats_view();
    panes::body(
        content(&view, &shell.ui.dialogs.panels, &shell.i18n),
        STATS_PANE_ID,
    )
}
