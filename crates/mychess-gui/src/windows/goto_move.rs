//! "Go to move number" (spec M2-4, D198; Scid `GotoMoveNumber`).

use iced::Element;
use iced::widget::{button, text, text_input};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::widgets::modal::{Card, modal};
use crate::windows::dialog_ui::{DialogMessage, GOTO_FIELD_ID, GotoDraft};

/// Id of the box.
pub const GOTO_BOX_ID: &str = "goto-box";

fn dialog(message: DialogMessage) -> Message {
    Message::Dialog(message)
}

/// The box: a field for the number, an error when it is not one.
pub fn view<'a>(draft: &GotoDraft, i18n: &I18n) -> Element<'a, Message> {
    let field = text_input(&i18n.tr("goto-move-placeholder"), &draft.text)
        .id(GOTO_FIELD_ID)
        .on_input(|t| dialog(DialogMessage::GotoEdited(t)))
        .on_submit(dialog(DialogMessage::GotoSubmit))
        .padding(5);
    let mut body = iced::widget::column![text(i18n.tr("goto-move-prompt")), field].spacing(6);
    if draft.invalid {
        body = body.push(text(i18n.tr("goto-move-invalid")).style(text::danger));
    }
    modal(Card {
        id: GOTO_BOX_ID,
        title: i18n.tr("goto-move-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("dialog-cancel")))
                .on_press(dialog(DialogMessage::CloseGoto))
                .style(button::secondary)
                .into(),
            button(text(i18n.tr("dialog-ok")))
                .on_press(dialog(DialogMessage::GotoSubmit))
                .into(),
        ],
        width: 360.0,
    })
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;

    use super::*;
    use crate::i18n::Language;

    fn sim(draft: &GotoDraft, language: Language) -> Simulator<'static, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(700.0, 400.0),
            view(draft, &I18n::new(language)),
        )
    }

    #[test]
    fn ok_and_cancel_emit_the_dialog_messages() {
        let draft = GotoDraft::default();
        let mut s = sim(&draft, Language::EnUs);
        s.click("OK").expect("ok");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::Dialog(DialogMessage::GotoSubmit)]
        ));
        let mut s = sim(&draft, Language::EnUs);
        s.click("Cancel").expect("cancel");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::Dialog(DialogMessage::CloseGoto)]
        ));
    }

    #[test]
    fn typing_reports_the_text_and_an_invalid_draft_shows_the_error() {
        let mut s = sim(&GotoDraft::default(), Language::EnUs);
        s.click(iced::widget::Id::new(GOTO_FIELD_ID))
            .expect("field");
        s.typewrite("12");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.last(),
            Some(Message::Dialog(DialogMessage::GotoEdited(t))) if t == "12"
        ));
        let bad = GotoDraft {
            text: "x".into(),
            invalid: true,
        };
        let mut s = sim(&bad, Language::PtBr);
        assert!(s.find("Informe um número de lance maior que zero.").is_ok());
    }
}
