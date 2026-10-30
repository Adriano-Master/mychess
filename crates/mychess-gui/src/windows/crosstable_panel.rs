//! The crosstable panel (spec M6-4d, D3430-D3434): the table of the
//! tournament of the game on screen, drawn with widgets from the
//! [`CrosstableView`] that the application offers. A click on a result
//! selects that game (a strip offers "Browse" and "Load"), a click on a round
//! sets the filter, and the headers sort.

use std::sync::Arc;

use fluent_bundle::FluentArgs;
use iced::alignment::Horizontal;
use iced::widget::{
    Id, button, column, container, row, scrollable, space, text, text_input, tooltip,
};
use iced::{Alignment, Color, Element, Length};
use mychess_app::Message as AppMessage;
use mychess_app::crosstable::{
    ClashView, CrossCellView, CrossFlag, CrossFormat, CrossLabels, CrossMode, CrossOutcome,
    CrossRowView, CrossSort, CrossTableView, CrossWarning, CrosstableBody, CrosstableMessage,
    CrosstableView, ExportFormat,
};
use mychess_core::{Color as Side, PartialDate};

use super::data_panels::{CROSS_COLOR_ID, DataPanelsMessage, DataPanelsUi};
use super::panel_kit::{self, BODY_TEXT, action, bold, cell, check, choice, danger, flow, muted};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, Tokens};
use crate::windows::panes;

/// Id of the body of the panel (for tests and automation).
pub const CROSSTABLE_PANE_ID: &str = "crosstable-pane";

const NUMBER_W: f32 = 30.0;
const NAME_W: f32 = 170.0;
const ELO_W: f32 = 46.0;
const TITLE_W: f32 = 34.0;
const AGE_W: f32 = 36.0;
const NATION_W: f32 = 44.0;
const SCORE_W: f32 = 76.0;
const TIE_W: f32 = 56.0;
const CLASH_W: f32 = 22.0;
const ROUND_W: f32 = 64.0;
const PERF_W: f32 = 84.0;
const TALLY_W: f32 = 90.0;

fn app(message: CrosstableMessage) -> Message {
    Message::App(AppMessage::Crosstable(message))
}

fn ui(message: DataPanelsMessage) -> Message {
    Message::Panels(message)
}

/// The translated words of the table, sent to the application so the
/// exported files and the header use them (D3413).
pub fn labels(i18n: &I18n) -> CrossLabels {
    CrossLabels {
        player: i18n.tr("crosstab-col-player"),
        rating: i18n.tr("crosstab-col-rating"),
        title: i18n.tr("crosstab-col-title"),
        age: i18n.tr("crosstab-col-age"),
        nation: i18n.tr("crosstab-col-nation"),
        score: i18n.tr("crosstab-col-score"),
        tie: i18n.tr("crosstab-col-tie"),
        performance: i18n.tr("crosstab-col-performance"),
        change: i18n.tr("crosstab-col-change"),
        tallies: i18n.tr("crosstab-col-tallies"),
        average_rating: i18n.tr("crosstab-average-rating"),
        category: i18n.tr("crosstab-category"),
        games: i18n.tr("crosstab-games-word"),
    }
}

/// `2020.05` or `2020.05.01 - 2020.06.30`: the dates of the tournament with
/// the unknown tail cut (`.??`).
pub fn date_range_text(first: PartialDate, last: PartialDate) -> String {
    fn cut(date: PartialDate) -> String {
        let mut text = date.to_string();
        while text.ends_with(".??") {
            text.truncate(text.len() - 3);
        }
        if text == "????" { String::new() } else { text }
    }
    let (a, b) = (cut(first), cut(last));
    if a == b || b.is_empty() {
        a
    } else if a.is_empty() {
        b
    } else {
        format!("{a} - {b}")
    }
}

fn outcome_color(t: &Tokens, outcome: CrossOutcome) -> Color {
    match outcome {
        CrossOutcome::Win => t.nag.good,
        CrossOutcome::Loss => t.danger,
        CrossOutcome::Draw => t.text,
        CrossOutcome::Unknown => t.muted,
    }
}

/// The symbol of a result in the all-play-all grid.
fn grid_symbol(outcome: CrossOutcome) -> &'static str {
    match outcome {
        CrossOutcome::Win => "1",
        CrossOutcome::Loss => "0",
        CrossOutcome::Draw => "\u{bd}",
        CrossOutcome::Unknown => "*",
    }
}

/// The symbol of a result in the Swiss grid.
fn swiss_symbol(outcome: CrossOutcome) -> &'static str {
    match outcome {
        CrossOutcome::Win => "+",
        CrossOutcome::Loss => "-",
        CrossOutcome::Draw => "=",
        CrossOutcome::Unknown => "*",
    }
}

fn clash_button<'a>(
    label: String,
    clash: &ClashView,
    selected: Option<u32>,
) -> Element<'a, Message> {
    let outcome = clash.outcome;
    let game = clash.game;
    let marked = clash.current || selected == Some(game);
    let content = text(label)
        .size(BODY_TEXT)
        .style(move |theme: &iced::Theme| text::Style {
            color: Some(outcome_color(&Tokens::of(theme), outcome)),
        });
    button(content)
        .padding([1, 3])
        .style(move |theme: &iced::Theme, status| {
            let t = Tokens::of(theme);
            let background = if marked {
                Some(theme::tokens::mix(t.accent, t.panel, 0.28).into())
            } else if status == button::Status::Hovered {
                Some(t.hover.into())
            } else {
                None
            };
            button::Style {
                background,
                text_color: t.text,
                border: iced::Border {
                    radius: t.radius.chip.into(),
                    ..iced::Border::default()
                },
                ..button::Style::default()
            }
        })
        .on_press(ui(DataPanelsMessage::SelectGame(Some(game))))
        .into()
}

fn header_cell<'a>(
    label: &str,
    width: f32,
    align: Horizontal,
    sort: Option<CrossSort>,
) -> Element<'a, Message> {
    let content = cell(label.to_owned(), width, align);
    match sort {
        Some(sort) => button(content)
            .padding(0)
            .style(button::text)
            .on_press(app(CrosstableMessage::SetSort(sort)))
            .into(),
        None => content.into(),
    }
}

fn shorten(name: &str, width: f32) -> String {
    // About 7 logical pixels a character at the size of the table.
    let room = (width / 7.0).floor().max(2.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let room = room as usize;
    name.chars().take(room).collect()
}

struct Grid {
    table: Arc<CrossTableView>,
    labels: CrossLabels,
    colors: bool,
    row_color: Option<Color>,
    swiss_colors: bool,
    numbered: bool,
    selected: Option<u32>,
    /// Pixels the grid is scrolled down.
    scroll: f32,
    /// The height of the viewport (`0.0`: unknown).
    view_height: f32,
}

/// The height of the header of the grid.
pub const HEADER_H: f32 = 28.0;
/// The height of a row of the grid.
pub const ROW_H: f32 = 26.0;
/// The gap under each row.
const ROW_GAP: f32 = 1.0;
/// The space above the first row of a group (D3419).
const GROUP_GAP: f32 = 8.0;
/// The viewport height assumed until the grid is scrolled once.
const MIN_VIEW_H: f32 = 1200.0;
/// Pixels built above and below the viewport.
const MARGIN_PX: f32 = 120.0;

/// The height a row takes in the grid, gap included.
pub fn slot_height(group_break: bool) -> f32 {
    ROW_H + ROW_GAP + if group_break { GROUP_GAP } else { 0.0 }
}

/// The rows to build for a grid whose rows take `heights` pixels, scrolled
/// `scroll` pixels down (the header takes [`HEADER_H`]) in a viewport
/// `view_height` high: the first row, how many, the pixels above them and
/// the pixels below.
pub fn row_window(heights: &[f32], scroll: f32, view_height: f32) -> (usize, usize, f32, f32) {
    let total: f32 = heights.iter().sum();
    let view = view_height.max(MIN_VIEW_H);
    let scroll = if scroll.is_finite() { scroll } else { 0.0 };
    let scroll = scroll.clamp(0.0, (HEADER_H + total - view).max(0.0));
    let from = scroll - HEADER_H - MARGIN_PX;
    let to = scroll - HEADER_H + view + MARGIN_PX;
    let mut top = 0.0;
    let mut before = 0.0;
    let mut first = None;
    let mut count = 0;
    let mut built = 0.0;
    for (i, height) in heights.iter().enumerate() {
        let bottom = top + height;
        if bottom > from && top < to {
            if first.is_none() {
                first = Some(i);
                before = top;
            }
            count += 1;
            built += height;
        }
        top = bottom;
    }
    match first {
        Some(first) => (first, count, before, (total - before - built).max(0.0)),
        None => (heights.len(), 0, total, 0.0),
    }
}

impl Grid {
    fn clash_width(&self) -> f32 {
        let per = u16::try_from(self.table.max_clashes.max(1)).unwrap_or(1);
        CLASH_W * f32::from(per)
    }

    fn header<'a>(&self) -> Element<'a, Message> {
        let t = &*self.table;
        let l = &self.labels;
        let c = t.columns;
        let mut r = row![
            cell("#".into(), NUMBER_W, Horizontal::Right),
            header_cell(&l.player, NAME_W, Horizontal::Left, Some(CrossSort::Name)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if c.ratings {
            r = r.push(header_cell(
                &l.rating,
                ELO_W,
                Horizontal::Right,
                Some(CrossSort::Rating),
            ));
        }
        if c.titles {
            r = r.push(header_cell(&l.title, TITLE_W, Horizontal::Left, None));
        }
        if c.ages {
            r = r.push(header_cell(&l.age, AGE_W, Horizontal::Right, None));
        }
        if c.countries {
            r = r.push(header_cell(
                &l.nation,
                NATION_W,
                Horizontal::Left,
                Some(CrossSort::Country),
            ));
        }
        r = r.push(header_cell(
            &l.score,
            SCORE_W,
            Horizontal::Right,
            Some(CrossSort::Score),
        ));
        if c.tiebreaks {
            r = r.push(cell(l.tie.clone(), TIE_W, Horizontal::Right));
        }
        match t.mode {
            CrossMode::AllPlayAll => {
                let width = self.clash_width();
                for (i, player) in t.rows.iter().enumerate() {
                    let label = if self.numbered {
                        player.rank.to_string()
                    } else {
                        shorten(&player.name, width)
                    };
                    let head = cell(label, width, Horizontal::Center);
                    let head: Element<'a, Message> = if self.numbered {
                        head.into()
                    } else {
                        tooltip(
                            head,
                            text(format!("{}: {}", i + 1, player.name)).size(12),
                            tooltip::Position::Bottom,
                        )
                        .style(theme::tooltip_box)
                        .into()
                    };
                    r = r.push(head);
                }
            }
            CrossMode::Swiss => {
                for round in 1..=t.max_round {
                    r = r.push(
                        button(cell(round.to_string(), ROUND_W, Horizontal::Center))
                            .padding(0)
                            .style(button::text)
                            .on_press(app(CrosstableMessage::SetFilter(Some(round)))),
                    );
                }
            }
            CrossMode::Knockout => {}
        }
        if c.ratings {
            r = r.push(cell(
                format!("{} {}", l.performance, l.change),
                PERF_W,
                Horizontal::Right,
            ));
        }
        if c.tallies {
            r = r.push(cell("+ - =".into(), TALLY_W, Horizontal::Left));
        }
        container(r)
            .padding([3, 8])
            .style(theme::list_header)
            .into()
    }

    fn cell_of<'a>(&self, player: &CrossRowView, index: usize) -> Element<'a, Message> {
        let t = &*self.table;
        match t.mode {
            CrossMode::AllPlayAll => {
                let width = self.clash_width();
                match player.cells.get(index) {
                    Some(CrossCellView::Games(clashes)) if !clashes.is_empty() => {
                        let mut r = row![].spacing(0);
                        for clash in clashes {
                            r = r.push(clash_button(
                                grid_symbol(clash.outcome).to_owned(),
                                clash,
                                self.selected,
                            ));
                        }
                        container(r)
                            .width(Length::Fixed(width))
                            .center_x(Length::Fixed(width))
                            .into()
                    }
                    _ => cell(".".into(), width, Horizontal::Center)
                        .style(theme::muted_text)
                        .into(),
                }
            }
            _ => match player.cells.get(index) {
                Some(CrossCellView::Round(Some(clash))) => {
                    let opponent = t.rows.get(clash.opponent).map_or(0, |o| o.rank);
                    let side = if self.swiss_colors {
                        match clash.color {
                            Side::White => "w",
                            Side::Black => "b",
                        }
                    } else {
                        ""
                    };
                    let label = format!("{opponent}{side}{}", swiss_symbol(clash.outcome));
                    container(clash_button(label, clash, self.selected))
                        .width(Length::Fixed(ROUND_W))
                        .center_x(Length::Fixed(ROUND_W))
                        .into()
                }
                _ => cell("....".into(), ROUND_W, Horizontal::Center)
                    .style(theme::muted_text)
                    .into(),
            },
        }
    }

    fn row<'a>(&self, i: usize, player: &CrossRowView) -> Element<'a, Message> {
        let c = self.table.columns;
        let mut r = row![
            cell(format!("{}:", player.rank), NUMBER_W, Horizontal::Right),
            cell(player.name.clone(), NAME_W, Horizontal::Left),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if c.ratings {
            let elo = if player.elo > 0 {
                player.elo.to_string()
            } else {
                String::new()
            };
            r = r.push(cell(elo, ELO_W, Horizontal::Right));
        }
        if c.titles {
            r = r.push(cell(player.title.clone(), TITLE_W, Horizontal::Left));
        }
        if c.ages {
            let age = player.age.map(|a| a.to_string()).unwrap_or_default();
            r = r.push(cell(age, AGE_W, Horizontal::Right));
        }
        if c.countries {
            r = r.push(cell(player.country.clone(), NATION_W, Horizontal::Left));
        }
        r = r.push(cell(
            format!("{} / {}", player.score, player.games),
            SCORE_W,
            Horizontal::Right,
        ));
        if c.tiebreaks {
            r = r.push(cell(player.tiebreak.clone(), TIE_W, Horizontal::Right));
        }
        for index in 0..player.cells.len() {
            r = r.push(self.cell_of(player, index));
        }
        if c.ratings {
            let perf = player
                .performance
                .map_or_else(String::new, |p| match p.change {
                    Some(change) => format!("{:4} {:+3}", p.rating, change),
                    None => format!("{:4}", p.rating),
                });
            r = r.push(cell(perf, PERF_W, Horizontal::Right));
        }
        if c.tallies {
            r = r.push(cell(
                format!("(+{} -{} ={})", player.won, player.lost, player.drawn),
                TALLY_W,
                Horizontal::Left,
            ));
        }
        let tint = if self.colors && i % 2 == 1 {
            self.row_color
        } else {
            None
        };
        let line = container(r)
            .padding([2, 8])
            .height(Length::Fixed(ROW_H))
            .style(move |_theme: &iced::Theme| container::Style {
                background: tint.map(|c| c.scale_alpha(0.30).into()),
                ..container::Style::default()
            });
        let slot = slot_height(player.group_break);
        let content: Element<'a, Message> = if player.group_break {
            column![space::vertical().height(GROUP_GAP), line].into()
        } else {
            line.into()
        };
        container(content).height(Length::Fixed(slot)).into()
    }

    fn knockout<'a>(&self) -> Element<'a, Message> {
        let t = &*self.table;
        let name = |i: usize| t.rows.get(i).map(|r| r.name.clone()).unwrap_or_default();
        let mut blocks = column![].spacing(6);
        for round in &t.knockout {
            blocks = blocks.push(
                button(
                    text(format!("{}:", round.round))
                        .size(BODY_TEXT)
                        .font(bold()),
                )
                .padding([2, 8])
                .style(button::text)
                .on_press(app(CrosstableMessage::SetFilter(Some(round.round)))),
            );
            for m in &round.matches {
                let mut r = row![cell(name(m.first), NAME_W, Horizontal::Left)]
                    .spacing(4)
                    .align_y(Alignment::Center);
                for clash in &m.games {
                    r = r.push(clash_button(
                        grid_symbol(clash.outcome).to_owned(),
                        clash,
                        self.selected,
                    ));
                }
                r = r.push(cell(
                    format!(" {} / {}", m.score, m.games.len()),
                    SCORE_W,
                    Horizontal::Right,
                ));
                r = r.push(cell(name(m.second), NAME_W, Horizontal::Left));
                blocks = blocks.push(container(r).padding([0, 16]));
            }
        }
        blocks.into()
    }

    fn view<'a>(&self) -> Element<'a, Message> {
        if self.table.mode == CrossMode::Knockout {
            return self.knockout();
        }
        // Only the rows near the viewport become widgets: a table of 300
        // players has 90 000 cells.
        let heights: Vec<f32> = self
            .table
            .rows
            .iter()
            .map(|r| slot_height(r.group_break))
            .collect();
        let (first, count, before, after) = row_window(&heights, self.scroll, self.view_height);
        let mut rows = column![
            container(self.header()).height(Length::Fixed(HEADER_H)),
            space::vertical().height(Length::Fixed(before)),
        ];
        for (i, player) in self.table.rows.iter().enumerate().skip(first).take(count) {
            rows = rows.push(self.row(i, player));
        }
        rows.push(space::vertical().height(Length::Fixed(after)))
            .into()
    }
}

fn flag_row<'a>(view: &CrosstableView, i18n: &I18n) -> Element<'a, Message> {
    let p = &view.prefs;
    // Titles, ages and countries need the spelling file of the players
    // (M6-3): their items stay disabled until a provider exists (D3365).
    let flags: [(CrossFlag, &str, bool, bool); 14] = [
        (
            CrossFlag::Ratings,
            "crosstab-opt-ratings",
            p.show_ratings,
            true,
        ),
        (
            CrossFlag::Titles,
            "crosstab-opt-titles",
            p.show_titles,
            false,
        ),
        (CrossFlag::Ages, "crosstab-opt-ages", p.show_ages, false),
        (
            CrossFlag::Countries,
            "crosstab-opt-countries",
            p.show_countries,
            false,
        ),
        (
            CrossFlag::Tallies,
            "crosstab-opt-tallies",
            p.show_tallies,
            true,
        ),
        (
            CrossFlag::Tiebreaks,
            "crosstab-opt-tiebreaks",
            p.show_tiebreaks,
            true,
        ),
        (
            CrossFlag::IncludeDeleted,
            "crosstab-opt-deleted",
            p.include_deleted,
            true,
        ),
        (
            CrossFlag::SwissColors,
            "crosstab-opt-colors",
            p.swiss_colors,
            true,
        ),
        (
            CrossFlag::NumberedColumns,
            "crosstab-opt-numbered",
            p.numbered_columns,
            true,
        ),
        (
            CrossFlag::HeadToHead,
            "crosstab-opt-head-to-head",
            p.head_to_head,
            true,
        ),
        (
            CrossFlag::TiebreakWins,
            "crosstab-opt-wins",
            p.tiebreak_wins,
            true,
        ),
        (
            CrossFlag::ThreePoints,
            "crosstab-opt-three-points",
            p.three_points,
            true,
        ),
        (
            CrossFlag::SeparateGroups,
            "crosstab-opt-groups",
            p.separate_groups,
            true,
        ),
        (
            CrossFlag::ColorRows,
            "crosstab-opt-color-rows",
            p.color_rows,
            true,
        ),
    ];
    let mut r = row![].spacing(14);
    for (flag, key, on, enabled) in flags {
        let change = enabled.then_some(move |b: bool| app(CrosstableMessage::SetFlag(flag, b)));
        r = r.push(check(i18n.tr(key), on, change));
    }
    flow(r)
}

fn options<'a>(view: &CrosstableView, state: &DataPanelsUi, i18n: &I18n) -> Element<'a, Message> {
    let mut formats = row![muted(i18n.tr("crosstab-format-label"))]
        .spacing(4)
        .align_y(Alignment::Center);
    for (format, key) in [
        (CrossFormat::Auto, "crosstab-format-auto"),
        (CrossFormat::AllPlayAll, "crosstab-format-all-play-all"),
        (CrossFormat::Swiss, "crosstab-format-swiss"),
        (CrossFormat::Knockout, "crosstab-format-knockout"),
    ] {
        formats = formats.push(choice(
            i18n.tr(key),
            view.prefs.format == format,
            Some(app(CrosstableMessage::SetFormat(format))),
        ));
    }
    let mut sorts = row![muted(i18n.tr("crosstab-sort-label"))]
        .spacing(4)
        .align_y(Alignment::Center);
    for (sort, key) in [
        (CrossSort::Score, "crosstab-sort-score"),
        (CrossSort::Name, "crosstab-sort-name"),
        (CrossSort::Country, "crosstab-sort-country"),
        (CrossSort::Rating, "crosstab-sort-rating"),
    ] {
        sorts = sorts.push(choice(
            i18n.tr(key),
            view.prefs.sort == sort,
            Some(app(CrosstableMessage::SetSort(sort))),
        ));
    }
    let typed = state
        .cross_color_text
        .clone()
        .unwrap_or_else(|| view.prefs.row_color.clone());
    let color = row![
        muted(i18n.tr("crosstab-opt-row-color")),
        text_input("#cccccc", &typed)
            .id(Id::new(CROSS_COLOR_ID))
            .on_input(|v| ui(DataPanelsMessage::RowColorEdit(v)))
            .on_submit(ui(DataPanelsMessage::RowColorSubmit))
            .size(BODY_TEXT)
            .padding([2, 6])
            .width(Length::Fixed(90.0)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    container(column![flow(formats), flow(sorts), flag_row(view, i18n), color].spacing(8))
        .padding(8)
        .style(theme::list_header)
        .into()
}

fn toolbar<'a>(view: &CrosstableView, state: &DataPanelsUi, i18n: &I18n) -> Element<'a, Message> {
    let has_table = matches!(view.body, CrosstableBody::Table(_));
    let mut r = row![action(
        i18n.tr("crosstab-refresh"),
        Some(app(CrosstableMessage::Refresh))
    )]
    .spacing(6)
    .align_y(Alignment::Center);
    if view.progress.is_some() || matches!(view.body, CrosstableBody::Computing) {
        r = r.push(action(
            i18n.tr("crosstab-stop"),
            Some(app(CrosstableMessage::Cancel)),
        ));
    }
    r = r.push(choice(
        i18n.tr("crosstab-options"),
        state.cross_options_open,
        Some(ui(DataPanelsMessage::ToggleCrossOptions)),
    ));
    r = r.push(action(
        i18n.tr("crosstab-set-filter"),
        has_table.then(|| app(CrosstableMessage::SetFilter(None))),
    ));
    r = r.push(action(
        i18n.tr("crosstab-add-filter"),
        has_table.then(|| app(CrosstableMessage::AddToFilter(None))),
    ));
    for (format, key) in [
        (ExportFormat::Text, "crosstab-export-text"),
        (ExportFormat::Html, "crosstab-export-html"),
        (ExportFormat::Latex, "crosstab-export-latex"),
    ] {
        r = r.push(action(
            i18n.tr(key),
            has_table.then(|| ui(DataPanelsMessage::PickExport(format))),
        ));
    }
    r = r.push(action(
        i18n.tr("crosstab-copy"),
        has_table.then(|| app(CrosstableMessage::Copy)),
    ));
    flow(r)
}

fn summary<'a>(t: &CrossTableView, i18n: &I18n) -> Element<'a, Message> {
    let mut parts = Vec::new();
    if t.average_elo > 0 {
        let mut args = FluentArgs::new();
        args.set("n", i64::from(t.average_elo));
        parts.push(i18n.tr_args("crosstab-average", &args));
        if t.category > 0 && t.mode == CrossMode::AllPlayAll {
            let mut args = FluentArgs::new();
            args.set("n", i64::from(t.category));
            parts.push(i18n.tr_args("crosstab-category-n", &args));
        }
    }
    if t.game_count > 1 {
        let (white, black, draws, none) = t.results;
        let mut args = FluentArgs::new();
        args.set("n", i64::from(t.game_count));
        args.set("w", i64::from(white));
        args.set("b", i64::from(black));
        args.set("d", i64::from(draws));
        parts.push(i18n.tr_args("crosstab-games-total", &args));
        if none > 0 {
            let mut args = FluentArgs::new();
            args.set("k", i64::from(none));
            parts.push(i18n.tr_args("crosstab-unfinished", &args));
        }
    }
    muted(parts.join("   "))
}

fn warnings<'a>(t: &CrossTableView, i18n: &I18n) -> Vec<Element<'a, Message>> {
    t.warnings
        .iter()
        .map(|w| {
            let mut args = FluentArgs::new();
            if let CrossWarning::Unreadable(n) = w {
                args.set("n", i64::from(*n));
            }
            danger(i18n.tr_args(w.message_key(), &args))
        })
        .collect()
}

fn game_strip<'a>(
    game: u32,
    i18n: &I18n,
    merge: Option<mychess_app::SlotId>,
) -> Element<'a, Message> {
    let mut args = FluentArgs::new();
    args.set("n", i64::from(game));
    container(
        row![
            text(i18n.tr_args("crosstab-game", &args))
                .size(BODY_TEXT)
                .font(bold()),
            action(
                i18n.tr("crosstab-browse"),
                Some(ui(DataPanelsMessage::BrowseGame(game)))
            ),
            action(
                i18n.tr("crosstab-load"),
                Some(app(CrosstableMessage::LoadGame(game)))
            ),
            // Spec M7-6d: merge the game into the game of the editor.
            action(
                i18n.tr("merge-button"),
                merge.map(
                    |slot| Message::App(crate::windows::merge_dialog::open_message_app(slot, game))
                )
            ),
            action(
                i18n.tr("crosstab-close-strip"),
                Some(ui(DataPanelsMessage::SelectGame(None)))
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .style(theme::list_header)
    .into()
}

fn body<'a>(
    view: &CrosstableView,
    state: &DataPanelsUi,
    i18n: &I18n,
    merge: Option<mychess_app::SlotId>,
) -> Element<'a, Message> {
    match &view.body {
        CrosstableBody::Table(table) => {
            let grid = Grid {
                table: Arc::clone(table),
                labels: view.labels.clone(),
                colors: view.prefs.color_rows,
                row_color: panel_kit::parse_hex(&view.prefs.row_color),
                swiss_colors: view.prefs.swiss_colors,
                numbered: view.prefs.numbered_columns,
                selected: state.cross_selected,
                scroll: state.cross_scroll,
                view_height: state.cross_view_h,
            };
            let mut head = column![
                text(table.event.clone()).size(15).font(bold()),
                muted(format!(
                    "{}, {}",
                    table.site,
                    date_range_text(table.first_date, table.last_date)
                )),
                summary(table, i18n),
            ]
            .spacing(2);
            for w in warnings(table, i18n) {
                head = head.push(w);
            }
            let mut page = column![head].spacing(6);
            if let Some(game) = state.cross_selected {
                page = page.push(game_strip(game, i18n, merge));
            }
            // The grid is as wide as its columns: it scrolls both ways.
            let dim = view.stale;
            let grid_view =
                container(grid.view()).style(move |theme: &iced::Theme| container::Style {
                    text_color: dim.then(|| Tokens::of(theme).muted),
                    ..container::Style::default()
                });
            page.push(
                scrollable(grid_view)
                    .on_scroll(|viewport| {
                        ui(DataPanelsMessage::CrossScrolled {
                            offset: viewport.absolute_offset().y,
                            height: viewport.bounds().height,
                        })
                    })
                    .direction(scrollable::Direction::Both {
                        vertical: scrollable::Scrollbar::default(),
                        horizontal: scrollable::Scrollbar::default(),
                    })
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .into()
        }
        CrosstableBody::Empty(notice) => muted(i18n.tr(notice.message_key())),
        CrosstableBody::Computing => {
            let (done, total) = view.progress.unwrap_or((0, 0));
            column![
                muted(i18n.tr("crosstab-computing")),
                panel_kit::progress(done, total)
            ]
            .spacing(6)
            .into()
        }
        CrosstableBody::Closed => space::vertical().into(),
    }
}

/// The content of the panel for `view`.
pub fn content<'a>(
    view: &CrosstableView,
    state: &DataPanelsUi,
    i18n: &I18n,
    merge: Option<mychess_app::SlotId>,
) -> Element<'a, Message> {
    let mut page = column![toolbar(view, state, i18n)].spacing(8).padding(8);
    if state.cross_options_open {
        page = page.push(options(view, state, i18n));
    }
    if view.stale
        && let Some((done, total)) = view.progress
    {
        page = page.push(panel_kit::progress(done, total));
    }
    // A stale table may be replaced under the pointer: no merge from it.
    page = page.push(body(view, state, i18n, merge.filter(|_| !view.stale)));
    if let Some(notice) = view.notice {
        page = page.push(muted(i18n.tr(notice.message_key())));
    }
    page.into()
}

/// The panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let view = shell.app.crosstable_view();
    panes::body(
        content(
            &view,
            &shell.ui.dialogs.panels,
            &shell.i18n,
            shell
                .app
                .crosstable_slot()
                .filter(|_| crate::windows::merge_dialog::can_merge(&shell.app)),
        ),
        CROSSTABLE_PANE_ID,
    )
}
