//! The cards of the operations of the maintenance window and the
//! confirmation (spec M7-1, sections 4 to 14).

use fluent_bundle::FluentArgs;
use iced::widget::{button, checkbox, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length};
use mychess_app::maintenance::data::{CleanerReport, StepStatus};
use mychess_app::maintenance::{
    CleanerPrefs, DeleteChoice, MAX_SORT_CRITERIA, MaintReport, MaintRequest, MaintenanceMessage,
    MaintenancePrefs, MaintenanceView, NameScopeChoice, ScopeChoice, SortCriterion, StripChoice,
    TagRequest, TwinPrefs,
};

use super::text::{
    base_type_label, cause_key, confirm_key, criterion_label, status_key, step_key, weak_key,
};
use super::{
    MAINT_AUTOLOAD_ID, MAINT_CHECKER_ID, MAINT_CONFIRM_ID, MAINT_CUSTOM_ID, MAINT_DESCRIPTION_ID,
    MAINT_FORM_ID, MAINT_NAME_NEW_ID, MAINT_NAME_OLD_ID, MAINT_TAG_NAME_ID, MAINT_TAG_VALUE_ID,
    MaintEdit, MaintForm, MaintUiMessage, NameUiKind, ScopeField, common_sort, ui,
};
use crate::shell::{Message, Shell};
use crate::widgets::modal::{Card, default_button, modal};
use crate::windows::panel_kit::{BODY_TEXT, action, bold, check, choice, danger, flow, muted};

/// How many items of a report the card lists (the rest is counted).
const REPORT_ITEMS: usize = 30;

const CARD_WIDTH: f32 = 620.0;

fn app(message: MaintenanceMessage) -> Message {
    ui(MaintUiMessage::App(message))
}

fn primary<'a>(label: String, message: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(BODY_TEXT))
        .padding([4, 14])
        .style(default_button)
        .on_press_maybe(message)
        .into()
}

fn close_button<'a>(shell: &Shell) -> Element<'a, Message> {
    action(
        shell.i18n.tr("maint-ui-close"),
        Some(ui(MaintUiMessage::CloseForm)),
    )
}

fn labelled(label: String, body: Element<'_, Message>) -> Element<'_, Message> {
    row![
        text(label)
            .size(BODY_TEXT)
            .width(Length::Fixed(170.0))
            .style(text::secondary),
        body
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn heading<'a>(label: String) -> Element<'a, Message> {
    text(label).size(BODY_TEXT + 1.0).font(bold()).into()
}

fn field<'a>(
    id: &'static str,
    value: &str,
    on_input: impl Fn(String) -> MaintEdit + 'a,
    submit: Option<Message>,
) -> Element<'a, Message> {
    let mut input = text_input("", value)
        .id(id)
        .size(BODY_TEXT)
        .padding(4)
        .on_input(move |text| ui(MaintUiMessage::Edit(on_input(text))));
    if let Some(message) = submit {
        input = input.on_submit(message);
    }
    input.into()
}

fn prefs_choice<'a>(
    label: String,
    active: bool,
    prefs: &MaintenancePrefs,
    set: impl FnOnce(&mut MaintenancePrefs),
) -> Element<'a, Message> {
    let mut next = prefs.clone();
    set(&mut next);
    choice(
        label,
        active,
        Some(ui(MaintUiMessage::Prefs(Box::new(next)))),
    )
}

fn card<'a>(
    shell: &Shell,
    title_key: &str,
    body: Element<'a, Message>,
    mut buttons: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    buttons.insert(0, close_button(shell));
    modal(Card {
        id: MAINT_FORM_ID,
        title: shell.i18n.tr(title_key),
        body: scrollable(container(body).padding([0, 12]).width(Length::Fill))
            .height(Length::Shrink)
            .into(),
        buttons,
        width: CARD_WIDTH,
    })
}

/// The card of the open form, if one is.
pub(super) fn overlay<'a>(shell: &Shell, view: &MaintenanceView) -> Option<Element<'a, Message>> {
    let form = shell.ui.dialogs.maint.form?;
    let busy = view.working();
    Some(match form {
        MaintForm::Compact => compact(shell, view, busy),
        MaintForm::Sort => sort(shell, view, busy),
        MaintForm::Twins => twins(shell, view, busy),
        MaintForm::Cleaner => cleaner(shell, view, busy),
        MaintForm::Check => check_form(shell, busy),
        MaintForm::Strip => strip(shell, view, busy),
        MaintForm::Tags => tags(shell, view, busy),
        MaintForm::Names => names(shell, view, busy),
        MaintForm::Autoload => autoload(shell, view, busy),
        MaintForm::BaseType => base_type(shell, view, busy),
        MaintForm::Description => description(shell, busy),
        MaintForm::CustomFlags => custom_flags(shell, busy),
        MaintForm::Repair => repair(shell, busy),
    })
}

/// What `Enter` does in the open form.
pub(super) fn default_message(shell: &Shell, view: &MaintenanceView) -> Option<Message> {
    let ui_state = &shell.ui.dialogs.maint;
    if view.working() {
        return None;
    }
    let run_close =
        |request: Option<MaintRequest>| request.map(|r| ui(MaintUiMessage::RunClose(r)));
    match ui_state.form? {
        MaintForm::Compact | MaintForm::BaseType | MaintForm::CustomFlags => None,
        MaintForm::Sort => run_close(ui_state.sort_request()),
        MaintForm::Twins => run_close(Some(MaintRequest::FindTwins)),
        MaintForm::Cleaner => Some(ui(MaintUiMessage::Run(MaintRequest::Cleaner))),
        MaintForm::Check => Some(ui(MaintUiMessage::Run(MaintRequest::Check {
            scope: ScopeChoice::All,
        }))),
        MaintForm::Strip => run_close(Some(MaintRequest::Strip {
            what: ui_state.strip,
            scope: view.prefs.strip_scope,
        })),
        MaintForm::Tags => ui_state
            .tag_add_request()
            .map(|r| ui(MaintUiMessage::Run(r))),
        MaintForm::Names => run_close(ui_state.names.request(&view.prefs)),
        MaintForm::Autoload => Some(app(MaintenanceMessage::AutoLoadText(
            ui_state.autoload.clone(),
        ))),
        MaintForm::Description => run_close(Some(MaintRequest::Header(
            mychess_app::maintenance::HeaderEdit::Description(ui_state.description.clone()),
        ))),
        MaintForm::Repair => repair_request(shell).map(|r| ui(MaintUiMessage::RunClose(r))),
    }
}

fn repair_request(shell: &Shell) -> Option<MaintRequest> {
    let state = &shell.ui.dialogs.maint;
    state.repair_path.clone().map(|path| MaintRequest::Repair {
        path,
        drop_unreadable: state.repair_drop,
    })
}

// ---- compact ----

fn kb(bytes: u64) -> String {
    (bytes / 1024).to_string()
}

#[allow(clippy::too_many_lines)] // two tables and their buttons
fn compact<'a>(shell: &Shell, view: &MaintenanceView, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut names = column![].spacing(4);
    let mut games = column![].spacing(4);
    let mut can_names = false;
    let mut can_games = false;
    if let Some(s) = &view.stats {
        let kinds = [
            "maint-compact-players",
            "maint-compact-events",
            "maint-compact-sites",
            "maint-compact-rounds",
        ];
        for (key, usage) in kinds.into_iter().zip(s.names) {
            can_names |= usage.unused > 0;
            names = names.push(
                row![
                    text(i18n.tr(key))
                        .size(BODY_TEXT)
                        .width(Length::Fixed(90.0)),
                    text(usage.total.to_string())
                        .size(BODY_TEXT)
                        .width(Length::Fixed(80.0)),
                    text(usage.unused.to_string())
                        .size(BODY_TEXT)
                        .width(Length::Fixed(80.0)),
                ]
                .spacing(6),
            );
        }
        let deleted = mychess_app::maintenance::MaintFlag::Delete
            .counter()
            .and_then(|i| s.flags.get(i).copied())
            .unwrap_or(0);
        can_games = deleted > 0 || s.games_file_bytes != s.compacted_bytes;
        games = games
            .push(
                row![
                    text(i18n.tr("maint-compact-current"))
                        .size(BODY_TEXT)
                        .width(Length::Fixed(150.0)),
                    text(s.games.to_string())
                        .size(BODY_TEXT)
                        .width(Length::Fixed(80.0)),
                    text(kb(s.games_file_bytes)).size(BODY_TEXT),
                ]
                .spacing(6),
            )
            .push(
                row![
                    text(i18n.tr("maint-compact-after"))
                        .size(BODY_TEXT)
                        .width(Length::Fixed(150.0)),
                    text(s.games.saturating_sub(deleted).to_string())
                        .size(BODY_TEXT)
                        .width(Length::Fixed(80.0)),
                    text(kb(s.compacted_bytes)).size(BODY_TEXT),
                ]
                .spacing(6),
            );
    } else {
        names = names.push(muted(i18n.tr("maint-ui-counting")));
    }
    let names_head = row![
        text("").width(Length::Fixed(90.0)),
        text(i18n.tr("maint-compact-names-col"))
            .size(BODY_TEXT)
            .font(bold())
            .width(Length::Fixed(80.0)),
        text(i18n.tr("maint-compact-unused"))
            .size(BODY_TEXT)
            .font(bold()),
    ]
    .spacing(6);
    let games_head = row![
        text("").width(Length::Fixed(150.0)),
        text(i18n.tr("maint-compact-games-col"))
            .size(BODY_TEXT)
            .font(bold())
            .width(Length::Fixed(80.0)),
        text(i18n.tr("maint-compact-size"))
            .size(BODY_TEXT)
            .font(bold()),
    ]
    .spacing(6);
    let names_note = if view.stats.is_some() && !can_names {
        muted(i18n.tr("maint-compact-names-clean"))
    } else {
        column![].into()
    };
    let games_note = if view.stats.is_some() && !can_games {
        muted(i18n.tr("maint-compact-games-clean"))
    } else {
        column![].into()
    };
    let enabled = !busy && view.stats.is_some();
    let body = column![
        heading(i18n.tr("maint-compact-name-file")),
        names_head,
        names,
        names_note,
        action(
            i18n.tr("maint-compact-names"),
            (enabled && can_names).then(|| ui(MaintUiMessage::RunClose(MaintRequest::Compact {
                names: true,
                games: false
            }))),
        ),
        heading(i18n.tr("maint-compact-game-file")),
        games_head,
        games,
        games_note,
        action(
            i18n.tr("maint-compact-games"),
            (enabled && can_games).then(|| ui(MaintUiMessage::RunClose(MaintRequest::Compact {
                names: false,
                games: true
            }))),
        ),
    ]
    .spacing(8);
    card(shell, "maint-compact", body.into(), vec![])
}

// ---- sort ----

fn sort<'a>(shell: &Shell, _view: &MaintenanceView, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let state = &shell.ui.dialogs.maint;
    let listing = if state.sort_keys.is_empty() {
        i18n.tr("maint-sort-none")
    } else {
        state
            .sort_keys
            .iter()
            .map(|k| criterion_label(i18n, *k))
            .collect::<Vec<_>>()
            .join(" > ")
    };
    let full = state.sort_keys.len() >= MAX_SORT_CRITERIA;
    let mut add = row![].spacing(4);
    for criterion in SortCriterion::ALL {
        add = add.push(action(
            criterion_label(i18n, *criterion),
            (!full && !busy).then(|| ui(MaintUiMessage::Edit(MaintEdit::SortAdd(*criterion)))),
        ));
    }
    let mut common = row![].spacing(4);
    for (index, key) in [
        "maint-sort-common-1",
        "maint-sort-common-2",
        "maint-sort-common-3",
    ]
    .into_iter()
    .enumerate()
    {
        common = common.push(action(
            i18n.tr(key),
            Some(ui(MaintUiMessage::Edit(MaintEdit::SortCommon(index)))),
        ));
    }
    let _ = common_sort;
    let body = column![
        labelled(
            i18n.tr("maint-sort-criteria"),
            text(listing).size(BODY_TEXT).into()
        ),
        heading(i18n.tr("maint-sort-add")),
        flow(add),
        heading(i18n.tr("maint-sort-common")),
        flow(common),
        row![
            action(
                i18n.tr("maint-sort-remove-last"),
                Some(ui(MaintUiMessage::Edit(MaintEdit::SortRemoveLast)))
            ),
            action(
                i18n.tr("maint-sort-clear"),
                Some(ui(MaintUiMessage::Edit(MaintEdit::SortClear)))
            ),
        ]
        .spacing(6),
        check(
            i18n.tr("maint-sort-reverse"),
            state.sort_reverse,
            Some(|on| ui(MaintUiMessage::Edit(MaintEdit::SortReverse(on)))),
        ),
        check(
            i18n.tr("maint-sort-compact-after"),
            state.sort_compact,
            Some(|on| ui(MaintUiMessage::Edit(MaintEdit::SortCompact(on)))),
        ),
        muted(i18n.tr("maint-sort-note")),
    ]
    .spacing(8);
    let go = state
        .sort_request()
        .filter(|_| !busy)
        .map(|r| ui(MaintUiMessage::RunClose(r)));
    card(
        shell,
        "maint-sort",
        body.into(),
        vec![primary(i18n.tr("maint-sort-go"), go)],
    )
}

// ---- duplicates ----

type TwinField = (
    &'static str,
    fn(&TwinPrefs) -> bool,
    fn(&mut TwinPrefs, bool),
);

const TWIN_CRITERIA: [TwinField; 10] = [
    (
        "maint-twins-colors",
        |p| p.same_colors,
        |p, v| p.same_colors = v,
    ),
    (
        "maint-twins-event",
        |p| p.same_event,
        |p, v| p.same_event = v,
    ),
    ("maint-twins-site", |p| p.same_site, |p, v| p.same_site = v),
    (
        "maint-twins-round",
        |p| p.same_round,
        |p, v| p.same_round = v,
    ),
    ("maint-twins-year", |p| p.same_year, |p, v| p.same_year = v),
    (
        "maint-twins-month",
        |p| p.same_month,
        |p, v| p.same_month = v,
    ),
    ("maint-twins-day", |p| p.same_day, |p, v| p.same_day = v),
    (
        "maint-twins-result",
        |p| p.same_result,
        |p, v| p.same_result = v,
    ),
    ("maint-twins-eco", |p| p.same_eco, |p, v| p.same_eco = v),
    (
        "maint-twins-moves",
        |p| p.same_moves,
        |p, v| p.same_moves = v,
    ),
];

const TWIN_OPTIONS: [TwinField; 5] = [
    (
        "maint-twins-skip-short",
        |p| p.skip_short,
        |p, v| p.skip_short = v,
    ),
    (
        "maint-twins-undelete-first",
        |p| p.undelete_first,
        |p, v| p.undelete_first = v,
    ),
    (
        "maint-twins-set-filter",
        |p| p.set_filter,
        |p, v| p.set_filter = v,
    ),
    (
        "maint-twins-keep-commented",
        |p| p.keep_commented,
        |p, v| p.keep_commented = v,
    ),
    (
        "maint-twins-keep-variations",
        |p| p.keep_with_variations,
        |p, v| p.keep_with_variations = v,
    ),
];

fn twin_checks<'a>(
    shell: &Shell,
    prefs: &MaintenancePrefs,
    fields: &'static [TwinField],
) -> Element<'a, Message> {
    let mut list = column![].spacing(3);
    for (key, get, set) in fields {
        let (get, set) = (*get, *set);
        let base = prefs.clone();
        list = list.push(check(
            shell.i18n.tr(key),
            get(&prefs.twins),
            Some(move |on| {
                let mut next = base.clone();
                set(&mut next.twins, on);
                ui(MaintUiMessage::Prefs(Box::new(next)))
            }),
        ));
    }
    list.into()
}

fn twins<'a>(shell: &Shell, view: &MaintenanceView, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let prefs = &view.prefs;
    let t = &prefs.twins;
    let exact = |label: &str, on: bool, value: bool| {
        prefs_choice(
            i18n.tr(label),
            t.exact_names == value && on,
            prefs,
            move |p| {
                p.twins.exact_names = value;
            },
        )
    };
    let scope = |label: &str, only_filter: bool| {
        prefs_choice(
            i18n.tr(label),
            t.only_filter == only_filter,
            prefs,
            move |p| p.twins.only_filter = only_filter,
        )
    };
    let which = |label: &str, value: DeleteChoice| {
        prefs_choice(i18n.tr(label), t.delete == value, prefs, move |p| {
            p.twins.delete = value;
        })
    };
    let body = column![
        muted(i18n.tr("maint-twins-note")),
        heading(i18n.tr("maint-twins-criteria")),
        row![
            exact("maint-twins-names-exact", true, true),
            exact("maint-twins-names-four", true, false)
        ]
        .spacing(4),
        twin_checks(shell, prefs, &TWIN_CRITERIA),
        heading(i18n.tr("maint-twins-which-games")),
        row![
            scope("maint-twins-scope-all", false),
            scope("maint-twins-scope-filter", true)
        ]
        .spacing(4),
        heading(i18n.tr("maint-twins-when-deleting")),
        twin_checks(shell, prefs, &TWIN_OPTIONS),
        heading(i18n.tr("maint-twins-which-delete")),
        flow(
            row![
                which("maint-twins-delete-shorter", DeleteChoice::Shorter),
                which("maint-twins-delete-lower", DeleteChoice::LowerNumber),
                which("maint-twins-delete-higher", DeleteChoice::HigherNumber),
            ]
            .spacing(4)
        ),
    ]
    .spacing(8);
    let mut defaults = prefs.clone();
    defaults.twins = TwinPrefs::default();
    let buttons = vec![
        action(
            i18n.tr("maint-twins-defaults"),
            Some(ui(MaintUiMessage::Prefs(Box::new(defaults)))),
        ),
        action(
            i18n.tr("maint-open-checker"),
            (view.twin_pairs > 0 && !busy)
                .then(|| ui(MaintUiMessage::App(MaintenanceMessage::OpenChecker))),
        ),
        primary(
            i18n.tr("maint-twins-go"),
            (!busy).then(|| ui(MaintUiMessage::RunClose(MaintRequest::FindTwins))),
        ),
    ];
    card(shell, "maint-duplicates", body.into(), buttons)
}

// ---- cleaner ----

type CleanerField = (
    &'static str,
    fn(&CleanerPrefs) -> bool,
    fn(&mut CleanerPrefs, bool),
);

const CLEANER_STEPS: [CleanerField; 10] = [
    (
        "maint-cleaner-step-players",
        |p| p.players,
        |p, v| p.players = v,
    ),
    (
        "maint-cleaner-step-events",
        |p| p.events,
        |p, v| p.events = v,
    ),
    ("maint-cleaner-step-sites", |p| p.sites, |p, v| p.sites = v),
    (
        "maint-cleaner-step-rounds",
        |p| p.rounds,
        |p, v| p.rounds = v,
    ),
    ("maint-cleaner-step-eco", |p| p.eco, |p, v| p.eco = v),
    ("maint-cleaner-step-elo", |p| p.elo, |p, v| p.elo = v),
    ("maint-cleaner-step-twins", |p| p.twins, |p, v| p.twins = v),
    (
        "maint-cleaner-step-compact-names",
        |p| p.compact_names,
        |p, v| p.compact_names = v,
    ),
    (
        "maint-cleaner-step-compact-games",
        |p| p.compact_games,
        |p, v| p.compact_games = v,
    ),
    ("maint-cleaner-step-tree", |p| p.tree, |p, v| p.tree = v),
];

/// The detail of a step, translated: the application sends Fluent keys; a
/// failed step with anything else shows the generic failure.
fn detail_text(
    i18n: &crate::i18n::I18n,
    record: &mychess_app::maintenance::data::StepRecord,
) -> Option<String> {
    if record.detail.starts_with("maint-cleaner-detail-") {
        return Some(i18n.tr(&record.detail));
    }
    (record.status == StepStatus::Failed).then(|| i18n.tr("maint-cleaner-detail-failed"))
}

fn cleaner_report<'a>(shell: &Shell, report: &CleanerReport) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut list = column![heading(i18n.tr("maint-cleaner-report"))].spacing(3);
    for record in report.records() {
        let tone = |s: String| -> Element<'a, Message> {
            if record.status == StepStatus::Failed {
                danger(s)
            } else {
                text(s).size(BODY_TEXT).into()
            }
        };
        let mut line = format!(
            "{}: {}",
            i18n.tr(step_key(record.step)),
            i18n.tr(status_key(record.status))
        );
        if let Some(detail) = detail_text(i18n, record) {
            line.push_str(" (");
            line.push_str(&detail);
            line.push(')');
        }
        list = list.push(tone(line));
    }
    list.into()
}

fn cleaner<'a>(shell: &Shell, view: &MaintenanceView, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut steps = column![].spacing(3);
    for (key, get, set) in &CLEANER_STEPS {
        let (get, set) = (*get, *set);
        let base = view.prefs.clone();
        steps = steps.push(check(
            i18n.tr(key),
            get(&view.prefs.cleaner),
            Some(move |on| {
                let mut next = base.clone();
                set(&mut next.cleaner, on);
                ui(MaintUiMessage::Prefs(Box::new(next)))
            }),
        ));
    }
    let mut body = column![muted(i18n.tr("maint-cleaner-help")), steps].spacing(10);
    if let Some(MaintReport::Cleaner(report)) = shell.app.maintenance().last_report() {
        body = body.push(cleaner_report(shell, report));
    }
    card(
        shell,
        "maint-cleaner",
        body.into(),
        vec![primary(
            i18n.tr("maint-cleaner-go"),
            (!busy).then(|| ui(MaintUiMessage::Run(MaintRequest::Cleaner))),
        )],
    )
}

// ---- check games ----

fn check_form<'a>(shell: &Shell, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut body = column![
        row![
            action(
                i18n.tr("maint-check-all"),
                (!busy).then(|| ui(MaintUiMessage::Run(MaintRequest::Check {
                    scope: ScopeChoice::All
                }))),
            ),
            action(
                i18n.tr("maint-check-filter"),
                (!busy).then(|| ui(MaintUiMessage::Run(MaintRequest::Check {
                    scope: ScopeChoice::Filter
                }))),
            ),
        ]
        .spacing(6)
    ]
    .spacing(10);
    if let Some(report) = shell.app.maintenance().check_report() {
        let mut args = FluentArgs::new();
        args.set("examined", i64::from(report.examined));
        args.set("problems", i64::from(report.problems));
        body = body.push(text(i18n.tr_args("maint-check-summary", &args)).size(BODY_TEXT));
        let mut list = column![].spacing(2);
        for issue in report.issues.iter().take(REPORT_ITEMS) {
            let mut args = FluentArgs::new();
            args.set("game", i64::from(issue.game));
            args.set("cause", i18n.tr(cause_key(issue.cause)));
            list = list.push(text(i18n.tr_args("maint-check-item", &args)).size(BODY_TEXT));
        }
        body = body.push(list);
        let more = usize::try_from(report.problems)
            .unwrap_or(usize::MAX)
            .saturating_sub(REPORT_ITEMS.min(report.issues.len()));
        if more > 0 {
            let mut args = FluentArgs::new();
            args.set("count", i64::try_from(more).unwrap_or(i64::MAX));
            body = body.push(muted(i18n.tr_args("maint-more", &args)));
        }
    }
    card(shell, "maint-check", body.into(), vec![])
}

// ---- strip ----

fn scope_choices<'a>(
    shell: &Shell,
    field: ScopeField,
    current: ScopeChoice,
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let one = |label: &str, value: ScopeChoice| {
        choice(
            i18n.tr(label),
            current == value,
            Some(ui(MaintUiMessage::Scope(field, value))),
        )
    };
    row![
        one("maint-scope-all", ScopeChoice::All),
        one("maint-scope-filter", ScopeChoice::Filter)
    ]
    .spacing(4)
    .into()
}

fn strip<'a>(shell: &Shell, view: &MaintenanceView, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let what = shell.ui.dialogs.maint.strip;
    let pick = |label: &str, value: StripChoice| {
        choice(
            i18n.tr(label),
            what == value,
            Some(ui(MaintUiMessage::Edit(MaintEdit::Strip(value)))),
        )
    };
    let body = column![
        muted(i18n.tr("maint-strip-note")),
        row![
            pick("maint-strip-comments", StripChoice::Comments),
            pick("maint-strip-variations", StripChoice::Variations)
        ]
        .spacing(4),
        scope_choices(shell, ScopeField::Strip, view.prefs.strip_scope),
    ]
    .spacing(10);
    let go = (!busy).then(|| {
        ui(MaintUiMessage::RunClose(MaintRequest::Strip {
            what,
            scope: view.prefs.strip_scope,
        }))
    });
    card(
        shell,
        "maint-strip-title",
        body.into(),
        vec![primary(i18n.tr("maint-strip-go"), go)],
    )
}

// ---- extra tags ----

fn tags<'a>(shell: &Shell, view: &MaintenanceView, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let state = &shell.ui.dialogs.maint;
    let mut list = column![].spacing(2);
    if let Some(found) = shell.app.maintenance().tag_list() {
        for tag in found.listing.tags.iter().take(200) {
            let selected = state.tags.selected.as_deref() == Some(tag.name.as_str());
            list = list.push(choice(
                format!("{}  ({})", tag.name, tag.games),
                selected,
                Some(ui(MaintUiMessage::Edit(MaintEdit::TagSelect(Some(
                    tag.name.clone(),
                ))))),
            ));
        }
        if found.listing.interrupted {
            list = list.push(danger(i18n.tr("maint-tags-interrupted")));
        }
    }
    let chosen = state.tags.selected.clone();
    let on_selected = |request: fn(String) -> TagRequest| {
        chosen
            .clone()
            .filter(|_| !busy)
            .map(|name| ui(MaintUiMessage::Run(MaintRequest::Tags(request(name)))))
    };
    let add = state.tag_add_request().filter(|_| !busy);
    let body = column![
        scope_choices(shell, ScopeField::Tags, view.prefs.tag_scope),
        row![
            action(
                i18n.tr("maint-tags-list"),
                (!busy).then(|| ui(MaintUiMessage::Run(MaintRequest::Tags(TagRequest::List)))),
            ),
            action(
                i18n.tr("maint-tags-filter"),
                on_selected(TagRequest::Filter)
            ),
            action(i18n.tr("maint-tag-strip"), on_selected(TagRequest::Strip)),
        ]
        .spacing(6),
        container(scrollable(list).height(Length::Fixed(160.0))).id("maint-tags-list"),
        heading(i18n.tr("maint-tags-add")),
        labelled(
            i18n.tr("maint-tags-name"),
            field(
                MAINT_TAG_NAME_ID,
                &state.tags.name,
                MaintEdit::TagName,
                add.clone().map(|r| ui(MaintUiMessage::Run(r)))
            ),
        ),
        labelled(
            i18n.tr("maint-tags-value"),
            field(
                MAINT_TAG_VALUE_ID,
                &state.tags.value,
                MaintEdit::TagValue,
                None
            ),
        ),
        muted(i18n.tr("maint-tags-note")),
    ]
    .spacing(8);
    card(
        shell,
        "maint-tags",
        body.into(),
        vec![primary(
            i18n.tr("maint-tags-add-go"),
            add.map(|r| ui(MaintUiMessage::Run(r))),
        )],
    )
}

// ---- name editor ----

fn names<'a>(shell: &Shell, view: &MaintenanceView, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let draft = &shell.ui.dialogs.maint.names;
    let mut kinds = row![].spacing(4);
    for kind in NameUiKind::ALL {
        kinds = kinds.push(choice(
            i18n.tr(kind.key()),
            draft.kind == kind,
            Some(ui(MaintUiMessage::Edit(MaintEdit::NamesKind(kind)))),
        ));
    }
    let scope = |label: &str, value: NameScopeChoice| {
        prefs_choice(
            i18n.tr(label),
            view.prefs.name_editor_scope == value,
            &view.prefs,
            move |p| p.name_editor_scope = value,
        )
    };
    let request = draft.request(&view.prefs).filter(|_| !busy);
    let submit = request.clone().map(|r| ui(MaintUiMessage::RunClose(r)));
    let rating = draft.kind == NameUiKind::Rating;
    let dates = matches!(draft.kind, NameUiKind::Date | NameUiKind::EventDate);
    let old_label = if rating {
        "maint-names-player"
    } else if dates {
        "maint-names-old-date"
    } else {
        "maint-names-replace"
    };
    let mut body = column![
        heading(i18n.tr("maint-names-type")),
        flow(kinds),
        heading(i18n.tr("maint-names-games")),
        row![
            scope("maint-scope-all", NameScopeChoice::All),
            scope("maint-scope-filter", NameScopeChoice::Filter),
            scope("maint-names-scope-tournament", NameScopeChoice::Tournament),
        ]
        .spacing(4),
        labelled(
            i18n.tr(old_label),
            field(
                MAINT_NAME_OLD_ID,
                &draft.old,
                MaintEdit::NamesOld,
                submit.clone()
            ),
        ),
    ]
    .spacing(8);
    body = if rating {
        body.push(labelled(
            i18n.tr("maint-names-elo"),
            field(
                MAINT_NAME_NEW_ID,
                &draft.elo,
                MaintEdit::NamesElo,
                submit.clone(),
            ),
        ))
    } else {
        body.push(labelled(
            i18n.tr(if dates {
                "maint-names-new-date"
            } else {
                "maint-names-with"
            }),
            field(
                MAINT_NAME_NEW_ID,
                &draft.new,
                MaintEdit::NamesNew,
                submit.clone(),
            ),
        ))
    };
    if !dates {
        body = body.push(check(
            i18n.tr("maint-names-pattern"),
            draft.pattern,
            Some(|on| ui(MaintUiMessage::Edit(MaintEdit::NamesPattern(on)))),
        ));
    }
    card(
        shell,
        "maint-names-editor",
        body.into(),
        vec![primary(
            i18n.tr("maint-names-go"),
            request.map(|r| ui(MaintUiMessage::RunClose(r))),
        )],
    )
}

// ---- header fields ----

fn autoload<'a>(shell: &Shell, view: &MaintenanceView, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let state = &shell.ui.dialogs.maint;
    let apply = |text: String| (!busy).then(|| app(MaintenanceMessage::AutoLoadText(text)));
    let current = shell
        .app
        .bases
        .slot(view.slot)
        .and_then(|s| s.current_game)
        .map(|n| n.to_string());
    let body = column![
        labelled(
            i18n.tr("maint-autoload-number"),
            field(
                MAINT_AUTOLOAD_ID,
                &state.autoload,
                MaintEdit::Autoload,
                apply(state.autoload.clone())
            ),
        ),
        row![
            action(i18n.tr("maint-autoload-none"), apply("0".to_owned())),
            action(i18n.tr("maint-autoload-first"), apply("1".to_owned())),
            action(i18n.tr("maint-autoload-current"), current.and_then(apply)),
            action(
                i18n.tr("maint-autoload-last"),
                (view.games > 0)
                    .then(|| view.games.to_string())
                    .and_then(apply)
            ),
        ]
        .spacing(6),
    ]
    .spacing(10);
    card(
        shell,
        "maint-autoload",
        body.into(),
        vec![primary(
            i18n.tr("maint-ui-apply"),
            apply(state.autoload.clone()),
        )],
    )
}

fn base_type<'a>(shell: &Shell, view: &MaintenanceView, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let current = view.header.as_ref().map(|h| h.base_type);
    let mut list = row![].spacing(4);
    for n in 0..32u32 {
        // The types 1 to 3 belong to the system (the temporary database, the
        // Clipbase, a PGN file): they are kept only where they are.
        let reserved = (1..=3).contains(&n) && current != Some(n);
        list = list.push(choice(
            base_type_label(i18n, n),
            current == Some(n),
            (!reserved && !busy).then(|| {
                ui(MaintUiMessage::RunClose(MaintRequest::Header(
                    mychess_app::maintenance::HeaderEdit::BaseType(n),
                )))
            }),
        ));
    }
    card(shell, "maint-type", flow(list), vec![])
}

fn description<'a>(shell: &Shell, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let text_value = shell.ui.dialogs.maint.description.clone();
    let go = (!busy).then(|| {
        ui(MaintUiMessage::RunClose(MaintRequest::Header(
            mychess_app::maintenance::HeaderEdit::Description(text_value.clone()),
        )))
    });
    let body = field(
        MAINT_DESCRIPTION_ID,
        &shell.ui.dialogs.maint.description,
        MaintEdit::Description,
        go.clone(),
    );
    card(
        shell,
        "maint-description",
        body,
        vec![primary(i18n.tr("maint-ui-apply"), go)],
    )
}

fn custom_flags<'a>(shell: &Shell, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut body = column![muted(i18n.tr("maint-custom-note"))].spacing(8);
    for (i, value) in shell.ui.dialogs.maint.custom.iter().enumerate() {
        let number = u8::try_from(i + 1).unwrap_or(1);
        let id: &'static str = [
            MAINT_CUSTOM_ID,
            "maint-custom-1",
            "maint-custom-2",
            "maint-custom-3",
            "maint-custom-4",
            "maint-custom-5",
        ][i];
        let go = (!busy).then(|| {
            ui(MaintUiMessage::Run(MaintRequest::Header(
                mychess_app::maintenance::HeaderEdit::CustomFlag {
                    number,
                    text: value.clone(),
                },
            )))
        });
        body = body.push(
            row![
                container(field(
                    id,
                    value,
                    move |t| MaintEdit::Custom(i, t),
                    go.clone()
                ))
                .width(Length::Fixed(220.0)),
                action(i18n.tr("maint-ui-apply"), go),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        let _ = number;
    }
    card(shell, "maint-custom-flags", body.into(), vec![])
}

// ---- repair ----

fn repair<'a>(shell: &Shell, busy: bool) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let state = &shell.ui.dialogs.maint;
    let path = state
        .repair_path
        .as_ref()
        .map_or_else(|| i18n.tr("maint-repair-none"), |p| p.display().to_string());
    let mut body = column![
        muted(i18n.tr("maint-repair-note")),
        row![
            text(path).size(BODY_TEXT).width(Length::Fill),
            action(
                i18n.tr("maint-repair-choose"),
                Some(ui(MaintUiMessage::PickRepairFile))
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        checkbox(state.repair_drop)
            .label(i18n.tr("maint-repair-drop"))
            .size(15)
            .text_size(BODY_TEXT)
            .on_toggle(|on| ui(MaintUiMessage::Edit(MaintEdit::RepairDrop(on)))),
    ]
    .spacing(10);
    for backup in shell.app.maintenance().backups() {
        let mut args = FluentArgs::new();
        args.set("path", backup.display().to_string());
        body = body.push(muted(i18n.tr_args("maint-repair-backup-kept", &args)));
    }
    let go = repair_request(shell)
        .filter(|_| !busy)
        .map(|r| ui(MaintUiMessage::RunClose(r)));
    card(
        shell,
        "maint-repair",
        body.into(),
        vec![primary(i18n.tr("maint-repair-go"), go)],
    )
}

// ---- the confirmation ----

/// The question before something that cannot be undone.
pub(super) fn confirm_overlay<'a>(
    shell: &Shell,
    view: &MaintenanceView,
) -> Option<Element<'a, Message>> {
    let confirm = view.confirm.as_ref()?;
    let i18n = &shell.i18n;
    let mut args = FluentArgs::new();
    args.set("count", i64::from(confirm.count));
    let mut body = column![].spacing(10);
    for weak in &confirm.weak {
        body = body.push(danger(i18n.tr(weak_key(*weak))));
    }
    body = body.push(text(i18n.tr_args(confirm_key(confirm.kind), &args)).size(BODY_TEXT + 1.0));
    if confirm.backup_offered {
        body = body.push(check(
            i18n.tr("maint-confirm-backup"),
            confirm.backup,
            Some(|on| app(MaintenanceMessage::SetBackup(on))),
        ));
    }
    let _ = MAINT_CHECKER_ID;
    Some(modal(Card {
        id: MAINT_CONFIRM_ID,
        title: i18n.tr("maint-ui-confirm"),
        body: body.into(),
        buttons: vec![
            action(
                i18n.tr("dialog-cancel"),
                Some(app(MaintenanceMessage::Confirm(false))),
            ),
            primary(
                i18n.tr("dialog-ok"),
                Some(app(MaintenanceMessage::Confirm(true))),
            ),
        ],
        width: 480.0,
    }))
}
