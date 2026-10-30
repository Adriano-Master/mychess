//! The modal cards of the EPD window: the questions of the application
//! (save before closing, remove a position), the form of the analysis, the
//! opcode to strip, the list of ignored lines and the question on quitting
//! (spec M7-5c).

use fluent_bundle::FluentArgs;
use iced::widget::{button, column, row, scrollable, text, text_input};
use iced::{Element, Length};
use mychess_app::epd_window::{AnnotateMode, EpdCloseChoice, EpdMessage, EpdTabView, RunScope};

use super::{
    AnalyzeDraft, EPD_ANALYZE_BOX_ID, EPD_CLOSE_BOX_ID, EPD_QUIT_BOX_ID, EPD_REMOVE_BOX_ID,
    EPD_REPORT_BOX_ID, EPD_SECONDS_ID, EPD_STRIP_BOX_ID, EPD_STRIP_ID, EPD_UNDO_BOX_ID, EpdForm,
    EpdUiMessage, epd, text as words,
};
use crate::shell::{Message, Shell};
use crate::widgets::modal::{Card, modal};
use crate::windows::panel_kit::{BODY_TEXT, choice, danger, muted};

fn ui(message: EpdUiMessage) -> Message {
    Message::Epd(message)
}

fn push<'a>(label: String, message: Option<Message>) -> Element<'a, Message> {
    button(text(label)).on_press_maybe(message).into()
}

fn secondary<'a>(label: String, message: Message) -> Element<'a, Message> {
    button(text(label))
        .style(button::secondary)
        .on_press(message)
        .into()
}

fn labelled(label: String, body: Element<'_, Message>) -> Element<'_, Message> {
    row![
        text(label).size(BODY_TEXT).width(Length::Fixed(150.0)),
        body
    ]
    .spacing(6)
    .into()
}

/// The question "save before closing?" of the file in `tab`.
fn close_question<'a>(shell: &Shell, tab: &EpdTabView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let answer = |c| epd(EpdMessage::AnswerClose(c));
    modal(Card {
        id: EPD_CLOSE_BOX_ID,
        title: i18n.tr("epd-confirm-close-title"),
        body: column![
            text(tab.name.clone()).size(BODY_TEXT),
            text(i18n.tr("epd-confirm-close")).size(BODY_TEXT),
        ]
        .spacing(6)
        .into(),
        buttons: vec![
            secondary(i18n.tr("dialog-cancel"), answer(EpdCloseChoice::Cancel)),
            secondary(
                i18n.tr("dialog-discard-dont-save"),
                answer(EpdCloseChoice::Discard),
            ),
            push(
                i18n.tr("dialog-discard-save"),
                Some(answer(EpdCloseChoice::Save)),
            ),
        ],
        width: 460.0,
    })
}

fn remove_question<'a>(shell: &Shell) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    modal(Card {
        id: EPD_REMOVE_BOX_ID,
        title: i18n.tr("epd-confirm-remove-title"),
        body: text(i18n.tr("epd-confirm-remove")).size(BODY_TEXT).into(),
        buttons: vec![
            secondary(
                i18n.tr("dialog-cancel"),
                epd(EpdMessage::ConfirmRemove(false)),
            ),
            button(text(i18n.tr("epd-confirm-remove-yes")))
                .style(button::danger)
                .on_press(epd(EpdMessage::ConfirmRemove(true)))
                .into(),
        ],
        width: 420.0,
    })
}

fn analyze_form<'a>(shell: &'a Shell, draft: &AnalyzeDraft) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut engines = row![choice(
        i18n.tr("epd-engine-default"),
        draft.engine.is_none(),
        Some(ui(EpdUiMessage::AnalyzeEngine(None))),
    )]
    .spacing(4);
    let mut any = false;
    for entry in shell.app.engines.engines() {
        if entry.protocol != mychess_app::engines::Protocol::Uci {
            continue;
        }
        any = true;
        engines = engines.push(choice(
            entry.name.clone(),
            draft.engine == Some(entry.id),
            Some(ui(EpdUiMessage::AnalyzeEngine(Some(entry.id)))),
        ));
    }
    let mut page = column![labelled(i18n.tr("epd-engine"), engines.wrap().into())].spacing(8);
    if !any {
        page = page.push(muted(i18n.tr("epd-engine-none")));
    }
    let valid = draft.seconds_value().is_some();
    let seconds = text_input("", &draft.seconds)
        .id(EPD_SECONDS_ID)
        .on_input(|t| ui(EpdUiMessage::AnalyzeSeconds(t)))
        .on_submit_maybe((valid && any).then(|| ui(EpdUiMessage::AnalyzeStart)))
        .padding(4)
        .size(BODY_TEXT)
        .width(Length::Fixed(80.0));
    let mut seconds_row = row![seconds].spacing(8);
    if !valid {
        seconds_row = seconds_row.push(danger(i18n.tr("epd-seconds-range")));
    }
    page = page.push(labelled(i18n.tr("epd-seconds"), seconds_row.into()));
    let mut modes = row![].spacing(4);
    for mode in [
        AnnotateMode::Count,
        AnnotateMode::Annotate,
        AnnotateMode::Both,
    ] {
        modes = modes.push(choice(
            i18n.tr(words::mode_key(mode)),
            draft.mode == mode,
            Some(ui(EpdUiMessage::AnalyzeMode(mode))),
        ));
    }
    page = page.push(labelled(i18n.tr("epd-mode"), modes.wrap().into()));
    let mut scopes = row![].spacing(4);
    for scope in [RunScope::All, RunScope::FromSelected, RunScope::WithoutCe] {
        scopes = scopes.push(choice(
            i18n.tr(words::scope_key(scope)),
            draft.scope == scope,
            Some(ui(EpdUiMessage::AnalyzeScope(scope))),
        ));
    }
    page = page.push(labelled(i18n.tr("epd-scope"), scopes.wrap().into()));
    modal(Card {
        id: EPD_ANALYZE_BOX_ID,
        title: i18n.tr("epd-analyze-title"),
        body: page.into(),
        buttons: vec![
            secondary(i18n.tr("dialog-cancel"), ui(EpdUiMessage::CloseForm)),
            push(
                i18n.tr("epd-start"),
                (valid && any).then(|| ui(EpdUiMessage::AnalyzeStart)),
            ),
        ],
        width: 560.0,
    })
}

fn strip_form<'a>(shell: &Shell, opcode: &str) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let field = text_input("bm", opcode)
        .id(EPD_STRIP_ID)
        .on_input(|t| ui(EpdUiMessage::StripInput(t)))
        .on_submit(ui(EpdUiMessage::StripGo))
        .padding(4)
        .size(BODY_TEXT);
    modal(Card {
        id: EPD_STRIP_BOX_ID,
        title: i18n.tr("epd-strip-title"),
        body: column![text(i18n.tr("epd-strip-prompt")).size(BODY_TEXT), field]
            .spacing(6)
            .into(),
        buttons: vec![
            secondary(i18n.tr("dialog-cancel"), ui(EpdUiMessage::CloseForm)),
            push(
                i18n.tr("epd-strip-do"),
                (!opcode.trim().is_empty()).then(|| ui(EpdUiMessage::StripGo)),
            ),
        ],
        width: 420.0,
    })
}

fn report<'a>(
    shell: &Shell,
    id: mychess_app::epd_window::EpdFileId,
) -> Option<Element<'a, Message>> {
    let i18n = &shell.i18n;
    let (diagnostics, dropped) = shell.app.epd_diagnostics(id)?;
    let name = shell
        .app
        .epd_view()
        .tabs
        .iter()
        .find(|t| t.id == id)
        .map(|t| t.name.clone())
        .unwrap_or_default();
    let mut lines = column![].spacing(2);
    for d in diagnostics {
        let mut args = FluentArgs::new();
        args.set("line", i64::try_from(d.line).unwrap_or(i64::MAX));
        args.set("what", words::diagnostic_text(i18n, &d.kind));
        lines = lines.push(text(i18n.tr_args("epd-report-line", &args)).size(BODY_TEXT));
    }
    if dropped > 0 {
        let mut args = FluentArgs::new();
        args.set("count", i64::try_from(dropped).unwrap_or(i64::MAX));
        lines = lines.push(muted(i18n.tr_args("epd-report-more", &args)));
    }
    let mut args = FluentArgs::new();
    args.set("name", name);
    Some(modal(Card {
        id: EPD_REPORT_BOX_ID,
        title: i18n.tr_args("epd-report-title", &args),
        body: scrollable(lines).height(Length::Fixed(260.0)).into(),
        buttons: vec![push(
            i18n.tr("dialog-ok"),
            Some(ui(EpdUiMessage::CloseForm)),
        )],
        width: 560.0,
    }))
}

fn simple<'a>(
    shell: &Shell,
    id: &'static str,
    title: &str,
    body: String,
    no: Message,
    yes_label: &str,
    yes: Message,
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    modal(Card {
        id,
        title: i18n.tr(title),
        body: text(body).size(BODY_TEXT).into(),
        buttons: vec![
            secondary(i18n.tr("dialog-cancel"), no),
            button(text(i18n.tr(yes_label)))
                .style(button::danger)
                .on_press(yes)
                .into(),
        ],
        width: 460.0,
    })
}

/// The layer to put over the window, if a question or a form is open.
pub fn overlay(shell: &Shell) -> Option<Element<'_, Message>> {
    let i18n = &shell.i18n;
    let view = shell.app.epd_view();
    if let Some(id) = view.close_prompt
        && let Some(tab) = view.tabs.iter().find(|t| t.id == id)
    {
        return Some(close_question(shell, tab));
    }
    if view.tabs.iter().any(|t| t.pending_remove.is_some()) {
        return Some(remove_question(shell));
    }
    match shell.ui.dialogs.epd.form.as_ref()? {
        EpdForm::Analyze(draft) => Some(analyze_form(shell, draft)),
        EpdForm::ConfirmUndo(_) => Some(simple(
            shell,
            EPD_UNDO_BOX_ID,
            "epd-confirm-undo-title",
            i18n.tr("epd-confirm-undo"),
            ui(EpdUiMessage::ConfirmUndo(false)),
            "epd-start",
            ui(EpdUiMessage::ConfirmUndo(true)),
        )),
        EpdForm::Strip(opcode) => Some(strip_form(shell, opcode)),
        EpdForm::Report(id) => report(shell, *id),
        EpdForm::ConfirmQuit => {
            let mut args = FluentArgs::new();
            args.set(
                "count",
                i64::try_from(shell.app.epd_dirty_files().len()).unwrap_or(1),
            );
            Some(simple(
                shell,
                EPD_QUIT_BOX_ID,
                "epd-confirm-quit-title",
                i18n.tr_args("epd-confirm-quit", &args),
                ui(EpdUiMessage::ConfirmQuit(false)),
                "epd-confirm-quit-yes",
                ui(EpdUiMessage::ConfirmQuit(true)),
            ))
        }
    }
}
