//! The cache bar of the opening tree panel (spec M5-3b, D1760-D1789): the
//! "Cache" menu (save, fill, information), the size, the auto-save option,
//! the progress of the job that runs and the line with its last result.
//!
//! The application decides everything ([`CacheView`]); this module lays it
//! out and formats the numbers with the Fluent texts.

use fluent_bundle::FluentArgs;
use iced::widget::{button, checkbox, pick_list, row, text, tooltip};
use iced::{Alignment, Element};
use mychess_app::{
    CACHE_SIZES, CacheBusy, CacheFill, CacheInfo, CacheIssue, CacheJobKind, CacheStatus, CacheView,
    Message as AppMessage, TreeMessage,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};

use super::super::header_dialog::Labeled;

/// What the "Cache" menu offers, in menu order (Scid: the `File` menu of the
/// tree window: `TreeFileSave`, `TreeFileFill`, `TreeFileInfo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheAction {
    Save,
    Fill(CacheFill),
    Info,
}

impl CacheAction {
    /// Every entry of the menu.
    pub(crate) const ALL: [CacheAction; 5] = [
        CacheAction::Save,
        CacheAction::Fill(CacheFill::Base),
        CacheAction::Fill(CacheFill::Game),
        CacheAction::Fill(CacheFill::StandardLines),
        CacheAction::Info,
    ];

    /// The Fluent key of the entry.
    pub(crate) fn key(self) -> &'static str {
        match self {
            CacheAction::Save => "tree-cache-save",
            CacheAction::Fill(CacheFill::Base) => "tree-cache-fill-base",
            CacheAction::Fill(CacheFill::Game) => "tree-cache-fill-game",
            CacheAction::Fill(CacheFill::StandardLines) => "tree-cache-fill-lines",
            CacheAction::Info => "tree-cache-info",
        }
    }

    /// The message the entry sends.
    pub(crate) fn message(self) -> TreeMessage {
        match self {
            CacheAction::Save => TreeMessage::SaveCache,
            CacheAction::Fill(fill) => TreeMessage::FillCache(fill),
            CacheAction::Info => TreeMessage::CacheInfo,
        }
    }
}

fn tree_message(message: TreeMessage) -> Message {
    Message::App(AppMessage::Tree(message))
}

fn count(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The text of the job that runs.
pub(crate) fn busy_text(i18n: &I18n, busy: CacheBusy) -> String {
    match busy.kind {
        CacheJobKind::Save => i18n.tr("tree-cache-busy-save"),
        CacheJobKind::Info => i18n.tr("tree-cache-busy-info"),
        CacheJobKind::Fill(_) if busy.total == 0 => i18n.tr("tree-cache-busy-fill-unknown"),
        CacheJobKind::Fill(_) => {
            let mut args = FluentArgs::new();
            args.set("done", count(busy.done));
            args.set("total", count(busy.total));
            i18n.tr_args("tree-cache-busy-fill", &args)
        }
    }
}

fn info_text(i18n: &I18n, info: CacheInfo) -> String {
    let mut args = FluentArgs::new();
    args.set("used", count(info.used as u64));
    args.set("capacity", count(info.capacity as u64));
    args.set("backup", count(info.backup_used as u64));
    args.set("backup_capacity", count(info.backup_capacity as u64));
    let mut out = i18n.tr_args("tree-cache-info-text", &args);
    if let Some(games) = info.lowest_total {
        let mut args = FluentArgs::new();
        args.set("games", count(u64::from(games)));
        out.push(' ');
        out.push_str(&i18n.tr_args("tree-cache-info-lowest", &args));
    }
    out.push(' ');
    match info.file_bytes {
        Some(bytes) => {
            let mut args = FluentArgs::new();
            args.set("bytes", bytes.to_string());
            out.push_str(&i18n.tr_args("tree-cache-info-file", &args));
        }
        None => out.push_str(&i18n.tr("tree-cache-info-no-file")),
    }
    out
}

fn issue_text(i18n: &I18n, issue: &CacheIssue) -> String {
    let with_reason = |key: &str, reason: &str| {
        let mut args = FluentArgs::new();
        args.set("reason", reason.to_owned());
        i18n.tr_args(key, &args)
    };
    match issue {
        CacheIssue::NotScidBase => i18n.tr("tree-cache-failed-not-scid"),
        CacheIssue::Busy => i18n.tr("tree-cache-failed-busy"),
        CacheIssue::Discarding => i18n.tr("tree-cache-failed-discarding"),
        CacheIssue::NoBase => i18n.tr("tree-cache-failed-no-base"),
        CacheIssue::Stale => i18n.tr("tree-cache-failed-stale"),
        CacheIssue::Cancelled => i18n.tr("tree-cache-failed-cancelled"),
        CacheIssue::IllegalLine => i18n.tr("tree-cache-failed-illegal-line"),
        CacheIssue::File(reason) => with_reason("tree-cache-failed-file", reason),
        CacheIssue::Search(reason) => with_reason("tree-cache-failed-search", reason),
        _ => i18n.tr("tree-cache-failed-internal"),
    }
}

/// The line about the last result, if there is one.
pub(crate) fn status_text(i18n: &I18n, status: &CacheStatus) -> Option<String> {
    match status {
        CacheStatus::Saved { bytes, skipped } => {
            let mut args = FluentArgs::new();
            args.set("bytes", bytes.to_string());
            if *skipped == 0 {
                Some(i18n.tr_args("tree-cache-saved", &args))
            } else {
                args.set("skipped", count(*skipped as u64));
                Some(i18n.tr_args("tree-cache-saved-skipped", &args))
            }
        }
        CacheStatus::Filled { searches, hits } => {
            let mut args = FluentArgs::new();
            args.set("searches", count(u64::from(*searches)));
            args.set("hits", count(u64::from(*hits)));
            Some(i18n.tr_args("tree-cache-filled", &args))
        }
        CacheStatus::Info(info) => Some(info_text(i18n, *info)),
        CacheStatus::Failed(issue) => Some(issue_text(i18n, issue)),
        // `None` and the variants a later version adds.
        _ => None,
    }
}

fn small_button<'a>(label: String, message: TreeMessage) -> Element<'a, Message> {
    button(text(label).size(12))
        .padding([3, 8])
        .style(theme::command_button(ButtonLook::Secondary, false))
        .on_press(tree_message(message))
        .into()
}

/// The cache bar: the menu, the size and the auto-save option, then the
/// progress or the last result.
pub(crate) fn bar<'a>(view: &CacheView, i18n: &I18n) -> Element<'a, Message> {
    let actions: Vec<Labeled<CacheAction>> = CacheAction::ALL
        .into_iter()
        .map(|value| Labeled {
            value,
            label: i18n.tr(value.key()),
        })
        .collect();
    let sizes: Vec<Labeled<usize>> = CACHE_SIZES
        .into_iter()
        .map(|value| Labeled {
            value,
            label: value.to_string(),
        })
        .collect();
    let selected_size = Labeled {
        value: view.size,
        label: view.size.to_string(),
    };
    let mut items: Vec<Element<'a, Message>> = vec![
        pick_list(
            actions,
            None::<Labeled<CacheAction>>,
            |choice: Labeled<CacheAction>| tree_message(choice.value.message()),
        )
        .placeholder(i18n.tr("tree-cache"))
        .text_size(12)
        .padding(3)
        .into(),
        text(i18n.tr("tree-cache-size"))
            .size(12)
            .style(theme::muted_text)
            .into(),
        pick_list(sizes, Some(selected_size), |choice: Labeled<usize>| {
            tree_message(TreeMessage::SetCacheSize(choice.value))
        })
        .text_size(12)
        .padding(3)
        .into(),
        tooltip(
            checkbox(view.autosave)
                .label(i18n.tr("tree-cache-autosave"))
                .size(14)
                .text_size(12)
                .on_toggle(|on| tree_message(TreeMessage::SetCacheAutosave(on))),
            iced::widget::container(text(i18n.tr("tree-cache-autosave-tip")).size(12))
                .padding([4, 8])
                .style(theme::tooltip_box),
            tooltip::Position::Bottom,
        )
        .into(),
    ];
    if let Some(busy) = view.busy {
        items.push(
            text(busy_text(i18n, busy))
                .size(12)
                .style(theme::muted_text)
                .into(),
        );
        if matches!(busy.kind, CacheJobKind::Fill(_)) {
            items.push(small_button(
                i18n.tr("tree-cache-cancel"),
                TreeMessage::CancelCacheJob,
            ));
        }
    } else if let Some(line) = status_text(i18n, &view.status) {
        items.push(text(line).size(12).style(theme::muted_text).into());
        items.push(
            tooltip(
                button(icon(Icon::Close, 12.0, IconTone::Text))
                    .padding(3)
                    .style(theme::command_button(ButtonLook::Secondary, false))
                    .on_press(tree_message(TreeMessage::DismissCacheStatus)),
                iced::widget::container(text(i18n.tr("tree-cache-dismiss")).size(12))
                    .padding([4, 8])
                    .style(theme::tooltip_box),
                tooltip::Position::Bottom,
            )
            .into(),
        );
    }
    row::Row::with_children(items)
        .spacing(10)
        .align_y(Alignment::Center)
        .wrap()
        .into()
}
