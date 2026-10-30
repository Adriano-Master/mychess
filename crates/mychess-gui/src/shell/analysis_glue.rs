//! What the shell does for the analysis panel (spec M4-4): the messages
//! that need the front-end (modifier keys, file dialog, widget focus), the
//! keys of the panel (D448) and the clock of the open logs.

use iced::Task;
use mychess_app::engines::RunId;
use mychess_app::{AnalysisMessage, KeyInput, Message as AppMessage, ModeMessage};

use super::{Message, Shell};
use crate::app_api;
use crate::windows::analysis::{
    self, AnalysisUiMessage, EXCLUDE_INPUT_ID, LOG_SEARCH_ID, TRAINING_INPUT_ID, all_lines_mode,
    key_message_typed, parse_training_seconds, pv_prefix, run_of,
};
use crate::windows::layout::PaneKind;

fn analysis(message: AnalysisMessage) -> AppMessage {
    AppMessage::Analysis(message)
}

impl Shell {
    /// Handles a message of the analysis panel that only the front-end can
    /// finish.
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(super) fn update_analysis_ui(&mut self, message: AnalysisUiMessage) -> Task<Message> {
        match message {
            AnalysisUiMessage::Hover(on) => {
                self.ui.analysis.hovered = on;
                Task::none()
            }
            AnalysisUiMessage::AddAll { run, secondary } => {
                self.ui.analysis.focus = Some(run);
                let mode = all_lines_mode(secondary, self.ui.games.command);
                self.dispatch(analysis(AnalysisMessage::AddAllLines(run, mode)))
            }
            AnalysisUiMessage::OpenExclude(run) => {
                self.ui.analysis.focus = Some(run);
                self.ui.analysis.exclude_text = self
                    .app
                    .analysis_view(run)
                    .map(|v| v.exclusion_san.join(" "))
                    .unwrap_or_default();
                self.ui.analysis.focused_field = Some(EXCLUDE_INPUT_ID);
                Task::batch([
                    self.dispatch(analysis(AnalysisMessage::OpenExclude(run))),
                    iced::widget::operation::focus(EXCLUDE_INPUT_ID),
                ])
            }
            AnalysisUiMessage::ExcludeInput(text) => {
                self.ui.analysis.exclude_text = text;
                Task::none()
            }
            AnalysisUiMessage::ExcludeSubmit(run) => {
                let text = std::mem::take(&mut self.ui.analysis.exclude_text);
                self.release_field(EXCLUDE_INPUT_ID);
                self.dispatch(analysis(AnalysisMessage::SetExclusion(run, text)))
            }
            AnalysisUiMessage::OpenTraining(run) => {
                self.ui.analysis.focus = Some(run);
                let seconds = self.app.automove_time().as_secs_f64();
                self.ui.analysis.training = Some((run, format!("{seconds:.1}")));
                self.ui.analysis.focused_field = Some(TRAINING_INPUT_ID);
                iced::widget::operation::focus(TRAINING_INPUT_ID)
            }
            AnalysisUiMessage::TrainingInput(text) => {
                if let Some((_, draft)) = self.ui.analysis.training.as_mut() {
                    *draft = text;
                }
                Task::none()
            }
            AnalysisUiMessage::TrainingSubmit(run) => {
                let Some((_, draft)) = self.ui.analysis.training.as_ref() else {
                    return Task::none();
                };
                // Enter is always captured by the field: an invalid text is
                // ignored and the box stays open (D921).
                let Some(time) = parse_training_seconds(draft) else {
                    return Task::none();
                };
                // The training may have become unavailable while the box was
                // open (the engine stopped, a dialog or a mode took over):
                // the box closes and nothing is sent (D923).
                if !self.app.mode_availability(run).training {
                    self.close_training_box();
                    return Task::none();
                }
                self.close_training_box();
                Task::batch([
                    self.dispatch(AppMessage::Mode(ModeMessage::SetAutomoveTime(run, time))),
                    self.dispatch(AppMessage::Mode(ModeMessage::ToggleAutomove(run))),
                ])
            }
            AnalysisUiMessage::TrainingCancel => {
                self.close_training_box();
                Task::none()
            }
            // Only captures the Enter of the log filter.
            AnalysisUiMessage::LogFilterSubmit => Task::none(),
            AnalysisUiMessage::FieldFocus(field, on) => {
                if on {
                    self.ui.analysis.focused_field = Some(field);
                } else {
                    self.release_field(field);
                }
                Task::none()
            }
            AnalysisUiMessage::LogAuto(on) => {
                self.ui.analysis.log_auto = on;
                Task::none()
            }
            AnalysisUiMessage::LogTickAll => {
                let runs: Vec<RunId> = self
                    .app
                    .analysis
                    .as_slice()
                    .iter()
                    .filter(|v| v.log_open)
                    .map(|v| v.run)
                    .collect();
                let tasks: Vec<_> = runs
                    .into_iter()
                    .map(|run| self.dispatch(analysis(AnalysisMessage::LogTick(run))))
                    .collect();
                Task::batch(tasks)
            }
            AnalysisUiMessage::PickLogFile(run) => {
                self.ui.analysis.focus = Some(run);
                let dialog = crate::host::LogFileDialog::new(&self.i18n);
                Task::perform(dialog.run(), move |path| {
                    Message::Ui(super::UiMessage::Analysis(
                        AnalysisUiMessage::LogFilePicked(run, path),
                    ))
                })
            }
            AnalysisUiMessage::LogFilePicked(run, path) => match path {
                Some(path) => self.dispatch(analysis(AnalysisMessage::SaveLog(run, path))),
                None => Task::none(),
            },
            AnalysisUiMessage::PlayPv { run, line, index } => self.play_pv(run, line, index),
        }
    }

    /// Plays the moves of a PV up to the one clicked (D804). Nothing happens
    /// when the lines are about another position than the board's (a locked
    /// analysis, or lines that crossed a change of position).
    fn play_pv(&mut self, run: RunId, line: u8, index: usize) -> Task<Message> {
        self.ui.analysis.focus = Some(run);
        // The game against an engine locks the game: nothing is played
        // from a line (D1484).
        if self.app.play_live() {
            return Task::none();
        }
        let Some(view) = self.app.analysis_view(run) else {
            return Task::none();
        };
        let Some(entry) = view.lines.iter().find(|l| l.multipv == line) else {
            return Task::none();
        };
        let shown = app_api::board_view(&self.app).position.zobrist();
        if view.position().map(mychess_core::Position::zobrist) != Some(shown) {
            return Task::none();
        }
        let moves = pv_prefix(&entry.pv, index).to_vec();
        let tasks: Vec<_> = moves
            .into_iter()
            .map(|mv| self.dispatch(app_api::play_move(mv)))
            .collect();
        Task::batch(tasks)
    }

    /// The execution the keys of the panel act on: the section used last,
    /// else the first one.
    fn analysis_target_run(&self) -> Option<RunId> {
        let runs = self.app.engine_runs();
        self.ui
            .analysis
            .focus
            .filter(|f| runs.iter().any(|r| r.id == *f))
            .or_else(|| runs.first().map(|r| r.id))
    }

    /// The keys of the panel (`Enter`, `Ctrl+Enter`, `Space`, `1`-`5`): only
    /// with the pointer over the panel, no text field focused (the widget
    /// captured the key, or one of the fields of the panel has the focus,
    /// which `Ctrl+Enter` and others do not tell) and no layer open over the
    /// window ([`Shell::modal_layer_open`]), so the move entry of the board
    /// keeps `Enter` (D448, D811, D921). `typed` is the character of the
    /// key press: the digits follow it, not the key code (D922).
    pub(super) fn analysis_key(
        &mut self,
        input: &KeyInput,
        text_focused: bool,
        typed: Option<char>,
    ) -> Option<Task<Message>> {
        if !self.ui.is_visible(PaneKind::Analysis)
            || text_focused
            || self.ui.analysis.field_focused()
            || !self.ui.analysis.hovered
            || self.modal_layer_open()
        {
            return None;
        }
        let run = self.analysis_target_run()?;
        let max = self
            .app
            .engine_runs()
            .iter()
            .find(|r| r.id == run)
            .and_then(analysis::max_lines);
        let message = key_message_typed(input, typed, run, max)?;
        let pending = self.app.nag_sequence_deadline().is_some();
        // A half-typed NAG sequence owns the digits (D811, D923); any other
        // key of the panel ends it first, as the other keys do.
        if pending && matches!(message, AnalysisMessage::SetMultiPv(..)) {
            return None;
        }
        let mut tasks = vec![];
        if pending {
            tasks.push(self.dispatch(AppMessage::NagInterrupt));
        }
        tasks.push(self.dispatch(analysis(message)));
        Some(Task::batch(tasks))
    }

    /// `Esc` over the inline boxes of the panel: the training box and the
    /// exclusion box close (nothing is sent), and the log filter lets go of
    /// the focus (the widget does it itself). Returns whether the key was
    /// spent (D921).
    pub(super) fn analysis_escape(&mut self) -> Option<Task<Message>> {
        if !self.ui.is_visible(PaneKind::Analysis) {
            return None;
        }
        if self.ui.analysis.training.is_some() {
            self.close_training_box();
            return Some(Task::none());
        }
        let open = self
            .app
            .analysis
            .as_slice()
            .iter()
            .find(|v| v.exclude_open)
            .map(|v| v.run);
        if let Some(run) = open {
            self.release_field(EXCLUDE_INPUT_ID);
            return Some(self.dispatch(analysis(AnalysisMessage::CloseExclude(run))));
        }
        if self.ui.analysis.focused_field == Some(LOG_SEARCH_ID) {
            self.ui.analysis.focused_field = None;
            return Some(Task::none());
        }
        None
    }

    /// Whether an inline box of the panel is open (training or exclusion).
    pub(super) fn analysis_box_open(&self) -> bool {
        self.ui.is_visible(PaneKind::Analysis)
            && (self.ui.analysis.training.is_some()
                || self.app.analysis.as_slice().iter().any(|v| v.exclude_open))
    }

    fn close_training_box(&mut self) {
        self.ui.analysis.training = None;
        self.release_field(TRAINING_INPUT_ID);
    }

    /// The field is no longer focused (only if it was the one remembered).
    fn release_field(&mut self, field: &'static str) {
        if self.ui.analysis.focused_field == Some(field) {
            self.ui.analysis.focused_field = None;
        }
    }

    /// Closes the training box when the training stopped being available.
    pub(super) fn drop_stale_training_box(&mut self) {
        let stale = self
            .ui
            .analysis
            .training
            .as_ref()
            .is_some_and(|(run, _)| !self.app.mode_availability(*run).training);
        if stale {
            self.close_training_box();
        }
    }

    /// Asks the widgets which text field of the panel has the focus. Run
    /// after every click (the focus changes by clicking and `text_input` has
    /// no "focus lost" event).
    pub(super) fn probe_analysis_focus(&self) -> Task<Message> {
        if !self.ui.is_visible(PaneKind::Analysis) {
            return Task::none();
        }
        let tasks: Vec<_> = analysis::AnalysisUi::FIELD_IDS
            .into_iter()
            .map(|id| {
                iced::widget::operation::is_focused(id).map(move |on| {
                    Message::Ui(super::UiMessage::Analysis(AnalysisUiMessage::FieldFocus(
                        id, on,
                    )))
                })
            })
            .collect();
        Task::batch(tasks)
    }

    /// Remembers the section a message came from.
    pub(super) fn note_analysis_message(&mut self, message: &AppMessage) {
        if let AppMessage::Analysis(message) = message
            && let Some(run) = run_of(message)
            && !matches!(message, AnalysisMessage::LogTick(_))
        {
            self.ui.analysis.focus = Some(run);
        }
        match message {
            // Typing in the log filter: it has the focus.
            AppMessage::Analysis(AnalysisMessage::SetLogFilter(..)) => {
                self.ui.analysis.focused_field = Some(LOG_SEARCH_ID);
            }
            AppMessage::Analysis(AnalysisMessage::CloseExclude(_)) => {
                self.release_field(EXCLUDE_INPUT_ID);
            }
            _ => {}
        }
    }

    /// Opens the panel when an engine starts and it is closed: otherwise
    /// the user would start an engine and see nothing.
    pub(super) fn reveal_analysis_for_new_run(&mut self, runs_before: usize) -> Task<Message> {
        if self.app.engine_runs().len() > runs_before && !self.ui.is_open(PaneKind::Analysis) {
            self.ui.toggle_pane(PaneKind::Analysis);
            return self.remember_layout();
        }
        Task::none()
    }

    /// Whether a log is open and refreshes itself (the 250 ms clock runs
    /// only then).
    pub fn analysis_log_ticking(&self) -> bool {
        self.ui.analysis.log_auto
            && self.ui.is_open(PaneKind::Analysis)
            && self.app.analysis.as_slice().iter().any(|v| v.log_open)
    }
}

#[cfg(test)]
mod tests {
    use mychess_app::{Key, Modifiers, NamedKey};

    use super::*;
    use crate::shell::UiMessage;

    fn key(key: Key) -> KeyInput {
        KeyInput {
            key,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn the_panel_opens_closes_and_survives_other_panels_changing() {
        let mut shell = Shell::new(None);
        let toggle = |shell: &mut Shell, kind| {
            drop(shell.update(Message::Ui(UiMessage::TogglePane(kind))));
        };
        assert!(!shell.ui.is_open(PaneKind::Analysis));
        toggle(&mut shell, PaneKind::Analysis);
        assert!(shell.ui.is_open(PaneKind::Analysis));
        assert!(shell.ui.layout().panes().contains(&PaneKind::Analysis));
        // Another panel opening rebuilds the arrangement: the analysis stays.
        toggle(&mut shell, PaneKind::Info);
        assert!(shell.ui.layout().panes().contains(&PaneKind::Analysis));
        assert!(shell.ui.layout().is_valid());
        toggle(&mut shell, PaneKind::Analysis);
        assert!(!shell.ui.is_open(PaneKind::Analysis));
        assert!(!shell.ui.layout().panes().contains(&PaneKind::Analysis));
        // The menu action does the same.
        drop(shell.update(Message::Ui(UiMessage::Action(
            mychess_app::Action::ToggleAnalysisPane,
        ))));
        assert!(shell.ui.is_open(PaneKind::Analysis));
        drop(shell.update(Message::Ui(UiMessage::ResetLayout)));
        assert!(
            !shell.ui.is_open(PaneKind::Analysis),
            "the default has no analysis"
        );
    }

    #[test]
    fn a_saved_layout_with_the_panel_opens_it() {
        let mut shell = Shell::new(None);
        let layout = LayoutNodeOf::default_layout().open(PaneKind::Analysis);
        shell.ui.set_layout(&layout);
        assert!(shell.ui.is_open(PaneKind::Analysis));
    }

    type LayoutNodeOf = crate::windows::layout::LayoutNode;

    #[test]
    fn the_keys_of_the_panel_need_the_pointer_over_it_and_a_running_engine() {
        let mut shell = Shell::new(None);
        shell.ui.toggle_pane(PaneKind::Analysis);
        let enter = key(Key::Named(NamedKey::Enter));
        assert!(
            shell.analysis_key(&enter, false, None).is_none(),
            "the pointer is not over the panel"
        );
        shell.ui.analysis.hovered = true;
        assert!(
            shell.analysis_key(&enter, false, None).is_none(),
            "no engine is running: Enter stays with the move entry"
        );
        assert!(
            shell.analysis_key(&enter, true, None).is_none(),
            "a text field"
        );
    }

    #[test]
    fn the_log_clock_runs_only_with_an_open_log() {
        let mut shell = Shell::new(None);
        assert!(!shell.analysis_log_ticking());
        shell.ui.toggle_pane(PaneKind::Analysis);
        assert!(!shell.analysis_log_ticking(), "no log is open");
        shell.ui.analysis.log_auto = false;
        assert!(!shell.analysis_log_ticking());
    }

    #[test]
    fn a_message_of_a_section_remembers_it_for_the_keys() {
        let mut shell = Shell::new(None);
        shell.ui.games.command = true;
        let before = shell.app.engine_runs().len();
        // No such run: the message goes to the application, which ignores it.
        drop(shell.update_analysis_ui(AnalysisUiMessage::AddAll {
            run: RunId(99),
            secondary: false,
        }));
        assert_eq!(shell.app.engine_runs().len(), before);
        assert_eq!(shell.ui.analysis.focus, Some(RunId(99)));
    }
}
