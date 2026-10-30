//! The body of the opening report window: the header with the board, the
//! numbered sections, the theory table with its footnotes (spec M7-2c,
//! D3622, D3623).

use std::sync::Arc;

use iced::alignment::Horizontal;
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, text, tooltip};
use iced::{Alignment, Border, Element, Length, Size, Theme};
use mychess_app::opening_report::{OpeningReportMessage, OpeningReportView, ReportSelector};

use super::model::{
    Block, Cell, GameRow, GridModel, Hits, Model, NoteModel, Target, Theory, TheoryRow,
};
use super::{OPREP_BOARD_ID, OpRepUi, OpRepUiMessage, orep, ui};
use crate::app_api;
use crate::shell::{Message, Shell};
use crate::theme::{self, RowLook, Tokens};
use crate::widgets::board::geometry::Orientation;
use crate::widgets::board::{BoardData, BoardWidget};
use crate::widgets::context_menu::{self, Entry};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::right_click::RightClick;
use crate::windows::panel_kit::{BODY_TEXT, bold, muted};

/// Width of a move cell of the theory table.
const MOVE_WIDTH: f32 = 62.0;
/// Height of a row of the theory table (fixed, so it can be windowed).
const THEORY_ROW_H: f32 = 20.0;
/// Height of the viewport of the theory table.
const THEORY_VIEW_H: f32 = 440.0;
/// Rows built above and below the viewport of the theory table.
const THEORY_OVERSCAN: usize = 6;
/// Pixels of a square of the board for each unit of the size preference.
const SQUARE_UNIT: f32 = 1.0;

/// The style of a link: the accent, no background, lighter on hover.
fn link_style(theme: &Theme, status: button::Status) -> button::Style {
    let t = Tokens::of(theme);
    let color = match status {
        button::Status::Hovered | button::Status::Pressed => {
            theme::tokens::mix(t.text, t.accent, 0.35)
        }
        button::Status::Disabled => t.muted,
        button::Status::Active => t.accent,
    };
    button::Style {
        background: None,
        text_color: color,
        border: Border::default(),
        ..button::Style::default()
    }
}

/// The message of a click on a target.
pub(super) fn target_message(target: Target) -> Message {
    match target {
        Target::Select(selector) => orep(OpeningReportMessage::Select(selector)),
        Target::Load(game) => orep(OpeningReportMessage::LoadGame(game)),
        Target::Note(n) => ui(OpRepUiMessage::NoteFocus(Some(n))),
    }
}

/// The id of the container of a link (for tests and automation).
pub fn target_id(target: Target) -> String {
    match target {
        Target::Select(ReportSelector::All) => "oprep-link-all".to_owned(),
        Target::Select(ReportSelector::Theme(n)) => format!("oprep-link-theme-{n}"),
        Target::Select(ReportSelector::Note(n)) => format!("oprep-link-select-note-{n}"),
        Target::Select(ReportSelector::MoveOrder(n)) => format!("oprep-link-order-{n}"),
        Target::Select(ReportSelector::Endgame(n)) => format!("oprep-link-end-{n}"),
        Target::Load(game) => format!("oprep-link-game-{game}"),
        Target::Note(n) => format!("oprep-link-note-{n}"),
    }
}

fn link<'a>(label: impl text::IntoFragment<'a>, size: f32, target: Target) -> Element<'a, Message> {
    container(
        button(text(label).size(size))
            .padding(0)
            .style(link_style)
            .on_press(target_message(target)),
    )
    .id(target_id(target))
    .into()
}

fn cell_element(cell: &Cell, width: f32) -> Element<'_, Message> {
    let align = if cell.right {
        Horizontal::Right
    } else {
        Horizontal::Left
    };
    let label = text(cell.text.as_str())
        .size(BODY_TEXT)
        .width(Length::Fixed(width))
        .align_x(align);
    match cell.target {
        Some(target) => container(
            button(label)
                .padding(0)
                .style(link_style)
                .on_press(target_message(target)),
        )
        .id(target_id(target))
        .into(),
        None => label.into(),
    }
}

fn highlight<'a>(content: impl Into<Element<'a, Message>>, hit: bool) -> Element<'a, Message> {
    container(content)
        .style(theme::list_row(RowLook {
            selected: hit,
            ..RowLook::default()
        }))
        .into()
}

fn grid<'a>(g: &'a GridModel, hits: &Hits, block: usize) -> Element<'a, Message> {
    let mut col = column![].spacing(2);
    let line = |cells: &'a [Cell]| -> iced::widget::Row<'a, Message> {
        let mut r = row![].spacing(0);
        for (i, cell) in cells.iter().enumerate() {
            r = r.push(cell_element(cell, g.widths.get(i).copied().unwrap_or(40.0)));
        }
        r
    };
    for header in &g.header {
        col = col.push(
            container(line(header))
                .style(theme::list_header)
                .padding([2, 0]),
        );
    }
    for (i, cells) in g.rows.iter().enumerate() {
        col = col.push(highlight(line(cells), hits.hit(block, i)));
    }
    scrollable(col)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default(),
        ))
        .into()
}

fn games<'a>(rows: &'a [GameRow], hits: &Hits, block: usize) -> Element<'a, Message> {
    let mut col = column![].spacing(2);
    for (i, r) in rows.iter().enumerate() {
        col = col.push(highlight(
            link(r.text.as_str(), BODY_TEXT, Target::Load(r.game)),
            hits.hit(block, i),
        ));
    }
    col.into()
}

/// The rows `[start, end)` of the theory table to build for a table of
/// `total` rows scrolled `offset` pixels.
pub(super) fn theory_window(offset: f32, total: usize) -> (usize, usize) {
    let offset = if offset.is_finite() {
        (offset - THEORY_ROW_H).max(0.0)
    } else {
        0.0
    };
    // Small non-negative numbers of rows.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (first, visible) = (
        (offset / THEORY_ROW_H) as usize,
        (THEORY_VIEW_H / THEORY_ROW_H).ceil() as usize + 1,
    );
    let start = first.saturating_sub(THEORY_OVERSCAN).min(total);
    let end = first
        .saturating_add(visible)
        .saturating_add(THEORY_OVERSCAN)
        .min(total);
    (start, end)
}

fn rows_height(rows: usize) -> f32 {
    // The row counts stay far below 2^24.
    #[allow(clippy::cast_precision_loss)]
    let pixels = rows as f32 * THEORY_ROW_H;
    pixels
}

fn theory_row(r: &TheoryRow, hit: bool) -> Element<'_, Message> {
    let mut line = row![
        text(r.number.to_string())
            .size(12)
            .style(theme::muted_text)
            .width(Length::Fixed(28.0))
    ]
    .align_y(Alignment::Center);
    for (mv, note) in &r.cells {
        let mut cell = row![text(mv.as_str()).size(BODY_TEXT)]
            .spacing(2)
            .align_y(Alignment::Center);
        if *note > 0 {
            cell = cell.push(link(format!("[{note}]"), 10.0, Target::Note(*note)));
        }
        line = line.push(container(cell).width(Length::Fixed(MOVE_WIDTH)).clip(true));
    }
    line = line
        .push(
            text(format!("{}:", r.count))
                .size(BODY_TEXT)
                .align_x(Horizontal::Right)
                .width(Length::Fixed(50.0)),
        )
        .push(
            text(format!("{}%", r.score))
                .size(BODY_TEXT)
                .align_x(Horizontal::Right)
                .width(Length::Fixed(44.0)),
        );
    let text_line = Arc::clone(&r.line);
    RightClick::new(
        highlight(container(line).height(Length::Fixed(THEORY_ROW_H)), hit),
        move |at, _local| {
            ui(OpRepUiMessage::RowMenu {
                line: Arc::clone(&text_line),
                at,
            })
        },
    )
    .into()
}

fn theory<'a>(shell: &'a Shell, t: &'a Theory, hits: &Hits, block: usize) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut col = column![
        text(t.stem.as_str()).size(15).font(bold()),
        text(t.heading.as_str()).size(BODY_TEXT),
    ]
    .spacing(4);
    if !t.comment.is_empty() {
        col = col.push(muted(t.comment.clone()));
    }
    // The numbers of the moves over the cells.
    let mut head = row![space::horizontal().width(Length::Fixed(28.0))];
    for j in 0..8 {
        head = head.push(
            text((t.first_number + j).to_string())
                .size(12)
                .style(theme::muted_text)
                .width(Length::Fixed(2.0 * MOVE_WIDTH)),
        );
    }
    // Only the rows near the viewport become widgets: the table can hold
    // thousands of rows and the view runs on every message.
    let (start, end) = theory_window(shell.ui.dialogs.oprep.theory_scroll, t.rows.len());
    let mut table = column![
        container(head).height(Length::Fixed(THEORY_ROW_H)),
        space::vertical().height(Length::Fixed(rows_height(start)))
    ];
    for (i, r) in t.rows.iter().enumerate().take(end).skip(start) {
        table = table.push(theory_row(r, hits.hit(block, i)));
    }
    table = table.push(
        space::vertical().height(Length::Fixed(rows_height(t.rows.len().saturating_sub(end)))),
    );
    // A short table takes its own height; a long one scrolls inside.
    let height = (rows_height(t.rows.len() + 1) + 14.0).min(THEORY_VIEW_H);
    col = col.push(
        scrollable(table)
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .on_scroll(|viewport| ui(OpRepUiMessage::TheoryScrolled(viewport.absolute_offset().y)))
            .height(Length::Fixed(height)),
    );
    col = col.push(muted(i18n.tr("oprep-ui-copy-hint")));
    container(col).id("oprep-theory").into()
}

fn note<'a>(shell: &'a Shell, n: &'a NoteModel, hit: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("n", i64::from(n.number));
    let mut col = column![
        row![
            link(
                i18n.tr_args("oprep-ui-note", &args),
                BODY_TEXT,
                Target::Select(ReportSelector::Note(n.number))
            ),
            muted(i18n.tr("oprep-ui-note-select")),
        ]
        .spacing(8)
    ]
    .spacing(2);
    for line in &n.lines {
        col = col.push(text(line.as_str()).size(12));
    }
    container(highlight(col, hit))
        .id(format!("oprep-note-{}", n.number))
        .padding([2, 0])
        .into()
}

fn notes<'a>(
    shell: &'a Shell,
    notes: &'a [NoteModel],
    ui_state: &OpRepUi,
    hits: &Hits,
    block: usize,
) -> Element<'a, Message> {
    let mut col = column![].spacing(4);
    // The footnote of the marker clicked comes first.
    if let Some(n) = ui_state
        .note_focus
        .and_then(|f| notes.iter().find(|n| n.number == f))
    {
        col = col.push(note(shell, n, true));
    }
    for (i, n) in notes.iter().enumerate() {
        if ui_state.note_focus != Some(n.number) {
            col = col.push(note(shell, n, hits.hit(block, i)));
        }
    }
    col.into()
}

/// The board with the buttons that make it bigger or smaller. A click on
/// it turns it (D3623).
fn board_column<'a>(
    shell: &'a Shell,
    view: &OpeningReportView<'a>,
) -> Option<Element<'a, Message>> {
    let report = view.report?;
    let ui_state = &shell.ui.dialogs.oprep;
    let size = view.prefs.board_size;
    let side = f32::from(size) * 8.0 * SQUARE_UNIT;
    let appearance = app_api::appearance(&shell.app, shell.options);
    let data = BoardData {
        position: &report.position,
        last_move: None,
        marks: &[],
        orientation: if ui_state.flipped {
            Orientation::Black
        } else {
            Orientation::White
        },
        options: appearance.board,
        accepts_moves: false,
        brush: &ui_state.brush,
        pieces: shell.ui.pieces(),
        animation: None,
    };
    let board = BoardWidget::new(data, &ui_state.caches)
        .into_element()
        .map(|_| ui(OpRepUiMessage::Noop));
    let board = mouse_area(
        container(board)
            .width(Length::Fixed(side))
            .height(Length::Fixed(side)),
    )
    .on_press(ui(OpRepUiMessage::Flip));
    let i18n = &shell.i18n;
    let small = |glyph: Icon, tip: &str, up: bool, enabled: bool, id: &'static str| {
        let message = enabled.then(|| ui(OpRepUiMessage::BoardSize(up)));
        let tone = if enabled {
            IconTone::Text
        } else {
            IconTone::Disabled
        };
        let body = button(icon(glyph, 14.0, tone))
            .padding(3)
            .style(theme::command_button(theme::ButtonLook::Secondary, false))
            .on_press_maybe(message);
        let tip = container(text(i18n.tr(tip)).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box);
        container(tooltip(body, tip, tooltip::Position::Bottom)).id(id)
    };
    let range = mychess_app::opening_report::BOARD_SIZE_RANGE;
    Some(
        column![
            container(board).id(OPREP_BOARD_ID),
            row![
                small(
                    Icon::ZoomOut,
                    "oprep-ui-board-smaller",
                    false,
                    size > *range.start(),
                    "oprep-board-smaller"
                ),
                small(
                    Icon::ZoomIn,
                    "oprep-ui-board-bigger",
                    true,
                    size < *range.end(),
                    "oprep-board-bigger"
                ),
            ]
            .spacing(4),
        ]
        .spacing(4)
        .into(),
    )
}

fn header<'a>(
    shell: &'a Shell,
    view: &OpeningReportView<'a>,
    model: &'a Model,
) -> Element<'a, Message> {
    let mut facts = column![].spacing(3);
    for (label, value, target) in &model.header.rows {
        let value: Element<'a, Message> = match target {
            Some(t) => link(value.as_str(), BODY_TEXT, *t),
            None => text(value.as_str()).size(BODY_TEXT).into(),
        };
        facts = facts.push(
            row![
                text(format!("{label}:"))
                    .size(BODY_TEXT)
                    .font(bold())
                    .width(Length::Fixed(110.0)),
                value
            ]
            .spacing(6),
        );
    }
    for warning in &model.header.warnings {
        facts = facts.push(crate::windows::panel_kit::danger(warning.clone()));
    }
    let mut line = row![].spacing(16);
    if let Some(board) = board_column(shell, view) {
        line = line.push(board);
    }
    line.push(facts).into()
}

fn blocks<'a>(shell: &'a Shell, model: &'a Model) -> Element<'a, Message> {
    let ui_state = &shell.ui.dialogs.oprep;
    let hits = &ui_state.hits;
    let mut col = column![].spacing(6);
    for (i, block) in model.blocks.iter().enumerate() {
        col = col.push(match block {
            Block::Section(t) => container(text(t.as_str()).size(18).font(bold()))
                .padding([10, 0])
                .into(),
            Block::Sub(t) => text(t.as_str()).size(15).font(bold()).into(),
            Block::Para(t) => highlight(text(t.as_str()).size(BODY_TEXT), hits.hit(i, 0)),
            Block::Grid(g) => grid(g, hits, i),
            Block::Games(rows) => games(rows, hits, i),
            Block::Theory(t) => theory(shell, t, hits, i),
            Block::Notes(n) => notes(shell, n, ui_state, hits, i),
        });
    }
    col.into()
}

/// The report: header and sections, scrolled. Empty while there is no
/// report yet or the model is being built.
pub(super) fn content<'a>(shell: &'a Shell, view: &OpeningReportView<'a>) -> Element<'a, Message> {
    let cached = shell
        .ui
        .dialogs
        .oprep
        .cached
        .as_ref()
        .filter(|c| view.shared.as_ref() == Some(&c.report));
    let Some(cached) = cached else {
        return space::vertical().into();
    };
    let page = column![
        header(shell, view, &cached.model),
        blocks(shell, &cached.model)
    ]
    .spacing(12)
    .padding([0, 12]);
    container(scrollable(page).id("oprep-scroll"))
        .height(Length::Fill)
        .id("oprep-body")
        .into()
}

/// The menu of "copy line", when the right button was pressed on a row.
pub(super) fn row_menu_layer(shell: &Shell) -> Option<Element<'_, Message>> {
    let menu = shell.ui.dialogs.oprep.row_menu.as_ref()?;
    let window = shell.ui.window_size.unwrap_or(Size::new(1280.0, 800.0));
    let entries = vec![Entry::Item {
        label: shell.i18n.tr("oprep-ui-copy-line"),
        message: Some(ui(OpRepUiMessage::CopyLine)),
    }];
    let width = 180.0;
    let size = context_menu::shown_size(context_menu::card_size(&entries, width), window);
    let at = context_menu::place(menu.at, size, window);
    let card = context_menu::card(entries, width, window.height);
    Some(context_menu::layer(
        card,
        at,
        ui(OpRepUiMessage::CloseRowMenu),
    ))
}

#[cfg(test)]
mod tests {
    use iced::advanced::widget::Tree;

    use super::*;

    fn rows(n: usize) -> Vec<TheoryRow> {
        (0..n)
            .map(|i| TheoryRow {
                number: i + 1,
                cells: (0..16)
                    .map(|j| (format!("Nf{j}"), u32::from(j == 3)))
                    .collect(),
                count: 3,
                score: 50,
                line: Arc::from("1.e4 e5"),
            })
            .collect()
    }

    fn table(n: usize) -> Theory {
        Theory {
            stem: "1.e4".to_owned(),
            heading: "+1".to_owned(),
            first_number: 2,
            black_first: false,
            rows: rows(n),
            comment: String::new(),
        }
    }

    fn nodes(tree: &Tree) -> usize {
        1 + tree.children.iter().map(nodes).sum::<usize>()
    }

    fn widgets(shell: &Shell, t: &Theory) -> usize {
        let element = theory(shell, t, &Hits::default(), 0);
        nodes(&Tree::new(&element))
    }

    #[test]
    fn the_window_covers_the_viewport_and_never_passes_the_end() {
        assert_eq!(theory_window(0.0, 10_000).0, 0);
        let (start, end) = theory_window(0.0, 10_000);
        assert!(end - start <= 40, "{start}..{end}");
        let (start, end) = theory_window(THEORY_ROW_H * 1000.0, 10_000);
        assert!(start >= 990 && end <= 1040, "{start}..{end}");
        assert_eq!(theory_window(1.0e9, 10).0, 10);
        assert_eq!(theory_window(f32::NAN, 5), (0, 5));
        assert_eq!(theory_window(0.0, 0), (0, 0));
    }

    #[test]
    fn a_table_of_thousands_of_rows_builds_only_the_rows_near_the_viewport() {
        let shell = Shell::new(None);
        let small = widgets(&shell, &table(200));
        let big = widgets(&shell, &table(6000));
        assert_eq!(big, small, "the cost does not grow with the rows");
        // Each row is a few dozen widgets: the whole table would be far more.
        assert!(big < 40 * 60, "{big} widgets for 6000 rows");
    }
}
