//! The variation picker (spec M2-4, D203; Scid `showVars`): the main line
//! and the alternatives with their first moves; `0` to `9` pick one.

use iced::widget::{button, column, text};
use iced::{Element, Length};
use mychess_app::{AppState, GameMessage, Message as AppMessage};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::widgets::modal::{Card, modal};

/// Id of the box.
pub const VARIATIONS_BOX_ID: &str = "variations-box";

fn send(message: GameMessage) -> Message {
    Message::App(AppMessage::Game(message))
}

/// The box for the open picker of the session, if there is one.
pub fn view<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let picker = app.session.chooser()?;
    let mut rows = column![].spacing(4).width(Length::Fill);
    for choice in app.session.variation_choices() {
        let name = if choice.index == 0 {
            i18n.tr("nav-variation-main")
        } else {
            choice.index.to_string()
        };
        let line = choice.san.join(" ");
        let label = text(format!("{name}: {line}"));
        let style = if choice.index == picker.selected {
            button::primary
        } else {
            button::secondary
        };
        rows = rows.push(
            button(label)
                .width(Length::Fill)
                .style(style)
                .on_press(send(GameMessage::ChooserConfirm(Some(choice.index)))),
        );
    }
    Some(modal(Card {
        id: VARIATIONS_BOX_ID,
        title: i18n.tr("variations-title"),
        body: rows.into(),
        buttons: vec![
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(send(GameMessage::CloseVariationChooser))
                .into(),
        ],
        width: 420.0,
    }))
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::{Nav, update};

    use super::*;
    use crate::i18n::Language;

    fn app_with_two_replies() -> AppState {
        let mut app = AppState::default();
        let pos = app.session.position().clone();
        for uci in ["e2e4", "d2d4"] {
            let mv = pos.parse_uci(uci).expect("legal");
            update(
                &mut app,
                AppMessage::Game(GameMessage::PlayMove {
                    mv,
                    options: mychess_app::PlayOptions {
                        force_variation: true,
                    },
                }),
            );
            update(
                &mut app,
                AppMessage::Game(GameMessage::Navigate(Nav::Start)),
            );
        }
        app
    }

    #[test]
    fn nothing_is_shown_without_an_open_picker() {
        assert!(view(&AppState::default(), &I18n::new(Language::EnUs)).is_none());
    }

    #[test]
    fn rows_list_the_main_line_and_variations_and_a_click_enters_one() {
        let mut app = app_with_two_replies();
        update(
            &mut app,
            AppMessage::Game(GameMessage::OpenVariationChooser),
        );
        let element = view(&app, &I18n::new(Language::EnUs)).expect("open");
        let mut sim = Simulator::with_size(Settings::default(), Size::new(700.0, 500.0), element);
        assert!(sim.find("Main line: e4").is_ok());
        sim.click("1: d4").expect("variation row");
        let sent: Vec<Message> = sim.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Game(GameMessage::ChooserConfirm(
                Some(1)
            )))]
        ));
    }
}
