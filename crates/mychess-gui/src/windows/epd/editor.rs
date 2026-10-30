//! The editor of the opcodes of the selected record and the status line
//! (spec M7-5c, sections 2 and 4 of the behavior).

use std::fmt::Write as _;

use fluent_bundle::FluentArgs;
use iced::keyboard::key::Named;
use iced::widget::text_editor::{Binding, KeyPress, Status};
use iced::widget::{button, column, container, row, space, text, text_editor, text_input};
use iced::{Alignment, Element, Length, keyboard};
use mychess_app::epd_window::RunOutcome;
use mychess_app::epd_window::{EpdMessage, EpdTabView};

use super::{EPD_EDITOR_ID, EPD_FIND_ID, EpdUiMessage, epd};
use crate::shell::{Message, Shell};
use crate::theme;
use crate::windows::panel_kit::{BODY_TEXT, action, danger, muted};

/// Height of the text area.
const EDITOR_HEIGHT: f32 = 130.0;

/// `Cmd+Enter` writes the text into the record; the rest is the toolkit's.
fn key_binding(press: KeyPress) -> Option<Binding<Message>> {
    if !matches!(press.status, Status::Focused { .. }) {
        return None;
    }
    match press.key.as_ref() {
        keyboard::Key::Named(Named::Enter) if press.modifiers.command() => {
            Some(Binding::Custom(epd(EpdMessage::Commit)))
        }
        _ => Binding::from_key_press(press),
    }
}

/// The text of the record, its label and the warnings.
pub(super) fn view<'a>(shell: &'a Shell, tab: &EpdTabView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let ui = &shell.ui.dialogs.epd;
    let enabled = tab.editor_enabled;
    let mut title = row![
        text(i18n.tr("epd-editor-label")).size(BODY_TEXT),
        text(tab.fen.clone().unwrap_or_default())
            .size(12)
            .style(theme::muted_text),
        space::horizontal(),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if tab.editor_draft {
        title = title.push(action(i18n.tr("dialog-ok"), Some(epd(EpdMessage::Commit))));
    }
    let placeholder = if tab.read_only {
        i18n.tr("epd-editor-read-only")
    } else if tab.selected.is_none() {
        i18n.tr("epd-editor-no-record")
    } else {
        i18n.tr("epd-editor-placeholder")
    };
    let area = text_editor(&ui.content)
        .id(EPD_EDITOR_ID)
        .placeholder(placeholder)
        .height(Length::Fixed(EDITOR_HEIGHT))
        .size(BODY_TEXT)
        .on_action(|action| Message::Epd(EpdUiMessage::Editor(action)))
        .key_binding(key_binding)
        .style(move |theme, status| {
            // A file that cannot be edited shows the editor dimmed.
            let status = if enabled { status } else { Status::Disabled };
            text_editor::default(theme, status)
        });
    let mut page = column![title, area].spacing(4);
    for opcode in tab.nonstandard.iter().filter(|o| !o.is_empty()) {
        let mut args = FluentArgs::new();
        args.set("opcode", opcode.as_str());
        page = page.push(danger(i18n.tr_args("epd-nonstandard-note", &args)));
    }
    container(page).id("epd-editor-block").into()
}

/// The words of the line of the analysis, if there is one.
fn run_line(shell: &Shell, tab: &EpdTabView) -> Option<String> {
    let i18n = &shell.i18n;
    if let Some(run) = &tab.run {
        let mut args = FluentArgs::new();
        args.set("engine", run.engine.as_str());
        if run.starting {
            return Some(i18n.tr_args("epd-analyzing-starting", &args));
        }
        if run.stopping {
            return Some(i18n.tr("epd-analyzing-stopping"));
        }
        args.set("seconds", i64::from(run.seconds));
        args.set("done", i64::from(run.progress.done));
        args.set("total", i64::from(run.progress.total));
        return Some(i18n.tr_args("epd-analyzing", &args));
    }
    let (outcome, progress) = tab.last_run?;
    let mut args = FluentArgs::new();
    args.set("done", i64::from(progress.done));
    args.set("total", i64::from(progress.total));
    args.set("found", i64::from(progress.found));
    args.set("counted", i64::from(progress.counted));
    Some(i18n.tr_args(
        match outcome {
            RunOutcome::Finished => "epd-last-run-finished",
            RunOutcome::Interrupted => "epd-last-run-interrupted",
            _ => "epd-last-run-failed",
        },
        &args,
    ))
}

/// The status line: the count, the state of the file, the moves of the
/// board that the file knows, the analysis and the search field.
pub(super) fn status<'a>(shell: &'a Shell, tab: &EpdTabView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let ui = &shell.ui.dialogs.epd;
    let mut args = FluentArgs::new();
    args.set("count", i64::try_from(tab.records).unwrap_or(i64::MAX));
    let mut line = i18n.tr_args("epd-positions", &args);
    if tab.rows != tab.records {
        let mut shown = FluentArgs::new();
        shown.set("shown", i64::try_from(tab.rows).unwrap_or(i64::MAX));
        shown.set("total", i64::try_from(tab.records).unwrap_or(i64::MAX));
        let _ = write!(line, "  ({})", i18n.tr_args("epd-rows-shown", &shown));
    }
    if tab.read_only {
        let _ = write!(line, "  ({})", i18n.tr("epd-read-only"));
    } else if tab.dirty {
        let _ = write!(line, "  ({})", i18n.tr("epd-altered"));
    }
    if tab.busy {
        let _ = write!(line, "  ({})", i18n.tr("epd-busy-mark"));
    }
    let moves = if tab.book_moves.is_empty() {
        format!("[{}]", i18n.tr("epd-no-moves"))
    } else {
        let mut a = FluentArgs::new();
        a.set("count", i64::try_from(tab.book_moves.len()).unwrap_or(0));
        a.set("moves", tab.book_moves.join(" "));
        i18n.tr_args("epd-book-moves", &a)
    };
    let find = text_input(&i18n.tr("epd-find-placeholder"), &ui.find_input)
        .id(EPD_FIND_ID)
        .on_input(|t| Message::Epd(EpdUiMessage::FindInput(t)))
        .on_submit(Message::Epd(EpdUiMessage::Find(true)))
        .padding(3)
        .size(BODY_TEXT)
        .width(Length::Fixed(200.0));
    let mut bar = row![
        text(line).size(12),
        text(moves).size(12).style(theme::muted_text),
        space::horizontal(),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    if tab.skipped_lines > 0 {
        let mut a = FluentArgs::new();
        a.set(
            "count",
            i64::try_from(tab.skipped_lines).unwrap_or(i64::MAX),
        );
        bar = bar.push(
            button(text(i18n.tr_args("epd-skipped-lines", &a)).size(12))
                .padding([2, 6])
                .style(button::text)
                .on_press(Message::Epd(EpdUiMessage::OpenReport)),
        );
    }
    bar = bar
        .push(find)
        .push(super::toolbar::tool(
            i18n,
            crate::widgets::icon::Icon::SortUp,
            "epd-find-prev",
            Some(Message::Epd(EpdUiMessage::Find(false))),
            "epd-btn-find-prev",
        ))
        .push(super::toolbar::tool(
            i18n,
            crate::widgets::icon::Icon::SortDown,
            "epd-find-next",
            Some(Message::Epd(EpdUiMessage::Find(true))),
            "epd-btn-find-next",
        ));
    let mut page = column![container(bar).id("epd-status")].spacing(2);
    if let Some(run) = run_line(shell, tab) {
        page = page.push(container(muted(run)).id("epd-run-line"));
    }
    page.into()
}

#[cfg(test)]
mod tests {
    use iced::keyboard::key::{NativeCode, Physical};
    use iced::keyboard::{Key, Modifiers};

    use super::*;

    fn press(key: Named, modifiers: Modifiers, status: Status) -> KeyPress {
        KeyPress {
            key: Key::Named(key),
            modified_key: Key::Named(key),
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            modifiers,
            text: None,
            status,
        }
    }

    const FOCUSED: Status = Status::Focused { is_hovered: false };

    #[test]
    fn command_enter_in_the_focused_editor_commits_the_record() {
        let binding = key_binding(press(Named::Enter, Modifiers::COMMAND, FOCUSED));
        assert!(
            matches!(
                binding,
                Some(Binding::Custom(Message::App(mychess_app::Message::Epd(
                    EpdMessage::Commit
                ))))
            ),
            "{:?}",
            binding.map(|_| ())
        );
    }

    #[test]
    fn a_plain_enter_is_a_new_line_and_an_unfocused_editor_ignores_keys() {
        let plain = key_binding(press(Named::Enter, Modifiers::empty(), FOCUSED));
        assert!(matches!(plain, Some(Binding::Enter)));
        let unfocused = key_binding(press(Named::Enter, Modifiers::COMMAND, Status::Active));
        assert!(unfocused.is_none());
    }
}
