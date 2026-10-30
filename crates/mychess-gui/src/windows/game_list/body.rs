//! The panel: tabs, header, rows and scroll bar (spec M3-4, D536 to D543).

use std::path::Path;

use fluent_bundle::FluentArgs;
use iced::widget::{
    button, column, container, mouse_area, row, scrollable, sensor, space, text, tooltip,
    vertical_slider,
};
use iced::{Alignment, Element, Length, alignment, mouse};
use mychess_app::{
    BaseSlot, BasesMessage, ColumnSetup, GameRow, ListColumn, Message as AppMessage, SlotId,
    SortDir,
};

use super::columns::{Column, horizontal, visible_setups};
use super::keys::{max_first, wheel_rows};
use super::toolbar::toolbar;
use super::update::{DragPayload, GameListMessage, ListMenu, MenuTarget, TabDrag};
use super::{ROW_HEIGHT, SCROLLBAR_WIDTH, TAB_NAME_CHARS};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, RowLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::right_click::RightClick;
use crate::windows::panes;

/// The name shown in a tab: the file name cut to a few characters, or the
/// translated "Clipbase".
pub(super) fn tab_name(i18n: &I18n, id: SlotId, slot: &BaseSlot) -> String {
    if id.is_clipbase() {
        return i18n.tr("games-clipbase");
    }
    let name = slot.path.as_deref().map_or_else(String::new, |path| {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    });
    cut(&name, TAB_NAME_CHARS)
}

/// `text` cut to `max` characters with an ellipsis.
pub(super) fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The text of the counter of a tab: `filtered/total`.
pub(super) fn counter(slot: &BaseSlot) -> String {
    format!("{}/{}", slot.filtered(), slot.total())
}

pub(super) fn full_path(path: &Path) -> String {
    path.display().to_string()
}

pub(super) fn base_tab<'a>(
    shell: &'a Shell,
    id: SlotId,
    slot: &'a BaseSlot,
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let ui = &shell.ui.games;
    let active = shell.app.bases.current() == id;
    let drag = ui.drag;
    // A drop on this tab would copy: a drag that is not from this tab
    // (rows may also be dropped back on their own tab, which does nothing).
    let can_drop = drag.is_some_and(|d| d.from != id);
    let look = theme::TabLook {
        active,
        hovered: ui.hover_tab == Some(id) && drag.is_none(),
        drop_target: can_drop && ui.hover_tab == Some(id) && drag.is_some_and(|d| d.is_dragging()),
        drag_source: drag.is_some_and(|d| d.from == id && d.is_dragging()),
    };
    let mut label = row![
        text(tab_name(i18n, id, slot)).size(13),
        text(counter(slot)).size(12).style(theme::muted_text),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if slot.read_only && !id.is_clipbase() {
        label = label.push(
            text(i18n.tr("games-read-only"))
                .size(11)
                .style(theme::muted_text),
        );
    }
    let surface = container(label)
        .padding([6, 10])
        .style(theme::tab_surface(look));
    // The tab sees the whole gesture: press (a drag may start), enter and
    // exit (the target), release (a click selects, a drop copies). What
    // the release means is decided here from the drag in progress, since
    // a mouse area publishes one fixed message.
    let released = if ui.drag_cancelled {
        Message::Games(GameListMessage::DragEnd)
    } else if can_drop {
        Message::Games(GameListMessage::TabDrop(id))
    } else if drag.is_some_and(|d| d.payload == DragPayload::Selection) {
        Message::Games(GameListMessage::DragEnd)
    } else {
        Message::Games(GameListMessage::TabClick(id))
    };
    let interaction = match drag {
        Some(d) if d.is_dragging() && can_drop => mouse::Interaction::Copy,
        Some(d) if d.is_dragging() => mouse::Interaction::Grabbing,
        _ => mouse::Interaction::Pointer,
    };
    let select = mouse_area(surface)
        .on_press(Message::Games(GameListMessage::TabPress(id)))
        .on_release(released)
        .on_enter(Message::Games(GameListMessage::TabHover(Some(id))))
        .on_exit(Message::Games(GameListMessage::TabHover(None)))
        .interaction(interaction);
    let select: Element<'a, Message> = match &slot.path {
        Some(path) if drag.is_none() => tooltip(
            select,
            container(text(full_path(path)).size(12))
                .padding([4, 8])
                .style(theme::tooltip_box),
            tooltip::Position::Bottom,
        )
        .into(),
        _ => select.into(),
    };
    let select: Element<'a, Message> = RightClick::new(select, move |window, _| {
        Message::Games(GameListMessage::Menu(ListMenu {
            target: MenuTarget::Tab(id),
            position: window,
        }))
    })
    .into();
    let close_key = if id.is_clipbase() {
        "games-clear-clipbase"
    } else {
        "games-close-base"
    };
    let close = tooltip(
        button(text("×").size(14))
            .padding([4, 6])
            .style(theme::tab_button(false))
            .on_press(Message::App(AppMessage::Bases(BasesMessage::Close(id)))),
        container(text(i18n.tr(close_key)).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Bottom,
    );
    let underline = container(text(""))
        .width(Length::Fill)
        .height(Length::Fixed(2.0))
        .style(theme::tab_underline(active));
    container(column![
        row![select, close].align_y(Alignment::Center),
        underline
    ])
    .id(format!("games-tab-{}", id.number()))
    .into()
}

/// The words of the drag, shown at the end of the strip while the pointer
/// is over another tab: "Copy 12 games from A to B".
fn drag_hint<'a>(shell: &Shell) -> Option<Element<'a, Message>> {
    let drag = shell.ui.games.drag.filter(TabDrag::is_dragging)?;
    let to = drag.over?;
    let from_slot = shell.app.bases.slot(drag.from)?;
    let to_slot = shell.app.bases.slot(to)?;
    let i18n = &shell.i18n;
    let count = match drag.payload {
        DragPayload::Filter => from_slot.filtered(),
        DragPayload::Selection => u32::try_from(from_slot.list.selection.len()).unwrap_or(u32::MAX),
    };
    let mut args = FluentArgs::new();
    args.set("count", count);
    args.set("from", tab_name(i18n, drag.from, from_slot));
    args.set("to", tab_name(i18n, to, to_slot));
    Some(
        text(i18n.tr_args("games-drag-hint", &args))
            .size(12)
            .style(theme::accent_text)
            .into(),
    )
}

pub(super) fn tab_bar(shell: &Shell) -> Element<'_, Message> {
    let mut tabs = row![].spacing(4).padding([4, 8]).align_y(Alignment::Center);
    for (id, slot) in shell.app.bases.iter() {
        tabs = tabs.push(base_tab(shell, id, slot));
    }
    if let Some(hint) = drag_hint(shell) {
        tabs = tabs.push(hint);
    }
    scrollable(tabs)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new()
                .width(3)
                .scroller_width(3)
                .margin(0),
        ))
        .into()
}

pub(super) fn hairline<'a>() -> Element<'a, Message> {
    container(text(""))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(theme::divider)
        .into()
}

/// the column.
pub(super) fn cell<'a>(content: String, setup: ColumnSetup) -> Element<'a, Message> {
    container(
        text(content)
            .size(13)
            .width(Length::Fill)
            .align_x(horizontal(setup.align))
            .wrapping(text::Wrapping::None),
    )
    .width(Length::FillPortion(setup.chars))
    .padding([0, 6])
    .clip(true)
    .into()
}

/// The header cell of a column: the name, the arrow of the sort and the
/// clicks (left sorts, right opens the menu of the column).
pub(super) fn header_cell<'a>(
    i18n: &I18n,
    setup: ColumnSetup,
    sort: Option<(ListColumn, SortDir)>,
) -> Element<'a, Message> {
    let column = Column::of(setup.column);
    let mut label = row![
        text(i18n.tr(column.label_key()))
            .size(13)
            .wrapping(text::Wrapping::None)
    ]
    .spacing(2)
    .align_y(Alignment::Center);
    if let Some((sorted, dir)) = sort
        && sorted == setup.column
    {
        let arrow = match dir {
            SortDir::Ascending => Icon::SortUp,
            SortDir::Descending => Icon::SortDown,
        };
        label = label.push(icon(arrow, 12.0, IconTone::Accent));
    }
    let body = container(label)
        .width(Length::FillPortion(setup.chars))
        .align_x(horizontal(setup.align))
        .padding([0, 6])
        .clip(true);
    let sortable = setup.column.is_sortable();
    let mut area = mouse_area(body);
    if sortable {
        area = area.on_press(Message::Games(GameListMessage::Header(column)));
    }
    let area: Element<'a, Message> = if sortable {
        tooltip(
            area,
            container(text(i18n.tr("games-tip-sort")).size(12))
                .padding([4, 8])
                .style(theme::tooltip_box),
            tooltip::Position::Bottom,
        )
        .into()
    } else {
        area.into()
    };
    RightClick::new(area, move |window, _| {
        Message::Games(GameListMessage::Menu(ListMenu {
            target: MenuTarget::Header(column),
            position: window,
        }))
    })
    .into()
}

pub(super) fn header_row<'a>(
    i18n: &I18n,
    setups: &[ColumnSetup],
    sort: Option<(ListColumn, SortDir)>,
) -> Element<'a, Message> {
    let cells = setups.iter().map(|setup| header_cell(i18n, *setup, sort));
    container(row(cells).align_y(Alignment::Center))
        .id("games-header")
        .padding([0, 4])
        .height(Length::Fixed(ROW_HEIGHT))
        .align_y(alignment::Vertical::Center)
        .style(theme::list_header)
        .into()
}

pub(super) fn game_row<'a>(
    i18n: &I18n,
    setups: &[ColumnSetup],
    game: &GameRow,
    look: RowLook,
    slot: SlotId,
) -> Element<'a, Message> {
    let cells: Vec<Element<'a, Message>> = if game.unreadable {
        let first = setups
            .first()
            .copied()
            .unwrap_or(ColumnSetup::new(ListColumn::Number));
        vec![
            cell(game.number.to_string(), first),
            container(
                text(i18n.tr("games-unreadable"))
                    .size(13)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::FillPortion(
                setups.iter().skip(1).map(|c| c.chars).sum::<u16>().max(1),
            ))
            .padding([0, 6])
            .clip(true)
            .into(),
        ]
    } else {
        setups
            .iter()
            .map(|setup| cell(Column::of(setup.column).cell(game), *setup))
            .collect()
    };
    let body = container(row(cells).align_y(Alignment::Center))
        .id(format!("games-row-{}", game.number))
        .padding([0, 4])
        .width(Length::Fill)
        .height(Length::Fixed(ROW_HEIGHT))
        .align_y(alignment::Vertical::Center)
        .style(theme::list_row(look));
    mouse_area(body)
        .on_enter(Message::Games(GameListMessage::RowHover(game.number)))
        .on_press(Message::Games(GameListMessage::Click {
            slot,
            number: game.number,
        }))
        .on_double_click(Message::Games(GameListMessage::Open {
            slot,
            number: game.number,
        }))
        .on_release(Message::Games(GameListMessage::RowRelease {
            slot,
            number: game.number,
        }))
        .into()
}

pub(super) fn empty_note<'a>(i18n: &I18n, slot: &BaseSlot) -> Element<'a, Message> {
    let key = if slot.total() == 0 {
        "games-empty"
    } else {
        "games-empty-filter"
    };
    container(text(i18n.tr(key)).style(theme::muted_text))
        .center_x(Length::Fill)
        .padding(16)
        .into()
}

/// The scroll bar: a vertical slider turned upside down (its minimum is at
/// the bottom), or a blank strip when everything fits.
pub(super) fn scrollbar<'a>(
    id: SlotId,
    first: u32,
    filtered: u32,
    rows: usize,
) -> Element<'a, Message> {
    let last = max_first(filtered, rows);
    if last <= 1 {
        return space::horizontal().width(SCROLLBAR_WIDTH).into();
    }
    let first = first.clamp(1, last);
    vertical_slider(1..=last, last + 1 - first, move |value| {
        Message::Games(GameListMessage::ScrollTo {
            slot: id,
            first: last + 1 - value.clamp(1, last),
        })
    })
    .width(SCROLLBAR_WIDTH)
    .style(theme::list_scrollbar)
    .into()
}

pub(super) fn list_body(shell: &Shell) -> Element<'_, Message> {
    let id = shell.app.bases.current();
    let Some(slot) = shell.app.bases.slot(id) else {
        return space::vertical().into();
    };
    let i18n = &shell.i18n;
    let ui = &shell.ui.games;
    let setups = visible_setups(&shell.app.prefs.game_list.columns, ui.width);
    let rows = u32::try_from(ui.rows).unwrap_or(u32::MAX);
    let games = shell.app.bases.list_page(id, slot.list.first, rows);
    let numbers: Vec<u32> = games.iter().map(|g| g.number).collect();
    let mut list = column![].width(Length::Fill);
    for (index, game) in games.iter().enumerate() {
        let look = RowLook {
            zebra: usize::try_from(slot.list.first)
                .unwrap_or(0)
                .wrapping_add(index)
                % 2
                == 1,
            selected: slot.list.selection.contains(&game.number),
            current: slot.current_game == Some(game.number),
            dimmed: game.deleted || game.unreadable,
        };
        list = list.push(game_row(i18n, &setups, game, look, id));
    }
    if games.is_empty() {
        list = list.push(empty_note(i18n, slot));
    }
    let area = mouse_area(
        container(list)
            .id("games-list")
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true),
    )
    .on_scroll(move |delta| {
        Message::Games(GameListMessage::Scroll {
            slot: id,
            rows: wheel_rows(delta),
        })
    });
    let area = RightClick::new(area, move |window, local| {
        // The row under the pointer; the blank part of the list has none.
        let index = (local.y.max(0.0) / ROW_HEIGHT).floor();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let number = numbers.get(index as usize).copied();
        Message::Games(match number {
            Some(number) => GameListMessage::Menu(ListMenu {
                target: MenuTarget::Row(number),
                position: window,
            }),
            None => GameListMessage::CloseMenu,
        })
    });
    let area = sensor(area).on_resize(|size| Message::Games(GameListMessage::Viewport(size)));
    row![
        column![header_row(i18n, &setups, slot.list.sort), area]
            .width(Length::Fill)
            .height(Length::Fill),
        column![
            space::vertical().height(Length::Fixed(ROW_HEIGHT)),
            scrollbar(id, slot.list.first, slot.filtered(), ui.rows),
        ]
        .height(Length::Fill),
    ]
    .padding([0, 4])
    .into()
}

/// The panel: the tabs of the open databases, the toolbar and the list of
/// the selected one.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let id = shell.app.bases.current();
    let mut page = column![tab_bar(shell), hairline()];
    if let Some(slot) = shell.app.bases.slot(id) {
        page = page.push(toolbar(shell, id, slot));
    }
    page = page.push(list_body(shell));
    let body = panes::body(page, "games-pane");
    mouse_area(body)
        .on_enter(Message::Games(GameListMessage::Hover(true)))
        .on_exit(Message::Games(GameListMessage::Hover(false)))
        .into()
}
