//! The dialog "Generate player report", the question about the Clipbase and
//! the save form (spec M7-3c, M7-3/D4201-D4204).

use iced::widget::{column, container, row, stack, text, text_input};
use iced::{Alignment, Element, Length};
use mychess_app::player_report::{
    NameMatching, PlayerColor, PlayerReportView, PlayerStart, ReportFormat, ReportPart,
};

use super::{
    PREPORT_CONFIRM_ID, PREPORT_DIALOG_ID, PREPORT_NAME_ID, PREPORT_OVERWRITE_ID,
    PREPORT_SAVE_BOX_ID, PrForm, PrUiMessage, SaveDraft, ui,
};
use crate::shell::{Message, Shell};
use crate::widgets::modal::{Card, modal};
use crate::windows::panel_kit::{BODY_TEXT, action, check, choice, muted};

/// The key of the name of a part of the report.
pub(super) fn part_key(part: ReportPart) -> &'static str {
    match part {
        ReportPart::Full => "preport-ui-part-full",
        ReportPart::Compact => "preport-ui-part-compact",
        ReportPart::TableOnly => "preport-ui-part-table",
    }
}

/// The key of the name of a format.
pub(super) fn format_key(format: ReportFormat) -> &'static str {
    match format {
        ReportFormat::Text => "preport-ui-fmt-text",
        ReportFormat::Html => "preport-ui-fmt-html",
        ReportFormat::Latex => "preport-ui-fmt-latex",
    }
}

fn labelled(label: String, body: Element<'_, Message>) -> Element<'_, Message> {
    row![
        text(label).size(BODY_TEXT).width(Length::Fixed(130.0)),
        body
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn with_id<'a>(id: &'static str, body: Element<'a, Message>) -> Element<'a, Message> {
    container(body).id(id).into()
}

#[allow(clippy::too_many_lines)] // one form, one widget per line
fn dialog_card(shell: &Shell) -> Option<Element<'_, Message>> {
    let i18n = &shell.i18n;
    let draft = shell.ui.dialogs.preport.draft.as_ref()?;
    let view = shell.app.player_report_dialog()?;
    let usable = {
        let name = draft.name.trim();
        !name.is_empty() && name != "?"
    };
    let go = usable.then(|| ui(PrUiMessage::Generate));
    let name = text_input("", &draft.name)
        .id(PREPORT_NAME_ID)
        .on_input(|t| ui(PrUiMessage::Name(t)))
        .on_submit_maybe(go.clone())
        .padding(4)
        .size(BODY_TEXT)
        .width(Length::Fill);

    let mut recent = row![].spacing(4);
    for (i, name) in view.history.iter().enumerate() {
        recent = recent.push(with_id_owned(
            format!("preport-recent-{i}"),
            choice(
                name.clone(),
                draft.name == *name,
                Some(ui(PrUiMessage::Recent(name.clone()))),
            ),
        ));
    }
    let mut game_buttons = row![].spacing(6);
    for (color, name, id) in [
        (PlayerColor::White, &view.white_name, "preport-white"),
        (PlayerColor::Black, &view.black_name, "preport-black"),
    ] {
        let (key, plain) = if color == PlayerColor::White {
            ("preport-ui-white-of", "preport-ui-white")
        } else {
            ("preport-ui-black-of", "preport-ui-black")
        };
        let label = match name {
            Some(n) => {
                let mut args = fluent_bundle::FluentArgs::new();
                args.set("name", n.as_str());
                i18n.tr_args(key, &args)
            }
            None => i18n.tr(plain),
        };
        game_buttons = game_buttons.push(with_id(
            id,
            action(
                label,
                name.is_some().then(|| ui(PrUiMessage::FromGame(color))),
            ),
        ));
    }

    let colors = row![
        with_id(
            "preport-color-white",
            choice(
                i18n.tr("preport-color-white"),
                draft.color == PlayerColor::White,
                Some(ui(PrUiMessage::Color(PlayerColor::White))),
            ),
        ),
        with_id(
            "preport-color-black",
            choice(
                i18n.tr("preport-color-black"),
                draft.color == PlayerColor::Black,
                Some(ui(PrUiMessage::Color(PlayerColor::Black))),
            ),
        ),
    ]
    .spacing(4);
    let mut matching = row![].spacing(4);
    for (m, key, id) in [
        (
            NameMatching::Exact,
            "preport-matching-exact",
            "preport-match-exact",
        ),
        (
            NameMatching::Prefix,
            "preport-matching-prefix",
            "preport-match-prefix",
        ),
        (
            NameMatching::Contains,
            "preport-matching-contains",
            "preport-match-contains",
        ),
    ] {
        matching = matching.push(with_id(
            id,
            choice(
                i18n.tr(key),
                draft.matching == m,
                Some(ui(PrUiMessage::Matching(m))),
            ),
        ));
    }
    let starts = row![
        with_id(
            "preport-start-beginning",
            choice(
                i18n.tr("preport-start-beginning"),
                draft.start == PlayerStart::Beginning,
                Some(ui(PrUiMessage::Start(PlayerStart::Beginning))),
            ),
        ),
        with_id(
            "preport-start-position",
            choice(
                i18n.tr("preport-start-position"),
                draft.start == PlayerStart::Position,
                Some(ui(PrUiMessage::Start(PlayerStart::Position))),
            ),
        ),
    ]
    .spacing(4);
    let clipbase = check(
        i18n.tr("preport-clipbase"),
        draft.copy_to_clipbase && view.clipbase_enabled,
        view.clipbase_enabled
            .then_some(|on| ui(PrUiMessage::Clipbase(on))),
    );

    let mut body = column![
        labelled(i18n.tr("preport-player"), name.into()),
        game_buttons,
    ]
    .spacing(8);
    if !view.history.is_empty() {
        body = body.push(muted(i18n.tr("preport-ui-recent")));
        body = body.push(recent.wrap());
    }
    body = body
        .push(labelled(i18n.tr("preport-color"), colors.into()))
        .push(labelled(
            i18n.tr("preport-matching"),
            matching.wrap().into(),
        ))
        .push(labelled(i18n.tr("preport-start"), starts.into()))
        .push(with_id("preport-clipbase", clipbase));

    Some(modal(Card {
        id: PREPORT_DIALOG_ID,
        title: i18n.tr("preport-dialog-title"),
        body: body.into(),
        buttons: vec![
            with_id(
                "preport-cancel",
                action(i18n.tr("dialog-cancel"), Some(ui(PrUiMessage::Cancel))),
            ),
            with_id("preport-ok", action(i18n.tr("dialog-ok"), go)),
        ],
        width: 640.0,
    }))
}

fn with_id_owned(id: String, body: Element<'_, Message>) -> Element<'_, Message> {
    container(body).id(id).into()
}

fn confirm_card(shell: &Shell) -> Element<'_, Message> {
    let i18n = &shell.i18n;
    modal(Card {
        id: PREPORT_CONFIRM_ID,
        title: i18n.tr("preport-title"),
        body: text(i18n.tr("preport-clipbase-confirm"))
            .size(BODY_TEXT)
            .into(),
        buttons: vec![
            with_id(
                "preport-confirm-no",
                action(
                    i18n.tr("dialog-cancel"),
                    Some(ui(PrUiMessage::Confirm(false))),
                ),
            ),
            with_id(
                "preport-confirm-yes",
                action(i18n.tr("dialog-ok"), Some(ui(PrUiMessage::Confirm(true)))),
            ),
        ],
        width: 460.0,
    })
}

/// "Replace the file?": the name came out of the file dialog with the
/// extension of the format, so the dialog could not ask. `Enter` is not
/// bound: the answer "yes" is a click (D4211).
fn overwrite_card<'a>(shell: &'a Shell, path: &std::path::Path) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    modal(Card {
        id: PREPORT_OVERWRITE_ID,
        title: i18n.tr("preport-title"),
        body: text({
            let mut args = fluent_bundle::FluentArgs::new();
            args.set("name", name.as_str());
            i18n.tr_args("preport-overwrite-confirm", &args)
        })
        .size(BODY_TEXT)
        .into(),
        buttons: vec![
            with_id(
                "preport-overwrite-no",
                action(
                    i18n.tr("dialog-cancel"),
                    Some(ui(PrUiMessage::ConfirmSave(false))),
                ),
            ),
            with_id(
                "preport-overwrite-yes",
                action(
                    i18n.tr("export-ui-replace"),
                    Some(ui(PrUiMessage::ConfirmSave(true))),
                ),
            ),
        ],
        width: 460.0,
    })
}

fn save_card(shell: &Shell, draft: SaveDraft) -> Element<'_, Message> {
    let i18n = &shell.i18n;
    let mut parts = row![].spacing(4);
    for p in [ReportPart::Full, ReportPart::Compact, ReportPart::TableOnly] {
        parts = parts.push(choice(
            i18n.tr(part_key(p)),
            draft.part == p,
            Some(ui(PrUiMessage::SavePart(p))),
        ));
    }
    let mut formats = row![].spacing(4);
    for f in [ReportFormat::Text, ReportFormat::Html, ReportFormat::Latex] {
        formats = formats.push(choice(
            i18n.tr(format_key(f)),
            draft.format == f,
            Some(ui(PrUiMessage::SaveFormat(f))),
        ));
    }
    modal(Card {
        id: PREPORT_SAVE_BOX_ID,
        title: i18n.tr("preport-ui-save-title"),
        body: column![
            labelled(i18n.tr("preport-ui-part"), parts.wrap().into()),
            labelled(i18n.tr("preport-ui-format"), formats.wrap().into()),
        ]
        .spacing(8)
        .into(),
        buttons: vec![
            with_id(
                "preport-save-cancel",
                action(i18n.tr("dialog-cancel"), Some(ui(PrUiMessage::CloseForm))),
            ),
            with_id(
                "preport-save-choose",
                action(
                    i18n.tr("preport-ui-save-choose"),
                    Some(ui(PrUiMessage::SaveChoose)),
                ),
            ),
        ],
        width: 560.0,
    })
}

/// The dialog and the cards over the window: the question about the
/// Clipbase is above the dialog, the save form above the report.
pub(super) fn overlay<'a>(
    shell: &'a Shell,
    view: &PlayerReportView<'_>,
) -> Option<Element<'a, Message>> {
    let mut layers = stack![];
    let mut any = false;
    if let Some(PrForm::Save(draft)) = shell.ui.dialogs.preport.form {
        layers = layers.push(save_card(shell, draft));
        any = true;
    }
    if let Some(card) = dialog_card(shell) {
        layers = layers.push(card);
        any = true;
    }
    if view.confirm_clipbase {
        layers = layers.push(confirm_card(shell));
        any = true;
    }
    if let Some(path) = view.confirm_overwrite {
        layers = layers.push(overwrite_card(shell, path));
        any = true;
    }
    any.then(|| layers.into())
}
