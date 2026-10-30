//! The "Resign" and "Rematch" buttons of the notation panel (spec M8-12,
//! D3100-D3104). A slim bar over the command bar: icon and short text,
//! shown only while the user has a game to give up or to play again. The
//! buttons send the same actions as the Game menu and the ribbon
//! ([`Action::ResignGame`], [`Action::Rematch`]); which mode answers is
//! decided by [`AppState::resign_mode`] and [`AppState::rematch_mode`], so
//! nothing here reads the engine, the network or the clock.

use iced::widget::{button, container, row, text, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::{Action, AppState};

use crate::i18n::I18n;
use crate::shell::{Message, UiMessage};
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};

/// Id of the bar.
pub const GAME_COMMANDS_ID: &str = "game-commands";
/// Id of the Resign button.
pub const RESIGN_BUTTON_ID: &str = "game-command-resign";
/// Id of the Rematch button.
pub const REMATCH_BUTTON_ID: &str = "game-command-rematch";

fn command<'a>(
    id: &'static str,
    glyph: Icon,
    tone: IconTone,
    label: String,
    tip: String,
    action: Action,
) -> Element<'a, Message> {
    let content = row![icon(glyph, 14.0, tone), text(label).size(12)]
        .spacing(6)
        .align_y(Alignment::Center);
    let body = container(
        button(content)
            .padding([3, 8])
            .style(theme::command_button(ButtonLook::Primary, false))
            .on_press(Message::Ui(UiMessage::Action(action))),
    )
    .id(id);
    tooltip(body, text(tip).size(12), tooltip::Position::Top)
        .style(theme::tooltip_box)
        .into()
}

/// The bar, or `None` when there is nothing to resign or to play again.
pub fn bar<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let resign = app.resign_mode().is_some();
    let rematch = app.rematch_mode().is_some();
    if !resign && !rematch {
        return None;
    }
    let mut buttons = row![].spacing(6).align_y(Alignment::Center);
    if resign {
        buttons = buttons.push(command(
            RESIGN_BUTTON_ID,
            Icon::Resign,
            IconTone::Danger,
            i18n.tr("game-cmd-resign"),
            i18n.tr("game-cmd-resign-tip"),
            Action::ResignGame,
        ));
    }
    if rematch {
        buttons = buttons.push(command(
            REMATCH_BUTTON_ID,
            Icon::Rematch,
            IconTone::Accent,
            i18n.tr("game-cmd-rematch"),
            i18n.tr("game-cmd-rematch-tip"),
            Action::Rematch,
        ));
    }
    Some(
        container(buttons)
            .id(GAME_COMMANDS_ID)
            .padding([2, 8])
            .width(Length::Fill)
            .into(),
    )
}
