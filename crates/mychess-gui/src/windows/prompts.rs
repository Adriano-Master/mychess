//! The questions of the editing session: "replace the move?", the
//! promotion piece and "remove N comments?" (spec M2-4, D188, D190 and
//! rules 45-48).
//!
//! The application owns the question (`GameSession::prompt`); this module
//! shows it and turns the answer into a message. The board already asks for
//! the promotion piece by itself, so the promotion box only appears for a
//! move that came without the piece (typed text, for example).

use fluent_bundle::FluentArgs;
use iced::Element;
use iced::widget::{button, checkbox, column, text};
use mychess_app::{
    Key, KeyInput, Message as AppMessage, Modifiers, NamedKey, Prompt, ReplaceChoice, StripKind,
};
use mychess_core::Role;

use crate::app_api;
use crate::i18n::I18n;
use crate::shell::Message;
use crate::widgets::modal::{Card, modal};
use crate::windows::dialog_ui::DialogMessage;

/// The question text with the "don't ask again" box under it.
fn body_with_box<'a>(question: String, i18n: &I18n, dont_ask: bool) -> Element<'a, Message> {
    column![
        text(question),
        checkbox(dont_ask)
            .label(i18n.tr("prompt-dont-ask-again"))
            .on_toggle(|on| Message::Dialog(DialogMessage::DontAskAgain(on))),
    ]
    .spacing(12)
    .into()
}

/// Id of the "replace move" box.
pub const REPLACE_BOX_ID: &str = "replace-prompt";
/// Id of the promotion box.
pub const PROMOTION_BOX_ID: &str = "promotion-prompt";
/// Id of the "remove?" box.
pub const STRIP_BOX_ID: &str = "strip-prompt";

fn answer<'a>(label: String, message: AppMessage, is_default: bool) -> Element<'a, Message> {
    let button = button(text(label)).on_press(Message::App(message));
    if is_default {
        button.style(button::primary).into()
    } else {
        button.style(button::secondary).into()
    }
}

/// The five choices of the "move already exists" question, in the order of
/// the Scid; the default is "add variation".
fn replace_box<'a>(i18n: &I18n, dont_ask: bool) -> Element<'a, Message> {
    let choices = [
        ("prompt-replace-replace", ReplaceChoice::Replace),
        ("prompt-replace-new-main", ReplaceChoice::NewMainLine),
        ("prompt-replace-add-variation", ReplaceChoice::AddVariation),
        ("prompt-replace-try", ReplaceChoice::Trial),
        ("dialog-cancel", ReplaceChoice::Cancel),
    ];
    let buttons = choices
        .into_iter()
        .map(|(key, choice)| {
            answer(
                i18n.tr(key),
                app_api::answer_replace(choice),
                choice == ReplaceChoice::AddVariation,
            )
        })
        .collect();
    modal(Card {
        id: REPLACE_BOX_ID,
        title: i18n.tr("prompt-replace-title"),
        body: body_with_box(i18n.tr("prompt-replace-body"), i18n, dont_ask),
        buttons,
        width: 640.0,
    })
}

fn promotion_box<'a>(i18n: &I18n) -> Element<'a, Message> {
    let choices = [
        ("board-promote-queen", Role::Queen),
        ("board-promote-rook", Role::Rook),
        ("board-promote-bishop", Role::Bishop),
        ("board-promote-knight", Role::Knight),
    ];
    let mut buttons: Vec<Element<'a, Message>> = choices
        .into_iter()
        .map(|(key, role)| {
            answer(
                i18n.tr(key),
                app_api::answer_promotion(Some(role)),
                role == Role::Queen,
            )
        })
        .collect();
    buttons.push(answer(
        i18n.tr("dialog-cancel"),
        app_api::answer_promotion(None),
        false,
    ));
    modal(Card {
        id: PROMOTION_BOX_ID,
        title: i18n.tr("prompt-promotion-title"),
        body: column![].into(),
        buttons,
        width: 520.0,
    })
}

/// The Fluent key of the question about removing `kind`.
fn strip_key(kind: StripKind) -> &'static str {
    match kind {
        StripKind::Comments => "prompt-strip-comments",
        StripKind::Variations => "prompt-strip-variations",
        StripKind::TruncateAfter(_) => "prompt-truncate-after",
        StripKind::TruncateFrom(_) => "prompt-truncate-from",
        StripKind::TruncateBefore(_) => "prompt-truncate-before",
        _ => "prompt-strip-other",
    }
}

fn strip_box<'a>(
    i18n: &I18n,
    kind: StripKind,
    count: usize,
    dont_ask: bool,
) -> Element<'a, Message> {
    let mut args = FluentArgs::new();
    args.set("count", i64::try_from(count).unwrap_or(i64::MAX));
    modal(Card {
        id: STRIP_BOX_ID,
        title: i18n.tr("prompt-strip-title"),
        body: body_with_box(i18n.tr_args(strip_key(kind), &args), i18n, dont_ask),
        buttons: vec![
            answer(
                i18n.tr("dialog-cancel"),
                app_api::answer_strip(false),
                false,
            ),
            answer(
                i18n.tr("prompt-confirm-remove"),
                app_api::answer_strip(true),
                true,
            ),
        ],
        width: 460.0,
    })
}

/// The box for `prompt`; `dont_ask` is the state of its "don't ask again"
/// box.
pub fn view<'a>(prompt: &Prompt, i18n: &I18n, dont_ask: bool) -> Element<'a, Message> {
    match prompt {
        Prompt::ReplaceMove(_) => replace_box(i18n, dont_ask),
        Prompt::ConfirmStrip { kind, count } => strip_box(i18n, *kind, *count, dont_ask),
        Prompt::ChoosePromotion { .. } => promotion_box(i18n),
    }
}

/// What `Esc` answers: cancel.
pub fn cancel_answer(prompt: &Prompt) -> AppMessage {
    match prompt {
        Prompt::ReplaceMove(_) => app_api::answer_replace(ReplaceChoice::Cancel),
        Prompt::ConfirmStrip { .. } => app_api::answer_strip(false),
        Prompt::ChoosePromotion { .. } => app_api::answer_promotion(None),
    }
}

/// What `Enter` answers: the default button.
pub fn default_answer(prompt: &Prompt) -> AppMessage {
    match prompt {
        Prompt::ReplaceMove(_) => app_api::answer_replace(ReplaceChoice::AddVariation),
        Prompt::ConfirmStrip { .. } => app_api::answer_strip(true),
        Prompt::ChoosePromotion { .. } => app_api::answer_promotion(Some(Role::Queen)),
    }
}

/// The letter keys of the boxes: `R`, `M`, `V`, `T` for the replace
/// question (rule 45) and `Q`, `R`, `B`, `N` for the promotion.
pub fn key_answer(prompt: &Prompt, input: &KeyInput) -> Option<AppMessage> {
    if input.modifiers != Modifiers::NONE {
        return None;
    }
    let Key::Char(c) = input.key else {
        return match input.key {
            Key::Named(NamedKey::Escape) => Some(cancel_answer(prompt)),
            Key::Named(NamedKey::Enter) => Some(default_answer(prompt)),
            Key::Named(_) | Key::Char(_) => None,
        };
    };
    match prompt {
        Prompt::ReplaceMove(_) => {
            let choice = match c {
                'r' => ReplaceChoice::Replace,
                'm' => ReplaceChoice::NewMainLine,
                'v' => ReplaceChoice::AddVariation,
                't' => ReplaceChoice::Trial,
                _ => return None,
            };
            Some(app_api::answer_replace(choice))
        }
        Prompt::ChoosePromotion { .. } => {
            let role = match c {
                'q' => Role::Queen,
                'r' => Role::Rook,
                'b' => Role::Bishop,
                'n' => Role::Knight,
                _ => return None,
            };
            Some(app_api::answer_promotion(Some(role)))
        }
        Prompt::ConfirmStrip { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::PendingMove;
    use mychess_core::{Move, Position};

    use super::*;
    use crate::i18n::Language;

    fn replace_prompt() -> Prompt {
        let pos = Position::new();
        let mv: Move = pos.parse_uci("e2e4").expect("legal");
        Prompt::ReplaceMove(PendingMove {
            parent: mychess_core::Game::new().root(),
            mv,
        })
    }

    fn click(element: Element<'_, Message>, label: &str) -> Vec<Message> {
        let mut sim = Simulator::with_size(Settings::default(), Size::new(1000.0, 600.0), element);
        sim.click(label).expect("button");
        sim.into_messages().collect()
    }

    fn game(message: &AppMessage) -> &mychess_app::GameMessage {
        match message {
            AppMessage::Game(m) => m,
            other => panic!("not a game message: {other:?}"),
        }
    }

    #[test]
    fn the_replace_box_has_the_five_choices_of_the_scid() {
        let i18n = I18n::new(Language::EnUs);
        for (label, expected) in [
            ("Replace move", ReplaceChoice::Replace),
            ("New main line", ReplaceChoice::NewMainLine),
            ("Add variation", ReplaceChoice::AddVariation),
            ("Try variation", ReplaceChoice::Trial),
            ("Cancel", ReplaceChoice::Cancel),
        ] {
            let sent = click(view(&replace_prompt(), &i18n, false), label);
            match sent.as_slice() {
                [Message::App(message)] => assert_eq!(
                    game(message),
                    &mychess_app::GameMessage::AnswerReplace(expected),
                    "{label}"
                ),
                other => panic!("{label}: {other:?}"),
            }
        }
    }

    #[test]
    fn the_replace_box_is_translated() {
        let pt = I18n::new(Language::PtBr);
        let mut sim = Simulator::with_size(
            Settings::default(),
            Size::new(1000.0, 600.0),
            view(&replace_prompt(), &pt, false),
        );
        for label in ["Substituir lance", "Nova linha principal", "Cancelar"] {
            assert!(sim.find(label).is_ok(), "{label}");
        }
        assert!(sim.find(iced::widget::Id::new(REPLACE_BOX_ID)).is_ok());
    }

    #[test]
    fn letters_enter_and_escape_answer_the_replace_question() {
        let prompt = replace_prompt();
        let key = |c| KeyInput {
            key: Key::Char(c),
            modifiers: Modifiers::NONE,
        };
        let named = |n| KeyInput {
            key: Key::Named(n),
            modifiers: Modifiers::NONE,
        };
        let choice = |input: KeyInput| match key_answer(&prompt, &input) {
            Some(AppMessage::Game(mychess_app::GameMessage::AnswerReplace(c))) => Some(c),
            _ => None,
        };
        assert_eq!(choice(key('r')), Some(ReplaceChoice::Replace));
        assert_eq!(choice(key('m')), Some(ReplaceChoice::NewMainLine));
        assert_eq!(choice(key('v')), Some(ReplaceChoice::AddVariation));
        assert_eq!(choice(key('t')), Some(ReplaceChoice::Trial));
        assert_eq!(
            choice(named(NamedKey::Enter)),
            Some(ReplaceChoice::AddVariation)
        );
        assert_eq!(choice(named(NamedKey::Escape)), Some(ReplaceChoice::Cancel));
        assert_eq!(choice(key('x')), None);
        let with_command = KeyInput {
            key: Key::Char('r'),
            modifiers: Modifiers::COMMAND,
        };
        assert!(key_answer(&prompt, &with_command).is_none());
    }

    #[test]
    fn the_strip_box_counts_and_answers() {
        let en = I18n::new(Language::EnUs);
        let one = Prompt::ConfirmStrip {
            kind: StripKind::Comments,
            count: 1,
        };
        let many = Prompt::ConfirmStrip {
            kind: StripKind::Variations,
            count: 7,
        };
        let mut sim = Simulator::with_size(
            Settings::default(),
            Size::new(900.0, 500.0),
            view(&one, &en, false),
        );
        assert!(sim.find("Remove 1 comment from this game?").is_ok());
        let mut sim = Simulator::with_size(
            Settings::default(),
            Size::new(900.0, 500.0),
            view(&many, &en, false),
        );
        assert!(sim.find("Remove 7 variations from this game?").is_ok());
        let sent = click(view(&many, &en, false), "Remove");
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Game(
                mychess_app::GameMessage::AnswerStrip(true)
            ))]
        ));
        let sent = click(view(&many, &en, false), "Cancel");
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Game(
                mychess_app::GameMessage::AnswerStrip(false)
            ))]
        ));
    }

    #[test]
    fn the_promotion_box_answers_with_the_piece() {
        let en = I18n::new(Language::EnUs);
        let prompt = Prompt::ChoosePromotion {
            from: mychess_core::Square::A7,
            to: mychess_core::Square::A8,
            force_variation: false,
        };
        let sent = click(view(&prompt, &en, false), "Knight");
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Game(
                mychess_app::GameMessage::AnswerPromotion(Some(Role::Knight))
            ))]
        ));
        assert_eq!(
            cancel_answer(&prompt),
            AppMessage::Game(mychess_app::GameMessage::AnswerPromotion(None))
        );
    }
}
