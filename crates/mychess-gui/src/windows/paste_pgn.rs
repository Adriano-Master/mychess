//! "Paste PGN" (spec M2-4, D197; Scid `tools/import.tcl`): a text area and
//! the report of the last import, with every diagnostic translated.

use fluent_bundle::FluentArgs;
use iced::widget::{button, column, container, scrollable, text, text_editor};
use iced::{Element, Length};
use mychess_app::{
    AppState, ImportDialog, ImportIssue, ImportMessage, ImportNotice, Message as AppMessage,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::dialog_ui::{DialogMessage, DialogUi, PASTE_EDITOR_ID};

/// Id of the box.
pub const PASTE_BOX_ID: &str = "paste-pgn-box";

fn to_app(message: ImportMessage) -> Message {
    Message::App(AppMessage::Import(message))
}

fn count(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The translated text of one remark of the import.
pub fn notice_text(i18n: &I18n, notice: &ImportNotice) -> String {
    let mut args = FluentArgs::new();
    match &notice.issue {
        ImportIssue::TextOutsideGames { bytes } => {
            args.set("bytes", i64::try_from(*bytes).unwrap_or(i64::MAX));
        }
        ImportIssue::Truncated { games_read, .. } => {
            args.set("games", i64::try_from(*games_read).unwrap_or(i64::MAX));
        }
        _ => {}
    }
    let message = i18n.tr_args(notice.issue.message_key(), &args);
    let Some(line) = notice.line else {
        return message;
    };
    let mut place = FluentArgs::new();
    place.set("line", i64::try_from(line).unwrap_or(i64::MAX));
    place.set("message", message);
    match notice.column {
        Some(column) => {
            place.set("column", i64::from(column));
            i18n.tr_args("import-position-column", &place)
        }
        None => i18n.tr_args("import-position-line", &place),
    }
}

fn report<'a>(dialog: &ImportDialog, i18n: &I18n) -> Element<'a, Message> {
    let Some(report) = &dialog.report else {
        return column![].into();
    };
    let mut list = column![].spacing(2);
    let problems = report.notices.iter().filter(|n| n.is_problem()).count();
    let mut args = FluentArgs::new();
    args.set("count", count(problems));
    list = list.push(text(i18n.tr_args("paste-pgn-errors", &args)));
    for notice in &report.notices {
        let line = text(notice_text(i18n, notice)).size(13);
        list = list.push(if notice.is_problem() {
            line.style(text::danger)
        } else {
            line.style(theme::muted_text)
        });
    }
    scrollable(list).height(Length::Fixed(110.0)).into()
}

/// The box.
pub fn view<'a>(app: &'a AppState, ui: &'a DialogUi, i18n: &I18n) -> Option<Element<'a, Message>> {
    let dialog = app.import.as_ref()?;
    let editor = text_editor(&ui.paste.content)
        .id(PASTE_EDITOR_ID)
        .placeholder(i18n.tr("paste-pgn-placeholder"))
        .height(Length::Fixed(240.0))
        .on_action(|a| Message::Dialog(DialogMessage::PasteEdit(a)));
    let body = column![editor, report(dialog, i18n)].spacing(8);
    let busy = dialog.request.is_some();
    let import = button(text(i18n.tr("paste-pgn-import"))).on_press_maybe(
        (!busy && !dialog.text.trim().is_empty()).then(|| to_app(ImportMessage::Import)),
    );
    Some(modal(Card {
        id: PASTE_BOX_ID,
        title: i18n.tr("paste-pgn-title"),
        body: container(body).width(Length::Fill).into(),
        buttons: vec![
            button(text(i18n.tr("paste-pgn-current")))
                .style(button::secondary)
                .on_press(to_app(ImportMessage::PasteCurrentGame))
                .into(),
            button(text(i18n.tr("action-clear")))
                .style(button::secondary)
                .on_press(to_app(ImportMessage::Clear))
                .into(),
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(to_app(ImportMessage::Close))
                .into(),
            import.into(),
        ],
        width: 640.0,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::{Effect, update};

    use super::*;
    use crate::i18n::Language;

    fn open(text: &str) -> (AppState, DialogUi) {
        let mut app = AppState::default();
        update(&mut app, AppMessage::Import(ImportMessage::Open));
        update(
            &mut app,
            AppMessage::Import(ImportMessage::TextEdited(text.to_owned())),
        );
        let mut ui = DialogUi::default();
        ui.sync(&app);
        (app, ui)
    }

    /// Runs the import for real: the scan is an effect of the application.
    fn import(text: &str) -> AppState {
        let (mut app, _) = open(text);
        let effects = update(&mut app, AppMessage::Import(ImportMessage::Import));
        for effect in effects {
            if matches!(effect, Effect::ScanPgn { .. }) {
                let answer = crate::test_support::run_effect(effect);
                update(&mut app, answer);
            }
        }
        app
    }

    fn click(app: &AppState, ui: &DialogUi, label: &str) -> Vec<Message> {
        let element = view(app, ui, &I18n::new(Language::EnUs)).expect("open");
        let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 600.0), element);
        sim.click(label).expect("button");
        sim.into_messages().collect()
    }

    #[test]
    fn closed_dialog_has_no_view() {
        let app = AppState::default();
        assert!(view(&app, &DialogUi::default(), &I18n::default()).is_none());
    }

    #[test]
    fn import_and_cancel_send_the_import_messages() {
        let (app, ui) = open("1. e4 e5");
        let sent = click(&app, &ui, "Import");
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Import(ImportMessage::Import))]
        ));
        let sent = click(&app, &ui, "Cancel");
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Import(ImportMessage::Close))]
        ));
        let sent = click(&app, &ui, "Clear");
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Import(ImportMessage::Clear))]
        ));
    }

    #[test]
    fn import_is_disabled_without_text() {
        let (app, ui) = open("   ");
        assert!(click(&app, &ui, "Import").is_empty());
    }

    #[test]
    fn a_text_without_moves_reports_a_translated_problem() {
        let app = import("hello there");
        let report = app
            .import
            .as_ref()
            .and_then(|d| d.report.as_ref())
            .expect("report");
        assert!(report.has_problems());
        let notices = &report.notices;
        let en = I18n::new(Language::EnUs);
        assert!(
            notices
                .iter()
                .any(|n| notice_text(&en, n).contains("no move")),
            "{:?}",
            notices
                .iter()
                .map(|n| notice_text(&en, n))
                .collect::<Vec<_>>()
        );
        let ui = DialogUi::default();
        let element = view(&app, &ui, &en).expect("open");
        let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 600.0), element);
        let problems = report.notices.iter().filter(|n| n.is_problem()).count();
        assert!(problems >= 1);
        let expected = if problems == 1 {
            "1 problem found".to_owned()
        } else {
            format!("{problems} problems found")
        };
        assert!(sim.find(expected.as_str()).is_ok());
    }

    #[test]
    fn diagnostics_carry_their_line_and_column() {
        let app = import("1. e4 e5 ( 2. Nf3");
        let report = app
            .import
            .as_ref()
            .and_then(|d| d.report.as_ref())
            .expect("report");
        let located: Vec<&ImportNotice> =
            report.notices.iter().filter(|n| n.line.is_some()).collect();
        assert!(!located.is_empty());
        let en = notice_text(&I18n::new(Language::EnUs), located[0]);
        assert!(en.starts_with("Line "), "{en}");
        let pt = notice_text(&I18n::new(Language::PtBr), located[0]);
        assert!(pt.starts_with("Linha "), "{pt}");
    }
}
