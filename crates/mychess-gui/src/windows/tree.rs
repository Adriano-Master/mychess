//! The opening tree panel (spec M5-2, D960-D971, D982-D985): the table of
//! the moves played from the position of the game, with the tricolour bar
//! of each row, the options bar and the messages that replace the table.
//!
//! The application prepares everything ([`TreeView`]: rows, flags, bar
//! segments); this module only lays it out, formats the numbers with the
//! Fluent texts and draws the bars. All sizes are logical (no physical
//! pixels). The panel never takes keys: the arrows stay with the board.

use fluent_bundle::FluentArgs;
use iced::alignment::Horizontal;
use iced::widget::canvas::{self, Frame};
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, tooltip,
};
use iced::{Alignment, Element, Length, Point, Rectangle, Size, mouse};
use mychess_app::{
    BAR_WIDTH, BarSegments, MaskOnlyRow, MaskTarget, Message as AppMessage, TreeBody, TreeFastMode,
    TreeIssue, TreeMessage, TreeRowView, TreeSort, TreeTotalView, TreeView,
};

mod best;
mod cache;

use super::header_dialog::Labeled;
use super::layout::PaneKind;
use super::mask::cells::{self, COMMENT_WIDTH, MARKS_WIDTH, MaskCtx};
use super::mask::{self, Hover, MaskUiMessage};
use super::panes;
use crate::i18n::I18n;
use crate::keymap::Platform;
use crate::shell::{Message, Shell, UiMessage};
use crate::theme::tree_colors::TreeBarColors;
use crate::theme::{self, ButtonLook, RowLook, Tokens};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::right_click::RightClick;

/// Width of the move column.
const MOVE_WIDTH: f32 = 72.0;
/// Width of the game count.
const GAMES_WIDTH: f32 = 60.0;
/// Width of the share of the games.
const SHARE_WIDTH: f32 = 56.0;
/// Width of the score and draw columns.
const PERCENT_WIDTH: f32 = 56.0;
/// Width of the rating, performance and year columns.
const NUMBER_WIDTH: f32 = 44.0;
/// Width of the ECO column.
const ECO_WIDTH: f32 = 40.0;
/// Height of the bar.
const BAR_HEIGHT: f32 = 10.0;
/// Text size of the table.
const TABLE_TEXT: f32 = 12.0;
/// Space between the cells.
const CELL_SPACING: f32 = 8.0;
/// Horizontal padding of a row, both sides.
const ROW_PADDING: f32 = 16.0;

// ---- the bar ----

/// The kind of a part of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    /// White's wins.
    White,
    /// The draws.
    Grey,
    /// Black's wins.
    Black,
}

/// The parts of the bar as `(part, from, to)` in bar units (`0..=61`),
/// back to front: the grey base, then the white and the black parts that
/// are drawn (D969).
pub(crate) fn bar_parts(bar: BarSegments) -> Vec<(Part, f32, f32)> {
    let mut parts = vec![(Part::Grey, 0.0, BAR_WIDTH)];
    if let Some(end) = bar.white_end {
        parts.push((Part::White, 0.0, end.clamp(0.0, BAR_WIDTH)));
    }
    if let Some(start) = bar.black_start {
        parts.push((Part::Black, start.clamp(0.0, BAR_WIDTH), BAR_WIDTH));
    }
    parts
}

struct BarCanvas {
    bar: BarSegments,
    dimmed: bool,
}

impl canvas::Program<Message> for BarCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let colors = TreeBarColors::of(&Tokens::of(theme));
        let alpha = if self.dimmed { 0.5 } else { 1.0 };
        let scale = bounds.width / BAR_WIDTH;
        let mut frame = Frame::new(renderer, bounds.size());
        for (part, from, to) in bar_parts(self.bar) {
            let color = match part {
                Part::White => colors.white,
                Part::Grey => colors.grey,
                Part::Black => colors.black,
            };
            if to > from {
                frame.fill_rectangle(
                    Point::new(from * scale, 0.0),
                    Size::new((to - from) * scale, bounds.height),
                    color.scale_alpha(alpha),
                );
            }
        }
        frame.stroke_rectangle(
            Point::new(0.5, 0.5),
            Size::new(bounds.width - 1.0, bounds.height - 1.0),
            canvas::Stroke::default()
                .with_width(1.0)
                .with_color(colors.outline),
        );
        vec![frame.into_geometry()]
    }
}

fn bar_widget<'a>(bar: BarSegments, dimmed: bool) -> Element<'a, Message> {
    canvas::Canvas::new(BarCanvas { bar, dimmed })
        .width(Length::Fixed(BAR_WIDTH))
        .height(Length::Fixed(BAR_HEIGHT))
        .into()
}

// ---- text ----

/// `45.3%` with the decimal separator of the language.
pub(crate) fn percent_text(i18n: &I18n, (whole, tenth): (u32, u32)) -> String {
    let mut args = FluentArgs::new();
    args.set("whole", whole.to_string());
    args.set("tenth", tenth.to_string());
    i18n.tr_args("tree-percent", &args)
}

fn whole_percent_text(i18n: &I18n, value: u32) -> String {
    let mut args = FluentArgs::new();
    args.set("value", value.to_string());
    i18n.tr_args("tree-percent-whole", &args)
}

fn optional(value: Option<u32>) -> String {
    value.map_or_else(String::new, |v| v.to_string())
}

/// The text that takes the place of the table, if it is not shown.
pub(crate) fn body_message(i18n: &I18n, body: &TreeBody) -> Option<String> {
    match body {
        TreeBody::Closed | TreeBody::Rows => None,
        TreeBody::Unavailable => Some(i18n.tr("tree-unavailable")),
        TreeBody::Loading => Some(i18n.tr("tree-loading")),
        TreeBody::Empty => Some(i18n.tr("tree-empty")),
        TreeBody::Issue(issue) => Some(issue_text(i18n, issue)),
    }
}

fn issue_text(i18n: &I18n, issue: &TreeIssue) -> String {
    match issue {
        // Never shown as an error: the next search is on its way.
        TreeIssue::Cancelled => i18n.tr("tree-loading"),
        TreeIssue::TooManyMoves => i18n.tr("tree-issue-too-many-moves"),
        TreeIssue::UnsupportedPosition => i18n.tr("tree-issue-unsupported"),
        TreeIssue::FilterSize => i18n.tr("tree-issue-filter-size"),
        TreeIssue::Read(reason) => {
            let mut args = FluentArgs::new();
            args.set("reason", reason.clone());
            i18n.tr_args("tree-issue-read", &args)
        }
        _ => i18n.tr("tree-issue-internal"),
    }
}

// ---- the table ----

fn cell<'a>(content: String, width: f32, align: Horizontal) -> Element<'a, Message> {
    text(content)
        .size(TABLE_TEXT)
        .width(Length::Fixed(width))
        .align_x(align)
        .into()
}

fn right<'a>(content: String, width: f32) -> Element<'a, Message> {
    cell(content, width, Horizontal::Right)
}

fn header_row<'a>(i18n: &I18n, short: bool, mask: &MaskCtx) -> Element<'a, Message> {
    let mut cells = row![].spacing(CELL_SPACING);
    // The group of marks keeps its room in the header (D1827).
    if mask.marks_visible() {
        cells = cells.push(cells::marks(None));
    }
    cells = cells.push(cell(i18n.tr("tree-col-move"), MOVE_WIDTH, Horizontal::Left));
    let mut cells = cells.extend([
        right(
            i18n.tr("tree-col-games"),
            GAMES_WIDTH + SHARE_WIDTH + CELL_SPACING,
        ),
        right(i18n.tr("tree-col-score"), PERCENT_WIDTH),
        right(i18n.tr("tree-col-draws"), PERCENT_WIDTH),
        cell(String::new(), BAR_WIDTH, Horizontal::Left),
    ]);
    if !short {
        cells = cells
            .push(right(i18n.tr("tree-col-elo"), NUMBER_WIDTH))
            .push(right(i18n.tr("tree-col-perf"), NUMBER_WIDTH))
            .push(right(i18n.tr("tree-col-year"), NUMBER_WIDTH))
            .push(cell(i18n.tr("tree-col-eco"), ECO_WIDTH, Horizontal::Left));
    }
    if mask.open {
        cells = cells.push(cell(
            i18n.tr("tree-mask-col-comment"),
            COMMENT_WIDTH,
            Horizontal::Left,
        ));
    }
    container(cells)
        .padding([4, 8])
        .width(Length::Fill)
        .style(theme::list_header)
        .into()
}

fn data_cells<'a>(
    i18n: &I18n,
    data: &TreeRowView,
    short: bool,
    dimmed: bool,
    mask: &MaskCtx,
) -> Element<'a, Message> {
    // Training hides the move and the ECO code (`hidden`).
    let san = if data.hidden {
        String::new()
    } else {
        data.san.clone()
    };
    let mut cells = row![].spacing(CELL_SPACING).align_y(Alignment::Center);
    if mask.marks_visible() {
        cells = cells.push(cells::marks(data.mask.as_ref()));
    }
    // The text of a move that is in the mask has the highlight (D1827).
    let in_mask = data.mask.as_ref().is_some_and(|m| m.in_mask);
    cells = cells.push(if in_mask {
        cells::move_text(san, MOVE_WIDTH, true, false)
    } else {
        cell(san, MOVE_WIDTH, Horizontal::Left)
    });
    cells = cells.extend([
        right(data.games.to_string(), GAMES_WIDTH),
        right(percent_text(i18n, data.share), SHARE_WIDTH),
        right(percent_text(i18n, data.score), PERCENT_WIDTH),
        right(whole_percent_text(i18n, data.draw_percent), PERCENT_WIDTH),
        bar_widget(data.bar, dimmed),
    ]);
    if !short {
        let eco = if data.hidden {
            String::new()
        } else {
            data.eco.clone().unwrap_or_default()
        };
        cells = cells
            .push(right(optional(data.avg_elo), NUMBER_WIDTH))
            .push(right(optional(data.perf), NUMBER_WIDTH))
            .push(right(optional(data.avg_year), NUMBER_WIDTH))
            .push(cell(eco, ECO_WIDTH, Horizontal::Left));
    }
    if mask.open {
        cells = cells.push(cells::comment_cell(
            data.mask.as_ref(),
            Hover::Row(MaskTarget::Row(data.index)),
            mask,
        ));
    }
    cells.into()
}

/// A row that reacts to the pointer: transparent, the hover tint over the
/// zebra of the container around it.
fn click_style(theme: &iced::Theme, status: button::Status) -> button::Style {
    let t = Tokens::of(theme);
    let background = match status {
        button::Status::Hovered => Some(t.hover.into()),
        button::Status::Pressed => Some(t.pressed.into()),
        button::Status::Active | button::Status::Disabled => None,
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
}

/// The right button on a row of the table opens the context menu of the
/// mask (D1830); the rows with no mask cell (`[end]`, the training) have
/// none.
fn with_context(
    body: Element<'_, Message>,
    target: MaskTarget,
    san: String,
    in_mask: bool,
    command: bool,
) -> Element<'_, Message> {
    RightClick::new(body, move |position, _| {
        Message::MaskUi(MaskUiMessage::OpenContext {
            target: target.clone(),
            san: san.clone(),
            in_mask,
            position,
            command,
        })
    })
    .into()
}

fn data_row<'a>(
    i18n: &I18n,
    data: &TreeRowView,
    short: bool,
    dimmed: bool,
    mask: &MaskCtx,
) -> Element<'a, Message> {
    let cells = data_cells(i18n, data, short, dimmed, mask);
    // Only a row a click does something to is a button (D965): the others
    // have neither the pointer nor the hover.
    let inner: Element<'a, Message> = if data.clickable {
        button(cells)
            .padding([2, 8])
            .width(Length::Fill)
            .style(click_style)
            .on_press(Message::App(AppMessage::Tree(TreeMessage::RowClicked(
                data.index,
            ))))
            .into()
    } else {
        container(cells).padding([2, 8]).width(Length::Fill).into()
    };
    let row = container(inner)
        .id(format!("tree-row-{}", data.index))
        .width(Length::Fill)
        .style(theme::list_row(RowLook {
            zebra: data.index % 2 == 1,
            selected: false,
            current: data.is_next_move,
            dimmed,
        }));
    match &data.mask {
        Some(cell) if mask.open => with_context(
            row.into(),
            MaskTarget::Row(data.index),
            data.san.clone(),
            cell.in_mask,
            mask.command,
        ),
        _ => row.into(),
    }
}

/// A move that is in the mask and in no game (D1828): the stats are dashes
/// and the move has the highlight, or the danger tone when it is illegal.
fn only_row<'a>(
    i18n: &I18n,
    data: &MaskOnlyRow,
    zebra: bool,
    short: bool,
    mask: &MaskCtx,
) -> Element<'a, Message> {
    let dash = |width: f32| right("–".to_owned(), width);
    let mut cells = row![].spacing(CELL_SPACING).align_y(Alignment::Center);
    if mask.marks_visible() {
        cells = cells.push(cells::marks(Some(&data.cell)));
    }
    let san = if data.legal {
        data.san.clone()
    } else {
        format!("{} {}", data.san, i18n.tr("tree-mask-illegal-move"))
    };
    cells = cells.push(cells::move_text(san, MOVE_WIDTH, true, !data.legal));
    cells = cells.extend([
        dash(GAMES_WIDTH),
        dash(SHARE_WIDTH),
        dash(PERCENT_WIDTH),
        dash(PERCENT_WIDTH),
        cell(String::new(), BAR_WIDTH, Horizontal::Left),
    ]);
    if !short {
        cells = cells.extend([
            dash(NUMBER_WIDTH),
            dash(NUMBER_WIDTH),
            dash(NUMBER_WIDTH),
            cell(String::new(), ECO_WIDTH, Horizontal::Left),
        ]);
    }
    cells = cells.push(cells::comment_cell(
        Some(&data.cell),
        Hover::Row(MaskTarget::MaskOnly(data.index)),
        mask,
    ));
    let inner: Element<'a, Message> = if data.clickable {
        button(cells)
            .padding([2, 8])
            .width(Length::Fill)
            .style(click_style)
            .on_press(mask::mask_message(mychess_app::MaskMessage::PlayOnlyRow(
                data.index,
            )))
            .into()
    } else {
        container(cells).padding([2, 8]).width(Length::Fill).into()
    };
    let body = container(inner)
        .id(format!("tree-mask-only-{}", data.index))
        .width(Length::Fill)
        .style(theme::list_row(RowLook {
            zebra,
            ..RowLook::default()
        }));
    let tip = if data.legal {
        i18n.tr("tree-mask-only-row-tip")
    } else {
        i18n.tr("tree-mask-illegal-move-tip")
    };
    let body = tooltip(
        body,
        container(text(tip).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Bottom,
    );
    with_context(
        body.into(),
        MaskTarget::MaskOnly(data.index),
        data.san.clone(),
        true,
        mask.command,
    )
}

fn total_row<'a>(
    i18n: &I18n,
    total: &TreeTotalView,
    short: bool,
    dimmed: bool,
    mask: &MaskCtx,
) -> Element<'a, Message> {
    let mut cells = row![].spacing(CELL_SPACING);
    // The total keeps the room of the marks and of the comment (D1827).
    if mask.marks_visible() {
        cells = cells.push(cells::marks(None));
    }
    cells = cells.extend([
        cell(i18n.tr("tree-total"), MOVE_WIDTH, Horizontal::Left),
        right(total.games.to_string(), GAMES_WIDTH),
        right(percent_text(i18n, (100, 0)), SHARE_WIDTH),
        right(percent_text(i18n, total.score), PERCENT_WIDTH),
        right(whole_percent_text(i18n, total.draw_percent), PERCENT_WIDTH),
        // The total has an empty bar (D969).
        cell(String::new(), BAR_WIDTH, Horizontal::Left),
    ]);
    if !short {
        cells = cells
            .push(right(optional(total.avg_elo), NUMBER_WIDTH))
            .push(right(optional(total.perf), NUMBER_WIDTH))
            .push(right(optional(total.avg_year), NUMBER_WIDTH))
            .push(cell(String::new(), ECO_WIDTH, Horizontal::Left));
    }
    container(cells)
        .id("tree-total")
        .padding([2, 8])
        .width(Length::Fill)
        .style(theme::list_row(RowLook {
            dimmed,
            ..RowLook::default()
        }))
        .into()
}

fn separator<'a>() -> Element<'a, Message> {
    container("")
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(theme::divider)
        .into()
}

/// Width of the table: the cells, the space between them and the padding
/// of a row. The rows have this width so the zebra and the hover cover the
/// whole row even when the panel is narrower and the table scrolls. An open
/// mask adds the comment column, and the marks when they are shown.
fn table_width(short: bool, mask: &MaskCtx) -> f32 {
    let mut cells = [
        MOVE_WIDTH,
        GAMES_WIDTH,
        SHARE_WIDTH,
        PERCENT_WIDTH,
        PERCENT_WIDTH,
        BAR_WIDTH,
    ]
    .to_vec();
    if !short {
        cells.extend([NUMBER_WIDTH, NUMBER_WIDTH, NUMBER_WIDTH, ECO_WIDTH]);
    }
    if mask.marks_visible() {
        cells.push(MARKS_WIDTH);
    }
    if mask.open {
        cells.push(COMMENT_WIDTH);
    }
    let gaps = cells.len().saturating_sub(1);
    // `gaps` is at most 12.
    #[allow(clippy::cast_precision_loss)]
    let spacing = gaps as f32 * CELL_SPACING;
    cells.iter().sum::<f32>() + spacing + ROW_PADDING
}

fn table<'a>(view: &TreeView, i18n: &I18n, mask: &MaskCtx) -> Element<'a, Message> {
    let dimmed = view.searching;
    let mut rows = column![];
    for data in &view.rows {
        rows = rows.push(data_row(i18n, data, view.short, dimmed, mask));
    }
    let mut body = column![rows, separator()].spacing(2);
    if let Some(total) = &view.total {
        body = body.push(total_row(i18n, total, view.short, dimmed, mask));
    }
    // The moves only the mask has come after the total (D1828).
    if let Some(only) = view.mask.as_ref().filter(|_| mask.open) {
        for (index, data) in only.only_rows.iter().enumerate() {
            body = body.push(only_row(i18n, data, index % 2 == 1, view.short, mask));
        }
    }
    let page = column![
        header_row(i18n, view.short, mask),
        scrollable(body).width(Length::Fill).height(Length::Fill),
    ]
    .spacing(2)
    .width(Length::Fixed(table_width(view.short, mask)))
    .height(Length::Fill);
    scrollable(page)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default(),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ---- the options bar ----

fn sort_key(sort: TreeSort) -> &'static str {
    match sort {
        TreeSort::Alphabetical => "tree-sort-alphabetical",
        TreeSort::Eco => "tree-sort-eco",
        TreeSort::Frequency => "tree-sort-frequency",
        TreeSort::Score => "tree-sort-score",
    }
}

fn mode_key(mode: TreeFastMode) -> &'static str {
    match mode {
        TreeFastMode::Slow => "tree-mode-slow",
        TreeFastMode::Fast => "tree-mode-fast",
        TreeFastMode::FastThenSlow => "tree-mode-fast-then-slow",
    }
}

fn tree_message(message: TreeMessage) -> Message {
    Message::App(AppMessage::Tree(message))
}

fn option_box<'a>(
    i18n: &I18n,
    label_key: &str,
    tip_key: Option<&str>,
    on: bool,
    message: TreeMessage,
) -> Element<'a, Message> {
    let body = checkbox(on)
        .label(i18n.tr(label_key))
        .size(14)
        .text_size(12)
        .on_toggle(move |_| tree_message(message.clone()));
    match tip_key {
        Some(tip) => tooltip(
            body,
            container(text(i18n.tr(tip)).size(12))
                .padding([4, 8])
                .style(theme::tooltip_box),
            tooltip::Position::Bottom,
        )
        .into(),
        None => body.into(),
    }
}

fn training_box<'a>(view: &TreeView, i18n: &I18n) -> Element<'a, Message> {
    let on = view.training;
    let body = checkbox(on)
        .label(i18n.tr("tree-training"))
        .size(14)
        .text_size(12)
        .on_toggle_maybe(
            view.training_available
                .then_some(move |_: bool| tree_message(TreeMessage::SetTraining(!on))),
        );
    tooltip(
        body,
        container(text(i18n.tr("tree-training-tip")).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Bottom,
    )
    .into()
}

#[allow(clippy::too_many_lines)] // one flat row of controls
fn toolbar<'a>(view: &TreeView, i18n: &I18n, mask: &MaskCtx) -> Element<'a, Message> {
    let sorts: Vec<Labeled<TreeSort>> = TreeSort::ALL
        .into_iter()
        .map(|value| Labeled {
            value,
            label: i18n.tr(sort_key(value)),
        })
        .collect();
    let modes: Vec<Labeled<TreeFastMode>> = TreeFastMode::ALL
        .into_iter()
        .map(|value| Labeled {
            value,
            label: i18n.tr(mode_key(value)),
        })
        .collect();
    let close = tooltip(
        button(icon(Icon::Close, 14.0, IconTone::Text))
            .padding(4)
            .style(theme::command_button(ButtonLook::Secondary, false))
            .on_press(Message::Ui(UiMessage::TogglePane(PaneKind::Tree))),
        container(text(i18n.tr("tree-close")).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Bottom,
    );
    let mut items: Vec<Element<'a, Message>> = vec![
        option_box(
            i18n,
            "tree-refresh",
            Some("tree-refresh-tip"),
            view.autorefresh,
            TreeMessage::ToggleAutoRefresh,
        ),
        option_box(
            i18n,
            "tree-adjust",
            Some("tree-adjust-tip"),
            view.adjust,
            TreeMessage::SetAdjust(!view.adjust),
        ),
        text(i18n.tr("tree-sort"))
            .size(12)
            .style(theme::muted_text)
            .into(),
        pick_list(
            sorts,
            Some(Labeled {
                value: view.sort,
                label: i18n.tr(sort_key(view.sort)),
            }),
            |choice: Labeled<TreeSort>| tree_message(TreeMessage::SetSort(choice.value)),
        )
        .text_size(12)
        .padding(3)
        .into(),
        text(i18n.tr("tree-mode"))
            .size(12)
            .style(theme::muted_text)
            .into(),
        pick_list(
            modes,
            Some(Labeled {
                value: view.fast_mode,
                label: i18n.tr(mode_key(view.fast_mode)),
            }),
            |choice: Labeled<TreeFastMode>| tree_message(TreeMessage::SetFastMode(choice.value)),
        )
        .text_size(12)
        .padding(3)
        .into(),
        option_box(
            i18n,
            "tree-short",
            None,
            view.short,
            TreeMessage::SetShort(!view.short),
        ),
    ];
    items.push(option_box(
        i18n,
        "tree-lock",
        Some("tree-lock-tip"),
        view.locked,
        TreeMessage::SetLocked(!view.locked),
    ));
    items.push(training_box(view, i18n));
    items.push(mask::menu::bar(mask, i18n, Platform::current()));
    items.push(
        button(text(i18n.tr("tree-best")).size(12))
            .padding([3, 8])
            .style(theme::command_button(
                ButtonLook::Secondary,
                view.best.is_some(),
            ))
            .on_press(Message::App(AppMessage::Tree(TreeMessage::Best(
                mychess_app::BestMessage::Toggle,
            ))))
            .into(),
    );
    items.push(
        button(text(i18n.tr("tree-open-base")).size(12))
            .padding([3, 8])
            .style(theme::command_button(ButtonLook::Secondary, false))
            .on_press(tree_message(TreeMessage::OpenBaseAsTree))
            .into(),
    );
    if view.searching {
        items.push(
            text(i18n.tr("tree-searching"))
                .size(12)
                .style(theme::muted_text)
                .into(),
        );
    }
    let bar = row::Row::with_children(items)
        .spacing(10)
        .align_y(Alignment::Center)
        .wrap();
    row![bar, close].spacing(8).align_y(Alignment::Start).into()
}

// ---- the panel ----

/// The line that names the database of the tree.
fn target_line(view: &TreeView, i18n: &I18n) -> Option<String> {
    if view.target_is_clipbase {
        return Some(i18n.tr("tree-target-clipbase"));
    }
    let name = view.target_name.clone()?;
    let mut args = FluentArgs::new();
    args.set("name", name);
    Some(i18n.tr_args("tree-target", &args))
}

/// The content of the panel; `mods` are the `(shift, command)` keys held
/// (they choose how a click on a best game changes the selection). The mask
/// part is taken from the snapshot alone (see [`content_masked`]).
#[cfg(test)]
pub(crate) fn content_with<'a>(
    view: &TreeView,
    i18n: &I18n,
    mods: (bool, bool),
) -> Element<'a, Message> {
    content_masked(view, i18n, mods, &MaskCtx::from_view(view))
}

/// The content of the panel with what the shell knows about the masks.
pub(crate) fn content_masked<'a>(
    view: &TreeView,
    i18n: &I18n,
    mods: (bool, bool),
    mask: &MaskCtx,
) -> Element<'a, Message> {
    let main: Element<'a, Message> = match body_message(i18n, &view.body) {
        Some(message) => container(text(message).size(13).style(theme::muted_text))
            .center(Length::Fill)
            .into(),
        None => table(view, i18n, mask),
    };
    let mut page = column![toolbar(view, i18n, mask), cache::bar(&view.cache, i18n)];
    if let Some(busy) = &mask.busy {
        page = page.push(mask::menu::progress(busy, i18n));
    }
    if let Some(name) = target_line(view, i18n) {
        page = page.push(text(name).size(12).style(theme::muted_text));
    }
    if let Some(mask_view) = view.mask.as_ref().filter(|_| mask.open) {
        page = page.push(cells::header(mask_view, mask, i18n));
    }
    let main = container(main).height(Length::FillPortion(3));
    page = page.push(main);
    if let Some(best) = &view.best {
        page = page.push(
            container(best::section(
                best,
                i18n,
                view.searching,
                mods,
                mask.can_merge,
            ))
            .height(Length::FillPortion(2)),
        );
    }
    let mut page = page.spacing(6).padding(8).height(Length::Fill);
    if view.unreadable > 0 {
        let mut args = FluentArgs::new();
        // Numeric, so the plural selector of the message works.
        args.set("count", i64::try_from(view.unreadable).unwrap_or(i64::MAX));
        page = page.push(
            text(i18n.tr_args("tree-unreadable", &args))
                .size(12)
                .style(theme::muted_text),
        );
    }
    page.into()
}

/// The tree panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    // `TreeView` is cheap to build (at most 60 rows) and the elements own
    // their texts, so the temporary can be dropped here.
    let tree = shell.app.tree_view();
    let mods = (shell.ui.games.shift, shell.ui.games.command);
    let mask = MaskCtx::of(shell, &tree);
    panes::body(content_masked(&tree, &shell.i18n, mods, &mask), "tree-pane")
}

#[cfg(test)]
mod tests;
