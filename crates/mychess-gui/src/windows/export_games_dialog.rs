//! The "Export games" window (spec M3-9, D2001-D2004): the options, the wait
//! for the file dialog, the progress of the job and the report. The
//! application keeps the state (`AppState::games_export_view`); the window
//! draws it, sends [`ListMessage::CancelJob`] to stop and
//! [`GamesExportMessage::Close`] to leave.

use std::path::Path;

use fluent_bundle::FluentArgs;
use iced::widget::{button, checkbox, column, container, progress_bar, radio, scrollable, text};
use iced::{Element, Length};
use mychess_app::export_docs::{ConfirmKind, ExportDocsMessage, ExportDocsView, ExportFormat};
use mychess_app::{
    AppState, ExportScope, GamesExportMessage, GamesExportNotice, GamesExportResult,
    GamesExportStepView, GamesExportView, ListMessage, Message as AppMessage,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::export_options::{format_options, format_selector};

/// Id of the box of the window.
pub const EXPORT_GAMES_BOX_ID: &str = "export-games-box";

fn send(message: GamesExportMessage) -> Message {
    Message::App(AppMessage::GamesExport(message))
}

/// The message that stops the job.
pub fn stop_message_app() -> AppMessage {
    AppMessage::Bases(mychess_app::BasesMessage::List(ListMessage::CancelJob))
}

/// The message that closes the window.
pub fn close_message_app() -> AppMessage {
    AppMessage::GamesExport(GamesExportMessage::Close)
}

/// The message of Enter in the options: asks for the file.
pub fn choose_message_app() -> AppMessage {
    AppMessage::GamesExport(GamesExportMessage::Choose)
}

/// Id of the box of the question about an existing file (spec M7-4c).
pub const EXPORT_CONFIRM_BOX_ID: &str = "export-confirm-box";

/// The message of Esc while the question about an existing file is open
/// (it answers "no"); `None` when no question is open.
pub fn escape_confirm_message(app: &AppState) -> Option<AppMessage> {
    app.export_docs_view()
        .confirm
        .map(|_| AppMessage::ExportDocs(ExportDocsMessage::Confirm(false)))
}

/// The message of Enter while the question is open: it answers "no", as
/// "replace" and "append" are destructive and "yes" needs a click (D4301).
pub fn enter_confirm_message(app: &AppState) -> Option<AppMessage> {
    app.export_docs_view()
        .confirm
        .map(|_| AppMessage::ExportDocs(ExportDocsMessage::Confirm(false)))
}

/// The box, when the application has the window open.
pub fn view<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let view = app.games_export_view()?;
    Some(export_box(&view, &app.export_docs_view(), i18n))
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn count_label(i18n: &I18n, key: &str, count: u32) -> String {
    let mut args = FluentArgs::new();
    args.set("count", i64::from(count));
    i18n.tr_args(key, &args)
}

fn scope_row<'a>(
    view: &GamesExportView<'_>,
    needs_db: bool,
    scope: ExportScope,
    label: String,
) -> Element<'a, Message> {
    if view.counts.available(scope) && !(needs_db && scope == ExportScope::Current) {
        radio(label, scope, Some(view.draft.scope), |s| {
            send(GamesExportMessage::SetScope(s))
        })
        .size(15)
        .text_size(13)
        .into()
    } else {
        text(label).size(13).style(theme::muted_text).into()
    }
}

fn toggle<'a>(
    label: String,
    on: bool,
    enabled: bool,
    change: impl Fn(bool) -> GamesExportMessage + 'a,
) -> Element<'a, Message> {
    checkbox(on)
        .label(label)
        .size(15)
        .text_size(13)
        .on_toggle_maybe(enabled.then_some(move |v| send(change(v))))
        .into()
}

fn options_body<'a>(
    view: &GamesExportView<'_>,
    docs: &ExportDocsView<'_>,
    i18n: &I18n,
) -> Element<'a, Message> {
    let draft = view.draft;
    let counts = view.counts;
    let format = docs.prefs.format;
    // The comments, variations and marks are the options of the PGN (and of
    // the PGN file beside the viewer).
    let pgn_options = matches!(format, ExportFormat::Pgn | ExportFormat::Viewer);
    let mut col = column![
        format_selector(docs, i18n),
        text(i18n.tr("export-games-what")).size(13),
        scope_row(
            view,
            format.needs_database(),
            ExportScope::Selection,
            count_label(i18n, "export-games-scope-selection", counts.selection)
        ),
        scope_row(
            view,
            format.needs_database(),
            ExportScope::Filter,
            count_label(i18n, "export-games-scope-filter", counts.filter)
        ),
        scope_row(
            view,
            format.needs_database(),
            ExportScope::All,
            count_label(i18n, "export-games-scope-all", counts.all)
        ),
        scope_row(
            view,
            format.needs_database(),
            ExportScope::Current,
            i18n.tr("export-games-scope-current")
        ),
    ]
    .spacing(8);
    if pgn_options {
        col = col
            .push(toggle(
                i18n.tr("export-games-comments"),
                draft.comments,
                true,
                GamesExportMessage::SetComments,
            ))
            .push(toggle(
                i18n.tr("export-games-variations"),
                draft.variations,
                true,
                GamesExportMessage::SetVariations,
            ))
            .push(toggle(
                i18n.tr("export-games-marks"),
                draft.marks,
                true,
                GamesExportMessage::SetMarks,
            ));
    }
    col = col
        .push(toggle(
            i18n.tr("export-games-only-tactics"),
            draft.only_tactics,
            draft.scope != ExportScope::Current,
            GamesExportMessage::SetOnlyTactics,
        ))
        .push(toggle(
            i18n.tr("export-games-append"),
            draft.append && format.can_append(),
            format.can_append(),
            GamesExportMessage::SetAppend,
        ))
        .push(format_options(docs, i18n));
    scrollable(col)
        .id("export-options-scroll")
        .height(Length::Shrink)
        .into()
}

/// The percentage of the games handled, in 0..=100, and as a fraction.
fn fraction(progress: (u64, u64)) -> (f32, i64) {
    let (done, total) = progress;
    if total == 0 {
        return (0.0, 0);
    }
    // A ratio to draw: the precision of an `f32` is plenty.
    #[allow(clippy::cast_precision_loss)]
    let ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
    // Between 0 and 100 after the clamp.
    #[allow(clippy::cast_possible_truncation)]
    let percent = (ratio * 100.0).round() as i64;
    (ratio, percent)
}

fn big(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The lines of the report (D2004).
pub fn summary_lines(result: &GamesExportResult, i18n: &I18n) -> Vec<String> {
    let report = &result.report;
    let mut lines = Vec::new();
    if let Some(error) = &result.error {
        let mut args = FluentArgs::new();
        args.set("reason", i18n.tr(error.message_key()));
        lines.push(i18n.tr_args("export-games-error", &args));
        return lines;
    }
    let mut args = FluentArgs::new();
    args.set("written", big(report.written));
    args.set("excluded", big(report.excluded));
    args.set("skipped", big(report.skipped));
    lines.push(i18n.tr_args("export-games-summary", &args));
    if report.appended {
        lines.push(i18n.tr("export-games-appended"));
    }
    // The extra figures of the formats of the spec M7-4.
    for (key, count) in [
        ("export-report-repeated", report.repeated),
        ("export-report-not-selected", report.not_selected),
        ("export-report-volumes", u64::from(report.volumes)),
    ] {
        if count > 0 {
            let mut args = FluentArgs::new();
            args.set("count", big(count));
            lines.push(i18n.tr_args(key, &args));
        }
    }
    if let Some(problem) = report.problem {
        lines.push(i18n.tr(problem.message_key()));
    }
    lines
}

/// The text of one skipped game of the report.
pub fn skip_line(i18n: &I18n, number: u32, cause: mychess_app::ExportSkipCause) -> String {
    let mut args = FluentArgs::new();
    args.set("game", i64::from(number));
    args.set("text", i18n.tr(cause.message_key()));
    i18n.tr_args("export-games-line", &args)
}

/// How many file names the overwrite question lists before "and N more".
const CONFIRM_NAMES_SHOWN: usize = 5;

/// The question about a file that exists (spec M7-4, D3750): replace it, or
/// add the games to it.
fn confirm_box<'a>(
    i18n: &I18n,
    kind: ConfirmKind,
    files: &[std::path::PathBuf],
) -> Element<'a, Message> {
    // Up to 5000 files may be involved: name the first few and count the rest.
    let mut names = files
        .iter()
        .take(CONFIRM_NAMES_SHOWN)
        .map(|f| name_of(f))
        .collect::<Vec<_>>()
        .join(", ");
    if files.len() > CONFIRM_NAMES_SHOWN {
        let mut more = FluentArgs::new();
        more.set(
            "count",
            i64::try_from(files.len() - CONFIRM_NAMES_SHOWN).unwrap_or(0),
        );
        names.push(' ');
        names.push_str(&i18n.tr_args("export-confirm-and-more", &more));
    }
    let mut args = FluentArgs::new();
    args.set("files", names);
    args.set("count", i64::try_from(files.len()).unwrap_or(0));
    let (key, ok_key) = match kind {
        ConfirmKind::Replace => ("export-confirm-replace", "export-ui-replace"),
        ConfirmKind::Append => ("export-confirm-append", "export-ui-add"),
    };
    modal(Card {
        id: EXPORT_CONFIRM_BOX_ID,
        title: i18n.tr("export-ui-confirm-title"),
        body: text(i18n.tr_args(key, &args)).size(13).into(),
        buttons: vec![
            container(
                button(text(i18n.tr("export-games-cancel")))
                    .style(button::secondary)
                    .on_press(Message::App(AppMessage::ExportDocs(
                        ExportDocsMessage::Confirm(false),
                    ))),
            )
            .id("export-confirm-no")
            .into(),
            container(button(text(i18n.tr(ok_key))).on_press(Message::App(
                AppMessage::ExportDocs(ExportDocsMessage::Confirm(true)),
            )))
            .id("export-confirm-yes")
            .into(),
        ],
        width: 520.0,
    })
}

fn export_box<'a>(
    view: &GamesExportView<'_>,
    docs: &ExportDocsView<'_>,
    i18n: &I18n,
) -> Element<'a, Message> {
    if let (GamesExportStepView::Options, Some(confirm)) = (&view.step, &docs.confirm) {
        return confirm_box(i18n, confirm.kind, confirm.files);
    }
    let mut buttons: Vec<Element<'a, Message>> = Vec::new();
    let body: Element<'a, Message> = match &view.step {
        GamesExportStepView::Options => {
            buttons.push(
                button(text(i18n.tr("export-games-cancel")))
                    .style(button::secondary)
                    .on_press(send(GamesExportMessage::Close))
                    .into(),
            );
            buttons.push(
                button(text(i18n.tr("export-games-choose")))
                    .on_press_maybe(
                        view.counts
                            .available(view.draft.scope)
                            .then(|| send(GamesExportMessage::Choose)),
                    )
                    .into(),
            );
            options_body(view, docs, i18n)
        }
        GamesExportStepView::Picking => {
            buttons.push(
                button(text(i18n.tr("export-games-cancel")))
                    .style(button::secondary)
                    .on_press(send(GamesExportMessage::Close))
                    .into(),
            );
            text(i18n.tr("export-games-picking")).size(13).into()
        }
        GamesExportStepView::Running { path, progress } => {
            buttons.push(
                button(text(i18n.tr("export-games-stop")))
                    .style(button::secondary)
                    .on_press(Message::App(stop_message_app()))
                    .into(),
            );
            let (ratio, percent) = fraction(*progress);
            let mut name = FluentArgs::new();
            name.set("name", name_of(path));
            let mut args = FluentArgs::new();
            args.set("done", big(progress.0));
            args.set("total", big(progress.1));
            args.set("percent", percent);
            column![
                text(i18n.tr_args("export-games-destination", &name)).size(13),
                text(i18n.tr_args("export-games-progress", &args)).size(12),
                progress_bar(0.0..=1.0, ratio)
                    .length(Length::Fill)
                    .girth(Length::Fixed(8.0)),
            ]
            .spacing(8)
            .into()
        }
        GamesExportStepView::Done(result) => {
            buttons.push(
                button(text(i18n.tr("export-games-close")))
                    .on_press(send(GamesExportMessage::Close))
                    .into(),
            );
            let mut name = FluentArgs::new();
            name.set("name", name_of(&result.path));
            let mut body =
                column![text(i18n.tr_args("export-games-destination", &name)).size(13)].spacing(8);
            for line in summary_lines(result, i18n) {
                body = body.push(text(line).size(13));
            }
            let mut remarks = column![].spacing(2);
            for (number, cause) in &result.report.lines {
                remarks = remarks.push(text(skip_line(i18n, *number, *cause)).size(12));
            }
            let listed = u64::try_from(result.report.lines.len()).unwrap_or(0);
            if result.report.skipped > listed {
                let mut more = FluentArgs::new();
                more.set("count", big(result.report.skipped - listed));
                remarks = remarks.push(
                    text(i18n.tr_args("export-games-more", &more))
                        .size(12)
                        .style(theme::muted_text),
                );
            }
            if result.report.skipped > 0 {
                body = body.push(scrollable(remarks).height(Length::Fixed(160.0)));
            }
            body.into()
        }
    };
    modal(Card {
        id: EXPORT_GAMES_BOX_ID,
        title: i18n.tr("export-games-title"),
        body,
        buttons,
        width: 520.0,
    })
}

/// The text of the remark of the status bar.
pub fn notice_text(i18n: &I18n, notice: &GamesExportNotice) -> String {
    let mut args = FluentArgs::new();
    match notice {
        GamesExportNotice::Exported { count, name } => {
            args.set("count", big(*count));
            args.set("name", name.clone());
        }
        GamesExportNotice::ExportedWithSkips {
            count,
            skipped,
            name,
        } => {
            args.set("count", big(*count));
            args.set("skipped", big(*skipped));
            args.set("name", name.clone());
        }
        GamesExportNotice::StoppedAppended { count } => args.set("count", big(*count)),
        GamesExportNotice::AllSkipped { skipped } => args.set("skipped", big(*skipped)),
        GamesExportNotice::ViewerExported {
            volumes,
            count,
            name,
        } => {
            args.set("volumes", i64::from(*volumes));
            args.set("count", big(*count));
            args.set("name", name.clone());
        }
        GamesExportNotice::EpdExported {
            count,
            repeated,
            name,
        } => {
            args.set("count", big(*count));
            args.set("repeated", big(*repeated));
            args.set("name", name.clone());
        }
        GamesExportNotice::ImageSaved { name } => args.set("name", name.clone()),
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}
