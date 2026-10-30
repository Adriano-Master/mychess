//! The part of the shell that serves the EPD panel (spec M7-5c): the events
//! of its fields and forms, the pane that opens when a file arrives, the
//! editor that follows the selected record, the list that scrolls to it and
//! the question on quitting with unsaved files.

use std::hash::{BuildHasher, Hasher};

use iced::Task;
use iced::widget::scrollable::AbsoluteOffset;
use mychess_app::epd_window::EpdMessage;

use super::{Message, Shell};
use crate::windows::epd::{
    AnalyzeDraft, EPD_FIELD_IDS, EPD_LIST_ID, EpdForm, EpdUiMessage, active_tab, set_prefs,
};
use crate::windows::layout::PaneKind;

/// A number for "random position" that no one can predict (no dependency).
fn random_seed() -> u64 {
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}

impl Shell {
    /// Whether a text field of the EPD panel has the focus.
    pub(super) fn epd_field_focused(&self) -> bool {
        self.ui.dialogs.epd.focused.is_some() && self.ui.is_visible(PaneKind::Epd)
    }

    /// Whether a form or question of the EPD window covers the window.
    pub(super) fn epd_layer_open(&self) -> bool {
        self.ui.dialogs.epd.layer_open(&self.app)
    }

    /// Asks the widgets which text field of the panel has the focus.
    pub(super) fn probe_epd_focus() -> Task<Message> {
        Task::batch(EPD_FIELD_IDS.into_iter().map(|id| {
            iced::widget::operation::is_focused(id)
                .map(move |on| Message::Epd(EpdUiMessage::Focus(id, on)))
        }))
    }

    /// Esc closes the form that is open; the questions of the application
    /// are answered "no". Returns `None` when no layer of the window is open.
    pub(super) fn epd_escape(&mut self) -> Option<Task<Message>> {
        let view = self.app.epd_view();
        if view.close_prompt.is_some() {
            return Some(
                self.dispatch(mychess_app::Message::Epd(EpdMessage::AnswerClose(
                    mychess_app::epd_window::EpdCloseChoice::Cancel,
                ))),
            );
        }
        if view.tabs.iter().any(|t| t.pending_remove.is_some()) {
            return Some(
                self.dispatch(mychess_app::Message::Epd(EpdMessage::ConfirmRemove(false))),
            );
        }
        let ui = &mut self.ui.dialogs.epd;
        let form = ui.form.take()?;
        // The form that was open when the quit question came is back.
        if form == EpdForm::ConfirmQuit {
            ui.form = ui.form_before_quit.take();
        }
        ui.focused = None;
        Some(Task::none())
    }

    /// Asks about unsaved EPD files before quitting. `true`: the question
    /// is open and the quit waits for the answer.
    pub(super) fn epd_quit_pending(&mut self) -> bool {
        if self.ui.dialogs.epd.quit_confirmed {
            return false;
        }
        // An unconfirmed draft of the editor and a batch edit in flight
        // count too: they would be lost (D3860). The draft is not written
        // here, so "keep editing" finds the editor as it was.
        if self.app.epd_unsaved_files().is_empty() {
            return false;
        }
        let ui = &mut self.ui.dialogs.epd;
        // A text form that was open (the opcode to strip) comes back after
        // the answer "no" instead of losing what was typed (D3864).
        if ui.form != Some(EpdForm::ConfirmQuit) {
            ui.form_before_quit = ui.form.take();
        }
        ui.form = Some(EpdForm::ConfirmQuit);
        ui.focused = None;
        true
    }

    /// Brings the panel up to date with the application: the pane opens
    /// when a file arrived, the editor shows the selected record and the
    /// list scrolls to it.
    pub(super) fn sync_epd(&mut self) -> Task<Message> {
        // A form that closed (by the keyboard, too) took its field with
        // it: the focus must not stay "in a text field" (D3863).
        // A report of a file that is gone draws no card but would still
        // hold the keyboard: it is dropped (D3869).
        let view = self.app.epd_view();
        let stale =
            |id: &mychess_app::epd_window::EpdFileId| !view.tabs.iter().any(|t| t.id == *id);
        if let Some(EpdForm::Report(id)) = &self.ui.dialogs.epd.form
            && stale(id)
        {
            self.ui.dialogs.epd.form = None;
        }
        if let Some(EpdForm::Report(id)) = &self.ui.dialogs.epd.form_before_quit
            && stale(id)
        {
            self.ui.dialogs.epd.form_before_quit = None;
        }
        let open = self.ui.dialogs.epd.form.is_some();
        if self.ui.dialogs.epd.form_was_open && !open {
            self.ui.dialogs.epd.focused = None;
        }
        self.ui.dialogs.epd.form_was_open = open;
        let mut tasks = Vec::new();
        let count = view.tabs.len();
        if count > self.ui.dialogs.epd.seen_tabs {
            tasks.push(self.change_panes(|ui| ui.open_pane(PaneKind::Epd)));
        }
        self.ui.dialogs.epd.seen_tabs = count;
        let ui = &mut self.ui.dialogs.epd;
        let Some(tab) = active_tab(&view) else {
            if !ui.content.text().is_empty() {
                ui.set_content("");
            }
            ui.revealed = None;
            return Task::batch(tasks);
        };
        if ui.content.text() != tab.editor_text {
            ui.set_content(&tab.editor_text);
        }
        // The selected row is brought into view once per change.
        let key = tab.selected_row.map(|row| (tab.id, row));
        if key != ui.revealed {
            ui.revealed = key;
            if let Some((_, row)) = key {
                let top = crate::windows::epd::row_top(row);
                let bottom = top + crate::windows::epd::ROW_HEIGHT;
                let height = if ui.view_h > 0.0 { ui.view_h } else { 400.0 };
                if top < ui.scroll || bottom > ui.scroll + height {
                    tasks.push(iced::widget::operation::scroll_to(
                        EPD_LIST_ID,
                        AbsoluteOffset {
                            x: None,
                            y: Some((top - height / 2.0).max(0.0)),
                        },
                    ));
                }
            }
        }
        Task::batch(tasks)
    }

    /// An event of the EPD panel that only the front-end handles.
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(super) fn update_epd(&mut self, message: EpdUiMessage) -> Task<Message> {
        match message {
            EpdUiMessage::Editor(action) => {
                let view = self.app.epd_view();
                let enabled = active_tab(&view).is_some_and(|t| t.editor_enabled);
                let edit = action.is_edit();
                if edit && !enabled {
                    return Task::none();
                }
                self.ui.dialogs.epd.content.perform(action);
                if edit {
                    let text = self.ui.dialogs.epd.content.text();
                    return self.dispatch(mychess_app::Message::Epd(EpdMessage::Edit(text)));
                }
                Task::none()
            }
            EpdUiMessage::Scrolled { offset, height } => {
                let ui = &mut self.ui.dialogs.epd;
                ui.scroll = if offset.is_finite() {
                    offset.max(0.0)
                } else {
                    0.0
                };
                ui.view_h = if height.is_finite() {
                    height.max(0.0)
                } else {
                    0.0
                };
                Task::none()
            }
            EpdUiMessage::FindInput(text) => {
                self.ui.dialogs.epd.find_input = text;
                Task::none()
            }
            EpdUiMessage::Find(forward) => {
                let text = self.ui.dialogs.epd.find_input.clone();
                if text.is_empty() {
                    return Task::none();
                }
                self.dispatch(mychess_app::Message::Epd(EpdMessage::Find(text, forward)))
            }
            EpdUiMessage::Random => {
                self.dispatch(mychess_app::Message::Epd(EpdMessage::Random(random_seed())))
            }
            EpdUiMessage::OpenAnalyze => {
                let draft = AnalyzeDraft::from_prefs(&self.app.prefs.epd);
                self.ui.dialogs.epd.form = Some(EpdForm::Analyze(draft));
                iced::widget::operation::focus(crate::windows::epd::EPD_SECONDS_ID)
            }
            EpdUiMessage::OpenStrip => {
                self.ui.dialogs.epd.form = Some(EpdForm::Strip(String::new()));
                iced::widget::operation::focus(crate::windows::epd::EPD_STRIP_ID)
            }
            EpdUiMessage::OpenReport => {
                let view = self.app.epd_view();
                if let Some(tab) = active_tab(&view) {
                    self.ui.dialogs.epd.form = Some(EpdForm::Report(tab.id));
                }
                Task::none()
            }
            EpdUiMessage::CloseForm => {
                self.ui.dialogs.epd.form = None;
                self.ui.dialogs.epd.focused = None;
                Task::none()
            }
            EpdUiMessage::AnalyzeEngine(engine) => {
                if let Some(EpdForm::Analyze(draft)) = &mut self.ui.dialogs.epd.form {
                    draft.engine = engine;
                }
                Task::none()
            }
            EpdUiMessage::AnalyzeSeconds(text) => {
                if let Some(EpdForm::Analyze(draft)) = &mut self.ui.dialogs.epd.form {
                    draft.seconds = text.chars().filter(char::is_ascii_digit).take(3).collect();
                }
                Task::none()
            }
            EpdUiMessage::AnalyzeMode(mode) => {
                if let Some(EpdForm::Analyze(draft)) = &mut self.ui.dialogs.epd.form {
                    draft.mode = mode;
                }
                Task::none()
            }
            EpdUiMessage::AnalyzeScope(scope) => {
                if let Some(EpdForm::Analyze(draft)) = &mut self.ui.dialogs.epd.form {
                    draft.scope = scope;
                }
                Task::none()
            }
            EpdUiMessage::AnalyzeStart => {
                let Some(EpdForm::Analyze(draft)) = &self.ui.dialogs.epd.form else {
                    return Task::none();
                };
                let Some(config) = draft.config() else {
                    return Task::none();
                };
                let view = self.app.epd_view();
                let undo = active_tab(&view).is_some_and(|t| t.can_undo_run);
                if undo && self.app.prefs.epd.confirm_run_undo {
                    self.ui.dialogs.epd.form = Some(EpdForm::ConfirmUndo(config));
                    return Task::none();
                }
                self.start_analysis(config)
            }
            EpdUiMessage::ConfirmUndo(yes) => {
                let Some(EpdForm::ConfirmUndo(config)) = self.ui.dialogs.epd.form.clone() else {
                    return Task::none();
                };
                if yes {
                    self.start_analysis(config)
                } else {
                    self.ui.dialogs.epd.form = None;
                    self.ui.dialogs.epd.focused = None;
                    Task::none()
                }
            }
            EpdUiMessage::StripInput(text) => {
                if let Some(EpdForm::Strip(opcode)) = &mut self.ui.dialogs.epd.form {
                    *opcode = text;
                }
                Task::none()
            }
            EpdUiMessage::StripGo => {
                let Some(EpdForm::Strip(opcode)) = self.ui.dialogs.epd.form.take() else {
                    return Task::none();
                };
                self.ui.dialogs.epd.focused = None;
                let opcode = opcode.trim().to_owned();
                if opcode.is_empty() {
                    return Task::none();
                }
                self.dispatch(mychess_app::Message::Epd(EpdMessage::StripOpcode(opcode)))
            }
            EpdUiMessage::SetFlip(on) => {
                let mut prefs = (*self.app.prefs.epd).clone();
                prefs.flip_to_side_to_move = on;
                self.dispatch(set_prefs(prefs))
            }
            EpdUiMessage::SetDialectScid(on) => {
                let mut prefs = (*self.app.prefs.epd).clone();
                prefs.dialect = if on {
                    mychess_app::epd_window::Dialect::Scid
                } else {
                    mychess_app::epd_window::Dialect::Standard
                };
                self.dispatch(set_prefs(prefs))
            }
            EpdUiMessage::Focus(id, on) => {
                let ui = &mut self.ui.dialogs.epd;
                if on {
                    ui.focused = Some(id);
                    return Task::none();
                }
                if ui.focused != Some(id) {
                    return Task::none();
                }
                ui.focused = None;
                // Leaving the editor writes the text into the record.
                if id == crate::windows::epd::EPD_EDITOR_ID {
                    let view = self.app.epd_view();
                    if active_tab(&view).is_some_and(|t| t.editor_draft) {
                        return self.dispatch(mychess_app::Message::Epd(EpdMessage::Commit));
                    }
                }
                Task::none()
            }
            EpdUiMessage::ConfirmQuit(yes) => {
                let ui = &mut self.ui.dialogs.epd;
                ui.form = None;
                ui.focused = None;
                if !yes {
                    ui.form = ui.form_before_quit.take();
                    return Task::none();
                }
                ui.form_before_quit = None;
                // The answer holds for this request only: when the quit
                // does not go on (another question, a cancel), the next
                // request asks again (D3859).
                ui.quit_confirmed = true;
                let task = self.request_quit();
                self.ui.dialogs.epd.quit_confirmed = false;
                task
            }
        }
    }

    /// Remembers the choices of the form in the preferences and starts.
    fn start_analysis(&mut self, config: mychess_app::epd_window::AnnotateConfig) -> Task<Message> {
        self.ui.dialogs.epd.form = None;
        self.ui.dialogs.epd.focused = None;
        let mut prefs = (*self.app.prefs.epd).clone();
        prefs.annotate_seconds = config.seconds;
        prefs.annotate_mode = config.mode;
        prefs.annotate_engine = config.engine.map(|e| e.0);
        let mut tasks = vec![self.dispatch(set_prefs(prefs))];
        tasks.push(self.dispatch(mychess_app::Message::Epd(EpdMessage::Annotate(config))));
        Task::batch(tasks)
    }

    /// Runs the actions of the EPD navigation keys and the pane toggle.
    pub(super) fn epd_action(&mut self, action: mychess_app::Action) -> Option<Task<Message>> {
        use mychess_app::Action;
        let message = match action {
            Action::ToggleEpdPane => {
                return Some(self.change_panes(|ui| ui.toggle_pane(PaneKind::Epd)));
            }
            Action::EpdFirst => EpdMessage::First,
            Action::EpdPrev => EpdMessage::Prev,
            Action::EpdNext => EpdMessage::Next,
            Action::EpdLast => EpdMessage::Last,
            _ => return None,
        };
        // The text editor uses the same keys to select; with the panel
        // closed the keys do nothing.
        if self.epd_field_focused() || !self.ui.is_visible(PaneKind::Epd) {
            return Some(Task::none());
        }
        Some(self.dispatch(mychess_app::Message::Epd(message)))
    }
}
