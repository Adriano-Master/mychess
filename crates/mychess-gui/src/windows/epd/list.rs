//! The virtualized list of positions (spec M7-5c, D3824): only the rows
//! near the viewport become widgets; the others are two spacers of the
//! height they would have.

use iced::alignment::Horizontal;
use iced::widget::{button, column, container, row, scrollable, space, text};
use iced::{Element, Length};
use mychess_app::epd_window::{EpdMessage, EpdTabView};

use super::{EPD_LIST_ID, epd};
use crate::shell::{Message, Shell};
use crate::theme::{self, ButtonLook, RowLook};
use crate::windows::epd::EpdUiMessage;
use crate::windows::panel_kit::{BODY_TEXT, cell};

/// Height of a row of the list.
pub const ROW_HEIGHT: f32 = 24.0;
/// Rows drawn above and below the viewport.
const OVERSCAN: usize = 8;
/// The height assumed before the list reports its own.
const DEFAULT_VIEWPORT: f32 = 600.0;
const NUMBER_W: f32 = 64.0;
const ID_W: f32 = 140.0;

/// The rows `[start, end)` that must be built for a list of `total` rows
/// scrolled `offset` pixels down in a viewport `height` tall.
pub(super) fn window(offset: f32, height: f32, total: usize) -> (usize, usize) {
    let height = if height.is_finite() && height > 0.0 {
        height
    } else {
        DEFAULT_VIEWPORT
    };
    let offset = if offset.is_finite() {
        offset.max(0.0)
    } else {
        0.0
    };
    // Both are small non-negative numbers of rows.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (first, visible) = (
        (offset / ROW_HEIGHT) as usize,
        (height / ROW_HEIGHT).ceil() as usize + 1,
    );
    let start = first.saturating_sub(OVERSCAN).min(total);
    let end = first
        .saturating_add(visible)
        .saturating_add(OVERSCAN)
        .min(total);
    (start, end)
}

/// Pixels that `rows` rows take.
fn spacer(rows: usize) -> f32 {
    // The row counts of the app stay far below 2^24.
    #[allow(clippy::cast_precision_loss)]
    let pixels = rows as f32 * ROW_HEIGHT;
    pixels
}

/// The header and the scrollable rows.
pub(super) fn view<'a>(shell: &'a Shell, tab: &EpdTabView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let ui = &shell.ui.dialogs.epd;
    let header = container(
        row![
            cell(i18n.tr("epd-col-number"), NUMBER_W, Horizontal::Right),
            text(i18n.tr("epd-col-fen"))
                .size(BODY_TEXT)
                .width(Length::Fill),
            cell(i18n.tr("epd-col-id"), ID_W, Horizontal::Left),
        ]
        .spacing(8)
        .padding([2, 8]),
    )
    .style(theme::list_header);
    let (start, end) = window(ui.scroll, ui.view_h, tab.rows);
    let mut body = column![space::vertical().height(Length::Fixed(spacer(start)))];
    for r in shell.app.epd_rows(tab.id, start..end) {
        let selected = tab.selected == Some(r.index);
        let cells = row![
            cell(r.number.to_string(), NUMBER_W, Horizontal::Right),
            text(r.fen).size(BODY_TEXT).width(Length::Fill),
            cell(r.id.unwrap_or_default(), ID_W, Horizontal::Left),
        ]
        .spacing(8);
        let click = button(cells)
            .padding([2, 8])
            .width(Length::Fill)
            .height(Length::Fixed(ROW_HEIGHT))
            .style(theme::command_button(ButtonLook::Secondary, false))
            .on_press(epd(EpdMessage::GoTo(r.index)));
        body = body.push(
            container(click)
                .id(format!("epd-row-{}", r.number))
                .width(Length::Fill)
                .style(theme::list_row(RowLook {
                    zebra: r.number % 2 == 0,
                    selected,
                    ..RowLook::default()
                })),
        );
    }
    body = body.push(space::vertical().height(Length::Fixed(spacer(tab.rows.saturating_sub(end)))));
    let list = scrollable(body)
        .id(EPD_LIST_ID)
        .on_scroll(|viewport| {
            Message::Epd(EpdUiMessage::Scrolled {
                offset: viewport.absolute_offset().y,
                height: viewport.bounds().height,
            })
        })
        .width(Length::Fill)
        .height(Length::Fill);
    column![header, list].height(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_rows_near_the_viewport_are_built() {
        let (start, end) = window(0.0, 480.0, 200_000);
        assert_eq!(start, 0);
        assert!(end <= 40, "{end}");
        let (start, end) = window(ROW_HEIGHT * 1000.0, 480.0, 200_000);
        assert!(start >= 990 && end <= 1040, "{start}..{end}");
    }

    #[test]
    fn the_window_never_passes_the_end_and_survives_bad_numbers() {
        assert_eq!(window(1.0e9, 600.0, 10).0, 10);
        assert_eq!(window(f32::NAN, f32::NAN, 5), (0, 5));
        assert_eq!(window(0.0, 600.0, 0), (0, 0));
    }
}
