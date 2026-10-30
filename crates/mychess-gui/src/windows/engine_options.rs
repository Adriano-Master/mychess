//! The UCI options dialog (spec M4-3, D430): one control per option the
//! engine announced, generated from `OptionsDialog` (the important ones
//! first). `check` is a box, `spin` a field with a step, `combo` a list,
//! `string` a text field and `button` a button that acts on the running
//! engine at once. Save validates every option and refuses the whole form
//! with the message beside the field.

use fluent_bundle::FluentArgs;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input,
};
use iced::{Element, Length};
use mychess_app::AppState;
use mychess_app::engines::{
    EngineMessage, EngineOption, EngineRunState, OptionKind, OptionsDialog, OptionsMessage,
    OptionsStatus, default_text, spin_step,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::engine_text::{failure_text, option_error_text, option_label};
use crate::windows::engines_dialog::send;

/// Id of the box.
pub const OPTIONS_BOX_ID: &str = "engine-options-box";

const LIST_HEIGHT: f32 = 380.0;
const LABEL_WIDTH: f32 = 230.0;

fn msg(message: OptionsMessage) -> Message {
    send(EngineMessage::Options(message))
}

fn set(name: &str, value: impl Into<String>) -> Message {
    msg(OptionsMessage::Set {
        name: name.to_owned(),
        value: value.into(),
    })
}

/// The new text of a `spin` after one step (`direction` is `1` or `-1`),
/// kept inside the range.
pub(crate) fn stepped(
    current: &str,
    direction: i64,
    name: &str,
    min: i64,
    max: i64,
    default: i64,
) -> String {
    let now = current.trim().parse::<i64>().unwrap_or(default);
    let step = spin_step(name, min, max);
    now.saturating_add(direction.saturating_mul(step))
        .clamp(min, max)
        .to_string()
}

fn control<'a>(
    option: &EngineOption,
    dialog: &OptionsDialog,
    running: bool,
    i18n: &I18n,
) -> Element<'a, Message> {
    let name = option.name.clone();
    let shown = option_label(i18n, &name);
    let value = dialog.value_of(&name).unwrap_or_default().to_owned();
    let mut default_args = FluentArgs::new();
    default_args.set("default", default_text(option));
    let hint = || {
        text(i18n.tr_args("engines-options-default", &default_args))
            .size(12)
            .style(theme::muted_text)
    };
    let label = |content: String| text(content).width(Length::Fixed(LABEL_WIDTH));
    let line: Element<'a, Message> = match &option.kind {
        OptionKind::Check { .. } => {
            let key = name.clone();
            row![
                checkbox(value == "true")
                    .label(shown)
                    .on_toggle(move |on| set(&key, on.to_string()))
                    .width(Length::Fixed(LABEL_WIDTH)),
                hint(),
            ]
            .spacing(8)
            .into()
        }
        OptionKind::Spin { default, min, max } => {
            let (min, max, default) = (*min, *max, *default);
            let key = name.clone();
            let down = set(&name, stepped(&value, -1, &name, min, max, default));
            let up = set(&name, stepped(&value, 1, &name, min, max, default));
            row![
                label(shown),
                button(text("-"))
                    .style(button::secondary)
                    .padding([2, 10])
                    .on_press(down),
                text_input("", &value)
                    .on_input(move |v| set(&key, v))
                    .padding(4)
                    .width(Length::Fixed(110.0)),
                button(text("+"))
                    .style(button::secondary)
                    .padding([2, 10])
                    .on_press(up),
                hint(),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into()
        }
        OptionKind::Combo { vars, .. } => {
            let key = name.clone();
            row![
                label(shown),
                pick_list(vars.clone(), Some(value), move |v: String| set(&key, v)),
                hint(),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into()
        }
        OptionKind::String { default } => {
            let key = name.clone();
            // Scid `uci.tcl:768-772`: the default goes beside the label when
            // it is not empty. Enter in this field is not captured: it
            // saves the form (D743, D750).
            let mut line = row![
                label(shown),
                text_input("", &value)
                    .on_input(move |v| set(&key, v))
                    .padding(4)
                    .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);
            if !default.is_empty() {
                line = line.push(hint());
            }
            line.into()
        }
        OptionKind::Button => button(text(shown))
            .style(button::secondary)
            .on_press_maybe(running.then(|| msg(OptionsMessage::Press(name.clone()))))
            .into(),
    };
    let mut cell = column![line].spacing(2);
    if let Some(error) = dialog.error_of(&option.name) {
        cell = cell.push(
            text(option_error_text(i18n, error))
                .size(12)
                .style(text::danger),
        );
    }
    cell.into()
}

/// The box, when the application has the options dialog open.
pub fn view<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let dialog = app.engine_dialog.as_ref()?.options.as_ref()?;
    let entry = app.engines.get(dialog.engine)?;
    let running = app
        .engine_runs
        .of_engine(dialog.engine)
        .is_some_and(|r| r.state == EngineRunState::Running);
    Some(options_box(&entry.name, dialog, running, i18n))
}

fn options_box<'a>(
    engine_name: &str,
    dialog: &OptionsDialog,
    running: bool,
    i18n: &I18n,
) -> Element<'a, Message> {
    let body: Element<'a, Message> = match &dialog.status {
        OptionsStatus::Probing => text(i18n.tr("engines-options-probing")).into(),
        OptionsStatus::NotUci => text(i18n.tr("engines-options-not-uci")).into(),
        OptionsStatus::Failed(failure) => {
            let mut args = FluentArgs::new();
            args.set("reason", failure_text(i18n, failure));
            text(i18n.tr_args("engines-options-failed", &args))
                .style(text::danger)
                .into()
        }
        _ => {
            let options = dialog.visible_options();
            if options.is_empty() {
                text(i18n.tr("engines-options-none")).into()
            } else {
                let mut list = column![].spacing(10).width(Length::Fill);
                for option in &options {
                    list = list.push(control(option, dialog, running, i18n));
                }
                let mut body = column![
                    scrollable(container(list).padding([0, 12])).height(Length::Fixed(LIST_HEIGHT))
                ]
                .spacing(6);
                if options.iter().any(|o| matches!(o.kind, OptionKind::Button)) && !running {
                    body = body.push(
                        text(i18n.tr("engines-options-button-hint"))
                            .size(12)
                            .style(theme::muted_text),
                    );
                }
                body.into()
            }
        }
    };
    let ready = dialog.status == OptionsStatus::Ready && !dialog.visible_options().is_empty();
    let mut args = FluentArgs::new();
    args.set("name", engine_name);
    let mut buttons: Vec<Element<'a, Message>> = vec![
        button(text(i18n.tr("engines-options-reset")))
            .style(button::secondary)
            .on_press_maybe(ready.then(|| msg(OptionsMessage::Reset)))
            .into(),
        button(text(i18n.tr("dialog-cancel")))
            .style(button::secondary)
            .on_press(msg(OptionsMessage::Close))
            .into(),
    ];
    buttons.push(
        button(text(i18n.tr("engines-options-save")))
            .on_press_maybe(ready.then(|| msg(OptionsMessage::Save)))
            .into(),
    );
    modal(Card {
        id: OPTIONS_BOX_ID,
        title: i18n.tr_args("engines-options-title", &args),
        body,
        buttons,
        width: 640.0,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::engines::{
        Capabilities, EngineDraft, EngineFailure, EngineIdentity, EngineList, FailureKind,
        OptionError,
    };

    use super::*;
    use crate::i18n::Language;

    fn option(name: &str, kind: OptionKind) -> EngineOption {
        EngineOption {
            name: name.to_owned(),
            kind,
        }
    }

    fn dialog() -> OptionsDialog {
        let mut list = EngineList::new();
        let id = list.add(EngineDraft::new("Fake", "/fake")).expect("add");
        let mut dialog = OptionsDialog::new(list.get(id).expect("entry"));
        dialog.set_identity(
            EngineIdentity {
                name: "Fake".to_owned(),
                author: None,
                options: vec![
                    option("Own Flag", OptionKind::Check { default: false }),
                    option(
                        "Hash",
                        OptionKind::Spin {
                            default: 16,
                            min: 1,
                            max: 1024,
                        },
                    ),
                    option(
                        "Style",
                        OptionKind::Combo {
                            default: "Normal".to_owned(),
                            vars: vec!["Solid".to_owned(), "Normal".to_owned()],
                        },
                    ),
                    option(
                        "Syzygy Path",
                        OptionKind::String {
                            default: String::new(),
                        },
                    ),
                    option("Clear Hash", OptionKind::Button),
                ],
                capabilities: Capabilities::default(),
            },
            &[],
        );
        dialog
    }

    fn sim(
        dialog: &OptionsDialog,
        running: bool,
        language: Language,
    ) -> Simulator<'static, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(1000.0, 800.0),
            options_box("Fake", dialog, running, &I18n::new(language)),
        )
    }

    fn option_messages(messages: Vec<Message>) -> Vec<OptionsMessage> {
        messages
            .into_iter()
            .filter_map(|m| match m {
                Message::App(mychess_app::Message::Engine(EngineMessage::Options(o))) => Some(o),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn stepping_stays_in_range_and_uses_the_step_of_the_option() {
        assert_eq!(stepped("16", 1, "Hash", 1, 1024, 16), "66");
        assert_eq!(stepped("1", -1, "Hash", 1, 1024, 16), "1");
        assert_eq!(stepped("1020", 1, "Hash", 1, 1024, 16), "1024");
        assert_eq!(stepped("abc", 1, "Skill Level", 0, 20, 20), "20");
        assert_eq!(stepped("3", 1, "MultiPV", 1, 500, 1), "4");
        assert_eq!(stepped("1", 1, "UCI_Elo", 1320, 3190, 1320), "1320");
    }

    #[test]
    fn there_is_one_control_per_option_with_the_default_beside_it() {
        let dialog = dialog();
        let mut s = sim(&dialog, true, Language::EnUs);
        for text in [
            "Own Flag",
            "Hash memory (MB)",
            "Style",
            "Syzygy Path",
            "Clear Hash",
            "default: 16",
            "default: false",
            "Restore defaults",
        ] {
            assert!(s.find(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn the_spin_buttons_send_the_next_value_and_the_check_box_toggles() {
        let dialog = dialog();
        let mut s = sim(&dialog, true, Language::EnUs);
        s.click("+").expect("plus");
        assert_eq!(
            option_messages(s.into_messages().collect()),
            [OptionsMessage::Set {
                name: "Hash".to_owned(),
                value: "66".to_owned()
            }]
        );
        let mut s = sim(&dialog, true, Language::EnUs);
        s.click("Own Flag").expect("check");
        assert_eq!(
            option_messages(s.into_messages().collect()),
            [OptionsMessage::Set {
                name: "Own Flag".to_owned(),
                value: "true".to_owned()
            }]
        );
    }

    #[test]
    fn a_button_option_acts_only_while_the_engine_runs_and_is_never_saved() {
        let dialog = dialog();
        let mut s = sim(&dialog, true, Language::EnUs);
        s.click("Clear Hash").expect("button");
        assert_eq!(
            option_messages(s.into_messages().collect()),
            [OptionsMessage::Press("Clear Hash".to_owned())]
        );
        let mut s = sim(&dialog, false, Language::EnUs);
        s.click("Clear Hash").expect("button");
        assert!(option_messages(s.into_messages().collect()).is_empty());
        assert!(dialog.value_of("Clear Hash").is_none());
    }

    #[test]
    fn save_reset_and_cancel_send_their_messages_and_errors_show_beside_the_field() {
        let mut dialog = dialog();
        dialog.errors = vec![(
            "Hash".to_owned(),
            OptionError::OutOfRange {
                name: "Hash".to_owned(),
                value: 0,
                min: 1,
                max: 1024,
            },
        )];
        let mut s = sim(&dialog, false, Language::EnUs);
        assert!(s.find("The value must be between 1 and 1024.").is_ok());
        for (label, expected) in [
            ("Save", OptionsMessage::Save),
            ("Restore defaults", OptionsMessage::Reset),
            ("Cancel", OptionsMessage::Close),
        ] {
            let mut s = sim(&dialog, false, Language::EnUs);
            s.click(label).expect("button");
            assert_eq!(
                option_messages(s.into_messages().collect()),
                [expected],
                "{label}"
            );
        }
    }

    #[test]
    fn while_probing_or_after_a_failure_there_is_no_form_to_save() {
        let mut dialog = dialog();
        dialog.status = OptionsStatus::Probing;
        let mut s = sim(&dialog, false, Language::PtBr);
        assert!(s.find("Iniciando a engine para ler as opções...").is_ok());
        s.click("Salvar").expect("button");
        assert!(option_messages(s.into_messages().collect()).is_empty());
        dialog.status = OptionsStatus::Failed(EngineFailure {
            kind: FailureKind::CannotStart,
            exit_code: None,
            stderr_tail: vec![],
        });
        let mut s = sim(&dialog, false, Language::EnUs);
        assert!(
            s.find("The engine could not be started: The program could not be started.")
                .is_ok()
        );
    }
}
