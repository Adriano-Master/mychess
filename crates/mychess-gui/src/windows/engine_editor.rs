//! The engine editor (spec M4-3, "Editar"): name, command (with a file
//! chooser), parameters as one line, folder, web page, Elo, protocol and key,
//! with "Test engine" and "Detect protocol". The state is
//! `AppState::engine_editor`; the box only draws it.
//!
//! Required fields (name, command) are drawn in bold, as in the Scid. The
//! error of a field is written under the form; the disk check of the
//! command is only a remark.

use fluent_bundle::FluentArgs;
use iced::font::Weight;
use iced::widget::{button, column, container, radio, row, text, text_input, tooltip};
use iced::{Element, Font, Length};
use mychess_app::AppState;
use mychess_app::engines::{
    DetectState, EditorMessage, EngineEditor, EngineMessage, Hotkey, Protocol, TestState,
};

use crate::i18n::I18n;
use crate::shell::{Message, UiMessage};
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::engine_text::{
    command_warning_text, editor_error_text, failure_text, protocol_label,
};
use crate::windows::engines_dialog::{EnginePick, send};

/// Id of the box.
pub const EDITOR_BOX_ID: &str = "engine-editor-box";
/// Id of the name field (focused when the editor opens).
pub const EDITOR_NAME_ID: &str = "engine-editor-name";

/// Ids of the radio groups (for tests: a radio draws its own label).
const PROTOCOL_UCI_ID: &str = "engine-protocol-uci";
const PROTOCOL_XBOARD_ID: &str = "engine-protocol-xboard";
const KEY_NONE_ID: &str = "engine-key-none";
const KEY_IDS: [&str; 3] = ["engine-key-f2", "engine-key-f3", "engine-key-f4"];

const LABEL_WIDTH: f32 = 110.0;

fn edit(message: EditorMessage) -> Message {
    send(EngineMessage::Editor(message))
}

fn label<'a>(content: String, required: bool) -> Element<'a, Message> {
    let mut label = text(content).width(Length::Fixed(LABEL_WIDTH));
    if required {
        label = label.font(Font {
            weight: Weight::Bold,
            ..Font::DEFAULT
        });
    }
    label.into()
}

fn field<'a>(
    name: String,
    required: bool,
    id: Option<&'static str>,
    value: &str,
    on_input: fn(String) -> EditorMessage,
) -> iced::widget::Row<'a, Message> {
    let mut input = text_input("", value)
        .on_input(move |v| edit(on_input(v)))
        .padding(5)
        .width(Length::Fill);
    if let Some(id) = id {
        input = input.id(id);
    }
    row![label(name, required), input]
        .spacing(8)
        .align_y(iced::Alignment::Center)
}

fn probe_button<'a>(label: String, tip: String, message: EditorMessage) -> Element<'a, Message> {
    tooltip(
        button(text(label))
            .style(button::secondary)
            .on_press(edit(message)),
        container(text(tip).size(13))
            .padding(6)
            .style(container::rounded_box),
        tooltip::Position::Bottom,
    )
    .into()
}

/// The box, when the application has the editor open.
pub fn view<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let editor = app.engine_editor.as_ref()?;
    Some(editor_box(editor, i18n))
}

fn test_line(editor: &EngineEditor, i18n: &I18n) -> Option<String> {
    Some(match editor.test.as_ref()? {
        TestState::Running => i18n.tr("engines-test-running"),
        TestState::Passed(identity) => {
            let mut args = FluentArgs::new();
            args.set("name", identity.name.as_str());
            args.set("protocol", protocol_label(editor.protocol).to_uppercase());
            args.set(
                "options",
                i64::try_from(identity.options.len()).unwrap_or(i64::MAX),
            );
            i18n.tr_args("engines-test-ok", &args)
        }
        TestState::Failed(failure) => {
            let mut args = FluentArgs::new();
            args.set("reason", failure_text(i18n, failure));
            i18n.tr_args("engines-test-failed", &args)
        }
        _ => return None,
    })
}

fn detect_line(editor: &EngineEditor, i18n: &I18n) -> Option<String> {
    Some(match editor.detect.as_ref()? {
        DetectState::Running => i18n.tr("engines-detect-running"),
        DetectState::Found(protocol) => {
            let mut args = FluentArgs::new();
            args.set("protocol", protocol_label(*protocol).to_uppercase());
            i18n.tr_args("engines-detect-found", &args)
        }
        DetectState::Failed(failure) => {
            let mut args = FluentArgs::new();
            args.set("reason", failure_text(i18n, failure));
            i18n.tr_args("engines-detect-failed", &args)
        }
        _ => return None,
    })
}

#[allow(clippy::too_many_lines)] // one flat form reads best
fn editor_box<'a>(editor: &EngineEditor, i18n: &I18n) -> Element<'a, Message> {
    let command_row = row![
        label(i18n.tr("engines-field-command"), true),
        text_input("", &editor.command)
            .on_input(|v| edit(EditorMessage::SetCommand(v)))
            .on_submit(edit(EditorMessage::Check))
            .padding(5)
            .width(Length::Fill),
        button(text(i18n.tr("engines-browse")))
            .style(button::secondary)
            .on_press(Message::Ui(UiMessage::PickEngineFile(EnginePick::Command))),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let protocol = row![
        label(i18n.tr("engines-field-protocol"), false),
        container(radio(
            i18n.tr("engines-protocol-uci"),
            Protocol::Uci,
            Some(editor.protocol),
            |p| edit(EditorMessage::SetProtocol(p)),
        ))
        .id(PROTOCOL_UCI_ID),
        container(radio(
            i18n.tr("engines-protocol-xboard"),
            Protocol::XBoard,
            Some(editor.protocol),
            |p| edit(EditorMessage::SetProtocol(p)),
        ))
        .id(PROTOCOL_XBOARD_ID),
    ]
    .spacing(16);

    let mut keys = row![label(i18n.tr("engines-field-key"), false)].spacing(16);
    keys = keys.push(
        container(radio(
            i18n.tr("engines-key-none"),
            None,
            Some(editor.hotkey),
            |k: Option<Hotkey>| edit(EditorMessage::SetHotkey(k)),
        ))
        .id(KEY_NONE_ID),
    );
    for (key, id) in Hotkey::ALL.into_iter().zip(KEY_IDS) {
        keys = keys.push(
            container(radio(
                key.name(),
                Some(key),
                Some(editor.hotkey),
                |k: Option<Hotkey>| edit(EditorMessage::SetHotkey(k)),
            ))
            .id(id),
        );
    }

    let probes = row![
        // Both run the program to identify it: say so (D752).
        probe_button(
            i18n.tr("engines-btn-test"),
            i18n.tr("engines-btn-test-tip"),
            EditorMessage::Test
        ),
        probe_button(
            i18n.tr("engines-btn-detect"),
            i18n.tr("engines-btn-detect-tip"),
            EditorMessage::Detect
        ),
    ]
    .spacing(8);

    let mut body = column![
        field(
            i18n.tr("engines-field-name"),
            true,
            Some(EDITOR_NAME_ID),
            &editor.name,
            EditorMessage::SetName,
        ),
        command_row,
        field(
            i18n.tr("engines-field-args"),
            false,
            None,
            &editor.args,
            EditorMessage::SetArgs,
        ),
        field(
            i18n.tr("engines-field-dir"),
            false,
            None,
            &editor.working_dir,
            EditorMessage::SetWorkingDir,
        ),
        field(
            i18n.tr("engines-field-url"),
            false,
            None,
            &editor.url,
            EditorMessage::SetUrl,
        ),
        field(
            i18n.tr("engines-field-elo"),
            false,
            None,
            &editor.elo,
            EditorMessage::SetElo,
        ),
        protocol,
        keys,
        probes,
    ]
    .spacing(8);
    body = body.push(
        text(i18n.tr("engines-required"))
            .size(12)
            .style(theme::muted_text),
    );
    for warning in &editor.warnings {
        body = body.push(text(command_warning_text(i18n, *warning)).size(13));
    }
    if let Some(line) = test_line(editor, i18n) {
        let failed = matches!(editor.test, Some(TestState::Failed(_)));
        let line = text(line).size(13);
        body = body.push(if failed {
            line.style(text::danger)
        } else {
            line
        });
    }
    if let Some(line) = detect_line(editor, i18n) {
        let mut line_row = row![text(line).size(13)].spacing(8);
        if matches!(editor.detect, Some(DetectState::Found(p)) if p != editor.protocol) {
            line_row = line_row.push(
                button(text(i18n.tr("engines-btn-use-detected")).size(13))
                    .style(button::secondary)
                    .padding([0, 8])
                    .on_press(edit(EditorMessage::UseDetected)),
            );
        }
        body = body.push(line_row);
    }
    if let Some(error) = &editor.error {
        body = body.push(text(editor_error_text(i18n, error)).style(text::danger));
    }
    let title_key = if editor.id.is_some() {
        "engines-edit-title"
    } else {
        "engines-edit-title-new"
    };
    let mut buttons: Vec<Element<'a, Message>> = Vec::new();
    if let Some(id) = editor.id {
        buttons.push(
            button(text(i18n.tr("engines-btn-options")))
                .style(button::secondary)
                .on_press(send(EngineMessage::OpenOptions(id)))
                .into(),
        );
    }
    buttons.push(
        button(text(i18n.tr("dialog-cancel")))
            .style(button::secondary)
            .on_press(edit(EditorMessage::Cancel))
            .into(),
    );
    buttons.push(
        button(text(i18n.tr("engines-btn-save")))
            .on_press(edit(EditorMessage::Save))
            .into(),
    );
    modal(Card {
        id: EDITOR_BOX_ID,
        title: i18n.tr(title_key),
        body: container(body).into(),
        buttons,
        width: 640.0,
    })
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::engines::{CommandWarning, EditorError, EngineFailure, FailureKind};

    use super::*;
    use crate::i18n::Language;

    fn sim(editor: &EngineEditor, language: Language) -> Simulator<'static, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(1000.0, 800.0),
            editor_box(editor, &I18n::new(language)),
        )
    }

    fn edit_messages(messages: Vec<Message>) -> Vec<EditorMessage> {
        messages
            .into_iter()
            .filter_map(|m| match m {
                Message::App(mychess_app::Message::Engine(EngineMessage::Editor(e))) => Some(e),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_buttons_send_the_editor_messages() {
        let editor = EngineEditor::new_engine();
        for (label, expected) in [
            ("Test engine", EditorMessage::Test),
            ("Detect protocol", EditorMessage::Detect),
            ("Save", EditorMessage::Save),
            ("Cancel", EditorMessage::Cancel),
        ] {
            let mut s = sim(&editor, Language::EnUs);
            s.click(label).expect("clickable");
            assert_eq!(
                edit_messages(s.into_messages().collect()),
                [expected],
                "{label}"
            );
        }
        for (id, expected) in [
            (
                PROTOCOL_XBOARD_ID,
                EditorMessage::SetProtocol(Protocol::XBoard),
            ),
            ("engine-key-f3", EditorMessage::SetHotkey(Some(Hotkey::F3))),
            (KEY_NONE_ID, EditorMessage::SetHotkey(None)),
        ] {
            let mut editor = editor.clone();
            // A selected radio does not publish: start from the other state.
            editor.protocol = Protocol::Uci;
            editor.hotkey = Some(Hotkey::F2);
            let mut s = sim(&editor, Language::EnUs);
            let label = id;
            s.click(iced::widget::Id::new(id)).expect("clickable");
            assert_eq!(
                edit_messages(s.into_messages().collect()),
                [expected],
                "{label}"
            );
        }
    }

    #[test]
    fn browse_asks_the_shell_for_a_file() {
        let mut s = sim(&EngineEditor::new_engine(), Language::EnUs);
        s.click("Browse...").expect("clickable");
        let messages: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            messages.as_slice(),
            [Message::Ui(UiMessage::PickEngineFile(EnginePick::Command))]
        ));
    }

    #[test]
    fn typing_in_the_name_field_edits_the_form() {
        let mut s = sim(&EngineEditor::new_engine(), Language::EnUs);
        s.click(iced::widget::Id::new(EDITOR_NAME_ID))
            .expect("field");
        s.typewrite("A");
        let edits = edit_messages(s.into_messages().collect());
        assert!(
            edits.contains(&EditorMessage::SetName("A".to_owned())),
            "{edits:?}"
        );
    }

    #[test]
    fn errors_warnings_and_the_test_result_are_shown_translated() {
        let mut editor = EngineEditor::new_engine();
        editor.error = Some(EditorError::EloNotANumber);
        editor.warnings = vec![CommandWarning::CommandNotFound];
        editor.test = Some(TestState::Failed(EngineFailure {
            kind: FailureKind::ExitedEarly,
            exit_code: Some(2),
            stderr_tail: vec![],
        }));
        let mut s = sim(&editor, Language::EnUs);
        for text in [
            "The Elo must be a number.",
            "The command was not found on this computer.",
            "Test failed: The engine ended before it was ready (exit code 2).",
            "Fields in bold are required; the others are optional.",
        ] {
            assert!(s.find(text).is_ok(), "{text}");
        }
        let mut pt = sim(&editor, Language::PtBr);
        assert!(pt.find("O Elo deve ser um número.").is_ok());
        assert!(pt.find("Testar engine").is_ok());
    }

    #[test]
    fn the_options_button_exists_only_for_a_saved_engine() {
        let mut s = sim(&EngineEditor::new_engine(), Language::EnUs);
        assert!(s.find("UCI options...").is_err());
        let mut editor = EngineEditor::new_engine();
        editor.id = Some(mychess_app::engines::EngineId(4));
        let mut s = sim(&editor, Language::EnUs);
        assert!(s.find("UCI options...").is_ok());
        assert!(s.find("Edit engine").is_ok());
    }
}
