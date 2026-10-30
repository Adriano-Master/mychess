//! The cards of the opening report window: options, save, exclude and the
//! favorites (spec M7-2c, D3618, D3620, D3624).

use iced::widget::{column, container, row, scrollable, text, text_input, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::opening_report::{
    Favorite, OpeningReportPrefs, OpeningReportView, ReportFormat, ReportPart,
};

use super::{
    FavDraft, OPREP_EXCLUDE_BOX_ID, OPREP_FAV_NAME_ID, OPREP_FAV_RENAME_ID, OPREP_FAVORITES_BOX_ID,
    OPREP_OPTIONS_BOX_ID, OPREP_SAVE_BOX_ID, OpRepForm, OpRepUiMessage, OptCount, OptFlag,
    SaveDraft, ui,
};
use crate::shell::{Message, Shell};
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::modal::{Card, modal};
use crate::windows::panel_kit::{BODY_TEXT, action, check, choice, muted};

/// The key of the name of a part of the report.
pub(super) fn part_key(part: ReportPart) -> &'static str {
    match part {
        ReportPart::Full => "oprep-part-full",
        ReportPart::Compact => "oprep-part-compact",
        ReportPart::TableOnly => "oprep-part-table",
    }
}

/// The key of the name of a format.
pub(super) fn format_key(format: ReportFormat) -> &'static str {
    match format {
        ReportFormat::Text => "oprep-fmt-text",
        ReportFormat::Html => "oprep-fmt-html",
        ReportFormat::Latex => "oprep-fmt-latex",
    }
}

fn labelled(label: String, body: Element<'_, Message>) -> Element<'_, Message> {
    row![
        text(label).size(BODY_TEXT).width(Length::Fixed(190.0)),
        body
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// A small icon button with a tooltip; without a message it is disabled.
fn icon_button<'a>(
    shell: &Shell,
    glyph: Icon,
    tip_key: &str,
    message: Option<Message>,
    id: String,
) -> Element<'a, Message> {
    let tone = if message.is_some() {
        IconTone::Text
    } else {
        IconTone::Disabled
    };
    let body = iced::widget::button(icon(glyph, 15.0, tone))
        .padding(4)
        .style(theme::command_button(ButtonLook::Secondary, false))
        .on_press_maybe(message);
    let tip = container(text(shell.i18n.tr(tip_key)).size(12))
        .padding([4, 8])
        .style(theme::tooltip_box);
    container(tooltip(body, tip, tooltip::Position::Bottom))
        .id(id)
        .into()
}

fn part_and_format<'a>(
    shell: &Shell,
    draft: SaveDraft,
    part: fn(ReportPart) -> OpRepUiMessage,
    format: fn(ReportFormat) -> OpRepUiMessage,
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut parts = row![].spacing(4);
    for p in [ReportPart::Full, ReportPart::Compact, ReportPart::TableOnly] {
        parts = parts.push(choice(
            i18n.tr(part_key(p)),
            draft.part == p,
            Some(ui(part(p))),
        ));
    }
    let mut formats = row![].spacing(4);
    for f in [ReportFormat::Text, ReportFormat::Html, ReportFormat::Latex] {
        formats = formats.push(choice(
            i18n.tr(format_key(f)),
            draft.format == f,
            Some(ui(format(f))),
        ));
    }
    column![
        labelled(i18n.tr("oprep-ui-part"), parts.wrap().into()),
        labelled(i18n.tr("oprep-ui-format"), formats.wrap().into()),
    ]
    .spacing(8)
    .into()
}

fn options_form<'a>(shell: &Shell, draft: &OpeningReportPrefs) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut flags = column![].spacing(4);
    for flag in OptFlag::ALL {
        flags = flags.push(check(
            i18n.tr(flag.label_key()),
            flag.get(draft),
            Some(move |on| ui(OpRepUiMessage::Flag(flag, on))),
        ));
    }
    let mut counts = column![].spacing(6);
    for count in OptCount::ALL {
        let value = count.get(draft);
        let name = count.label_key().trim_start_matches("oprep-opt-");
        let step = |up: bool, glyph: &'static str| {
            let message = count
                .can_step(value, up)
                .then(|| ui(OpRepUiMessage::Count(count, up)));
            container(action(glyph.to_owned(), message)).id(format!(
                "oprep-count-{name}-{}",
                if up { "up" } else { "down" }
            ))
        };
        counts = counts.push(labelled(
            i18n.tr(count.label_key()),
            row![
                step(false, "-"),
                container(text(value.to_string()).size(BODY_TEXT))
                    .id(format!("oprep-count-{name}-value"))
                    .width(Length::Fixed(64.0))
                    .center_x(Length::Fixed(64.0)),
                step(true, "+"),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
        ));
    }
    modal(Card {
        id: OPREP_OPTIONS_BOX_ID,
        title: i18n.tr("oprep-ui-options-title"),
        body: scrollable(
            row![flags, counts]
                .spacing(28)
                .padding([0, 12])
                .align_y(Alignment::Start),
        )
        .height(Length::Fixed(380.0))
        .into(),
        buttons: vec![
            container(action(
                i18n.tr("oprep-ui-defaults"),
                Some(ui(OpRepUiMessage::OptionsDefaults)),
            ))
            .id("oprep-options-defaults")
            .into(),
            container(action(
                i18n.tr("dialog-cancel"),
                Some(ui(OpRepUiMessage::CloseForm)),
            ))
            .id("oprep-options-cancel")
            .into(),
            container(action(
                i18n.tr("dialog-ok"),
                Some(ui(OpRepUiMessage::OptionsOk)),
            ))
            .id("oprep-options-ok")
            .into(),
        ],
        width: 640.0,
    })
}

fn save_form<'a>(shell: &Shell, draft: SaveDraft) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    modal(Card {
        id: OPREP_SAVE_BOX_ID,
        title: i18n.tr("oprep-ui-save-title"),
        body: part_and_format(
            shell,
            draft,
            OpRepUiMessage::SavePart,
            OpRepUiMessage::SaveFormat,
        ),
        buttons: vec![
            container(action(
                i18n.tr("dialog-cancel"),
                Some(ui(OpRepUiMessage::CloseForm)),
            ))
            .id("oprep-save-cancel")
            .into(),
            container(action(
                i18n.tr("oprep-ui-save-choose"),
                Some(ui(OpRepUiMessage::SaveChoose)),
            ))
            .id("oprep-save-choose")
            .into(),
        ],
        width: 560.0,
    })
}

fn exclude_form<'a>(shell: &Shell, view: &OpeningReportView<'_>) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut moves = row![
        container(choice(
            i18n.tr("oprep-ui-exclude-none"),
            view.exclude.is_none(),
            Some(ui(OpRepUiMessage::Exclude(None))),
        ))
        .id("oprep-exclude-none")
    ]
    .spacing(4);
    for san in &shell.app.opening_report_exclude_choices() {
        moves = moves.push(
            container(choice(
                san.clone(),
                view.exclude == Some(san.as_str()),
                Some(ui(OpRepUiMessage::Exclude(Some(san.clone())))),
            ))
            .id(format!("oprep-exclude-{san}")),
        );
    }
    modal(Card {
        id: OPREP_EXCLUDE_BOX_ID,
        title: i18n.tr("oprep-ui-exclude-title"),
        body: column![muted(i18n.tr("oprep-ui-exclude-hint")), moves.wrap()]
            .spacing(8)
            .into(),
        buttons: vec![
            container(action(
                i18n.tr("dialog-cancel"),
                Some(ui(OpRepUiMessage::CloseForm)),
            ))
            .id("oprep-exclude-cancel")
            .into(),
        ],
        width: 620.0,
    })
}

fn favorite_row<'a>(
    shell: &Shell,
    index: usize,
    count: usize,
    favorite: &Favorite,
    draft: &FavDraft,
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    if let Some((at, name)) = &draft.rename
        && *at == index
    {
        let valid = !name.trim().is_empty();
        let go = valid.then(|| ui(OpRepUiMessage::FavRenameGo));
        return row![
            text_input("", name)
                .id(OPREP_FAV_RENAME_ID)
                .on_input(|t| ui(OpRepUiMessage::FavRenameInput(t)))
                .on_submit_maybe(go.clone())
                .padding(4)
                .size(BODY_TEXT)
                .width(Length::Fill),
            container(action(i18n.tr("dialog-ok"), go)).id("oprep-fav-rename-ok"),
            container(action(
                i18n.tr("dialog-cancel"),
                Some(ui(OpRepUiMessage::FavRenameCancel)),
            ))
            .id("oprep-fav-rename-cancel"),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into();
    }
    row![
        column![
            text(favorite.name.clone()).size(BODY_TEXT),
            text(favorite.moves.clone())
                .size(11)
                .style(theme::muted_text),
        ]
        .width(Length::Fill),
        container(action(
            i18n.tr("oprep-ui-fav-report"),
            Some(ui(OpRepUiMessage::FavOpen(index))),
        ))
        .id(format!("oprep-fav-open-{index}")),
        icon_button(
            shell,
            Icon::Manage,
            "oprep-ui-fav-rename",
            Some(ui(OpRepUiMessage::FavRenameStart(index))),
            format!("oprep-fav-rename-{index}"),
        ),
        icon_button(
            shell,
            Icon::SortUp,
            "oprep-ui-fav-up",
            (index > 0).then(|| ui(OpRepUiMessage::FavUp(index))),
            format!("oprep-fav-up-{index}"),
        ),
        icon_button(
            shell,
            Icon::SortDown,
            "oprep-ui-fav-down",
            (index + 1 < count).then(|| ui(OpRepUiMessage::FavDown(index))),
            format!("oprep-fav-down-{index}"),
        ),
        icon_button(
            shell,
            Icon::Trash,
            "oprep-ui-fav-delete",
            Some(ui(OpRepUiMessage::FavDelete(index))),
            format!("oprep-fav-delete-{index}"),
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn favorites_form<'a>(
    shell: &Shell,
    view: &OpeningReportView<'_>,
    draft: &FavDraft,
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut list = column![].spacing(6);
    if view.favorites.is_empty() {
        list = list.push(muted(i18n.tr("oprep-fav-none")));
    }
    for (i, f) in view.favorites.iter().enumerate() {
        list = list.push(favorite_row(shell, i, view.favorites.len(), f, draft));
    }
    let name_ok = !draft.name.trim().is_empty();
    let add = name_ok.then(|| ui(OpRepUiMessage::FavAdd));
    let add_row = row![
        text_input(&i18n.tr("oprep-ui-fav-name"), &draft.name)
            .id(OPREP_FAV_NAME_ID)
            .on_input(|t| ui(OpRepUiMessage::FavName(t)))
            .on_submit_maybe(add.clone())
            .padding(4)
            .size(BODY_TEXT)
            .width(Length::Fill),
        container(action(i18n.tr("oprep-ui-fav-add"), add)).id("oprep-fav-add"),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let can_batch = !view.favorites.is_empty() && view.batch.is_none();
    let batch = column![
        text(i18n.tr("oprep-ui-fav-batch-title")).size(BODY_TEXT),
        part_and_format(
            shell,
            draft.batch,
            OpRepUiMessage::FavPart,
            OpRepUiMessage::FavFormat
        ),
        row![
            container(action(
                i18n.tr("oprep-ui-fav-import"),
                Some(ui(OpRepUiMessage::FavImportChoose)),
            ))
            .id("oprep-fav-import"),
            container(action(
                i18n.tr("oprep-ui-fav-generate"),
                can_batch.then(|| ui(OpRepUiMessage::FavBatchChoose)),
            ))
            .id("oprep-fav-generate"),
        ]
        .spacing(8),
    ]
    .spacing(8);
    modal(Card {
        id: OPREP_FAVORITES_BOX_ID,
        title: i18n.tr("oprep-ui-favorites-title"),
        body: column![
            scrollable(list).height(Length::Fixed(220.0)),
            add_row,
            batch
        ]
        .spacing(12)
        .into(),
        buttons: vec![
            container(action(
                i18n.tr("oprep-ui-close"),
                Some(ui(OpRepUiMessage::CloseForm)),
            ))
            .id("oprep-favorites-close")
            .into(),
        ],
        width: 680.0,
    })
}

/// The card to put over the window, if a form is open.
pub(super) fn overlay<'a>(
    shell: &'a Shell,
    view: &OpeningReportView<'_>,
) -> Option<Element<'a, Message>> {
    Some(match shell.ui.dialogs.oprep.form.as_ref()? {
        OpRepForm::Options(draft) => options_form(shell, draft),
        OpRepForm::Save(draft) => save_form(shell, *draft),
        OpRepForm::Exclude => exclude_form(shell, view),
        OpRepForm::Favorites(draft) => favorites_form(shell, view, draft),
    })
}
