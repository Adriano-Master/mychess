//! The header of the move list: the name of the opening with its ECO code,
//! and the icons of the actions (spec M2-5, D249).

use iced::widget::{button, container, row, text, tooltip};
use iced::{Alignment, Border, Element, Length, Theme};
use mychess_app::{AppState, Command, OpeningName};
use mychess_core::Tags;

use crate::app_api;
use crate::fonts;
use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme::{self, ButtonLook, Tokens};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::windows::notation::NotationMessage;

/// Height of the header in logical pixels (D249).
pub const HEIGHT: f32 = 36.0;

/// Id of the button of the display options.
pub const OPTIONS_ID: &str = "notation-options";
/// Id of the button that copies the game.
pub const COPY_ID: &str = "notation-copy-pgn";
/// Id of the button that saves the game.
pub const SAVE_ID: &str = "notation-save-pgn";

/// What the left side of the header says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HeaderModel {
    /// The name of the opening.
    pub name: Option<String>,
    /// The ECO code, from the opening or (without one) from the tag of the
    /// game.
    pub eco: Option<String>,
}

impl HeaderModel {
    /// Whether there is nothing to say about the opening.
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.eco.is_none()
    }
}

/// The opening to show: the one found along the path to the cursor
/// (`opening`), else only the code of the `ECO` tag of the game, if it has
/// one, else nothing.
pub fn model(opening: Option<&OpeningName>, tags: &Tags) -> HeaderModel {
    match opening {
        Some(found) => HeaderModel {
            name: Some(found.name.clone()),
            eco: Some(found.eco.to_string()),
        },
        None => HeaderModel {
            name: None,
            eco: tags.eco.map(|eco| eco.to_string()),
        },
    }
}

fn chip<'a>(label: String) -> Element<'a, Message> {
    container(text(label).size(12))
        .padding([1, 6])
        .style(|theme: &Theme| {
            let t = Tokens::of(theme);
            container::Style {
                text_color: Some(t.text),
                background: Some(t.raised.into()),
                border: Border {
                    radius: t.radius.chip.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }
        })
        .into()
}

fn action<'a>(
    id: &'static str,
    glyph: Icon,
    tip: String,
    message: Message,
    toggled: bool,
) -> Element<'a, Message> {
    let tone = if toggled {
        IconTone::OnAccent
    } else {
        IconTone::Text
    };
    let body = button(container(icon(glyph, 16.0, tone)).center(Length::Fill))
        .padding(0)
        .width(Length::Fixed(28.0))
        .height(Length::Fixed(28.0))
        .style(theme::command_button(ButtonLook::Secondary, toggled))
        .on_press(message);
    let with_tip = tooltip(
        body,
        container(text(tip).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Bottom,
    );
    container(with_tip).id(id).into()
}

/// The header: opening on the left, actions on the right. The left side is
/// empty when the opening is unknown, so the actions stay in place.
pub fn view<'a>(app: &AppState, i18n: &I18n, options_open: bool) -> Element<'a, Message> {
    let tags = app.session.game().tags();
    let shown = model(app.opening(), tags);
    let mut left = row![].spacing(8).align_y(Alignment::Center);
    if let Some(name) = shown.name {
        left = left.push(
            container(text(name).size(14).font(fonts::NOTO_SANS_BOLD))
                .clip(true)
                .width(Length::Shrink),
        );
    }
    if let Some(eco) = shown.eco {
        left = left.push(chip(eco));
    }
    let actions = row![
        action(
            OPTIONS_ID,
            Icon::Options,
            i18n.tr("notation-opt-display"),
            Message::Notation(NotationMessage::ToggleOptions),
            options_open,
        ),
        action(
            COPY_ID,
            Icon::Copy,
            i18n.tr("notation-copy-pgn"),
            Message::App(app_api::command(Command::CopyPgn)),
            false,
        ),
        action(
            SAVE_ID,
            Icon::Save,
            i18n.tr("notation-save-pgn"),
            Message::App(app_api::command(Command::SaveAs)),
            false,
        ),
    ]
    .spacing(2);
    container(
        row![container(left).width(Length::Fill).clip(true), actions]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .padding([0, 8])
    .height(Length::Fixed(HEIGHT))
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use mychess_core::Eco;
    use pretty_assertions::assert_eq;

    use super::*;

    fn opening() -> OpeningName {
        OpeningName {
            eco: "C20".parse::<Eco>().expect("eco"),
            name: "King's Pawn Game".to_owned(),
        }
    }

    #[test]
    fn a_found_opening_gives_its_name_and_code() {
        let shown = model(Some(&opening()), &Tags::default());
        assert_eq!(shown.name.as_deref(), Some("King's Pawn Game"));
        assert_eq!(shown.eco.as_deref(), Some("C20"));
        assert!(!shown.is_empty());
    }

    #[test]
    fn without_a_match_the_tag_of_the_game_gives_only_the_code() {
        let mut tags = Tags::default();
        tags.eco = Some("B91".parse().expect("eco"));
        let shown = model(None, &tags);
        assert_eq!(shown.name, None);
        assert_eq!(shown.eco.as_deref(), Some("B91"));
    }

    #[test]
    fn without_a_match_or_a_tag_there_is_nothing_to_say() {
        let shown = model(None, &Tags::default());
        assert!(shown.is_empty());
        assert_eq!(shown, HeaderModel::default());
    }

    #[test]
    fn the_opening_of_the_path_wins_over_the_tag() {
        let mut tags = Tags::default();
        tags.eco = Some("B91".parse().expect("eco"));
        assert_eq!(model(Some(&opening()), &tags).eco.as_deref(), Some("C20"));
    }

    fn sim<'a>(app: &AppState, i18n: &I18n, open: bool) -> iced_test::Simulator<'a, Message> {
        iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(360.0, 80.0),
            view(app, i18n, open),
        )
    }

    #[test]
    fn the_three_actions_are_buttons_with_ids_that_send_their_messages() {
        let app = AppState::default();
        let i18n = I18n::new(crate::i18n::Language::EnUs);
        let mut messages = Vec::new();
        for id in [OPTIONS_ID, COPY_ID, SAVE_ID] {
            let mut sim = sim(&app, &i18n, false);
            sim.click(iced::widget::Id::new(id)).expect("the button");
            messages.extend(sim.into_messages());
        }
        assert!(matches!(
            messages.as_slice(),
            [
                Message::Notation(NotationMessage::ToggleOptions),
                Message::App(mychess_app::Message::Command(Command::CopyPgn)),
                Message::App(mychess_app::Message::Command(Command::SaveAs)),
            ]
        ));
    }

    #[test]
    fn the_opening_and_its_code_are_shown_and_without_one_only_the_actions_stay() {
        use mychess_app::{AppState as App, update};
        let i18n = I18n::new(crate::i18n::Language::EnUs);
        let mut app = App::default();
        let book =
            mychess_core::EcoBook::parse("C20 \"King's Pawn Game\" 1.e4 e5 *\n").expect("book");
        app.eco_book = Some(mychess_app::SharedEco::new(book));
        let mut game = mychess_core::Game::new();
        let root = game.root();
        let e4 = {
            let mv = game
                .position_at(root)
                .expect("position")
                .parse_uci("e2e4")
                .expect("move");
            game.add_move(root, mv).expect("e4")
        };
        let mv = game
            .position_at(e4)
            .expect("position")
            .parse_uci("e7e5")
            .expect("move");
        game.add_move(e4, mv).expect("e5");
        app.session.replace_game(game);
        app.refresh_notation();
        drop(update(
            &mut app,
            mychess_app::Message::Game(mychess_app::GameMessage::Navigate(
                mychess_app::Nav::GameEnd,
            )),
        ));
        let mut shown = sim(&app, &i18n, false);
        assert!(shown.find("King's Pawn Game").is_ok());
        assert!(shown.find("C20").is_ok());
        // At the start of the game nothing is named.
        drop(update(
            &mut app,
            mychess_app::Message::Game(mychess_app::GameMessage::Navigate(mychess_app::Nav::Start)),
        ));
        let mut empty = sim(&app, &i18n, false);
        assert!(empty.find("King's Pawn Game").is_err());
        assert!(empty.find(iced::widget::Id::new(OPTIONS_ID)).is_ok());
    }
}
