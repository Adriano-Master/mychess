//! The body of the maintenance window: the header of the database, the
//! flags and the grid of operations (spec M7-1, section 1).

use fluent_bundle::FluentArgs;
use iced::widget::{column, container, pick_list, row, scrollable, text};
use iced::{Alignment, Element, Length};
use mychess_app::maintenance::{
    FlagAction, FlagScope, MaintFlag, MaintRequest, MaintenanceMessage, MaintenanceView,
    StripChoice,
};
use mychess_app::{Message as AppMessage, NameKind};

use super::text::{MARK_FLAGS, base_type_label, flag_label};
use super::{MaintForm, MaintUiMessage, ui};
use crate::shell::{Message, Shell};
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::windows::panel_kit::{BODY_TEXT, Choice, action, bold, flow, muted, progress};

const LABEL_WIDTH: f32 = 170.0;

/// The title bar: the name of the file, "Update" and "Close".
pub(super) fn title_bar<'a>(shell: &Shell, view: &MaintenanceView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let name = if view.file_name.is_empty() {
        match view.kind {
            Some(_) => i18n.tr("maint-clipbase"),
            None => String::new(),
        }
    } else {
        view.file_name.clone()
    };
    let idle = !view.working() && view.confirm.is_none();
    let close = iced::widget::button(icon(Icon::Close, 15.0, IconTone::Text))
        .padding(5)
        .style(theme::command_button(ButtonLook::Secondary, false))
        .on_press(ui(MaintUiMessage::Close));
    row![
        icon(Icon::Database, 18.0, IconTone::Accent),
        text(i18n.tr("maint-title")).size(18).font(bold()),
        text(name).size(BODY_TEXT).style(text::secondary),
        iced::widget::space::horizontal(),
        action(
            i18n.tr("maint-ui-refresh"),
            (idle && view.kind.is_some())
                .then(|| ui(MaintUiMessage::App(MaintenanceMessage::Refresh))),
        ),
        container(close).id("maint-close"),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

/// The job in flight (a bar and "Stop"), and under it the last remark of
/// the application about the maintenance: the layer of the window covers the
/// status bar, so the notice is drawn here (review I1).
pub(super) fn job_bar<'a>(shell: &Shell, view: &MaintenanceView) -> Element<'a, Message> {
    let notice = match &shell.ui.status_notice {
        Some(mychess_app::AppNotice::Maintenance(notice)) => {
            super::text::notice_text(&shell.i18n, notice)
        }
        _ => String::new(),
    };
    column![
        job_line(shell, view),
        container(text(notice).size(12).style(theme::muted_text)).id("maint-notice")
    ]
    .spacing(4)
    .into()
}

fn job_line<'a>(shell: &Shell, view: &MaintenanceView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    if view.previewing {
        let (done, total) = shell.app.maintenance().preview_progress().unwrap_or((0, 0));
        return container(
            row![
                text(i18n.tr("maint-ui-working")).size(BODY_TEXT),
                container(progress(done, total)).width(Length::Fill),
                action(
                    i18n.tr("maint-ui-stop"),
                    Some(ui(MaintUiMessage::App(MaintenanceMessage::Stop)))
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .id("maint-progress")
        .into();
    }
    let job = shell.app.list_job().filter(|_| view.busy.is_some());
    if let Some((_, control)) = job {
        let (done, total) = control.progress();
        return container(
            row![
                text(i18n.tr("maint-ui-working")).size(BODY_TEXT),
                container(progress(done, total)).width(Length::Fill),
                action(
                    i18n.tr("maint-ui-stop"),
                    Some(ui(MaintUiMessage::App(MaintenanceMessage::Stop)))
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .id("maint-progress")
        .into();
    }
    if view.stats_busy {
        return container(
            row![
                text(i18n.tr("maint-ui-counting")).size(BODY_TEXT),
                container(progress(0, 0)).width(Length::Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .id("maint-counting")
        .into();
    }
    column![].into()
}

fn kv(label: String, value: Element<'_, Message>) -> Element<'_, Message> {
    row![
        text(label)
            .size(BODY_TEXT)
            .width(Length::Fixed(LABEL_WIDTH))
            .style(text::secondary),
        value
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn plain<'a>(value: String) -> Element<'a, Message> {
    text(value).size(BODY_TEXT).into()
}

fn counted(i18n: &crate::i18n::I18n, n: u32, percent: u32) -> String {
    let mut args = FluentArgs::new();
    args.set("count", i64::from(n));
    args.set("percent", i64::from(percent));
    i18n.tr_args("maint-count-percent", &args)
}

fn section(title: String, body: Element<'_, Message>) -> Element<'_, Message> {
    column![
        text(title).size(14).font(bold()),
        container(body).padding([0, 8])
    ]
    .spacing(6)
    .into()
}

/// The scrollable body.
pub(super) fn body<'a>(shell: &Shell, view: &MaintenanceView) -> Element<'a, Message> {
    let mut content = column![].spacing(14).padding([0, 6]);
    if view.kind.is_none() {
        content = content.push(muted(shell.i18n.tr("maint-ui-no-base")));
    } else {
        content = content.push(header(shell, view));
        content = content.push(flags(shell, view));
    }
    content = content.push(operations(shell, view));
    container(scrollable(content).width(Length::Fill).id("maint-body"))
        .id("maint-content")
        .height(Length::Fill)
        .into()
}

#[allow(clippy::too_many_lines)] // one row per fact of the header
fn header<'a>(shell: &Shell, view: &MaintenanceView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let idle = !view.working() && view.confirm.is_none();
    let edit = |key: &str, form: MaintForm, enabled: bool| {
        action(
            i18n.tr(key),
            (enabled && idle).then(|| ui(MaintUiMessage::OpenForm(form))),
        )
    };
    let mut rows = column![].spacing(6);
    rows = rows.push(kv(i18n.tr("maint-database"), plain(view.file_name.clone())));
    if let Some(h) = &view.header {
        rows = rows.push(kv(
            i18n.tr("maint-type"),
            row![
                plain(base_type_label(i18n, h.base_type)),
                edit("maint-change", MaintForm::BaseType, view.access.header)
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
        ));
    }
    rows = rows.push(kv(i18n.tr("maint-games"), plain(view.games.to_string())));
    let stats = view.stats.as_ref();
    let filter_percent = view
        .filtered
        .saturating_mul(100)
        .checked_div(view.games)
        .unwrap_or(0);
    rows = rows.push(kv(
        i18n.tr("maint-in-filter"),
        plain(counted(i18n, view.filtered, filter_percent)),
    ));
    if let Some(s) = stats {
        let deleted = MaintFlag::Delete
            .counter()
            .and_then(|i| s.flags.get(i).copied())
            .unwrap_or(0);
        rows = rows.push(kv(
            i18n.tr("maint-deleted"),
            plain(counted(i18n, deleted, s.percent(deleted))),
        ));
        let marked = view
            .header
            .as_ref()
            .map(|_| shell.ui.dialogs.maint.flag)
            .and_then(MaintFlag::counter)
            .and_then(|i| s.flags.get(i).copied())
            .unwrap_or(0);
        rows = rows.push(kv(
            i18n.tr("maint-marked"),
            plain(counted(i18n, marked, s.percent(marked))),
        ));
        let range = |r: Option<(u16, u16, u16)>| {
            r.map_or_else(
                || i18n.tr("maint-range-empty"),
                |(low, high, mean)| {
                    let mut args = FluentArgs::new();
                    args.set("low", i64::from(low));
                    args.set("high", i64::from(high));
                    args.set("mean", i64::from(mean));
                    i18n.tr_args("maint-range", &args)
                },
            )
        };
        rows = rows.push(kv(i18n.tr("maint-years"), plain(range(s.years))));
        rows = rows.push(kv(i18n.tr("maint-ratings"), plain(range(s.ratings))));
    }
    let state = if view.read_only {
        "maint-ui-state-ro"
    } else {
        "maint-ui-state-rw"
    };
    let toggle = view.kind.is_some_and(|k| k != mychess_app::BaseKind::Pgn);
    rows = rows.push(kv(
        i18n.tr("maint-ui-state"),
        row![
            plain(i18n.tr(state)),
            action(
                i18n.tr("maint-change"),
                (toggle && idle).then(|| ui(MaintUiMessage::SetReadOnly(!view.read_only))),
            )
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
    ));
    if let Some(h) = &view.header {
        rows = rows.push(kv(
            i18n.tr("maint-description"),
            row![
                plain(h.description.clone()),
                edit("maint-ui-edit", MaintForm::Description, view.access.header)
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
        ));
        let labels = h
            .custom_flags
            .iter()
            .enumerate()
            .map(|(i, d)| {
                if d.is_empty() {
                    (i + 1).to_string()
                } else {
                    d.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("  ");
        rows = rows.push(kv(
            i18n.tr("maint-custom-flags"),
            row![
                plain(labels),
                edit("maint-ui-edit", MaintForm::CustomFlags, view.access.header)
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
        ));
    }
    section(i18n.tr("maint-ui-info"), rows.into())
}

fn scoped_buttons<'a>(
    shell: &Shell,
    enabled: bool,
    flag: MaintFlag,
    action_kind: FlagAction,
    keys: [&str; 3],
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut line = row![].spacing(6);
    for (scope, key) in [FlagScope::Current, FlagScope::Filter, FlagScope::All]
        .into_iter()
        .zip(keys)
    {
        line = line.push(action(
            i18n.tr(key),
            enabled.then(|| {
                ui(MaintUiMessage::Run(MaintRequest::Flags {
                    scope,
                    flag,
                    action: action_kind,
                }))
            }),
        ));
    }
    flow(line)
}

fn flags<'a>(shell: &Shell, view: &MaintenanceView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let enabled = view.access.flags && !view.working() && view.confirm.is_none();
    let custom = view
        .header
        .as_ref()
        .map(|h| h.custom_flags.clone())
        .unwrap_or_default();
    let options: Vec<Choice<MaintFlag>> = MARK_FLAGS
        .into_iter()
        .map(|value| Choice {
            value,
            label: flag_label(i18n, value, &custom),
        })
        .collect();
    let current = shell.ui.dialogs.maint.flag;
    let selected = Choice {
        value: current,
        label: flag_label(i18n, current, &custom),
    };
    let picker = pick_list(options, Some(selected), |c: Choice<MaintFlag>| {
        ui(MaintUiMessage::SelectFlag(c.value))
    })
    .text_size(BODY_TEXT)
    .padding(3);
    let body = column![
        scoped_buttons(
            shell,
            enabled,
            MaintFlag::Delete,
            FlagAction::Set,
            [
                "maint-delete-current",
                "maint-delete-filter",
                "maint-delete-all"
            ],
        ),
        scoped_buttons(
            shell,
            enabled,
            MaintFlag::Delete,
            FlagAction::Clear,
            [
                "maint-undelete-current",
                "maint-undelete-filter",
                "maint-undelete-all"
            ],
        ),
        row![text(i18n.tr("maint-flags")).size(BODY_TEXT), picker]
            .spacing(8)
            .align_y(Alignment::Center),
        scoped_buttons(
            shell,
            enabled,
            current,
            FlagAction::Set,
            ["maint-mark-current", "maint-mark-filter", "maint-mark-all"],
        ),
        scoped_buttons(
            shell,
            enabled,
            current,
            FlagAction::Clear,
            [
                "maint-unmark-current",
                "maint-unmark-filter",
                "maint-unmark-all"
            ],
        ),
    ]
    .spacing(8);
    section(i18n.tr("maint-flags"), body.into())
}

fn operations<'a>(shell: &Shell, view: &MaintenanceView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let idle = !view.working() && view.confirm.is_none();
    let access = view.access;
    let has_base = view.kind.is_some();
    let form = |key: &str, form: MaintForm, enabled: bool| {
        action(
            i18n.tr(key),
            (enabled && idle).then(|| ui(MaintUiMessage::OpenForm(form))),
        )
    };
    let tool = |key: &str, message: AppMessage, enabled: bool| {
        action(
            i18n.tr(key),
            (enabled && idle).then(|| ui(MaintUiMessage::Tool(Box::new(message)))),
        )
    };
    let spell = |kind: NameKind, key: &str| {
        tool(
            key,
            AppMessage::Spelling(mychess_app::SpellingMessage::OpenCheck(kind)),
            access.names,
        )
    };
    let spelling = row![
        spell(NameKind::Player, "maint-spell-players"),
        spell(NameKind::Event, "maint-spell-events"),
        spell(NameKind::Site, "maint-spell-sites"),
        spell(NameKind::Round, "maint-spell-rounds"),
    ]
    .spacing(6);
    let first = row![
        form("maint-compact", MaintForm::Compact, access.compact),
        form("maint-sort", MaintForm::Sort, access.sort),
        form("maint-names-editor", MaintForm::Names, access.names),
        tool(
            "maint-eco",
            AppMessage::Eco(mychess_app::EcoMessage::OpenClassify),
            has_base,
        ),
        tool(
            "maint-add-elo",
            AppMessage::Spelling(mychess_app::SpellingMessage::OpenAddElo),
            access.names,
        ),
        form("maint-duplicates", MaintForm::Twins, access.twins),
    ]
    .spacing(6);
    let second = row![
        form("maint-autoload", MaintForm::Autoload, access.header),
        form("maint-check", MaintForm::Check, access.check),
        form("maint-cleaner", MaintForm::Cleaner, access.cleaner),
        action(
            i18n.tr("maint-strip-comments"),
            (access.strip && idle).then(|| ui(MaintUiMessage::OpenStrip(StripChoice::Comments))),
        ),
        action(
            i18n.tr("maint-strip-variations"),
            (access.strip && idle).then(|| ui(MaintUiMessage::OpenStrip(StripChoice::Variations))),
        ),
        form("maint-tags", MaintForm::Tags, access.tag_list),
    ]
    .spacing(6);
    let third = row![
        action(
            i18n.tr("maint-drop-deleted"),
            (access.filter_deleted && idle)
                .then(|| ui(MaintUiMessage::Run(MaintRequest::DropDeletedFromFilter))),
        ),
        action(
            i18n.tr("maint-open-checker"),
            (view.twin_pairs > 0 && idle)
                .then(|| ui(MaintUiMessage::App(MaintenanceMessage::OpenChecker))),
        ),
        form("maint-repair", MaintForm::Repair, access.repair),
    ]
    .spacing(6);
    let body = column![
        section(i18n.tr("maint-spell"), flow(spelling)),
        section(
            i18n.tr("maint-operations"),
            column![flow(first), flow(second), flow(third)]
                .spacing(6)
                .into()
        ),
    ]
    .spacing(14);
    body.into()
}
