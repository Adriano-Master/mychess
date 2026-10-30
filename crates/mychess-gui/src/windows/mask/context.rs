//! The context menu of a row of the table (spec M5-4c, D1830): add or
//! remove the move, comment it, set its markers, color and NAG, and the
//! submenu that adds any legal move of the position.
//!
//! The menu is a card at the pointer, with a second card beside it for the
//! submenu that is open ([`ContextPage`]); both are made of the same
//! [`context_menu`] entries the other popups use.

use fluent_bundle::FluentArgs;
use iced::widget::{button, checkbox, column, row, text};
use iced::{Alignment, Element, Length, Padding, Size};
use mychess_app::{Marker, MarkerSlot, MaskColor, MaskEdit, MaskNag};

use super::state::{ContextMenu, ContextPage, MaskUiMessage};
use super::{cells, color_label, marker_label, nag_label};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme;
use crate::widgets::context_menu::{self, Entry, ROW_HEIGHT};

/// Width of the main card.
const MAIN_WIDTH: f32 = 240.0;
/// Width of the card of a submenu with one column.
const SUB_WIDTH: f32 = 230.0;
/// Width of a column of moves in "add this move".
const MOVE_COLUMN: f32 = 84.0;
/// Moves in a column of "add this move" (as the Scid's `contextMenu`).
pub const MOVES_PER_COLUMN: usize = 10;
/// Space between the two cards.
const GAP: f32 = 2.0;

fn ui(message: MaskUiMessage) -> Message {
    Message::MaskUi(message)
}

/// A row with a glyph and a label.
fn glyph_item<'a>(
    glyph: Option<Element<'a, Message>>,
    label: &str,
    message: Message,
) -> Entry<'a, Message> {
    let mut content = row![].spacing(8).align_y(Alignment::Center);
    if let Some(glyph) = glyph {
        content = content.push(glyph);
    }
    content = content.push(text(label.to_owned()).size(14));
    Entry::Block(
        button(content)
            .width(Length::Fill)
            .height(Length::Fixed(ROW_HEIGHT))
            .padding(Padding::from([4, 10]))
            .style(theme::menu_item)
            .on_press(message)
            .into(),
        ROW_HEIGHT,
    )
}

fn submenu_item<'a>(label: &str, page: ContextPage, enabled: bool) -> Entry<'a, Message> {
    Entry::Item {
        label: format!("{label}  ▸"),
        message: enabled.then(|| ui(MaskUiMessage::ContextPage(page))),
    }
}

/// The rows of the main card.
fn main_entries<'a>(
    menu: &ContextMenu,
    apply_line: bool,
    show_markers: bool,
    i18n: &I18n,
) -> Vec<Entry<'a, Message>> {
    let mut args = FluentArgs::new();
    args.set("move", menu.san.clone());
    vec![
        Entry::Item {
            label: i18n.tr_args("tree-mask-ctx-add", &args),
            message: (!menu.in_mask).then(|| ui(MaskUiMessage::ContextEdit(MaskEdit::Add))),
        },
        Entry::Item {
            label: i18n.tr_args("tree-mask-ctx-remove", &args),
            message: menu
                .in_mask
                .then(|| ui(MaskUiMessage::ContextEdit(MaskEdit::Remove))),
        },
        Entry::Separator,
        Entry::Item {
            label: i18n.tr("tree-mask-ctx-comment-move"),
            message: Some(ui(MaskUiMessage::ContextComment { position: false })),
        },
        Entry::Item {
            label: i18n.tr("tree-mask-ctx-comment-position"),
            message: Some(ui(MaskUiMessage::ContextComment { position: true })),
        },
        submenu_item(
            &i18n.tr("tree-mask-ctx-add-move"),
            ContextPage::AddMove,
            true,
        ),
        Entry::Separator,
        submenu_item(
            &i18n.tr("tree-mask-ctx-marker-1"),
            ContextPage::Marker(MarkerSlot::First),
            show_markers,
        ),
        submenu_item(
            &i18n.tr("tree-mask-ctx-marker-2"),
            ContextPage::Marker(MarkerSlot::Second),
            show_markers,
        ),
        submenu_item(
            &i18n.tr("tree-mask-ctx-color"),
            ContextPage::Color,
            show_markers,
        ),
        submenu_item(
            &i18n.tr("tree-mask-ctx-nag"),
            ContextPage::Nag,
            show_markers,
        ),
        Entry::Separator,
        Entry::Block(
            checkbox(apply_line)
                .label(i18n.tr("tree-mask-ctx-apply-line"))
                .size(14)
                .text_size(14)
                .on_toggle(|_| ui(MaskUiMessage::ToggleApplyLine))
                .into(),
            ROW_HEIGHT,
        ),
    ]
}

/// The rows of the markers of a slot, with their glyph and name.
fn marker_entries<'a>(slot: MarkerSlot, i18n: &I18n) -> Vec<Entry<'a, Message>> {
    let mut entries: Vec<Entry<'a, Message>> = Marker::ALL
        .iter()
        .map(|marker| {
            glyph_item(
                Some(cells::marker_glyph(marker, 14.0)),
                &marker_label(i18n, marker),
                ui(MaskUiMessage::ContextEdit(MaskEdit::Marker {
                    slot,
                    marker: Some(marker.clone()),
                })),
            )
        })
        .collect();
    entries.push(Entry::Separator);
    entries.push(Entry::Item {
        label: i18n.tr("tree-mask-marker-none"),
        message: Some(ui(MaskUiMessage::ContextEdit(MaskEdit::Marker {
            slot,
            marker: None,
        }))),
    });
    entries
}

fn color_entries<'a>(i18n: &I18n) -> Vec<Entry<'a, Message>> {
    MaskColor::STANDARD
        .iter()
        .map(|color| {
            glyph_item(
                Some(cells::swatch(color.clone(), 12.0)),
                &color_label(i18n, color),
                ui(MaskUiMessage::ContextEdit(MaskEdit::Color(color.clone()))),
            )
        })
        .collect()
}

fn nag_entries<'a>(i18n: &I18n) -> Vec<Entry<'a, Message>> {
    let mut entries: Vec<Entry<'a, Message>> = MaskNag::STANDARD
        .iter()
        .map(|nag| Entry::Item {
            label: nag_label(i18n, nag),
            message: Some(ui(MaskUiMessage::ContextEdit(MaskEdit::Nag(Some(
                nag.clone(),
            ))))),
        })
        .collect();
    entries.push(Entry::Separator);
    entries.push(Entry::Item {
        label: i18n.tr("tree-mask-nag-none"),
        message: Some(ui(MaskUiMessage::ContextEdit(MaskEdit::Nag(None)))),
    });
    entries
}

/// The most columns "add this move" has: with more moves than fit in four
/// columns of ten the columns grow taller (the card scrolls, D1905).
const MAX_COLUMNS: usize = 4;

/// The legal moves in columns of ten (four columns at most), as one block,
/// and the width and height it needs.
fn move_columns<'a>(moves: &[String]) -> (Element<'a, Message>, f32, f32) {
    let per_column = MOVES_PER_COLUMN.max(moves.len().div_ceil(MAX_COLUMNS));
    let columns = moves.len().div_ceil(per_column).max(1);
    let tallest = moves.len().min(per_column);
    let mut grid = row![].spacing(0);
    for chunk in moves.chunks(per_column) {
        let mut cell = column![].width(Length::Fixed(MOVE_COLUMN));
        for san in chunk {
            cell = cell.push(
                button(text(san.clone()).size(14))
                    .width(Length::Fill)
                    .height(Length::Fixed(ROW_HEIGHT))
                    .padding(Padding::from([4, 10]))
                    .style(theme::menu_item)
                    .on_press(ui(MaskUiMessage::ContextAddMove(san.clone()))),
            );
        }
        grid = grid.push(cell);
    }
    // At most `MAX_COLUMNS` columns of a few dozen rows.
    #[allow(clippy::cast_precision_loss)]
    let (width, height) = (columns as f32 * MOVE_COLUMN, tallest as f32 * ROW_HEIGHT);
    (grid.into(), width, height)
}

fn sub_entries<'a>(
    page: ContextPage,
    moves: &[String],
    i18n: &I18n,
) -> (Vec<Entry<'a, Message>>, f32) {
    match page {
        ContextPage::Marker(slot) => (marker_entries(slot, i18n), SUB_WIDTH),
        ContextPage::Color => (color_entries(i18n), SUB_WIDTH),
        ContextPage::Nag => (nag_entries(i18n), SUB_WIDTH),
        ContextPage::AddMove => {
            if moves.is_empty() {
                return (
                    vec![Entry::Heading(i18n.tr("tree-mask-ctx-no-moves"))],
                    SUB_WIDTH,
                );
            }
            let (grid, width, height) = move_columns(moves);
            (vec![Entry::Block(grid, height)], width)
        }
    }
}

/// The layer of the open context menu, if any. It only exists while the
/// mask is open (the menu closes with the mask).
pub fn overlay(shell: &Shell) -> Option<Element<'_, Message>> {
    let menu = shell.ui.dialogs.mask.context.as_ref()?;
    if !shell.app.mask_open() {
        return None;
    }
    let i18n = &shell.i18n;
    let show_markers = shell.app.prefs.mask.show_markers;
    let main = main_entries(menu, shell.ui.dialogs.mask.apply_line, show_markers, i18n);
    let window = shell.ui.window_size.unwrap_or(Size::new(1280.0, 800.0));
    let main_size = context_menu::card_size(&main, MAIN_WIDTH);
    let mut size = main_size;
    let main_card = context_menu::card(main, MAIN_WIDTH, window.height);
    let mut cards = row![main_card].spacing(GAP).align_y(Alignment::Start);
    if let Some(page) = menu.page {
        let moves = if page == ContextPage::AddMove {
            shell.app.mask_legal_moves()
        } else {
            Vec::new()
        };
        let (entries, width) = sub_entries(page, &moves, i18n);
        let sub_size = context_menu::card_size(&entries, width);
        size = Size::new(
            main_size.width + GAP + sub_size.width,
            main_size.height.max(sub_size.height),
        );
        cards = cards.push(context_menu::card(entries, width, window.height));
    }
    let size = context_menu::shown_size(size, window);
    let at = context_menu::place(menu.position, size, window);
    Some(context_menu::layer(
        cards.into(),
        at,
        ui(MaskUiMessage::CloseContext),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_moves_make_taller_columns_not_more_than_four() {
        let moves: Vec<String> = (0..218).map(|i| format!("m{i}")).collect();
        let (_, width, height) = move_columns(&moves);
        assert_eq!(width, 4.0 * MOVE_COLUMN);
        assert_eq!(height, 55.0 * ROW_HEIGHT);
    }

    #[test]
    fn moves_go_into_columns_of_ten() {
        let moves: Vec<String> = (0..23).map(|i| format!("m{i}")).collect();
        let (_, width, height) = move_columns(&moves);
        assert_eq!(width, 3.0 * MOVE_COLUMN);
        assert_eq!(height, 10.0 * ROW_HEIGHT);
        let (_, width, height) = move_columns(&(0..4).map(|i| format!("m{i}")).collect::<Vec<_>>());
        assert_eq!(width, MOVE_COLUMN);
        assert_eq!(height, 4.0 * ROW_HEIGHT);
    }
}
