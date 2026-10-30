//! The title bar of a pane of the grid and the context menu of its tabs
//! (spec M2-6, D1003, D1006, D1011).
//!
//! Layout of the bar: `[grip] [tab][tab]...` with, on the right edge, the
//! close button when the group has a single panel. In a group of two or
//! more, every tab has its own close button and a right click on it opens a
//! menu with "Detach". The iced pane grid starts a drag only from the free
//! space of the bar (the title content and the controls are not a pick
//! area), so the tabs never start one (D1006, D1019).

use iced::widget::pane_grid::{Controls, TitleBar};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text, tooltip};
use iced::{Alignment, Element, Length, Size};

use crate::shell::{Message, Shell, UiMessage};
use crate::theme::{self, ButtonLook};
use crate::widgets::context_menu::{self, Entry};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::right_click::RightClick;
use crate::windows::layout::{Group, PaneKind};

/// Width of the context menu of a tab.
const MENU_WIDTH: f32 = 180.0;

/// Width of the close button (12 px icon plus 5 px padding on each side). It
/// is fixed so a narrow group never squeezes it (D1029).
const CLOSE_WIDTH: f32 = 22.0;

/// Rough width of a character of a tab label at size 13, used to cap the
/// tab strip at its natural width (D1029).
const CHAR_WIDTH: f32 = 8.0;

/// The Fluent key of the title or tab label of a panel (the three panels of
/// the notation family keep the labels the tabs had, D1001).
pub fn label_key(kind: PaneKind) -> &'static str {
    match kind {
        PaneKind::Notation => "notation-tab-moves",
        PaneKind::Info => "notation-tab-info",
        PaneKind::Comment => "notation-tab-comment",
        PaneKind::GameList => "pane-games",
        PaneKind::Analysis => "pane-analysis",
        PaneKind::Graph => "pane-graph",
        PaneKind::Tree => "pane-tree",
        PaneKind::Tournament => "pane-tournament",
        PaneKind::Report => "pane-report",
        PaneKind::Book => "pane-book",
        PaneKind::Tablebase => "pane-tablebase",
        PaneKind::Crosstable => "pane-crosstable",
        PaneKind::FilterStats => "pane-filter-stats",
        PaneKind::RatingGraph => "pane-rating-graph",
        PaneKind::PieceTracker => "pane-piece-tracker",
        PaneKind::Epd => "pane-epd",
        // The board, and any panel a newer application may add.
        _ => "pane-board",
    }
}

/// Id of the tab of the panel (for tests and automation).
pub fn tab_id(kind: PaneKind) -> &'static str {
    match kind {
        PaneKind::Notation => "notation-tab-moves",
        PaneKind::Info => "notation-tab-info",
        PaneKind::Comment => "notation-tab-comment",
        PaneKind::GameList => "tab-games",
        PaneKind::Analysis => "tab-analysis",
        PaneKind::Graph => "tab-graph",
        PaneKind::Tree => "tab-tree",
        PaneKind::Tournament => "tab-tournament",
        PaneKind::Report => "tab-report",
        PaneKind::Book => "tab-book",
        PaneKind::Tablebase => "tab-tablebase",
        PaneKind::Crosstable => "tab-crosstable",
        PaneKind::FilterStats => "tab-filter-stats",
        PaneKind::RatingGraph => "tab-rating-graph",
        PaneKind::PieceTracker => "tab-piece-tracker",
        PaneKind::Epd => "tab-epd",
        _ => "tab-board",
    }
}

/// Id of the close button of the panel.
pub fn close_id(kind: PaneKind) -> &'static str {
    match kind {
        PaneKind::Notation => "pane-close-notation",
        PaneKind::Info => "pane-close-info",
        PaneKind::Comment => "pane-close-comment",
        PaneKind::GameList => "pane-close-games",
        PaneKind::Analysis => "pane-close-analysis",
        PaneKind::Graph => "pane-close-graph",
        PaneKind::Tree => "pane-close-tree",
        PaneKind::Tournament => "pane-close-tournament",
        PaneKind::Report => "pane-close-report",
        PaneKind::Book => "pane-close-book",
        PaneKind::Tablebase => "pane-close-tablebase",
        PaneKind::Crosstable => "pane-close-crosstable",
        PaneKind::FilterStats => "pane-close-filter-stats",
        PaneKind::RatingGraph => "pane-close-rating-graph",
        PaneKind::PieceTracker => "pane-close-piece-tracker",
        PaneKind::Epd => "pane-close-epd",
        _ => "pane-close-board",
    }
}

/// Whether the comment of the cursor is not empty (the dot on the tab).
fn has_comment(shell: &Shell) -> bool {
    let session = &shell.app.session;
    session
        .game()
        .comment(session.cursor())
        .ok()
        .flatten()
        .is_some_and(|c| !c.trim().is_empty())
}

/// The small "x" that closes the panel; the board has none.
fn close_button(shell: &Shell, kind: PaneKind) -> Option<Element<'_, Message>> {
    if kind == PaneKind::Board {
        return None;
    }
    let button = button(icon(Icon::Close, 12.0, IconTone::Muted))
        .padding(5)
        .style(theme::command_button(ButtonLook::Secondary, false))
        .on_press(Message::Ui(UiMessage::ClosePane(kind)));
    let tip = container(text(shell.i18n.tr("pane-close")).size(12))
        .padding([4, 8])
        .style(theme::tooltip_box);
    Some(
        container(tooltip(button, tip, tooltip::Position::Bottom))
            .width(Length::Fixed(CLOSE_WIDTH))
            .id(close_id(kind))
            .into(),
    )
}

/// Number of characters of the label of a tab (at least 1).
fn label_chars(shell: &Shell, kind: PaneKind) -> usize {
    shell.i18n.tr(label_key(kind)).chars().count().max(1)
}

/// The natural width of a tab, an upper estimate (label, paddings, comment
/// dot and close button).
fn tab_width(shell: &Shell, kind: PaneKind) -> f32 {
    let chars = f32::from(u16::try_from(label_chars(shell, kind)).unwrap_or(u16::MAX));
    let dot = if kind == PaneKind::Comment { 14.0 } else { 0.0 };
    let close = if kind == PaneKind::Board {
        0.0
    } else {
        CLOSE_WIDTH
    };
    chars * CHAR_WIDTH + 16.0 + dot + close
}

/// One tab. Its cell shares the strip with the others by `FillPortion`: in a
/// narrow group the label (clipped) gives way and the close button, of fixed
/// width, stays reachable (D1029).
fn tab(shell: &Shell, kind: PaneKind, active: bool) -> Element<'_, Message> {
    let mut label = row![
        text(shell.i18n.tr(label_key(kind)))
            .size(13)
            .wrapping(Wrapping::None)
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    if kind == PaneKind::Comment && has_comment(shell) {
        label = label.push(text("●").size(8).style(theme::accent_text));
    }
    let select = button(container(label).width(Length::Fill).clip(true))
        .width(Length::Fill)
        .padding([3, 8])
        .style(theme::tab_button(active))
        .on_press(Message::Ui(UiMessage::SelectTab { kind }));
    let mut head = row![select].width(Length::Fill).align_y(Alignment::Center);
    if let Some(close) = close_button(shell, kind) {
        head = head.push(close);
    }
    let underline = container(text(""))
        .width(Length::Fill)
        .height(Length::Fixed(2.0))
        .style(theme::tab_underline(active));
    let portion = u16::try_from(label_chars(shell, kind) + 4).unwrap_or(u16::MAX);
    let cell = container(column![head, underline].spacing(0).width(Length::Fill))
        .width(Length::FillPortion(portion))
        .id(tab_id(kind));
    RightClick::new(cell, move |position, _| {
        Message::Ui(UiMessage::OpenTabMenu { kind, position })
    })
    .into()
}

/// The title bar of the pane showing `group`.
pub fn title_bar<'a>(shell: &'a Shell, group: &'a Group) -> TitleBar<'a, Message> {
    let grip = icon(Icon::Grip, 14.0, IconTone::Muted);
    if let [only] = group.tabs.as_slice() {
        let content = row![
            grip,
            text(shell.i18n.tr(label_key(*only)))
                .size(12)
                .wrapping(Wrapping::None)
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        let bar = TitleBar::new(content)
            .padding([4, 8])
            .style(theme::pane_title);
        return match close_button(shell, *only) {
            Some(close) => bar.controls(Controls::new(close)),
            None => bar,
        };
    }
    let active = group.active_kind();
    let mut tabs = row![]
        .spacing(2)
        .width(Length::Fill)
        .align_y(Alignment::Center);
    let mut natural = 4.0;
    for kind in &group.tabs {
        tabs = tabs.push(tab(shell, *kind, *kind == active));
        natural += tab_width(shell, *kind) + 2.0;
    }
    // The strip is `Fixed` at its natural width, which the layout clamps to
    // the room there is: with room it leaves free space on the right to grab
    // the group (D1026); without, it takes what is there and the tabs share
    // it. A `Fixed` width (unlike `Fill`) also ends the "compression" the
    // `Shrink` bar imposes, so the `FillPortion` cells get real widths (D1029).
    let strip = container(tabs).width(Length::Fixed(natural)).clip(true);
    let content = row![grip, strip]
        .width(Length::Shrink)
        .spacing(4)
        .align_y(Alignment::Center);
    TitleBar::new(content)
        .padding([2, 8])
        .style(theme::pane_title)
}

/// The layer of the open context menu of a tab, if any.
pub fn menu_overlay(shell: &Shell) -> Option<Element<'_, Message>> {
    let menu = shell.ui.tab_menu?;
    let alone = shell
        .ui
        .panes
        .iter()
        .any(|(_, group)| group.contains(menu.kind) && group.tabs.len() < 2);
    let entries = vec![Entry::Item {
        label: shell.i18n.tr("tab-detach"),
        message: (!alone).then_some(Message::Ui(UiMessage::DetachTab(menu.kind))),
    }];
    let window = shell.ui.window_size.unwrap_or(Size::new(1280.0, 800.0));
    let size = context_menu::shown_size(context_menu::card_size(&entries, MENU_WIDTH), window);
    let at = context_menu::place(menu.position, size, window);
    let card = context_menu::card(entries, MENU_WIDTH, window.height);
    Some(context_menu::layer(
        card,
        at,
        Message::Ui(UiMessage::CloseTabMenu),
    ))
}
