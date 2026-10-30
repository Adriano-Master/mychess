//! Engine management: the list, the dialog and editor, the executions and
//! the position sent to the engines (spec M4-3, D424-D436).
//!
//! Pure bookkeeping: files, processes and probes are [`Effect`]s.

use std::path::PathBuf;
use std::sync::Arc;

use mychess_engine::{EngineCommand, EngineEvent};

use super::AppState;
use crate::effect::Effect;
use crate::engines::option_value;
use crate::engines::{
    AnalysisTarget, CommandError, DetectState, ENGINES_FILE_NAME, EditorError, EditorMessage,
    EngineDialog, EngineDraft, EngineEditor, EngineEntry, EngineFailure, EngineId, EngineMessage,
    EngineNotice, EngineRun, EngineRunState, FoundEngine, Hotkey, ImportReport, ListError,
    LoadedEngineList, MANY_RUNS, RunDefaults, RunId, ScidImport, StartMode, StoreError, TestState,
};
use crate::engines::{OptionsDialog, OptionsMessage, OptionsStatus, default_text};
use crate::error::{FileError, RequestId};
use crate::files::same_path;
use crate::flow::AppNotice;
use crate::prefs::{PrefChange, PrefsMessage};

/// Bookkeeping of the engine flows (not shown by the front-end).
#[derive(Debug, Default)]
pub(crate) struct EnginesFlow {
    /// `engines.toml` was asked for and has not been read yet.
    pub(crate) loading: bool,
    pub(crate) save_in_flight: bool,
    pub(crate) resave: bool,
    /// A quit waits for the save in flight.
    pub(crate) quit_waiting_save: bool,
    /// Where the quit is in ending the engines.
    pub(crate) shutdown: Shutdown,
    pub(crate) find: Option<RequestId>,
    pub(crate) import: Option<RequestId>,
    pub(crate) probes: Vec<RequestId>,
    /// The path each probe in flight is checking (no second one for it).
    pub(crate) probing_paths: Vec<(RequestId, PathBuf)>,
    /// The position last built for the engines: the key it was built for
    /// (changes of the game, cursor) and the target.
    pub(crate) target: Option<((u64, mychess_core::NodeId), Arc<AnalysisTarget>)>,
}

/// Progress of "quit ends every engine" (D435).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Shutdown {
    #[default]
    Idle,
    Waiting,
    Done,
}

impl AppState {
    /// The execution that feeds the evaluation bar and the graph (D434): the
    /// first one in `Running` with the analysis on, in the order of the
    /// list.
    pub fn main_engine(&self) -> Option<RunId> {
        self.engines.engines().iter().find_map(|entry| {
            self.engine_runs
                .of_engine(entry.id)
                .filter(|r| r.state == EngineRunState::Running && r.analyzing)
                .map(|r| r.id)
        })
    }

    /// The executions, in the order they started.
    pub fn engine_runs(&self) -> &[EngineRun] {
        self.engine_runs.as_slice()
    }

    fn notice_engine(&mut self, notice: EngineNotice) {
        self.notice = Some(AppNotice::Engine(notice));
    }

    fn engines_file(&self) -> Option<PathBuf> {
        self.flow
            .prefs_path
            .as_deref()
            .map(|prefs| prefs.with_file_name(ENGINES_FILE_NAME))
    }

    /// The effect that reads `engines.toml`, when startup asks for it.
    pub(crate) fn request_engines_load(&mut self, effects: &mut Vec<Effect>) {
        if let Some(path) = self.engines_file() {
            self.flow.engines.loading = true;
            effects.push(Effect::LoadEngines { path });
        }
    }

    #[allow(clippy::too_many_lines)] // one flat match over the messages reads best
    pub(crate) fn engine_message(&mut self, message: EngineMessage, effects: &mut Vec<Effect>) {
        match message {
            // Answers to effects: always handled.
            EngineMessage::Loaded(result) => self.engines_loaded(*result),
            EngineMessage::Saved(result) => self.engines_saved(&result, effects),
            EngineMessage::Checked { request, warnings } => {
                if let Some(editor) = self.engine_editor.as_mut()
                    && editor.check_request == Some(request)
                {
                    editor.check_request = None;
                    editor.warnings = warnings;
                }
            }
            EngineMessage::Tested { request, result }
                if self
                    .engine_dialog
                    .as_ref()
                    .and_then(|d| d.options.as_ref())
                    .is_some_and(|o| o.request == Some(request)) =>
            {
                self.options_probed(*result);
            }
            EngineMessage::Tested { request, result } => {
                if let Some(editor) = self.engine_editor.as_mut()
                    && editor.test_request == Some(request)
                {
                    editor.test_request = None;
                    editor.test = Some(match *result {
                        Ok(identity) => TestState::Passed(Box::new(identity)),
                        Err(failure) => TestState::Failed(failure),
                    });
                }
            }
            EngineMessage::Detected { request, result } => {
                if let Some(editor) = self.engine_editor.as_mut()
                    && editor.detect_request == Some(request)
                {
                    editor.detect_request = None;
                    editor.detect = Some(match result {
                        Ok(protocol) => DetectState::Found(protocol),
                        Err(failure) => DetectState::Failed(failure),
                    });
                }
            }
            EngineMessage::Found { request, engines } => self.found(request, engines),
            EngineMessage::FoundProbed {
                request,
                path,
                result,
            } => self.found_probed(request, path, result, effects),
            EngineMessage::ScidRead { request, result } => {
                self.scid_read(request, *result, effects);
            }
            EngineMessage::Started { run, identity, at } => {
                self.engine_started(run, *identity, at, effects);
                self.annotate_engine_ready(run, effects);
                self.epd_engine_ready(run, effects);
                self.play_engine_ready(run, effects);
            }
            EngineMessage::StartFailed { run, failure } => {
                self.annotate_engine_gone(run, false, effects);
                self.epd_engine_gone(run, false, effects);
                self.play_engine_gone(run, effects);
                self.engine_start_failed(run, failure);
            }
            EngineMessage::Stopped(run) => {
                self.annotate_engine_gone(run, false, effects);
                self.epd_engine_gone(run, false, effects);
                self.play_engine_gone(run, effects);
                drop(self.engine_runs.remove(run));
                self.analysis_run_stopped(run, effects);
            }
            EngineMessage::ShutdownDone => {
                if let Some(run) = self
                    .flow
                    .annotate
                    .as_ref()
                    .map(super::annotate_flow::Active::engine_run)
                {
                    self.annotate_engine_gone(run, false, effects);
                }
                if let Some(run) = self.epd.run.as_ref().map(|r| r.engine_run) {
                    self.epd_engine_gone(run, false, effects);
                }
                self.play_shutdown(effects);
                self.engine_runs.clear();
                if self.flow.engines.shutdown == Shutdown::Waiting {
                    self.flow.engines.shutdown = Shutdown::Done;
                    self.finish_quit(effects);
                }
            }
            EngineMessage::CommandDone { run, result } => {
                if matches!(result, Err(CommandError::NotRunning | CommandError::Closed)) {
                    self.annotate_engine_gone(run, false, effects);
                    self.epd_engine_gone(run, false, effects);
                    self.play_engine_gone(run, effects);
                    self.training_command_gone(run, effects);
                }
                if result == Err(CommandError::Refused) {
                    self.notice_engine(EngineNotice::CommandRefused);
                }
            }
            EngineMessage::CommandFailed { run, kind, error } => {
                if error == CommandError::Refused {
                    self.notice_engine(EngineNotice::CommandRefused);
                }
                self.annotate_command_failed(run, kind, effects);
                self.epd_command_failed(run, kind, effects);
                self.play_command_failed(run, kind, effects);
                self.training_command_failed(run, kind, effects);
            }
            EngineMessage::Event { run, event } => self.engine_event(run, *event, effects),
            // The commands of the user. Nothing changes while the list is
            // being read: the answer would replace it.
            _ if self.flow.engines.loading => {}
            EngineMessage::OpenDialog => {
                if self.engine_dialog.is_none() {
                    self.engine_dialog = Some(EngineDialog {
                        selected: self.engines.engines().first().map(|e| e.id),
                        ..EngineDialog::default()
                    });
                }
            }
            EngineMessage::CloseDialog => {
                self.engine_dialog = None;
                self.engine_editor = None;
            }
            EngineMessage::Select(id) => {
                if let Some(dialog) = self.engine_dialog.as_mut() {
                    dialog.selected = id.filter(|id| self.engines.get(*id).is_some());
                }
            }
            EngineMessage::SelectInitial(initial) => {
                if let Some(dialog) = self.engine_dialog.as_mut()
                    && let Some(id) = self.engines.find_by_initial(dialog.selected, initial)
                {
                    dialog.selected = Some(id);
                }
            }
            EngineMessage::New => self.engine_editor = Some(EngineEditor::new_engine()),
            EngineMessage::Edit(id) => match self.engines.get(id) {
                Some(entry) => {
                    let key = self.engines.hotkey_of(id);
                    self.engine_editor = Some(EngineEditor::for_entry(entry, key));
                }
                None => self.notice_engine(EngineNotice::NotFound),
            },
            EngineMessage::Copy(id) => match self.engines.copy(id) {
                Ok(copy) => {
                    self.select_engine(copy);
                    self.request_engines_save(effects);
                }
                Err(error) => self.notice_list_error(&error),
            },
            EngineMessage::Delete(id) => {
                if self.engines.get(id).is_some()
                    && let Some(dialog) = self.engine_dialog.as_mut()
                {
                    dialog.confirm_delete = Some(id);
                }
            }
            EngineMessage::CancelDelete => {
                if let Some(dialog) = self.engine_dialog.as_mut() {
                    dialog.confirm_delete = None;
                }
            }
            EngineMessage::ConfirmDelete(id) => self.confirm_delete(id, effects),
            EngineMessage::MoveUp(id) => {
                if self.engines.move_up(id) == Ok(true) {
                    self.request_engines_save(effects);
                }
            }
            EngineMessage::MoveDown(id) => {
                if self.engines.move_down(id) == Ok(true) {
                    self.request_engines_save(effects);
                }
            }
            EngineMessage::SetHotkey(id, key) => match self.engines.set_hotkey(id, key) {
                Ok(()) => self.request_engines_save(effects),
                Err(error) => self.notice_list_error(&error),
            },
            EngineMessage::Editor(message) => self.engine_editor_message(message, effects),
            EngineMessage::SetOption {
                engine,
                name,
                value,
            } => self.set_engine_option(engine, name, &value, effects),
            EngineMessage::OpenOptions(id) => self.open_options(id, effects),
            EngineMessage::Options(message) => self.options_message(message, effects),
            EngineMessage::SetAnalysisPrefs(prefs) => {
                self.prefs_message(PrefsMessage::Set(PrefChange::Analysis(prefs)), effects);
            }
            EngineMessage::Start(id, mode) => self.start_engine(id, mode, effects),
            EngineMessage::Stop(run) => self.stop_run(run, effects),
            EngineMessage::Restart(run) => {
                let Some(r) = self.engine_runs.get(run) else {
                    return;
                };
                let (engine, analyzing) = (r.engine, r.analyzing);
                if matches!(r.state, EngineRunState::Failed(_)) {
                    drop(self.engine_runs.remove(run));
                    let mode = if analyzing {
                        StartMode::Analyze
                    } else {
                        StartMode::Idle
                    };
                    self.start_engine(engine, mode, effects);
                }
            }
            EngineMessage::ToggleHotkey(key) => match self.engines.hotkey_owner(key) {
                Some(id) => self.start_engine(id, StartMode::Analyze, effects),
                None => self.notice_engine(EngineNotice::NoEngineOnKey(key)),
            },
            EngineMessage::ToggleMainAnalysis => self.toggle_main_analysis(effects),
            EngineMessage::FindInstalled => {
                let request = self.new_request();
                self.flow.engines.find = Some(request);
                if let Some(dialog) = self.engine_dialog.as_mut() {
                    dialog.finding = true;
                }
                effects.push(Effect::FindInstalledEngines { request });
            }
            EngineMessage::AddFound(path) => {
                if self
                    .engines
                    .engines()
                    .iter()
                    .any(|e| same_path(&e.command, &path))
                    || self
                        .flow
                        .engines
                        .probing_paths
                        .iter()
                        .any(|(_, p)| same_path(p, &path))
                {
                    return;
                }
                let request = self.new_request();
                self.flow.engines.probes.push(request);
                self.flow
                    .engines
                    .probing_paths
                    .push((request, path.clone()));
                if let Some(dialog) = self.engine_dialog.as_mut() {
                    dialog.probing += 1;
                }
                effects.push(Effect::ProbeFoundEngine {
                    request,
                    path,
                    defaults: self.prefs.analysis.run_defaults(),
                });
            }
            EngineMessage::ImportScid(path) => {
                let request = self.new_request();
                self.flow.engines.import = Some(request);
                effects.push(Effect::ImportScidEngines { request, path });
            }
            EngineMessage::DismissReports => {
                if let Some(dialog) = self.engine_dialog.as_mut() {
                    dialog.import = None;
                    dialog.found.clear();
                }
                self.engine_issues.clear();
            }
        }
    }

    // ---- the UCI options dialog (D430, D740-D742) ----

    fn open_options(&mut self, id: EngineId, effects: &mut Vec<Effect>) {
        if self.engine_dialog.is_none() {
            return;
        }
        let Some(entry) = self.engines.get(id).cloned() else {
            return self.notice_engine(EngineNotice::NotFound);
        };
        let mut dialog = OptionsDialog::new(&entry);
        // A running engine already told its options: no probe.
        let known = self
            .engine_runs
            .of_engine(id)
            .and_then(|r| r.identity.as_deref().cloned());
        if let Some(identity) = known {
            dialog.set_identity(identity, &entry.options);
        } else if dialog.status == OptionsStatus::Probing {
            let request = self.new_request();
            dialog.request = Some(request);
            effects.push(Effect::TestEngine {
                request,
                draft: Box::new(entry.to_draft()),
                defaults: self.prefs.analysis.run_defaults(),
            });
        }
        if let Some(d) = self.engine_dialog.as_mut() {
            d.options = Some(dialog);
        }
    }

    fn options_probed(&mut self, result: Result<mychess_engine::EngineIdentity, EngineFailure>) {
        let Some(dialog) = self.engine_dialog.as_mut().and_then(|d| d.options.as_mut()) else {
            return;
        };
        dialog.request = None;
        match result {
            Ok(identity) => {
                let saved = self
                    .engines
                    .get(dialog.engine)
                    .map(|e| e.options.clone())
                    .unwrap_or_default();
                dialog.set_identity(identity, &saved);
            }
            Err(failure) => dialog.status = OptionsStatus::Failed(failure),
        }
    }

    fn options_message(&mut self, message: OptionsMessage, effects: &mut Vec<Effect>) {
        let Some(dialog) = self.engine_dialog.as_mut().and_then(|d| d.options.as_mut()) else {
            return;
        };
        match message {
            OptionsMessage::Set { name, value } => dialog.set(&name, &value),
            OptionsMessage::Reset => dialog.reset(),
            OptionsMessage::Close => {
                if let Some(d) = self.engine_dialog.as_mut() {
                    d.options = None;
                }
            }
            OptionsMessage::Press(name) => {
                let engine = dialog.engine;
                let is_button = dialog.identity.as_deref().is_some_and(|i| {
                    i.options.iter().any(|o| {
                        o.name == name && matches!(o.kind, mychess_engine::OptionKind::Button)
                    })
                });
                let run = self
                    .engine_runs
                    .of_engine(engine)
                    .filter(|r| r.state == EngineRunState::Running)
                    .map(|r| r.id);
                if let (true, Some(run)) = (is_button, run) {
                    effects.push(Effect::EngineCommand {
                        run,
                        command: Box::new(EngineCommand::SetOption {
                            name,
                            value: mychess_engine::OptionValue::Bool(true),
                        }),
                        hub: self.engine_hub.clone(),
                    });
                }
            }
            OptionsMessage::Save => self.save_options(effects),
        }
    }

    fn save_options(&mut self, effects: &mut Vec<Effect>) {
        let Some(dialog) = self.engine_dialog.as_mut().and_then(|d| d.options.as_mut()) else {
            return;
        };
        // No form, no save: Enter while probing or after a failure must not
        // close the dialog nor rewrite `engines.toml` (D754).
        if dialog.status != OptionsStatus::Ready {
            return;
        }
        let saved = match dialog.checked() {
            Ok(saved) => saved,
            Err(errors) => {
                dialog.errors = errors;
                return;
            }
        };
        let id = dialog.engine;
        let Some(entry) = self.engines.get(id) else {
            return self.notice_engine(EngineNotice::NotFound);
        };
        // What the engine must be told: options whose effective value
        // changed (saved or default before, the form now).
        let mut changes = Vec::new();
        for option in dialog.visible_options() {
            if matches!(option.kind, mychess_engine::OptionKind::Button) {
                continue;
            }
            let now = dialog
                .value_of(&option.name)
                .and_then(|text| {
                    option
                        .validate(&mychess_engine::OptionValue::Text(text.to_owned()))
                        .ok()
                })
                .unwrap_or_else(|| option_value(&default_text(option)));
            let before = entry
                .options
                .iter()
                .find(|(n, _)| *n == option.name)
                .map_or_else(|| default_text(option), |(_, v)| v.clone());
            let now_text = match &now {
                mychess_engine::OptionValue::Bool(b) => b.to_string(),
                mychess_engine::OptionValue::Int(n) => n.to_string(),
                mychess_engine::OptionValue::Text(t) => t.clone(),
            };
            if now_text != before {
                changes.push((option.name.clone(), now));
            }
        }
        // Saved values of options the engine did not announce this time
        // are kept.
        let mut options = saved;
        for (name, value) in &entry.options {
            if !dialog.values.iter().any(|(n, _)| n == name) {
                options.push((name.clone(), value.clone()));
            }
        }
        if let Err(error) = self.engines.set_options(id, options) {
            return self.notice_list_error(&error);
        }
        if let Some(d) = self.engine_dialog.as_mut() {
            d.options = None;
        }
        self.request_engines_save(effects);
        if let Some(run) = self
            .engine_runs
            .of_engine(id)
            .filter(|r| r.state == EngineRunState::Running)
            .map(|r| r.id)
        {
            for (name, value) in changes {
                effects.push(Effect::EngineCommand {
                    run,
                    command: Box::new(EngineCommand::SetOption { name, value }),
                    hub: self.engine_hub.clone(),
                });
            }
        }
    }

    fn notice_list_error(&mut self, error: &ListError) {
        self.notice_engine(match error {
            ListError::TooManyEngines { .. } => EngineNotice::ListFull,
            ListError::Invalid(_) => EngineNotice::EntryInvalid,
            _ => EngineNotice::NotFound,
        });
    }

    fn select_engine(&mut self, id: EngineId) {
        if let Some(dialog) = self.engine_dialog.as_mut() {
            dialog.selected = Some(id);
        }
    }

    // ---- the file ----

    fn engines_loaded(&mut self, result: Result<LoadedEngineList, StoreError>) {
        self.flow.engines.loading = false;
        match result {
            Ok(loaded) => {
                self.engines = loaded.list;
                if !loaded.issues.is_empty() {
                    self.notice_engine(EngineNotice::FileFixed {
                        count: loaded.issues.len(),
                    });
                }
                self.engine_issues = loaded.issues;
            }
            Err(StoreError::NewerVersion { .. }) => self.notice_engine(EngineNotice::NewerVersion),
            Err(_) => self.notice_engine(EngineNotice::FileUnreadable),
        }
    }

    /// Asks for a save; at most one is in flight and a request made
    /// meanwhile becomes one more save with the newest list (D145, rule 2).
    pub(crate) fn request_engines_save(&mut self, effects: &mut Vec<Effect>) {
        let Some(path) = self.engines_file() else {
            return;
        };
        if self.flow.engines.loading {
            return;
        }
        if self.flow.engines.save_in_flight {
            self.flow.engines.resave = true;
            return;
        }
        self.flow.engines.save_in_flight = true;
        effects.push(Effect::SaveEngines {
            path,
            list: Box::new(self.engines.clone()),
        });
    }

    fn engines_saved(&mut self, result: &Result<(), StoreError>, effects: &mut Vec<Effect>) {
        self.flow.engines.save_in_flight = false;
        if result.is_err() {
            self.notice_engine(EngineNotice::SaveFailed);
        }
        if std::mem::take(&mut self.flow.engines.resave) {
            self.request_engines_save(effects);
        } else if std::mem::take(&mut self.flow.engines.quit_waiting_save) {
            self.finish_quit(effects);
        }
    }

    /// The part of quitting that concerns the engines. Returns whether the
    /// quit must wait (its answer calls `finish_quit` again).
    pub(crate) fn quit_waits_for_engines(&mut self, effects: &mut Vec<Effect>) -> bool {
        if self.flow.engines.save_in_flight {
            self.flow.engines.quit_waiting_save = true;
            return true;
        }
        match self.flow.engines.shutdown {
            // `Done` too: a quit that was aborted (or answered late) may
            // leave an engine alive that the next quit must end.
            Shutdown::Idle | Shutdown::Done if self.engine_runs.any_alive() => {
                self.flow.engines.shutdown = Shutdown::Waiting;
                effects.push(Effect::ShutdownEngines {
                    hub: self.engine_hub.clone(),
                });
                true
            }
            Shutdown::Waiting => true,
            _ => false,
        }
    }

    // ---- delete, options ----

    fn confirm_delete(&mut self, id: EngineId, effects: &mut Vec<Effect>) {
        let Some(dialog) = self.engine_dialog.as_mut() else {
            return;
        };
        if dialog.confirm_delete != Some(id) {
            return;
        }
        dialog.confirm_delete = None;
        // The execution ends first (spec rule 6).
        if let Some(run) = self.engine_runs.of_engine(id).map(|r| r.id) {
            self.stop_run(run, effects);
        }
        let neighbour = self.engines.position(id).and_then(|at| {
            let engines = self.engines.engines();
            engines
                .get(at + 1)
                .or_else(|| at.checked_sub(1).and_then(|p| engines.get(p)))
                .map(|e| e.id)
        });
        if self.engines.remove(id).is_ok() {
            if let Some(dialog) = self.engine_dialog.as_mut()
                && dialog.selected == Some(id)
            {
                dialog.selected = neighbour;
            }
            if self
                .engine_editor
                .as_ref()
                .is_some_and(|e| e.id == Some(id))
            {
                self.engine_editor = None;
            }
            self.request_engines_save(effects);
        }
    }

    fn set_engine_option(
        &mut self,
        id: EngineId,
        name: String,
        value: &str,
        effects: &mut Vec<Effect>,
    ) {
        let Some(entry) = self.engines.get(id) else {
            return self.notice_engine(EngineNotice::NotFound);
        };
        let mut options = entry.options.clone();
        match options.iter_mut().find(|(n, _)| *n == name) {
            Some((_, saved)) => value.clone_into(saved),
            None => options.push((name.clone(), value.to_owned())),
        }
        if let Err(error) = self.engines.set_options(id, options) {
            return self.notice_list_error(&error);
        }
        self.request_engines_save(effects);
        // A running engine takes the option at once.
        if let Some(run) = self
            .engine_runs
            .of_engine(id)
            .filter(|r| r.state == EngineRunState::Running)
            .map(|r| r.id)
        {
            effects.push(Effect::EngineCommand {
                run,
                command: Box::new(EngineCommand::SetOption {
                    name,
                    value: option_value(value),
                }),
                hub: self.engine_hub.clone(),
            });
        }
    }

    // ---- the editor ----

    fn engine_editor_message(&mut self, message: EditorMessage, effects: &mut Vec<Effect>) {
        let Some(editor) = self.engine_editor.as_mut() else {
            return;
        };
        match message {
            EditorMessage::Cancel => self.engine_editor = None,
            EditorMessage::Save => self.save_editor(effects),
            EditorMessage::Check => {
                let command = editor.command.trim().to_owned();
                if command.is_empty() {
                    editor.warnings.clear();
                    return;
                }
                let working_dir = editor.working_dir.trim().to_owned();
                let request = self.new_request();
                if let Some(editor) = self.engine_editor.as_mut() {
                    editor.check_request = Some(request);
                }
                effects.push(Effect::CheckEngineCommand {
                    request,
                    command: PathBuf::from(command),
                    working_dir: (!working_dir.is_empty()).then(|| PathBuf::from(working_dir)),
                });
            }
            EditorMessage::Test | EditorMessage::Detect => {
                let detect = message == EditorMessage::Detect;
                let draft = match editor.draft() {
                    Ok(draft) => draft,
                    // The probe needs only the command; a missing name is
                    // not in its way.
                    Err(EditorError::Invalid(crate::engines::ValidationError::EmptyName)) => {
                        let mut named = editor.clone();
                        "?".clone_into(&mut named.name);
                        match named.draft() {
                            Ok(draft) => draft,
                            Err(error) => {
                                editor.error = Some(error);
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        editor.error = Some(error);
                        return;
                    }
                };
                let request = self.new_request();
                if let Some(editor) = self.engine_editor.as_mut() {
                    editor.error = None;
                    if detect {
                        editor.detect_request = Some(request);
                        editor.detect = Some(DetectState::Running);
                    } else {
                        editor.test_request = Some(request);
                        editor.test = Some(TestState::Running);
                    }
                }
                let defaults = self.prefs.analysis.run_defaults();
                let draft = Box::new(draft);
                effects.push(if detect {
                    Effect::DetectEngine {
                        request,
                        draft,
                        defaults,
                    }
                } else {
                    Effect::TestEngine {
                        request,
                        draft,
                        defaults,
                    }
                });
            }
            EditorMessage::UseDetected => {
                if let Some(DetectState::Found(protocol)) = editor.detect.clone() {
                    editor.set(&EditorMessage::SetProtocol(protocol));
                }
            }
            other => editor.set(&other),
        }
    }

    fn save_editor(&mut self, effects: &mut Vec<Effect>) {
        let Some(editor) = self.engine_editor.as_mut() else {
            return;
        };
        let draft = match editor.draft() {
            Ok(draft) => draft,
            Err(error) => {
                editor.error = Some(error);
                return;
            }
        };
        let (id, hotkey, is_new) = (editor.id, editor.hotkey, editor.id.is_none());
        let result = match id {
            Some(id) => {
                let before = self.engines.get(id).cloned();
                let result = self.engines.edit(id, draft).map(|()| id);
                if result.is_ok() && self.runtime_fields_changed(before.as_ref(), id) {
                    self.notice_engine(EngineNotice::RestartToApply);
                }
                result
            }
            None => self.engines.add(draft),
        };
        let id = match result {
            Ok(id) => id,
            Err(ListError::Invalid(error)) => {
                if let Some(editor) = self.engine_editor.as_mut() {
                    editor.error = Some(EditorError::Invalid(error));
                }
                return;
            }
            Err(error) => {
                self.engine_editor = None;
                return self.notice_list_error(&error);
            }
        };
        self.engines.set_hotkey(id, hotkey).ok();
        if hotkey.is_none() && is_new {
            self.engines.assign_default_hotkeys();
        }
        self.engine_editor = None;
        self.select_engine(id);
        self.request_engines_save(effects);
    }

    /// Whether what a running engine was started with differs from the
    /// entry now, and an execution of it exists.
    fn runtime_fields_changed(&self, before: Option<&EngineEntry>, id: EngineId) -> bool {
        let (Some(before), Some(after)) = (before, self.engines.get(id)) else {
            return false;
        };
        self.engine_runs.of_engine(id).is_some()
            && (before.command != after.command
                || before.args != after.args
                || before.working_dir != after.working_dir
                || before.protocol != after.protocol)
    }

    // ---- finding and importing ----

    fn found(&mut self, request: RequestId, engines: Vec<FoundEngine>) {
        if self.flow.engines.find != Some(request) {
            return;
        }
        self.flow.engines.find = None;
        if let Some(dialog) = self.engine_dialog.as_mut() {
            dialog.finding = false;
            // What is already in the list is not offered again.
            dialog.found = engines
                .into_iter()
                .filter(|f| {
                    !self
                        .engines
                        .engines()
                        .iter()
                        .any(|e| same_path(&e.command, &f.path))
                })
                .collect();
        }
    }

    fn found_probed(
        &mut self,
        request: RequestId,
        path: PathBuf,
        result: Result<(mychess_engine::Protocol, String), EngineFailure>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(at) = self.flow.engines.probes.iter().position(|r| *r == request) else {
            return;
        };
        self.flow.engines.probes.remove(at);
        self.flow
            .engines
            .probing_paths
            .retain(|(r, _)| *r != request);
        if let Some(dialog) = self.engine_dialog.as_mut() {
            dialog.probing = dialog.probing.saturating_sub(1);
        }
        match result {
            Ok(_)
                if self
                    .engines
                    .engines()
                    .iter()
                    .any(|e| same_path(&e.command, &path)) =>
            {
                if let Some(dialog) = self.engine_dialog.as_mut() {
                    dialog.found.retain(|f| !same_path(&f.path, &path));
                }
            }
            Ok((protocol, name)) => {
                let mut draft = EngineDraft::new(name, path.clone());
                draft.protocol = protocol;
                match self.engines.add(draft) {
                    Ok(id) => {
                        self.engines.assign_default_hotkeys();
                        if let Some(dialog) = self.engine_dialog.as_mut() {
                            dialog.found.retain(|f| f.path != path);
                            dialog.selected = Some(id);
                        }
                        self.request_engines_save(effects);
                    }
                    Err(error) => self.notice_list_error(&error),
                }
            }
            Err(failure) => self.notice_engine(EngineNotice::FoundRejected { path, failure }),
        }
    }

    fn scid_read(
        &mut self,
        request: RequestId,
        result: Result<ScidImport, FileError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.engines.import != Some(request) {
            return;
        }
        self.flow.engines.import = None;
        let import = match result {
            Ok(import) => import,
            Err(error) => return self.notice_engine(EngineNotice::ImportFailed(error)),
        };
        if import.engines.is_empty() && import.skipped == 0 {
            return self.notice_engine(EngineNotice::NothingToImport);
        }
        let report: ImportReport = self.engines.import_scid(&import);
        self.notice_engine(EngineNotice::Imported {
            added: report.added.len(),
            duplicates: report.duplicates,
            invalid: report.invalid + report.over_limit,
        });
        if !report.added.is_empty() {
            self.request_engines_save(effects);
        }
        self.engine_dialog
            .get_or_insert_with(EngineDialog::default)
            .import = Some(report);
    }

    // ---- executions ----

    /// Starts the engine, or ends it if it runs (D433).
    fn start_engine(&mut self, id: EngineId, mode: StartMode, effects: &mut Vec<Effect>) {
        let Some(entry) = self.engines.get(id).cloned() else {
            return self.notice_engine(EngineNotice::NotFound);
        };
        if let Some(run) = self.engine_runs.of_engine(id) {
            let run_id = run.id;
            match run.state {
                EngineRunState::Starting | EngineRunState::Running => {
                    return self.stop_run(run_id, effects);
                }
                EngineRunState::Stopping => return,
                EngineRunState::Failed(_) => drop(self.engine_runs.remove(run_id)),
            }
        }
        let defaults = self.prefs.analysis.run_defaults();
        self.begin_run(entry, mode == StartMode::Analyze, defaults, effects);
    }

    /// Adds an execution of `entry` and asks for its start.
    pub(crate) fn begin_run(
        &mut self,
        entry: EngineEntry,
        analyzing: bool,
        defaults: RunDefaults,
        effects: &mut Vec<Effect>,
    ) -> RunId {
        let run = self.engine_runs.begin(entry.id, analyzing);
        if let Some(r) = self.engine_runs.get_mut(run) {
            r.low_priority = defaults.low_priority;
        }
        let count = self.engine_runs.len();
        if count >= MANY_RUNS {
            self.notice_engine(EngineNotice::ManyRunning { count });
        }
        effects.push(Effect::StartEngine {
            run,
            entry: Box::new(entry),
            defaults,
            hub: self.engine_hub.clone(),
        });
        run
    }

    pub(crate) fn stop_run(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        let Some(r) = self.engine_runs.get_mut(run) else {
            return;
        };
        match r.state {
            EngineRunState::Failed(_) => drop(self.engine_runs.remove(run)),
            EngineRunState::Stopping => {}
            EngineRunState::Starting | EngineRunState::Running => {
                r.state = EngineRunState::Stopping;
                r.sent = None;
                effects.push(Effect::StopEngine {
                    run,
                    hub: self.engine_hub.clone(),
                });
            }
        }
    }

    fn engine_started(
        &mut self,
        run: RunId,
        identity: mychess_engine::EngineIdentity,
        at: i64,
        effects: &mut Vec<Effect>,
    ) {
        let hub = self.engine_hub.clone();
        let Some(r) = self.engine_runs.get_mut(run) else {
            // The run was dropped while the engine started: end it.
            effects.push(Effect::StopEngine { run, hub });
            return;
        };
        if r.state != EngineRunState::Starting {
            if r.state == EngineRunState::Stopping {
                effects.push(Effect::StopEngine { run, hub });
            }
            return;
        }
        r.state = EngineRunState::Running;
        r.identity = Some(Box::new(identity));
        let engine = r.engine;
        if let Some(entry) = self.engines.get(engine) {
            let multipv = entry.multipv;
            if multipv > 1 {
                effects.push(Effect::EngineCommand {
                    run,
                    command: Box::new(EngineCommand::SetMultiPv(multipv)),
                    hub,
                });
            }
            self.engines.set_last_used(engine, at).ok();
            self.request_engines_save(effects);
        }
    }

    fn engine_start_failed(&mut self, run: RunId, failure: EngineFailure) {
        let Some(r) = self.engine_runs.get_mut(run) else {
            return;
        };
        match r.state {
            EngineRunState::Starting => {
                r.state = EngineRunState::Failed(failure.clone());
                let name = self.engine_name(run);
                self.notice_engine(EngineNotice::StartFailed { name, failure });
            }
            EngineRunState::Stopping => {
                // A restart (low priority switch) whose old engine never
                // came up: no new run follows, so say why (D446).
                let restarting = self.analysis.restarts.iter().any(|(r, _)| *r == run);
                let name = self.engine_name(run);
                drop(self.engine_runs.remove(run));
                if restarting {
                    self.notice_engine(EngineNotice::StartFailed { name, failure });
                }
            }
            _ => {}
        }
    }

    pub(crate) fn engine_name(&self, run: RunId) -> String {
        self.engine_runs
            .get(run)
            .and_then(|r| self.engines.get(r.engine))
            .map(|e| e.name.clone())
            .unwrap_or_default()
    }

    fn engine_event(&mut self, run: RunId, event: EngineEvent, effects: &mut Vec<Effect>) {
        // The searches of an annotation run are its own business.
        if self.annotate_event(run, &event, effects) {
            return;
        }
        // So are those of the analysis of the positions of an EPD file.
        if self.epd_event(run, &event, effects) {
            return;
        }
        if self.play_event(run, &event, effects) {
            return;
        }
        if self.training_event(run, &event, effects) {
            return;
        }
        let exited = matches!(event, EngineEvent::Exited(_));
        self.engine_event_other(run, event);
        // After the remark about the engine: the annotation's own remark
        // says where it stopped.
        if exited {
            self.annotate_engine_gone(run, false, effects);
            self.epd_engine_gone(run, false, effects);
            self.play_engine_gone(run, effects);
        }
    }

    fn engine_event_other(&mut self, run: RunId, event: EngineEvent) {
        // The contents of a search (lines, best moves) belong to the
        // analysis window (slice 4); here only the end of the session
        // matters.
        let event = match event {
            EngineEvent::Info(info) => return self.analysis_info(run, info),
            EngineEvent::SearchTagged { search, tag } => {
                return self.analysis_search_tagged(run, search, tag);
            }
            EngineEvent::SearchStarted(id) => return self.analysis_search_started(run, id),
            EngineEvent::BestMove(best) => return self.analysis_best_move(run, best),
            other => other,
        };
        let EngineEvent::Exited(info) = event else {
            return;
        };
        let name = self.engine_name(run);
        let Some(r) = self.engine_runs.get_mut(run) else {
            return;
        };
        if r.state == EngineRunState::Stopping {
            return;
        }
        r.sent = None;
        match EngineFailure::from_exit(&info) {
            Some(failure) => {
                r.state = EngineRunState::Failed(failure.clone());
                self.notice_engine(EngineNotice::Crashed { name, failure });
            }
            None => drop(self.engine_runs.remove(run)),
        }
    }

    fn toggle_main_analysis(&mut self, effects: &mut Vec<Effect>) {
        let run = self.engines.engines().iter().find_map(|entry| {
            self.engine_runs
                .of_engine(entry.id)
                .filter(|r| r.state == EngineRunState::Running)
                .map(|r| r.id)
        });
        let Some(run) = run else {
            return self.notice_engine(EngineNotice::NoEngineRunning);
        };
        // The engine of an annotation run is not the board's (spec M4-6),
        // nor is the one of a shoot-out or an automove (spec M4-8).
        if self.annotation_uses(run) || self.training_uses(run) {
            return;
        }
        let hub = self.engine_hub.clone();
        let Some(r) = self.engine_runs.get_mut(run) else {
            return;
        };
        r.analyzing = !r.analyzing;
        if !r.analyzing {
            r.sent = None;
            effects.push(Effect::EngineCommand {
                run,
                command: Box::new(EngineCommand::Stop),
                hub,
            });
        }
        // Turning it on: `refresh_engines` sends the position.
    }

    // ---- the position sent to the engines (D436) ----

    /// The path from the start of the game to the cursor, variations
    /// included. Reused while neither the game nor the cursor changed.
    pub(crate) fn analysis_target(&mut self) -> Option<Arc<AnalysisTarget>> {
        let key = (self.session.changes(), self.session.cursor());
        if let Some((cached, target)) = &self.flow.engines.target
            && *cached == key
        {
            return Some(Arc::clone(target));
        }
        let game = self.session.game();
        let path = game.path_to(self.session.cursor()).ok()?;
        let moves = path
            .iter()
            .map(|node| game.mv(*node))
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let target = Arc::new(AnalysisTarget {
            start: game.start_position().clone(),
            moves,
        });
        self.flow.engines.target = Some((key, Arc::clone(&target)));
        Some(target)
    }

    /// Sends the position shown to every engine that follows the board and
    /// has not got it yet; another game is announced first (`NewGame`).
    /// Called after every `update`: with no engine analysing it only looks
    /// at the executions.
    pub(crate) fn refresh_engines(&mut self, effects: &mut Vec<Effect>) {
        self.refresh_analysis();
        // The game against an engine owns the engines meanwhile (D1241): an
        // analysis turned on now starts when the game is over.
        if self.play_live() || self.assist_locked() {
            return;
        }
        if !self.engine_runs.as_slice().iter().any(EngineRun::is_active) {
            return;
        }
        let Some(target) = self.analysis_target() else {
            return;
        };
        let generation = self.session.generation();
        let hub = self.engine_hub.clone();
        let mut due = Vec::new();
        for run in self.engine_runs.iter_mut().filter(|r| r.is_active()) {
            if let Some((sent, game)) = &run.sent
                && *game == generation
                && (Arc::ptr_eq(sent, &target) || **sent == *target)
            {
                continue;
            }
            if run.seen_game.is_some_and(|game| game != generation) {
                effects.push(Effect::EngineCommand {
                    run: run.id,
                    command: Box::new(EngineCommand::NewGame),
                    hub: hub.clone(),
                });
            }
            run.seen_game = Some(generation);
            run.sent = Some((Arc::clone(&target), generation));
            due.push(run.id);
        }
        for run in due {
            self.send_search(run, &target, effects);
        }
    }

    /// F2-F4 (`Action::StartEngine`): the key of `number` (1 to 3).
    pub(crate) fn engine_key_action(&mut self, number: u8, effects: &mut Vec<Effect>) {
        let key = usize::from(number)
            .checked_sub(1)
            .and_then(|i| Hotkey::ALL.get(i).copied());
        if let Some(key) = key {
            self.engine_message(EngineMessage::ToggleHotkey(key), effects);
        }
    }
}
