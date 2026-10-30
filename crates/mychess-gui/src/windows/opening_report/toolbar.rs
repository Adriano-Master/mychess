//! The title bar, the buttons, the progress and the warnings of the opening
//! report window (spec M7-2c, D3621, D3628, D3629).

use fluent_bundle::FluentArgs;
use iced::widget::{button, container, row, space, text, text_input, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::opening_report::{
    OpeningReportMessage, OpeningReportView, ReportFailure, ReportPhase,
};

use super::{OPREP_FIND_ID, OpRepUiMessage, orep, ui};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::windows::panel_kit::{BODY_TEXT, action, danger, muted, progress};

/// A button with an icon and a label; without a message it is disabled.
fn tool<'a>(
    glyph: Icon,
    label: String,
    message: Option<Message>,
    id: &'static str,
) -> Element<'a, Message> {
    let tone = if message.is_some() {
        IconTone::Text
    } else {
        IconTone::Disabled
    };
    let body = button(
        row![icon(glyph, 16.0, tone), text(label).size(BODY_TEXT)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .style(theme::command_button(ButtonLook::Secondary, false))
    .on_press_maybe(message);
    container(body).id(id).into()
}

/// A button with a tooltip that explains why it is disabled.
fn with_tip<'a>(i18n: &I18n, body: Element<'a, Message>, tip_key: &str) -> Element<'a, Message> {
    let tip = container(text(i18n.tr(tip_key)).size(12))
        .padding([4, 8])
        .style(theme::tooltip_box);
    tooltip(body, tip, tooltip::Position::Bottom).into()
}

/// The title and the button that closes the window.
pub(super) fn title_bar<'a>(
    shell: &'a Shell,
    view: &OpeningReportView<'_>,
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let line = shell
        .ui
        .dialogs
        .oprep
        .model()
        .filter(|_| view.report.is_some())
        .map(|m| m.header.title.as_str())
        .unwrap_or_default();
    row![
        text(i18n.tr("oprep-title")).size(18),
        text(line).size(15).style(theme::accent_text),
        space::horizontal(),
        container(action(
            i18n.tr("oprep-ui-close"),
            Some(ui(OpRepUiMessage::Close))
        ))
        .id("oprep-close"),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

/// The buttons of the report and the search field.
pub(super) fn tools<'a>(shell: &'a Shell, view: &OpeningReportView<'_>) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let running = matches!(view.phase, ReportPhase::Generating { .. }) || view.batch.is_some();
    let has_report = view.report.is_some() && !running;
    let fresh = has_report && !view.stale;
    let on = |ok: bool, m: Message| ok.then_some(m);
    let games = view.report.is_some_and(|r| r.game_count() > 0);
    let exclude_label = match view.exclude {
        Some(mv) => format!("{}: {mv}", i18n.tr("oprep-ui-exclude")),
        None => i18n.tr("oprep-ui-exclude"),
    };
    let favorite_icon = if view.favorites.is_empty() {
        Icon::StarOff
    } else {
        Icon::Star
    };
    let count = if shell.ui.dialogs.oprep.find.is_empty() {
        String::new()
    } else {
        let mut args = FluentArgs::new();
        args.set(
            "count",
            i64::try_from(shell.ui.dialogs.oprep.hits.count()).unwrap_or(0),
        );
        i18n.tr_args("oprep-ui-matches", &args)
    };
    let find = text_input(&i18n.tr("oprep-ui-find"), &shell.ui.dialogs.oprep.find)
        .id(OPREP_FIND_ID)
        .on_input(|t| ui(OpRepUiMessage::Find(t)))
        .padding(4)
        .size(BODY_TEXT)
        .width(Length::Fixed(190.0));
    let latex = with_tip(
        i18n,
        tool(
            Icon::Report,
            i18n.tr("oprep-ui-view-latex"),
            None,
            "oprep-view-latex",
        ),
        "oprep-ui-view-latex-tip",
    );
    row![
        tool(
            Icon::Rematch,
            i18n.tr("oprep-ui-update"),
            on(!running, orep(OpeningReportMessage::Refresh)),
            "oprep-update",
        ),
        tool(
            Icon::Options,
            i18n.tr("oprep-ui-options"),
            Some(ui(OpRepUiMessage::OpenOptions)),
            "oprep-options",
        ),
        tool(
            Icon::Negate,
            exclude_label,
            on(fresh && games, ui(OpRepUiMessage::OpenExclude)),
            "oprep-exclude",
        ),
        tool(
            favorite_icon,
            i18n.tr("oprep-ui-favorites"),
            Some(ui(OpRepUiMessage::OpenFavorites)),
            "oprep-favorites",
        ),
        tool(
            Icon::AppendFile,
            i18n.tr("oprep-ui-save"),
            on(fresh, ui(OpRepUiMessage::OpenSave)),
            "oprep-save",
        ),
        tool(
            Icon::Report,
            i18n.tr("oprep-ui-view-html"),
            on(fresh, orep(OpeningReportMessage::ViewHtml)),
            "oprep-view-html",
        ),
        latex,
        tool(
            Icon::Merge,
            i18n.tr("oprep-ui-merge"),
            on(fresh && games, orep(OpeningReportMessage::MergeGames)),
            "oprep-merge",
        ),
        find,
        text(count).size(12).style(theme::muted_text),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .wrap()
    .into()
}

fn failure_key(failure: ReportFailure) -> &'static str {
    match failure {
        ReportFailure::UnsupportedPosition => "oprep-err-position",
        ReportFailure::BadLine => "oprep-err-line",
        _ => "oprep-err-failed",
    }
}

/// The lines under the buttons: out of date, in progress, interrupted,
/// failed, and the batch of the favorites.
pub(super) fn banners<'a>(shell: &'a Shell, view: &OpeningReportView<'_>) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut col = iced::widget::column![].spacing(6);
    match &view.phase {
        ReportPhase::Generating { search, generate } => {
            col = col.push(
                iced::widget::column![
                    row![
                        text(i18n.tr("oprep-ui-phase-search"))
                            .size(BODY_TEXT)
                            .width(Length::Fixed(330.0)),
                        container(progress(search.0, search.1)).width(Length::Fixed(260.0)),
                        container(action(
                            i18n.tr("oprep-ui-stop"),
                            Some(orep(OpeningReportMessage::Stop))
                        ))
                        .id("oprep-stop"),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    row![
                        text(i18n.tr("oprep-ui-phase-generate"))
                            .size(BODY_TEXT)
                            .width(Length::Fixed(330.0)),
                        container(progress(generate.0, generate.1)).width(Length::Fixed(260.0)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                ]
                .spacing(4),
            );
        }
        ReportPhase::SearchInterrupted => {
            col = col.push(danger(i18n.tr("oprep-interrupted")));
        }
        ReportPhase::Failed(failure) => {
            col = col.push(danger(i18n.tr(failure_key(*failure))));
        }
        _ => {}
    }
    if view.stale && !matches!(view.phase, ReportPhase::Generating { .. }) {
        col = col.push(
            row![
                danger(i18n.tr("oprep-stale")),
                container(action(
                    i18n.tr("oprep-ui-update"),
                    Some(orep(OpeningReportMessage::Refresh))
                ))
                .id("oprep-stale-update"),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    if let Some(batch) = &view.batch {
        let mut args = FluentArgs::new();
        args.set("current", i64::try_from(batch.current).unwrap_or(0));
        args.set("total", i64::try_from(batch.total).unwrap_or(0));
        args.set("name", batch.name.as_str());
        col = col.push(
            row![
                muted(i18n.tr_args("oprep-ui-batch", &args)),
                container(action(
                    i18n.tr("oprep-ui-stop"),
                    Some(orep(OpeningReportMessage::Favorite(
                        mychess_app::opening_report::FavoriteMessage::StopBatch
                    )))
                ))
                .id("oprep-batch-stop"),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    col.into()
}

/// The last remark of the application about the report.
pub(super) fn footer(shell: &Shell) -> Element<'_, Message> {
    let text_line = match &shell.ui.status_notice {
        Some(mychess_app::AppNotice::OpeningReport(notice)) => {
            super::text::notice_text(&shell.i18n, notice)
        }
        _ => String::new(),
    };
    container(text(text_line).size(12).style(theme::muted_text))
        .id("oprep-footer")
        .into()
}
