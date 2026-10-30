//! The first-run welcome and the tip of the day (spec M9-4, D4041-D4046).
//!
//! Two layers of the main window, like the About box: the application keeps
//! what they show ([`mychess_app::start`]); this module draws them. The
//! welcome offers the language, says where the files of the user are and
//! which engine was found in `Engine/`; the tip steps through the tips of
//! `assets/i18n/*/start.ftl`.

use fluent_bundle::FluentArgs;
use iced::widget::{button, checkbox, column, row, text};
use iced::{Element, Length};
use mychess_app::start::{
    FolderProblem, LayoutMode, StartMessage, StartNotice, TIP_COUNT, WelcomeState, tip_key,
};
use mychess_app::{AppState, Message as AppMessage};

use crate::app_api;
use crate::i18n::{I18n, LanguagePref};
use crate::shell::{Message, Shell};
use crate::widgets::modal::{Card, modal};

/// Id of the welcome card.
pub const WELCOME_ID: &str = "welcome-box";
/// Id of the tip card.
pub const TIP_ID: &str = "tip-box";

fn start(message: StartMessage) -> Message {
    Message::App(AppMessage::Start(message))
}

/// The layer to show: the welcome, else the tip.
pub fn layer(shell: &Shell) -> Option<Element<'_, Message>> {
    let state = shell.app.start();
    if let Some(welcome) = state.welcome() {
        return Some(welcome_card(&shell.app, welcome, &shell.i18n));
    }
    state
        .tip()
        .map(|index| tip_card(&shell.app, index, &shell.i18n))
}

/// The folder as shown to the user.
fn folder_label(path: &std::path::Path) -> String {
    path.display().to_string()
}

/// The name of an engine found in `Engine/`: its file name, no extension.
fn engine_label(path: &std::path::Path) -> String {
    path.file_stem()
        .map_or_else(|| folder_label(path), |s| s.to_string_lossy().into_owned())
}

/// The lines of the welcome that depend on the folder: where it is and
/// what happened to it.
fn folder_lines<'a>(welcome: &WelcomeState, i18n: &I18n) -> Vec<Element<'a, Message>> {
    let mut lines: Vec<Element<'a, Message>> = Vec::new();
    let Some(root) = welcome.root.as_deref() else {
        lines.push(text(i18n.tr("start-welcome-no-folder")).into());
        return lines;
    };
    let mut args = FluentArgs::new();
    args.set("path", folder_label(root));
    lines.push(text(i18n.tr_args("start-welcome-folder", &args)).into());
    lines.push(text(i18n.tr("start-welcome-subfolders")).into());
    match &welcome.report {
        None => lines.push(text(i18n.tr("start-welcome-preparing")).into()),
        Some(report) => {
            for problem in &report.problems {
                let mut args = FluentArgs::new();
                args.set("path", folder_label(problem.path()));
                let key = match problem {
                    FolderProblem::IsFile(_) => "start-welcome-problem-file",
                    FolderProblem::CreateFailed { .. } => "start-welcome-problem-create",
                };
                lines.push(text(i18n.tr_args(key, &args)).into());
            }
            match report.engines.first() {
                Some(engine) if report.problems.is_empty() => {
                    let mut args = FluentArgs::new();
                    args.set("name", engine_label(&engine.path));
                    lines.push(text(i18n.tr_args("start-welcome-engine-found", &args)).into());
                }
                None if report.problems.is_empty() => {
                    lines.push(text(i18n.tr("start-welcome-engine-none")).into());
                }
                _ => {}
            }
        }
    }
    if welcome.mode == LayoutMode::Portable {
        lines.push(text(i18n.tr("start-welcome-portable")).into());
    }
    lines
}

fn language_choice<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let current = app_api::language_pref(app);
    let choice = |label: String, pref: LanguagePref| {
        let b = button(text(label)).on_press(Message::App(app_api::set_language(pref)));
        if pref == current {
            b.style(button::primary)
        } else {
            b.style(button::secondary)
        }
    };
    row![
        text(i18n.tr("start-welcome-language")),
        choice(i18n.tr("menu-view-language-system"), LanguagePref::System),
        choice(i18n.tr("menu-view-language-en-us"), LanguagePref::EnUs),
        choice(i18n.tr("menu-view-language-pt-br"), LanguagePref::PtBr),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}

fn show_tip_box<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    checkbox(app.prefs.start.show_tip)
        .label(i18n.tr("start-show-tip"))
        .on_toggle(|on| start(StartMessage::SetShowTip(on)))
        .into()
}

fn welcome_card<'a>(app: &AppState, welcome: &WelcomeState, i18n: &I18n) -> Element<'a, Message> {
    let mut body = column![
        text(i18n.tr("start-welcome-intro")),
        language_choice(app, i18n)
    ]
    .spacing(10);
    for line in folder_lines(welcome, i18n) {
        body = body.push(line);
    }
    body = body.push(show_tip_box(app, i18n));
    modal(Card {
        id: WELCOME_ID,
        title: i18n.tr("start-welcome-title"),
        body: body.width(Length::Fill).into(),
        buttons: vec![
            button(text(i18n.tr("start-welcome-go")))
                .on_press(start(StartMessage::CloseWelcome))
                .style(button::primary)
                .into(),
        ],
        width: 560.0,
    })
}

fn tip_card<'a>(app: &AppState, index: u32, i18n: &I18n) -> Element<'a, Message> {
    let mut args = FluentArgs::new();
    args.set("number", i64::from(index) + 1);
    args.set("total", i64::from(TIP_COUNT));
    let body = column![
        text(i18n.tr_args("start-tip-number", &args)).size(14),
        text(i18n.tr(tip_key(index))),
        show_tip_box(app, i18n),
    ]
    .spacing(10)
    .width(Length::Fill);
    modal(Card {
        id: TIP_ID,
        title: i18n.tr("start-tip-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("start-tip-previous")))
                .on_press(start(StartMessage::PreviousTip))
                .style(button::secondary)
                .into(),
            button(text(i18n.tr("start-tip-next")))
                .on_press(start(StartMessage::NextTip))
                .style(button::secondary)
                .into(),
            button(text(i18n.tr("start-tip-close")))
                .on_press(start(StartMessage::CloseTip))
                .style(button::primary)
                .into(),
        ],
        width: 520.0,
    })
}

/// The text of a remark of the start-up for the status bar.
pub fn notice_text(i18n: &I18n, notice: &StartNotice) -> String {
    let mut args = FluentArgs::new();
    if let Some(path) = notice.path() {
        args.set("path", folder_label(path));
    }
    i18n.tr_args(notice.message_key(), &args)
}
