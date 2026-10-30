//! The "Engines" dialog (spec M4-3): the list with its columns, the buttons
//! of the Scid's `analysis.tcl` (New, Edit, Copy, Delete, Up, Down, UCI
//! options, Start), the search for installed engines, the import of an
//! `engines.dat`, the reports and the analysis options. The state lives in
//! `AppState::engine_dialog`; the box only draws it and sends messages.
//!
//! The list is not virtualized: it holds at most `MAX_ENGINES` (256) rows
//! and is edited rarely.

use std::path::PathBuf;

use fluent_bundle::FluentArgs;
use iced::widget::{
    button, checkbox, column, container, mouse_area, row, scrollable, space, text, text_input,
};
use iced::{Element, Length};
use mychess_app::engines::{
    EngineDialog, EngineEntry, EngineId, EngineMessage, EngineRun, EngineRunState, MANY_RUNS,
    StartMode,
};
use mychess_app::{AnalysisPrefs, AppState, Message as AppMessage};

use crate::i18n::I18n;
use crate::shell::{Message, UiMessage};
use crate::theme::{self, RowLook};
use crate::widgets::modal::{Card, modal};
use crate::widgets::right_click::RightClick;
use crate::windows::dialog_ui::{DialogMessage, PrefsNumber};
use crate::windows::engine_text::{
    format_date, import_warning_text, load_issue_text, protocol_label,
};

/// Id of the box.
pub const ENGINES_BOX_ID: &str = "engines-box";
/// Id of the box that confirms a deletion.
pub const ENGINES_DELETE_BOX_ID: &str = "engines-delete-box";

/// Height of the list, in logical pixels.
const LIST_HEIGHT: f32 = 260.0;
/// Height of the area of reports and found engines.
const REPORT_HEIGHT: f32 = 110.0;
const KEY_WIDTH: f32 = 60.0;
const TYPE_WIDTH: f32 = 72.0;
const ELO_WIDTH: f32 = 56.0;
const DATE_WIDTH: f32 = 100.0;

/// Which file the native dialog is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnginePick {
    /// The command of the editor.
    Command,
    /// An `engines.dat` of the Scid to import.
    ScidDat,
}

/// A message to the engine management.
pub(crate) fn send(message: EngineMessage) -> Message {
    Message::App(AppMessage::Engine(message))
}

fn tool<'a>(label: String, message: Option<Message>) -> Element<'a, Message> {
    button(text(label))
        .style(button::secondary)
        .on_press_maybe(message)
        .into()
}

fn cell<'a>(content: String, width: Length) -> Element<'a, Message> {
    text(content).width(width).into()
}

fn run_status(i18n: &I18n, run: Option<&EngineRun>) -> Option<String> {
    let key = match run?.state {
        EngineRunState::Starting => "engines-status-starting",
        EngineRunState::Running => "engines-status-running",
        EngineRunState::Stopping => "engines-status-stopping",
        _ => "engines-status-failed",
    };
    Some(i18n.tr(key))
}

fn list_row<'a>(
    app: &AppState,
    entry: &EngineEntry,
    selected: bool,
    zebra: bool,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut name = entry.name.clone();
    if let Some(status) = run_status(i18n, app.engine_runs.of_engine(entry.id)) {
        name = format!("{name}  [{status}]");
    }
    let key = app
        .engines
        .hotkey_of(entry.id)
        .map(|k| k.name().to_owned())
        .unwrap_or_default();
    let elo = entry.elo.map(|e| e.to_string()).unwrap_or_default();
    let used = entry.last_used.map(format_date).unwrap_or_default();
    let cells = row![
        cell(name, Length::Fill),
        cell(key, Length::Fixed(KEY_WIDTH)),
        cell(
            protocol_label(entry.protocol).to_owned(),
            Length::Fixed(TYPE_WIDTH)
        ),
        cell(elo, Length::Fixed(ELO_WIDTH)),
        cell(used, Length::Fixed(DATE_WIDTH)),
    ]
    .spacing(8);
    let surface = container(cells)
        .padding([3, 8])
        .width(Length::Fill)
        .style(theme::list_row(RowLook {
            zebra,
            selected,
            ..RowLook::default()
        }));
    mouse_area(surface)
        .on_press(send(EngineMessage::Select(Some(entry.id))))
        .on_double_click(send(EngineMessage::Start(entry.id, StartMode::Analyze)))
        .into()
}

fn header<'a>(i18n: &I18n) -> Element<'a, Message> {
    container(
        row![
            cell(i18n.tr("engines-col-name"), Length::Fill),
            cell(i18n.tr("engines-col-key"), Length::Fixed(KEY_WIDTH)),
            cell(i18n.tr("engines-col-type"), Length::Fixed(TYPE_WIDTH)),
            cell(i18n.tr("engines-col-elo"), Length::Fixed(ELO_WIDTH)),
            cell(i18n.tr("engines-col-last-used"), Length::Fixed(DATE_WIDTH)),
        ]
        .spacing(8),
    )
    .padding([3, 8])
    .style(theme::list_header)
    .into()
}

/// The report area: what the search found (to click), the report of the
/// last import and the remarks of reading `engines.toml`.
fn reports<'a>(app: &AppState, dialog: &EngineDialog, i18n: &I18n) -> Option<Element<'a, Message>> {
    let mut lines = column![].spacing(4);
    let mut any = false;
    if dialog.finding {
        lines = lines.push(text(i18n.tr("engines-finding")).style(theme::muted_text));
        any = true;
    }
    if dialog.probing > 0 {
        let mut args = FluentArgs::new();
        args.set("count", i64::try_from(dialog.probing).unwrap_or(i64::MAX));
        lines = lines.push(text(i18n.tr_args("engines-probing", &args)).style(theme::muted_text));
        any = true;
    }
    if !dialog.found.is_empty() {
        lines = lines.push(text(i18n.tr("engines-found-title")).size(13));
        // Adding runs the program to identify it (D752).
        lines = lines.push(
            text(i18n.tr("engines-found-note"))
                .size(12)
                .style(theme::muted_text),
        );
        for found in &dialog.found {
            let label = format!("{}  ({})", found.name, found.path.display());
            lines = lines.push(
                button(text(label).size(13))
                    .style(button::secondary)
                    .padding([2, 8])
                    .on_press(send(EngineMessage::AddFound(found.path.clone()))),
            );
        }
        any = true;
    }
    if let Some(import) = &dialog.import {
        let mut args = FluentArgs::new();
        args.set(
            "added",
            i64::try_from(import.added.len()).unwrap_or(i64::MAX),
        );
        args.set(
            "duplicates",
            i64::try_from(import.duplicates).unwrap_or(i64::MAX),
        );
        args.set(
            "invalid",
            i64::try_from(import.invalid + import.over_limit).unwrap_or(i64::MAX),
        );
        lines = lines.push(text(i18n.tr_args("engines-import-summary", &args)).size(13));
        if import.incomplete {
            lines = lines.push(text(i18n.tr("engines-import-incomplete")).size(13));
        }
        for warning in &import.warnings {
            lines = lines.push(text(import_warning_text(i18n, warning)).size(13));
        }
        any = true;
    }
    if !app.engine_issues.is_empty() {
        lines = lines.push(text(i18n.tr("engines-issues-title")).size(13));
        for issue in &app.engine_issues {
            lines = lines.push(text(load_issue_text(i18n, issue)).size(13));
        }
        any = true;
    }
    any.then(|| {
        let dismissable =
            !dialog.found.is_empty() || dialog.import.is_some() || !app.engine_issues.is_empty();
        let mut body = column![scrollable(lines).height(Length::Fixed(REPORT_HEIGHT))].spacing(4);
        if dismissable {
            body = body.push(tool(
                i18n.tr("engines-btn-dismiss"),
                Some(send(EngineMessage::DismissReports)),
            ));
        }
        container(body)
            .padding(6)
            .width(Length::Fill)
            .style(theme::list_header)
            .into()
    })
}

/// A numeric field of the analysis options. It shows what the user typed
/// (so it can be empty while edited) as long as that is the stored value.
/// It has an `on_submit` on purpose: Enter is then captured by the field and
/// never reaches the dialog's default answer, which would start the selected
/// engine (D750). The submit repeats the text, which changes nothing.
fn number_field<'a>(
    label: String,
    value: u32,
    typed: Option<&str>,
    field: PrefsNumber,
) -> Element<'a, Message> {
    let shown = match typed {
        Some(text) if text.parse::<u32>().unwrap_or(0) == value => text.to_owned(),
        _ => value.to_string(),
    };
    let submit = Message::Dialog(DialogMessage::EnginesPrefsText(field, shown.clone()));
    let input = text_input("", &shown)
        .width(Length::Fixed(80.0))
        .padding(4)
        .on_input(move |input| Message::Dialog(DialogMessage::EnginesPrefsText(field, input)))
        .on_submit(submit);
    row![text(label), input]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
}

fn analysis_options<'a>(
    prefs: AnalysisPrefs,
    panel: &PrefsPanel<'_>,
    i18n: &I18n,
) -> Element<'a, Message> {
    let toggle = |label: &str, on: bool, change: fn(&mut AnalysisPrefs, bool)| {
        checkbox(on)
            .label(label.to_owned())
            .on_toggle(move |value| {
                let mut next = prefs;
                change(&mut next, value);
                send(EngineMessage::SetAnalysisPrefs(next))
            })
    };
    column![
        text(i18n.tr("analysis-pref-title")).size(14),
        number_field(
            i18n.tr("analysis-pref-log-size"),
            prefs.log_lines,
            panel.log_text,
            PrefsNumber::LogLines,
        ),
        number_field(
            i18n.tr("analysis-pref-max-ply"),
            prefs.max_ply,
            panel.ply_text,
            PrefsNumber::MaxPly,
        ),
        toggle(
            &i18n.tr("analysis-pref-add-name"),
            prefs.name_in_comments,
            |p, v| p.name_in_comments = v
        ),
        toggle(
            &i18n.tr("analysis-pref-low-priority"),
            prefs.low_priority,
            |p, v| p.low_priority = v
        ),
        toggle(&i18n.tr("analysis-pref-wrap-pv"), prefs.wrap_pv, |p, v| p
            .wrap_pv =
            v),
        toggle(
            &i18n.tr("analysis-pref-show-info"),
            prefs.show_info,
            |p, v| p.show_info = v
        ),
    ]
    .spacing(6)
    .into()
}

/// The box, when the application has the dialog open and nothing on top of
/// it (the editor, the options and the deletion question are drawn by their
/// own modules).
pub fn view<'a>(app: &AppState, prefs_open: bool, i18n: &I18n) -> Option<Element<'a, Message>> {
    view_with(app, prefs_open.then(PrefsPanel::default), i18n)
}

/// What the unfolded analysis options need besides the application state:
/// the text being typed in each numeric field.
#[derive(Debug, Clone, Copy, Default)]
pub struct PrefsPanel<'a> {
    /// The log size field.
    pub log_text: Option<&'a str>,
    /// The max ply field.
    pub ply_text: Option<&'a str>,
}

/// [`view`] with the typed text of the numeric fields; `None` keeps the
/// analysis options folded.
pub fn view_with<'a>(
    app: &AppState,
    panel: Option<PrefsPanel<'_>>,
    i18n: &I18n,
) -> Option<Element<'a, Message>> {
    let dialog = app.engine_dialog.as_ref()?;
    Some(dialog_box(app, dialog, panel, i18n))
}

fn dialog_box<'a>(
    app: &AppState,
    dialog: &EngineDialog,
    panel: Option<PrefsPanel<'_>>,
    i18n: &I18n,
) -> Element<'a, Message> {
    let selected = dialog.selected.and_then(|id| app.engines.get(id));
    let id = selected.map(|e| e.id);
    let on = |message: fn(EngineId) -> EngineMessage| id.map(|id| send(message(id)));

    let mut list = column![].width(Length::Fill);
    if app.engines.is_empty() {
        list = list
            .push(container(text(i18n.tr("engines-empty")).style(theme::muted_text)).padding(8));
    }
    for (index, entry) in app.engines.engines().iter().enumerate() {
        list = list.push(list_row(
            app,
            entry,
            dialog.selected == Some(entry.id),
            index % 2 == 1,
            i18n,
        ));
    }
    let table = column![
        header(i18n),
        scrollable(list).height(Length::Fixed(LIST_HEIGHT))
    ];

    let is_uci = selected.is_some_and(|e| e.protocol == mychess_app::engines::Protocol::Uci);
    let tools = row![
        tool(i18n.tr("engines-btn-new"), Some(send(EngineMessage::New))),
        tool(i18n.tr("engines-btn-edit"), on(EngineMessage::Edit)),
        tool(i18n.tr("engines-btn-copy"), on(EngineMessage::Copy)),
        tool(i18n.tr("engines-btn-delete"), on(EngineMessage::Delete)),
        tool(i18n.tr("engines-btn-up"), on(EngineMessage::MoveUp)),
        tool(i18n.tr("engines-btn-down"), on(EngineMessage::MoveDown)),
        space::horizontal(),
        tool(
            i18n.tr("engines-btn-options"),
            id.filter(|_| is_uci)
                .map(|id| send(EngineMessage::OpenOptions(id))),
        ),
    ]
    .spacing(6);

    let discovery = row![
        tool(
            i18n.tr("engines-btn-find"),
            (!dialog.finding).then(|| send(EngineMessage::FindInstalled)),
        ),
        tool(
            i18n.tr("engines-btn-import"),
            Some(Message::Ui(UiMessage::PickEngineFile(EnginePick::ScidDat))),
        ),
        space::horizontal(),
        tool(
            i18n.tr("engines-btn-analysis-options"),
            Some(Message::Ui(UiMessage::ToggleEnginePrefs)),
        ),
    ]
    .spacing(6);

    let mut body = column![table, tools, discovery].spacing(8);
    if let Some(report) = reports(app, dialog, i18n) {
        body = body.push(report);
    }
    if app.engine_runs.len() >= MANY_RUNS {
        let mut args = FluentArgs::new();
        args.set(
            "count",
            i64::try_from(app.engine_runs.len()).unwrap_or(i64::MAX),
        );
        body = body.push(text(i18n.tr_args("engines-many-running", &args)).style(text::danger));
    }
    if let Some(panel) = panel {
        body = body.push(analysis_options(
            app.prefs.analysis.as_ref().to_owned(),
            &panel,
            i18n,
        ));
    }

    modal(Card {
        id: ENGINES_BOX_ID,
        title: i18n.tr("engines-title"),
        body: body.into(),
        buttons: vec![
            start_button(app, selected, i18n),
            button(text(i18n.tr("engines-btn-close")))
                .style(button::secondary)
                .on_press(send(EngineMessage::CloseDialog))
                .into(),
        ],
        width: 680.0,
    })
}

/// Start, Stop or Restart for the selected engine; the right button starts
/// without analysing (the Scid's "nostart").
fn start_button<'a>(
    app: &AppState,
    selected: Option<&EngineEntry>,
    i18n: &I18n,
) -> Element<'a, Message> {
    let Some(entry) = selected else {
        return button(text(i18n.tr("engines-btn-start"))).into();
    };
    let run = app.engine_runs.of_engine(entry.id);
    let (key, message, idle): (&str, Option<Message>, Option<Message>) = match run {
        None => (
            "engines-btn-start",
            Some(send(EngineMessage::Start(entry.id, StartMode::Analyze))),
            Some(send(EngineMessage::Start(entry.id, StartMode::Idle))),
        ),
        Some(r) => match r.state {
            EngineRunState::Starting | EngineRunState::Running => (
                "engines-btn-stop",
                Some(send(EngineMessage::Stop(r.id))),
                None,
            ),
            EngineRunState::Stopping => ("engines-btn-stop", None, None),
            _ => (
                "engines-btn-restart",
                Some(send(EngineMessage::Restart(r.id))),
                None,
            ),
        },
    };
    let button = button(text(i18n.tr(key))).on_press_maybe(message);
    match idle {
        Some(idle) => RightClick::new(button, move |_, _| idle.clone()).into(),
        None => button.into(),
    }
}

/// The question that confirms a deletion: the name and the command.
pub fn delete_view<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let id = app.engine_dialog.as_ref()?.confirm_delete?;
    let entry = app.engines.get(id)?;
    let mut args = FluentArgs::new();
    args.set("name", entry.name.as_str());
    args.set("command", entry.command.display().to_string());
    Some(modal(Card {
        id: ENGINES_DELETE_BOX_ID,
        title: i18n.tr("engines-delete-title"),
        body: text(i18n.tr_args("engines-delete-message", &args)).into(),
        buttons: vec![
            button(text(i18n.tr("dialog-cancel")))
                .style(button::primary)
                .on_press(send(EngineMessage::CancelDelete))
                .into(),
            button(text(i18n.tr("engines-btn-delete")))
                .style(button::danger)
                .on_press(send(EngineMessage::ConfirmDelete(id)))
                .into(),
        ],
        width: 480.0,
    }))
}

/// The messages that follow a pick of a file: the command fills the editor
/// (and the folder, when it is empty, with the folder of the file) and asks
/// for the disk check; an `engines.dat` is imported. A path that is not UTF-8
/// cannot be stored (spec rule 9) and gives nothing.
pub(crate) fn picked_messages(
    pick: EnginePick,
    path: Option<PathBuf>,
    working_dir_empty: bool,
) -> Vec<EngineMessage> {
    use mychess_app::engines::EditorMessage;
    let Some(path) = path else {
        return Vec::new();
    };
    match pick {
        EnginePick::ScidDat => vec![EngineMessage::ImportScid(path)],
        EnginePick::Command => {
            let Some(command) = path.to_str().map(str::to_owned) else {
                return Vec::new();
            };
            let mut out = vec![EngineMessage::Editor(EditorMessage::SetCommand(command))];
            if working_dir_empty
                && let Some(dir) = path.parent().and_then(|p| p.to_str())
                && !dir.is_empty()
            {
                out.push(EngineMessage::Editor(EditorMessage::SetWorkingDir(
                    dir.to_owned(),
                )));
            }
            out.push(EngineMessage::Editor(EditorMessage::Check));
            out
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::engines::{EngineDraft, Hotkey};

    use super::*;
    use crate::i18n::Language;

    fn state() -> (AppState, Vec<EngineId>) {
        let mut app = AppState::default();
        let mut ids = Vec::new();
        for (name, elo) in [("Stockfish", 3500), ("Lc0", 0), ("Crafty", 2900)] {
            let mut draft = EngineDraft::new(name, format!("/bin/{name}"));
            draft.elo = (elo > 0).then_some(elo);
            draft.last_used = Some(1_700_000_000);
            ids.push(app.engines.add(draft).expect("add"));
        }
        app.engines
            .set_hotkey(ids[0], Some(Hotkey::F2))
            .expect("key");
        drop(mychess_app::update(
            &mut app,
            AppMessage::Engine(EngineMessage::OpenDialog),
        ));
        (app, ids)
    }

    fn sim(app: &AppState, language: Language) -> Simulator<'static, Message> {
        let element = view(app, false, &I18n::new(language)).expect("dialog is open");
        Simulator::with_size(Settings::default(), Size::new(1000.0, 800.0), element)
    }

    fn sent(app: &AppState, click: &str) -> Vec<Message> {
        let mut s = sim(app, Language::EnUs);
        s.click(click).expect("clickable");
        s.into_messages().collect()
    }

    fn engine_message(message: &Message) -> Option<&EngineMessage> {
        match message {
            Message::App(AppMessage::Engine(m)) => Some(m),
            _ => None,
        }
    }

    #[test]
    fn the_list_shows_the_columns_of_every_engine() {
        let (app, _) = state();
        let mut s = sim(&app, Language::EnUs);
        for text in [
            "Name",
            "Key",
            "Type",
            "Elo",
            "Last used",
            "Stockfish",
            "Lc0",
            "Crafty",
            "F2",
            "3500",
            "uci",
            "2023-11-14",
        ] {
            assert!(s.find(text).is_ok(), "{text}");
        }
        assert!(s.find(iced::widget::Id::new(ENGINES_BOX_ID)).is_ok());
    }

    #[test]
    fn the_buttons_work_on_the_selected_engine() {
        let (app, ids) = state();
        let selected = ids[0];
        for (label, expected) in [
            ("Edit", EngineMessage::Edit(selected)),
            ("Copy", EngineMessage::Copy(selected)),
            ("Delete", EngineMessage::Delete(selected)),
            ("Up", EngineMessage::MoveUp(selected)),
            ("Down", EngineMessage::MoveDown(selected)),
            ("UCI options...", EngineMessage::OpenOptions(selected)),
            ("Start", EngineMessage::Start(selected, StartMode::Analyze)),
            ("New", EngineMessage::New),
            ("Close", EngineMessage::CloseDialog),
            ("Find installed engines", EngineMessage::FindInstalled),
        ] {
            let messages = sent(&app, label);
            assert_eq!(
                messages
                    .iter()
                    .filter_map(engine_message)
                    .collect::<Vec<_>>(),
                [&expected],
                "{label}"
            );
        }
    }

    #[test]
    fn rows_select_and_start_with_a_double_click() {
        let (app, ids) = state();
        let messages = sent(&app, "Lc0");
        assert!(
            messages
                .iter()
                .filter_map(engine_message)
                .any(|m| *m == EngineMessage::Select(Some(ids[1]))),
            "{messages:?}"
        );
    }

    #[test]
    fn without_a_selection_the_row_buttons_are_off() {
        let (mut app, _) = state();
        app.engine_dialog.as_mut().expect("dialog").selected = None;
        for label in [
            "Edit",
            "Copy",
            "Delete",
            "Up",
            "Down",
            "UCI options...",
            "Start",
        ] {
            let mut s = sim(&app, Language::EnUs);
            s.click(label).expect("exists");
            assert_eq!(s.into_messages().count(), 0, "{label}");
        }
    }

    #[test]
    fn a_running_engine_offers_stop_and_a_failed_one_restart() {
        let (mut app, ids) = state();
        let effects = mychess_app::update(
            &mut app,
            AppMessage::Engine(EngineMessage::Start(ids[0], StartMode::Analyze)),
        );
        assert!(!effects.is_empty());
        let run = app.engine_runs.of_engine(ids[0]).expect("run").id;
        let messages = sent(&app, "Stop");
        assert!(
            messages
                .iter()
                .filter_map(engine_message)
                .any(|m| *m == EngineMessage::Stop(run))
        );
        let mut s = sim(&app, Language::EnUs);
        assert!(s.find("Stockfish  [starting...]").is_ok());
    }

    #[test]
    fn the_empty_list_invites_to_add_and_the_box_is_translated() {
        let mut app = AppState::default();
        drop(mychess_app::update(
            &mut app,
            AppMessage::Engine(EngineMessage::OpenDialog),
        ));
        let mut s = sim(&app, Language::PtBr);
        for text in [
            "Novo",
            "Editar",
            "Excluir",
            "Fechar",
            "Procurar engines instaladas",
        ] {
            assert!(s.find(text).is_ok(), "{text}");
        }
        assert!(
            s.find("Ainda não há engines. Adicione uma ou procure as engines instaladas neste computador.")
                .is_ok()
        );
    }

    #[test]
    fn the_deletion_question_names_the_engine_and_the_command() {
        let (mut app, ids) = state();
        drop(mychess_app::update(
            &mut app,
            AppMessage::Engine(EngineMessage::Delete(ids[2])),
        ));
        let element = delete_view(&app, &I18n::new(Language::EnUs)).expect("question");
        let mut s = Simulator::with_size(Settings::default(), Size::new(900.0, 600.0), element);
        assert!(s.find("Delete the engine Crafty (/bin/Crafty)?").is_ok());
        s.click("Cancel").expect("button");
        let messages: Vec<Message> = s.into_messages().collect();
        assert_eq!(
            messages
                .iter()
                .filter_map(engine_message)
                .collect::<Vec<_>>(),
            [&EngineMessage::CancelDelete]
        );
    }

    fn panel_sim<'a>(app: &AppState, text: Option<&'a str>) -> Simulator<'a, Message> {
        let panel = PrefsPanel {
            log_text: text,
            ply_text: None,
        };
        let element = view_with(app, Some(panel), &I18n::new(Language::EnUs)).expect("open");
        Simulator::with_size(Settings::default(), Size::new(1000.0, 900.0), element)
    }

    #[test]
    fn enter_in_a_numeric_field_is_captured_by_an_inert_submit() {
        // D750: without an `on_submit` the Enter would reach the dialog and
        // start the selected engine.
        let (app, _) = state();
        let shown = app.prefs.analysis.log_lines.to_string();
        let mut s = panel_sim(&app, None);
        s.click(shown.as_str()).expect("the field");
        s.tap_key(iced::keyboard::key::Named::Enter);
        let messages: Vec<Message> = s.into_messages().collect();
        assert!(
            messages.iter().any(|m| matches!(
                m,
                Message::Dialog(DialogMessage::EnginesPrefsText(PrefsNumber::LogLines, t))
                    if *t == shown
            )),
            "{messages:?}"
        );
        assert!(
            !messages
                .iter()
                .filter_map(engine_message)
                .any(|m| matches!(m, EngineMessage::Start(..))),
            "{messages:?}"
        );
    }

    #[test]
    fn an_emptied_numeric_field_stays_empty_while_it_is_edited() {
        let (mut app, _) = state();
        app.prefs.analysis.log_lines = 0;
        app.prefs.analysis.max_ply = 5;
        let mut s = panel_sim(&app, Some(""));
        assert!(s.find("0").is_err(), "the typed text wins over the value");
        let mut s = panel_sim(&app, Some("7"));
        assert!(s.find("0").is_ok(), "a stale text gives way to the value");
    }

    #[test]
    fn a_picked_command_fills_the_form_and_the_folder_once() {
        use mychess_app::engines::EditorMessage;
        let path = PathBuf::from("engines").join("fish");
        let messages = picked_messages(EnginePick::Command, Some(path.clone()), true);
        assert_eq!(messages.len(), 3);
        assert_eq!(
            messages[0],
            EngineMessage::Editor(EditorMessage::SetCommand(
                path.to_string_lossy().into_owned()
            ))
        );
        assert_eq!(messages[2], EngineMessage::Editor(EditorMessage::Check));
        let keep = picked_messages(EnginePick::Command, Some(path.clone()), false);
        assert_eq!(keep.len(), 2);
        assert!(picked_messages(EnginePick::Command, None, true).is_empty());
        assert_eq!(
            picked_messages(EnginePick::ScidDat, Some(path.clone()), true),
            [EngineMessage::ImportScid(path)]
        );
    }
}
