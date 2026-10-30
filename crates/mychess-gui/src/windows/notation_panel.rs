//! The panels of the notation family (spec M2-6, D1001): the move list, the
//! game information and the comment editor are panels of their own, and
//! the command bar with the move field (M2-5, D225) is drawn under the
//! selected tab of the first group that has the move list (D1002).

use iced::widget::{column, container, scrollable, text};
use iced::{Element, Length};

use super::{comment_editor, game_commands, game_info, nav_bar, notation, panes};
use crate::keymap::Platform;
use crate::shell::{Message, Shell};
use crate::theme;
use crate::windows::layout::PaneKind;

/// The window height under which the move field is hidden (D228); typing
/// over the board still enters moves.
pub const MOVE_FIELD_MIN_HEIGHT: f32 = 560.0;

/// Whether the move field fits under the command bar for a window of this
/// height (`None`: not known yet, so it fits).
pub fn move_field_fits(window_height: Option<f32>) -> bool {
    window_height.is_none_or(|height| height >= MOVE_FIELD_MIN_HEIGHT)
}

fn hairline<'a>() -> Element<'a, Message> {
    container(text(""))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(theme::divider)
        .into()
}

/// The command bar and, when the window is tall enough, the move field
/// under it (D225, D228). It sits at the bottom of the notation panel, or
/// under the board when that panel is closed (D215).
pub fn footer(shell: &Shell) -> Element<'_, Message> {
    let bar = nav_bar::command_bar(&shell.app, &shell.i18n, Platform::current());
    let mut page = column![].spacing(2);
    // Resign and Rematch, only while there is a game for them (M8-12).
    if let Some(commands) = game_commands::bar(&shell.app, &shell.i18n) {
        page = page.push(commands);
    }
    page = page.push(bar);
    if move_field_fits(shell.ui.window_size.map(|size| size.height)) {
        page = page.push(container(nav_bar::move_field(&shell.app, &shell.i18n)).padding([2, 8]));
    }
    container(page).padding([4, 0]).width(Length::Fill).into()
}

/// The bodies of the panels that used to be tabs of one panel (spec M2-6,
/// D1001): the move list, the game information and the comment editor.
/// `with_bar` adds the command bar under the body (D1002).
pub fn view(shell: &Shell, kind: PaneKind, with_bar: bool) -> Element<'_, Message> {
    let (content, id): (Element<'_, Message>, &'static str) = match kind {
        PaneKind::Info => (
            scrollable(
                container(game_info::view(&shell.app.game_info(), &shell.i18n))
                    .width(Length::Fill)
                    .height(Length::Shrink),
            )
            .height(Length::Fill)
            .into(),
            "info-pane",
        ),
        PaneKind::Comment => (
            scrollable(comment_editor::view(shell))
                .height(Length::Fill)
                .into(),
            "comment-pane",
        ),
        _ => (notation::moves_tab(shell), "notation-pane"),
    };
    let page = container(content).width(Length::Fill).height(Length::Fill);
    if with_bar {
        under_bar(shell, page.into(), id)
    } else {
        panes::body(page, id)
    }
}

/// `body` with the command bar and the move field under it, in the panel
/// look (the group that owns the bar draws it under any tab, D1002).
pub fn under_bar<'a>(
    shell: &'a Shell,
    body: Element<'a, Message>,
    id: &'static str,
) -> Element<'a, Message> {
    let page = column![
        container(body).width(Length::Fill).height(Length::Fill),
        hairline(),
        footer(shell),
    ];
    panes::body(page, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_move_field_needs_a_tall_enough_window() {
        assert!(move_field_fits(None));
        assert!(move_field_fits(Some(560.0)));
        assert!(!move_field_fits(Some(559.0)));
    }
}
