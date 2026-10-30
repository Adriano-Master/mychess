//! "New Chess960 game": the number of the starting position (0 to 959,
//! Scharnagl numbering, 518 is the standard arrangement), a button for a
//! random one and a preview of the back rank (spec M9-6, D4503).

use iced::widget::{button, row, text, text_input};
use iced::{Element, Font};
use mychess_core::Position;

use crate::i18n::I18n;
use crate::shell::Message;
use crate::widgets::modal::{Card, modal};
use crate::windows::dialog_ui::DialogMessage;

/// Id of the box.
pub const CHESS960_BOX_ID: &str = "chess960-box";
/// Id of the number field.
pub const CHESS960_FIELD_ID: &str = "chess960-field";

/// Number of the standard arrangement, where the dialog opens.
const STANDARD_NUMBER: u16 = 518;
/// Number of starting positions.
const COUNT: u64 = 960;

/// What the dialog holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chess960Draft {
    /// What the user typed.
    pub text: String,
    /// The last try to start was not a number from 0 to 959.
    pub invalid: bool,
}

impl Default for Chess960Draft {
    fn default() -> Self {
        Chess960Draft {
            text: STANDARD_NUMBER.to_string(),
            invalid: false,
        }
    }
}

impl Chess960Draft {
    /// The number typed, if it is one of the 960.
    pub fn number(&self) -> Option<u16> {
        let n: u16 = self.text.trim().parse().ok()?;
        (u64::from(n) < COUNT).then_some(n)
    }

    /// A draft holding the position drawn by `seed`.
    pub fn random(seed: u64) -> Chess960Draft {
        let n = u16::try_from(seed % COUNT).unwrap_or(STANDARD_NUMBER);
        Chess960Draft {
            text: n.to_string(),
            invalid: false,
        }
    }

    /// The white back rank of the position typed (`RNBQKBNR`), if valid.
    pub fn back_rank(&self) -> Option<String> {
        let fen = Position::chess960_start(self.number()?).ok()?.fen();
        let placement = fen.split(' ').next()?;
        placement.rsplit('/').next().map(str::to_owned)
    }
}

fn dialog(message: DialogMessage) -> Message {
    Message::Dialog(message)
}

/// The box.
pub fn view<'a>(draft: &Chess960Draft, i18n: &I18n) -> Element<'a, Message> {
    let field = text_input(&i18n.tr("chess960-placeholder"), &draft.text)
        .id(CHESS960_FIELD_ID)
        .on_input(|t| dialog(DialogMessage::Chess960Edited(t)))
        .on_submit(dialog(DialogMessage::Chess960Submit))
        .padding(5);
    let random = button(text(i18n.tr("chess960-random")))
        .on_press(dialog(DialogMessage::Chess960Random))
        .style(button::secondary);
    let mut body = iced::widget::column![
        text(i18n.tr("chess960-prompt")),
        row![field, random].spacing(6)
    ]
    .spacing(8);
    match draft.back_rank() {
        Some(rank) => {
            let spaced = |s: &str| {
                s.chars().fold(String::new(), |mut out, c| {
                    out.push(c);
                    out.push(' ');
                    out
                })
            };
            body = body
                .push(text(spaced(&rank)).font(Font::MONOSPACE).size(26))
                .push(text(spaced("abcdefgh")).font(Font::MONOSPACE).size(14));
        }
        None => {
            body = body.push(text(i18n.tr("chess960-invalid")).style(text::danger));
        }
    }
    body = body.push(text(i18n.tr("chess960-hint")).size(13));
    modal(Card {
        id: CHESS960_BOX_ID,
        title: i18n.tr("chess960-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("dialog-cancel")))
                .on_press(dialog(DialogMessage::CloseChess960))
                .style(button::secondary)
                .into(),
            button(text(i18n.tr("chess960-start-play")))
                .style(button::secondary)
                .on_press_maybe(
                    draft
                        .number()
                        .map(|_| Message::Ui(crate::UiMessage::Chess960Play)),
                )
                .into(),
            button(text(i18n.tr("chess960-start")))
                .on_press(dialog(DialogMessage::Chess960Submit))
                .into(),
        ],
        width: 420.0,
    })
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;

    use super::*;
    use crate::i18n::Language;

    fn sim(draft: &Chess960Draft, language: Language) -> Simulator<'static, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(700.0, 500.0),
            view(draft, &I18n::new(language)),
        )
    }

    #[test]
    fn number_accepts_only_0_to_959() {
        let n = |t: &str| {
            Chess960Draft {
                text: t.into(),
                invalid: false,
            }
            .number()
        };
        assert_eq!(n("0"), Some(0));
        assert_eq!(n(" 959 "), Some(959));
        assert_eq!(n("960"), None);
        assert_eq!(n("-1"), None);
        assert_eq!(n("x"), None);
        assert_eq!(n(""), None);
        assert_eq!(Chess960Draft::default().number(), Some(518));
    }

    #[test]
    fn random_is_always_one_of_the_960() {
        for seed in [0, 1, 959, 960, u64::MAX, 123_456_789_012] {
            assert!(Chess960Draft::random(seed).number().is_some(), "{seed}");
        }
    }

    #[test]
    fn preview_shows_the_back_rank() {
        assert_eq!(
            Chess960Draft::default().back_rank().as_deref(),
            Some("RNBQKBNR")
        );
        let zero = Chess960Draft {
            text: "0".into(),
            invalid: false,
        };
        assert_eq!(zero.back_rank().as_deref(), Some("BBQNNRKR"));
        let bad = Chess960Draft {
            text: "960".into(),
            invalid: false,
        };
        assert_eq!(bad.back_rank(), None);
    }

    #[test]
    fn buttons_emit_the_dialog_messages_in_both_languages() {
        let draft = Chess960Draft::default();
        let mut s = sim(&draft, Language::EnUs);
        s.click("Start game").expect("start");
        assert!(matches!(
            s.into_messages().collect::<Vec<_>>().as_slice(),
            [Message::Dialog(DialogMessage::Chess960Submit)]
        ));
        let mut s = sim(&draft, Language::EnUs);
        s.click("Random").expect("random");
        assert!(matches!(
            s.into_messages().collect::<Vec<_>>().as_slice(),
            [Message::Dialog(DialogMessage::Chess960Random)]
        ));
        let mut s = sim(&draft, Language::PtBr);
        assert!(s.find("Iniciar partida").is_ok());
        assert!(s.find("Aleatória").is_ok());
    }

    #[test]
    fn the_play_button_starts_and_opens_the_play_dialog_only_for_a_valid_number() {
        let draft = Chess960Draft::default();
        let mut s = sim(&draft, Language::EnUs);
        s.click("Start and play the engine...").expect("button");
        assert!(matches!(
            s.into_messages().collect::<Vec<_>>().as_slice(),
            [Message::Ui(crate::UiMessage::Chess960Play)]
        ));
        let mut s = sim(&draft, Language::PtBr);
        assert!(s.find("Iniciar e jogar contra a engine...").is_ok());
        let bad = Chess960Draft {
            text: "999".into(),
            invalid: false,
        };
        let mut s = sim(&bad, Language::EnUs);
        s.click("Start and play the engine...").ok();
        assert_eq!(s.into_messages().count(), 0, "disabled without a number");
    }

    #[test]
    fn an_invalid_number_shows_the_error() {
        let bad = Chess960Draft {
            text: "999".into(),
            invalid: true,
        };
        let mut s = sim(&bad, Language::EnUs);
        assert!(
            s.find("Enter a number from 0 to 959.").is_ok(),
            "error text"
        );
    }
}
