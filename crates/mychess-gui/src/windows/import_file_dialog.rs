//! The "Importing PGN files" window (spec M3-8, D1983-D1985): the progress
//! while the job runs and the report when it ends. The application keeps
//! the state (`AppState::file_import_view`); the window draws it, sends
//! [`ListMessage::CancelJob`] to stop and [`FileImportMessage::Close`] to
//! leave.

use std::path::Path;

use fluent_bundle::FluentArgs;
use iced::widget::{button, column, progress_bar, scrollable, text};
use iced::{Element, Length};
use mychess_app::{
    AppState, FileImportMessage, FileImportResult, FileImportView, ImportLine, ListMessage,
    Message as AppMessage,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};

/// Id of the box of the window.
pub const IMPORT_FILE_BOX_ID: &str = "import-file-box";

/// The files listed before "... and N more".
const FILES_SHOWN: usize = 5;

/// The message that stops the job.
pub fn stop_message() -> Message {
    Message::App(AppMessage::Bases(mychess_app::BasesMessage::List(
        ListMessage::CancelJob,
    )))
}

/// The message that closes the window.
pub fn close_message() -> Message {
    Message::App(AppMessage::FileImport(FileImportMessage::Close))
}

/// The box, when the application has the window open.
pub fn view<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let view = app.file_import_view()?;
    Some(import_box(&view, i18n))
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The text of a remark of the report.
pub fn line_text(i18n: &I18n, files: &[std::path::PathBuf], line: &ImportLine) -> String {
    let mut args = FluentArgs::new();
    args.set(
        "file",
        files
            .get(line.file)
            .map_or_else(String::new, |f| name_of(f)),
    );
    args.set("game", i64::try_from(line.game).unwrap_or(i64::MAX));
    args.set("line", i64::try_from(line.line).unwrap_or(i64::MAX));
    args.set("text", i18n.tr(line.key));
    let key = if line.line > 0 {
        "file-import-line"
    } else if line.game > 0 {
        "file-import-line-game"
    } else {
        "file-import-line-file"
    };
    i18n.tr_args(key, &args)
}

/// The percentage of the bytes read, in 0..=100, and as a fraction.
fn fraction(bytes: (u64, u64)) -> (f32, i64) {
    let (done, total) = bytes;
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

fn summary_lines(view: &FileImportView<'_>, result: &FileImportResult, i18n: &I18n) -> Vec<String> {
    let report = &result.report;
    let mut args = FluentArgs::new();
    args.set("read", i64::try_from(report.games_read).unwrap_or(i64::MAX));
    args.set("added", i64::from(report.games_added));
    args.set(
        "skipped",
        i64::try_from(report.games_skipped).unwrap_or(i64::MAX),
    );
    args.set(
        "warnings",
        i64::try_from(report.warnings).unwrap_or(i64::MAX),
    );
    let mut lines = vec![i18n.tr_args("file-import-summary", &args)];
    if report.interrupted {
        lines.push(i18n.tr("file-import-interrupted"));
    }
    if let Some(error) = &result.error {
        let mut args = FluentArgs::new();
        args.set("reason", i18n.tr(error.message_key()));
        lines.push(i18n.tr_args("file-import-error", &args));
    }
    for (index, error) in &report.file_errors {
        let mut args = FluentArgs::new();
        args.set(
            "file",
            view.files
                .get(*index)
                .map_or_else(String::new, |f| name_of(f)),
        );
        args.set("reason", i18n.tr(error.message_key()));
        lines.push(i18n.tr_args("file-import-file-error", &args));
    }
    if report.games_added == 0 && result.error.is_none() && !report.interrupted {
        lines.push(i18n.tr("file-import-nothing"));
    }
    if view.new_base && report.games_added == 0 {
        lines.push(i18n.tr("file-import-removed"));
    }
    if result.opened {
        lines.push(i18n.tr("file-import-opened"));
    }
    if result.kept {
        let mut args = FluentArgs::new();
        args.set("name", name_of(view.target));
        lines.push(i18n.tr_args("file-import-kept", &args));
    }
    lines
}

fn import_box<'a>(view: &FileImportView<'_>, i18n: &I18n) -> Element<'a, Message> {
    let mut target = FluentArgs::new();
    target.set("name", name_of(view.target));
    let key = if view.new_base {
        "file-import-target-new"
    } else {
        "file-import-target-existing"
    };
    let mut body = column![text(i18n.tr_args(key, &target)).size(13)].spacing(8);
    let mut count = FluentArgs::new();
    count.set("count", i64::try_from(view.files.len()).unwrap_or(i64::MAX));
    body = body.push(
        text(i18n.tr_args("file-import-files", &count))
            .size(12)
            .style(theme::muted_text),
    );
    for file in view.files.iter().take(FILES_SHOWN) {
        body = body.push(text(name_of(file)).size(12).style(theme::muted_text));
    }
    if view.files.len() > FILES_SHOWN {
        let mut more = FluentArgs::new();
        more.set(
            "count",
            i64::try_from(view.files.len() - FILES_SHOWN).unwrap_or(i64::MAX),
        );
        body = body.push(
            text(i18n.tr_args("file-import-files-more", &more))
                .size(12)
                .style(theme::muted_text),
        );
    }
    let buttons: Vec<Element<'a, Message>> = if let Some(result) = view.result {
        let mut report = column![].spacing(4);
        for line in summary_lines(view, result, i18n) {
            report = report.push(text(line).size(13));
        }
        let mut remarks = column![].spacing(2);
        for line in &result.report.lines {
            remarks = remarks.push(text(line_text(i18n, view.files, line)).size(12));
        }
        if result.report.lines_dropped > 0 {
            let mut more = FluentArgs::new();
            more.set(
                "count",
                i64::try_from(result.report.lines_dropped).unwrap_or(i64::MAX),
            );
            remarks = remarks.push(
                text(i18n.tr_args("file-import-more", &more))
                    .size(12)
                    .style(theme::muted_text),
            );
        }
        body = body.push(report);
        if !result.report.lines.is_empty() {
            body = body.push(scrollable(remarks).height(Length::Fixed(180.0)));
        }
        vec![
            button(text(i18n.tr("file-import-close")))
                .on_press(close_message())
                .into(),
        ]
    } else {
        let (ratio, percent) = fraction(view.bytes);
        let mut args = FluentArgs::new();
        args.set(
            "file",
            i64::try_from(view.file + 1)
                .unwrap_or(i64::MAX)
                .min(i64::try_from(view.files.len()).unwrap_or(i64::MAX)),
        );
        args.set("total", i64::try_from(view.files.len()).unwrap_or(i64::MAX));
        args.set("games", i64::try_from(view.games).unwrap_or(i64::MAX));
        args.set("percent", percent);
        body = body
            .push(text(i18n.tr_args("file-import-progress", &args)).size(12))
            .push(
                progress_bar(0.0..=1.0, ratio)
                    .length(Length::Fill)
                    .girth(Length::Fixed(8.0)),
            );
        vec![
            button(text(i18n.tr("file-import-stop")))
                .style(button::secondary)
                .on_press(stop_message())
                .into(),
        ]
    };
    modal(Card {
        id: IMPORT_FILE_BOX_ID,
        title: i18n.tr("file-import-title"),
        body: body.into(),
        buttons,
        width: 560.0,
    })
}

/// [`stop_message`] as a message of the application.
pub fn stop_message_app() -> AppMessage {
    AppMessage::Bases(mychess_app::BasesMessage::List(ListMessage::CancelJob))
}

/// [`close_message`] as a message of the application.
pub fn close_message_app() -> AppMessage {
    AppMessage::FileImport(FileImportMessage::Close)
}

/// The text of the remark of the status bar.
pub fn notice_text(i18n: &I18n, notice: &mychess_app::FileImportNotice) -> String {
    use mychess_app::FileImportNotice as N;
    let mut args = FluentArgs::new();
    match notice {
        N::Imported { count } | N::Interrupted { count } => args.set("count", *count),
        N::Failed { count, error } => {
            args.set("count", *count);
            args.set("reason", i18n.tr(error.message_key()));
        }
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}
