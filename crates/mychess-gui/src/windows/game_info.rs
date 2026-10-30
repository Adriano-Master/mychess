//! The game information panel (spec M2-4, D183-D185, rules 30-36; Scid
//! `updateGameinfo`): players, event, opening, previous and next move,
//! material, comment and variations. Items that are unknown (`?`) open the
//! tags dialog on the right field.

use fluent_bundle::FluentArgs;
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};
use mychess_app::{
    GameInfoView, GameMessage, Message as AppMessage, Nav, NextMove, PlayerLine, PreviousMove,
    TagField, nag_text,
};
use mychess_core::{GameResult, Nag};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::windows::dialog_ui::DialogMessage;
use crate::windows::notation::NotationMessage;

fn nav(nav: Nav) -> Message {
    Message::App(AppMessage::Game(GameMessage::Navigate(nav)))
}

fn link<'a>(label: String, message: Message) -> Element<'a, Message> {
    button(text(label))
        .padding(0)
        .style(button::text)
        .on_press(message)
        .into()
}

fn nags_text(nags: &[Nag]) -> String {
    nags.iter()
        .map(|n| nag_text(*n, true).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `12.` or `12...` before a move.
fn numbered(number: u32, black: bool, san: &str, nags: &[Nag]) -> String {
    let dots = if black { "..." } else { "." };
    format!("{number}{dots} {san}{}", nags_text(nags))
}

fn player<'a>(line: &PlayerLine, field: TagField, i18n: &I18n) -> Element<'a, Message> {
    match &line.name {
        Some(name) => {
            let label = match line.rating {
                Some(elo) => format!("{name} ({elo})"),
                None => name.clone(),
            };
            text(label).size(18).into()
        }
        None => link(
            i18n.tr("info-unknown"),
            Message::Dialog(DialogMessage::OpenHeader(Some(field))),
        ),
    }
}

fn result_text(result: GameResult) -> String {
    format!("({})", result.as_str())
}

fn players_line<'a>(view: &GameInfoView, i18n: &I18n) -> Element<'a, Message> {
    let mut line = row![
        player(&view.white, TagField::White, i18n),
        text("--").size(18),
        player(&view.black, TagField::Black, i18n),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    if let Some(result) = view.result {
        line = line.push(text(result_text(result)).size(16));
    }
    line.into()
}

fn event_line<'a>(view: &GameInfoView, i18n: &I18n) -> Element<'a, Message> {
    let mut line = row![].spacing(8);
    if let Some(event) = &view.event {
        let mut label = format!("{}: {event}", i18n.tr("gameinfo-event"));
        if let Some(site) = &view.site {
            label.push_str(", ");
            label.push_str(site);
        }
        line = line.push(text(label));
    } else {
        line = line.push(text(format!("{}:", i18n.tr("gameinfo-event"))));
        line = line.push(link(
            i18n.tr("info-unknown"),
            Message::Dialog(DialogMessage::OpenHeader(Some(TagField::Event))),
        ));
    }
    let mut when = String::new();
    if let Some(date) = &view.date {
        when.push_str(date);
    }
    if let Some(round) = &view.round {
        let mut args = FluentArgs::new();
        args.set("round", round.as_str());
        if !when.is_empty() {
            when.push_str(", ");
        }
        when.push_str(&i18n.tr_args("gameinfo-round", &args));
    }
    if !when.is_empty() {
        line = line.push(text(format!("({when})")));
    }
    if let Some(eco) = &view.eco_tag {
        line = line.push(text(eco.to_string()));
    }
    if let Some(opening) = &view.opening {
        line = line.push(text(format!("{} {}", opening.eco, opening.name)).style(text::secondary));
    }
    line.wrap().into()
}

fn move_line<'a>(view: &GameInfoView, i18n: &I18n) -> Element<'a, Message> {
    let previous = match &view.previous {
        PreviousMove::Move {
            number,
            black,
            san,
            nags,
        } => text(numbered(*number, *black, san, nags)).into(),
        PreviousMove::StartOfLine => text(i18n.tr("gameinfo-start-of-line")).into(),
        _ => Element::from(text(i18n.tr("gameinfo-start-of-game"))),
    };
    let next_label = i18n.tr("gameinfo-next");
    let next: Element<'a, Message> = match &view.next {
        NextMove::Move {
            number,
            black,
            san,
            nags,
        } => link(
            format!("{next_label}: {}", numbered(*number, *black, san, nags)),
            nav(Nav::Forward(1)),
        ),
        NextMove::Hidden => text(format!("{next_label}: ({})", i18n.tr("gameinfo-hidden"))).into(),
        NextMove::EndOfLine => text(i18n.tr("gameinfo-end-of-line")).into(),
        _ => text(i18n.tr("gameinfo-end-of-game")).into(),
    };
    let mut line = row![previous].spacing(12);
    if view.in_variation {
        line = line.push(link(
            i18n.tr("gameinfo-exit-variation"),
            nav(Nav::ExitVariation),
        ));
    }
    line = line.push(next);
    line.wrap().into()
}

fn stats_line<'a>(view: &GameInfoView, i18n: &I18n) -> Element<'a, Message> {
    let mut line = row![text(format!(
        "{}: {}",
        i18n.tr("gameinfo-length"),
        view.length_moves
    ))]
    .spacing(12);
    if let Some(balance) = view.material {
        let sign = if balance > 0 { "+" } else { "" };
        line = line.push(text(format!(
            "{}: {sign}{balance}",
            i18n.tr("gameinfo-material")
        )));
    }
    line.into()
}

/// The panel content for `view`.
pub fn view<'a>(view: &GameInfoView, i18n: &I18n) -> Element<'a, Message> {
    let mut lines = column![
        players_line(view, i18n),
        event_line(view, i18n),
        move_line(view, i18n),
        stats_line(view, i18n),
    ]
    .spacing(6);
    if !view.variations.is_empty() {
        let mut list = column![text(format!("{}:", i18n.tr("gameinfo-variations")))].spacing(2);
        for (index, variation) in view.variations.iter().enumerate() {
            let nags: Vec<Nag> = variation.nag.into_iter().collect();
            let label = format!(
                "{}: {}",
                index + 1,
                numbered(variation.number, variation.black, &variation.san, &nags)
            );
            list = list.push(link(label, nav(Nav::EnterVariation(index + 1))));
        }
        lines = lines.push(list);
    }
    if let Some(comment) = &view.comment {
        // A click opens the comment editor (D185).
        lines = lines.push(
            button(text(comment.clone()).size(14).style(text::secondary))
                .padding(0)
                .style(button::text)
                .on_press(Message::Notation(NotationMessage::EditCursorComment)),
        );
    }
    if let Some(fen) = &view.fen {
        lines = lines.push(text(fen.clone()).size(12).style(theme::muted_text));
    }
    scrollable(container(lines).padding(10).width(Length::Fill)).into()
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::{AppState, update};

    use super::*;
    use crate::i18n::Language;

    fn sim(app: &AppState, language: Language) -> Simulator<'static, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(900.0, 500.0),
            view(&app.game_info(), &I18n::new(language)),
        )
    }

    fn play(app: &mut AppState, uci: &str, force: bool) {
        let mv = app.session.position().parse_uci(uci).expect("legal");
        update(
            app,
            AppMessage::Game(GameMessage::PlayMove {
                mv,
                options: mychess_app::PlayOptions {
                    force_variation: force,
                },
            }),
        );
    }

    #[test]
    fn unknown_names_are_links_that_open_the_dialog_on_the_field() {
        let app = AppState::default();
        let mut s = sim(&app, Language::EnUs);
        s.click("?").expect("first unknown name");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::Dialog(DialogMessage::OpenHeader(Some(
                TagField::White
            )))]
        ));
    }

    #[test]
    fn known_names_show_the_rating_and_the_next_move_navigates() {
        let mut app = AppState::default();
        update(
            &mut app,
            AppMessage::Info(mychess_app::GameInfoMessage::Open),
        );
        update(
            &mut app,
            AppMessage::Info(mychess_app::GameInfoMessage::SetText(
                TagField::White,
                "Kasparov".to_owned(),
            )),
        );
        update(
            &mut app,
            AppMessage::Info(mychess_app::GameInfoMessage::SetRating(
                mychess_app::PlayerSide::White,
                "2800".to_owned(),
            )),
        );
        update(
            &mut app,
            AppMessage::Info(mychess_app::GameInfoMessage::Apply),
        );
        play(&mut app, "e2e4", false);
        update(
            &mut app,
            AppMessage::Game(GameMessage::Navigate(Nav::Start)),
        );
        let mut s = sim(&app, Language::EnUs);
        assert!(s.find("Kasparov (2800)").is_ok());
        s.click("Next: 1. e4").expect("next move link");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Game(GameMessage::Navigate(
                Nav::Forward(1)
            )))]
        ));
    }

    #[test]
    fn variations_are_listed_and_entering_one_navigates() {
        let mut app = AppState::default();
        play(&mut app, "e2e4", false);
        update(
            &mut app,
            AppMessage::Game(GameMessage::Navigate(Nav::Start)),
        );
        play(&mut app, "d2d4", true);
        update(
            &mut app,
            AppMessage::Game(GameMessage::Navigate(Nav::Start)),
        );
        let mut s = sim(&app, Language::EnUs);
        s.click("1: 1. d4").expect("variation link");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Game(GameMessage::Navigate(
                Nav::EnterVariation(1)
            )))]
        ));
    }

    #[test]
    fn inside_a_variation_the_exit_link_appears_and_the_text_is_translated() {
        let mut app = AppState::default();
        play(&mut app, "e2e4", false);
        update(
            &mut app,
            AppMessage::Game(GameMessage::Navigate(Nav::Start)),
        );
        play(&mut app, "d2d4", true);
        let mut s = sim(&app, Language::PtBr);
        assert!(s.find("Tamanho: 1").is_ok());
        s.click("(Var)").expect("exit link");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Game(GameMessage::Navigate(
                Nav::ExitVariation
            )))]
        ));
    }

    #[test]
    fn material_and_start_of_game_are_shown() {
        let app = AppState::default();
        let mut s = sim(&app, Language::EnUs);
        assert!(s.find("Start of game").is_ok());
        assert!(s.find("Material: 0").is_ok());
    }
}
