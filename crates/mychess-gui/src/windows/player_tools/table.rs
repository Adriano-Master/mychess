//! A virtualized table with clickable headers, shared by the three lists of
//! the player tools: only the rows on screen (plus a margin) become widgets.

use iced::widget::{button, column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Length};

use super::{ListId, PlayerToolsMessage};
use crate::shell::Message;
use crate::theme::{self, RowLook};
use crate::widgets::icon::{Icon, IconTone, icon};

/// The height of a row of the finders.
pub const ROW_HEIGHT: f32 = 24.0;
/// Rows built above and below the visible ones.
const MARGIN: usize = 3;

/// The id of the scrollable of a list.
pub fn scroll_id(list: ListId) -> &'static str {
    match list {
        ListId::Players => "player-finder-list",
        ListId::Tournaments => "tournament-finder-list",
        ListId::Spell => "spell-check-list",
    }
}

/// The first row and the number of rows to build for `total` rows of
/// `row_height` pixels scrolled by `scroll`, in a list `height` pixels high.
pub fn window(scroll: f32, total: usize, height: f32, row_height: f32) -> (usize, usize) {
    let scroll = if scroll.is_finite() {
        scroll.max(0.0)
    } else {
        0.0
    };
    // Small non-negative quotients: the casts cannot wrap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let first = (scroll / row_height).floor() as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let fit = (height / row_height).ceil() as usize;
    let first = first.saturating_sub(MARGIN).min(total);
    let count = (fit + 2 * MARGIN + 1).min(total - first);
    (first, count)
}

/// How many rows a page of the list holds (for `PageUp` and `PageDown`).
pub fn page_rows(height: f32, row_height: f32) -> usize {
    // A positive, small quotient.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rows = (height / row_height).floor() as usize;
    rows.max(1)
}

/// The scroll offset that brings `row` into a list `height` high, from the
/// current offset `scroll`; `None` when it is in view already.
pub fn reveal(scroll: f32, row: usize, height: f32, row_height: f32) -> Option<f32> {
    // Row numbers are below 100 000: exact in an `f32`.
    #[allow(clippy::cast_precision_loss)]
    let top = row as f32 * row_height;
    let bottom = top + row_height;
    if top < scroll {
        Some(top)
    } else if bottom > scroll + height {
        Some(bottom - height)
    } else {
        None
    }
}

fn spacer<'a>(rows: usize, row_height: f32) -> Element<'a, Message> {
    // Row counts are small: exact in an `f32`.
    #[allow(clippy::cast_precision_loss)]
    let height = rows as f32 * row_height;
    space().height(Length::Fixed(height)).into()
}

/// The scrollable list: `build(index)` makes row `index`.
pub fn rows<'a>(
    list: ListId,
    total: usize,
    scroll: f32,
    height: f32,
    row_height: f32,
    build: impl Fn(usize) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let (first, count) = window(scroll, total, height, row_height);
    let mut body = column![spacer(first, row_height)].width(Length::Fill);
    for index in first..first + count {
        body = body.push(build(index));
    }
    body = body.push(spacer(total.saturating_sub(first + count), row_height));
    scrollable(body)
        .id(scroll_id(list))
        .height(Length::Fixed(height))
        .on_scroll(move |viewport| {
            Message::PlayerTools(PlayerToolsMessage::Scrolled(
                list,
                viewport.absolute_offset().y,
            ))
        })
        .into()
}

/// A row of the list: `cells` in a zebra or selected container, a click
/// sends `on_press` (and a double click `on_double`).
pub fn row_box(
    cells: Vec<Element<'_, Message>>,
    index: usize,
    selected: bool,
    on_press: Message,
    on_double: Option<Message>,
) -> Element<'_, Message> {
    let body = container(row(cells).spacing(8).align_y(Alignment::Center))
        .height(Length::Fixed(ROW_HEIGHT))
        .padding([0, 6])
        .width(Length::Fill)
        .style(theme::list_row(RowLook {
            zebra: index % 2 == 1,
            selected,
            ..RowLook::default()
        }));
    let area = iced::widget::mouse_area(body).on_press(on_press);
    match on_double {
        Some(message) => area.on_double_click(message).into(),
        None => area.into(),
    }
}

/// A cell of text of a row.
pub fn cell<'a>(content: impl Into<String>, width: Length) -> Element<'a, Message> {
    text(content.into()).size(13).width(width).into()
}

/// A header cell. A button when `on_press` is given: it shows an arrow when
/// the list is ordered by it.
pub fn header<'a>(
    label: String,
    width: Length,
    sorted: Option<bool>,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let mut content = row![text(label).size(12)]
        .spacing(4)
        .align_y(Alignment::Center);
    if let Some(descending) = sorted {
        content = content.push(icon(
            if descending {
                Icon::SortDown
            } else {
                Icon::SortUp
            },
            12.0,
            IconTone::Text,
        ));
    }
    match on_press {
        Some(message) => button(content)
            .padding([2, 4])
            .width(width)
            .style(theme::command_button(theme::ButtonLook::Secondary, false))
            .on_press(message)
            .into(),
        None => container(content).padding([2, 4]).width(width).into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_list_builds_a_bounded_window() {
        let (first, count) = window(0.0, 100_000, 400.0, ROW_HEIGHT);
        assert_eq!(first, 0);
        assert!(count < 40, "{count}");
        let (first, count) = window(240_000.0, 100_000, 400.0, ROW_HEIGHT);
        assert!(first > 9_000 && first + count <= 100_000);
    }

    #[test]
    fn a_short_list_is_built_whole() {
        assert_eq!(window(0.0, 5, 400.0, ROW_HEIGHT), (0, 5));
        assert_eq!(window(f32::NAN, 0, 400.0, ROW_HEIGHT), (0, 0));
    }

    #[test]
    fn reveal_moves_only_when_the_row_is_out_of_view() {
        assert_eq!(reveal(0.0, 2, 240.0, 24.0), None);
        assert_eq!(reveal(0.0, 20, 240.0, 24.0), Some(264.0));
        assert_eq!(reveal(480.0, 3, 240.0, 24.0), Some(72.0));
    }
}
