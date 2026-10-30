//! The list of the games of a PGN file (spec M2-2, D125; M2-4, rule 8 of the
//! GUI rules): a filter field and a virtualized list. Only the rows on
//! screen (plus a margin) become widgets, so a file with 50 000 games costs
//! the same as one with ten.

use fluent_bundle::FluentArgs;
use iced::widget::{
    button, column, container, mouse_area, row, scrollable, space, text, text_input,
};
use iced::{Element, Length};
use mychess_app::{AppState, ChooserMessage, GameChooser, GameHeader, Message as AppMessage};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::widgets::modal::{Card, modal};
use crate::windows::dialog_ui::{
    CHOOSER_FILTER_ID, CHOOSER_LIST_HEIGHT, CHOOSER_LIST_ID, CHOOSER_ROW_HEIGHT, DialogMessage,
    DialogUi,
};

/// Id of the box.
pub const CHOOSER_BOX_ID: &str = "game-chooser-box";

fn send(message: ChooserMessage) -> Message {
    Message::App(AppMessage::Chooser(message))
}

fn players(header: &GameHeader) -> String {
    let side = |name: &str, elo: Option<u16>| match elo {
        Some(elo) => format!("{name} ({elo})"),
        None => name.to_owned(),
    };
    format!(
        "{} - {}",
        side(&header.white, header.white_elo),
        side(&header.black, header.black_elo)
    )
}

fn game_row<'a>(index: usize, header: &GameHeader, selected: bool) -> Element<'a, Message> {
    let cells = row![
        text(format!("{}", index + 1)).width(Length::Fixed(56.0)),
        text(players(header)).width(Length::FillPortion(5)),
        text(header.result.as_str()).width(Length::Fixed(64.0)),
        text(header.event.clone()).width(Length::FillPortion(3)),
        text(header.date.short()).width(Length::Fixed(92.0)),
    ]
    .spacing(8);
    let cell = container(cells)
        .height(Length::Fixed(CHOOSER_ROW_HEIGHT))
        .padding([2, 6])
        .width(Length::Fill)
        .style(move |theme: &iced::Theme| {
            let palette = theme.extended_palette();
            if selected {
                container::Style {
                    background: Some(palette.primary.weak.color.into()),
                    text_color: Some(palette.primary.weak.text),
                    ..container::Style::default()
                }
            } else {
                container::Style::default()
            }
        });
    mouse_area(cell)
        .on_press(send(ChooserMessage::Select(index)))
        .on_double_click(send(ChooserMessage::Open(index)))
        .into()
}

fn spacer<'a>(rows: usize) -> Element<'a, Message> {
    // Row counts are small; `f32` holds them exactly.
    #[allow(clippy::cast_precision_loss)]
    let height = rows as f32 * CHOOSER_ROW_HEIGHT;
    space().height(Length::Fixed(height)).into()
}

/// The visible rows of `chooser`: the window `[first, first + count)` of the
/// filtered list, with spacers standing for the rest.
fn list<'a>(chooser: &GameChooser, ui: &DialogUi) -> Element<'a, Message> {
    let total = chooser.visible.len();
    let (first, count) = ui.chooser_window(total);
    let mut rows = column![spacer(first)].width(Length::Fill);
    for &index in chooser.visible.iter().skip(first).take(count) {
        if let Some(header) = chooser.collection.header(index) {
            rows = rows.push(game_row(index, header, chooser.selected == Some(index)));
        }
    }
    rows = rows.push(spacer(total.saturating_sub(first + count)));
    scrollable(rows)
        .id(CHOOSER_LIST_ID)
        .height(Length::Fixed(CHOOSER_LIST_HEIGHT))
        .on_scroll(|viewport| {
            Message::Dialog(DialogMessage::ChooserScrolled(viewport.absolute_offset().y))
        })
        .into()
}

/// The box, when the application has a chooser open.
pub fn view<'a>(app: &AppState, ui: &DialogUi, i18n: &I18n) -> Option<Element<'a, Message>> {
    let chooser = app.chooser.as_ref()?;
    let filter = text_input(&i18n.tr("chooser-filter-placeholder"), &chooser.filter)
        .id(CHOOSER_FILTER_ID)
        .on_input(|t| send(ChooserMessage::Filter(t)))
        .on_submit(send(ChooserMessage::Confirm))
        .padding(5);
    let mut args = FluentArgs::new();
    args.set(
        "shown",
        i64::try_from(chooser.visible.len()).unwrap_or(i64::MAX),
    );
    args.set(
        "total",
        i64::try_from(chooser.collection.len()).unwrap_or(i64::MAX),
    );
    let body = column![
        filter,
        list(chooser, ui),
        text(i18n.tr_args("chooser-count", &args)).size(13),
    ]
    .spacing(8);
    Some(modal(Card {
        id: CHOOSER_BOX_ID,
        title: i18n.tr("chooser-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(send(ChooserMessage::Cancel))
                .into(),
            button(text(i18n.tr("chooser-open")))
                .on_press_maybe(chooser.selected.map(|_| send(ChooserMessage::Confirm)))
                .into(),
        ],
        width: 820.0,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use std::fmt::Write;

    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::{Effect, PgnSource, update};

    use super::*;
    use crate::i18n::Language;

    /// Reads a text of `games` games through the application, as the
    /// import dialog does, and returns the state with its chooser open.
    fn app_with_games(games: usize) -> AppState {
        let mut pgn = String::new();
        for i in 0..games {
            let _ = writeln!(
                pgn,
                "[White \"Player {i}\"]\n[Black \"Rival {}\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n",
                i % 7
            );
        }
        let mut app = AppState::default();
        update(
            &mut app,
            AppMessage::Import(mychess_app::ImportMessage::Open),
        );
        update(
            &mut app,
            AppMessage::Import(mychess_app::ImportMessage::TextEdited(pgn)),
        );
        for effect in update(
            &mut app,
            AppMessage::Import(mychess_app::ImportMessage::Import),
        ) {
            if matches!(
                effect,
                Effect::ScanPgn {
                    source: PgnSource::Text(_),
                    ..
                }
            ) {
                let answer = crate::test_support::run_effect(effect);
                update(&mut app, answer);
            }
        }
        app
    }

    fn sim(app: &AppState, ui: &DialogUi) -> Simulator<'static, Message> {
        let element = view(app, ui, &I18n::new(Language::EnUs)).expect("open");
        Simulator::with_size(Settings::default(), Size::new(1000.0, 700.0), element)
    }

    #[test]
    fn a_big_file_builds_only_the_rows_on_screen() {
        let app = app_with_games(2000);
        let chooser = app.chooser.as_ref().expect("chooser");
        assert_eq!(chooser.collection.len(), 2000);
        let ui = DialogUi::default();
        let mut s = sim(&app, &ui);
        assert!(s.find("Player 0 - Rival 0").is_ok());
        assert!(
            s.find("Player 1500 - Rival 2").is_err(),
            "rows far from the viewport are not widgets"
        );
        assert!(s.find("2000 of 2000 games").is_ok());
        // Scrolled far down, the window moves.
        let mut ui = DialogUi::default();
        ui.chooser.scroll = CHOOSER_ROW_HEIGHT * 1500.0;
        let mut s = sim(&app, &ui);
        assert!(s.find("Player 1500 - Rival 2").is_ok());
        assert!(s.find("Player 0 - Rival 0").is_err());
    }

    #[test]
    fn filtering_changes_the_count_and_the_rows() {
        let mut app = app_with_games(300);
        update(
            &mut app,
            AppMessage::Chooser(ChooserMessage::Filter("Player 29".to_owned())),
        );
        let ui = DialogUi::default();
        let mut s = sim(&app, &ui);
        assert!(s.find("11 of 300 games").is_ok(), "29 and 290 to 299");
        assert!(s.find("Player 29 - Rival 1").is_ok());
        assert!(s.find("Player 0 - Rival 0").is_err());
    }

    #[test]
    fn clicking_a_row_selects_and_the_buttons_open_or_cancel() {
        let app = app_with_games(5);
        let ui = DialogUi::default();
        let mut s = sim(&app, &ui);
        s.click("Player 3 - Rival 3").expect("row");
        s.click("Open").expect("open");
        s.click("Cancel").expect("cancel");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [
                Message::App(AppMessage::Chooser(ChooserMessage::Select(3))),
                Message::App(AppMessage::Chooser(ChooserMessage::Confirm)),
                Message::App(AppMessage::Chooser(ChooserMessage::Cancel)),
            ]
        ));
    }

    #[test]
    fn the_filter_field_reports_what_is_typed() {
        let app = app_with_games(3);
        let ui = DialogUi::default();
        let mut s = sim(&app, &ui);
        s.click(iced::widget::Id::new(CHOOSER_FILTER_ID))
            .expect("field");
        s.typewrite("x");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.last(),
            Some(Message::App(AppMessage::Chooser(ChooserMessage::Filter(t)))) if t == "x"
        ));
    }
}
