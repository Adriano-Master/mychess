//! What the shell does for the notation panel: runs the outcome of its
//! messages and saves the comment draft before the game changes under it
//! (spec M2-4, D178).

use iced::Task;
use mychess_app::Message as AppMessage;

use super::ui_state::UiState;
use super::{Message, Shell};
use crate::app_api;
use crate::windows::comment_editor::{EDITOR_ID, EditorMessage};
use crate::windows::layout::PaneKind;
use crate::windows::notation::{NotationMessage, Outcome};

impl Shell {
    /// Handles a message of the notation panel.
    pub(super) fn update_notation(&mut self, message: NotationMessage) -> Task<Message> {
        let out = self.ui.notation.update(message, &self.app);
        self.run_notation_outcome(out)
    }

    fn run_notation_outcome(&mut self, out: Outcome) -> Task<Message> {
        let mut tasks = Vec::new();
        for message in out.messages {
            tasks.push(self.dispatch(message));
        }
        if let Some(notice) = out.notice {
            self.ui.status_notice = Some(notice);
            self.ui.dialogs.status.reset();
        }
        if out.focus_editor {
            // The editor needs its panel on screen (the user may have
            // closed it, or have another tab selected).
            if !self.ui.comment_visible() {
                self.ui.show_pane(PaneKind::Comment);
                tasks.push(self.sync_comment_editor());
                tasks.push(self.remember_layout());
            }
            tasks.push(iced::widget::operation::focus(EDITOR_ID));
        }
        if out.poll_editor_focus {
            tasks.push(
                iced::widget::operation::is_focused(EDITOR_ID).map(|focused| {
                    Message::Notation(NotationMessage::Editor(EditorMessage::FocusChecked(
                        focused,
                    )))
                }),
            );
        }
        Task::batch(tasks)
    }

    /// Changes the arrangement of the panels with `change` and keeps what
    /// depends on it: the comment draft is saved when the editor goes out of
    /// sight, the editor opens when its panel comes into sight, and the
    /// arrangement is remembered (spec M2-6, D1003, D1007).
    pub(super) fn change_panes(&mut self, change: impl FnOnce(&mut UiState)) -> Task<Message> {
        change(&mut self.ui);
        let mut tasks = vec![self.sync_comment_editor()];
        tasks.push(self.remember_layout());
        Task::batch(tasks)
    }

    /// Brings the comment editor up to date with the arrangement: the draft
    /// is saved when the editor went out of sight, and the editor opens when
    /// its panel came into sight (D1027). Safe to call after any change.
    pub(super) fn sync_comment_editor(&mut self) -> Task<Message> {
        match self.ui.take_comment_change() {
            Some(false) => {
                let out = self.ui.notation.hide_editor(&self.app);
                self.run_notation_outcome(out)
            }
            Some(true) => {
                let out = self.ui.notation.show_editor(&self.app);
                self.run_notation_outcome(out)
            }
            None => Task::none(),
        }
    }

    /// Shows or hides the comment editor, the Comment panel (`Ctrl+E`).
    pub(super) fn toggle_comment_editor(&mut self) -> Task<Message> {
        self.toggle_tab(PaneKind::Comment)
    }

    /// Shows or hides the Information panel (`Ctrl+I`).
    pub(super) fn toggle_info_tab(&mut self) -> Task<Message> {
        self.toggle_tab(PaneKind::Info)
    }

    /// The panel is shown (opened and selected) when it is not on screen,
    /// and put out of sight when it is.
    fn toggle_tab(&mut self, kind: PaneKind) -> Task<Message> {
        if self.ui.is_visible(kind) {
            self.change_panes(|ui| ui.hide_tab(kind))
        } else {
            self.change_panes(|ui| ui.show_pane(kind))
        }
    }

    /// Saves the draft of the comment editor when `message` is about to
    /// move the cursor or change the game. Returns the tasks of the saving
    /// messages, which run first.
    pub(super) fn save_comment_draft(&mut self, message: &AppMessage) -> Vec<Task<Message>> {
        if !(app_api::is_user_action(message) || matches!(message, AppMessage::Quit)) {
            return Vec::new();
        }
        let saving = self.ui.notation.take_commit(&self.app);
        saving.into_iter().map(|m| self.dispatch(m)).collect()
    }

    /// Closes the context menu of a tab, or the menu or the preview of the
    /// notation, if open (`Esc`).
    pub(super) fn close_notation_popups(&mut self) -> bool {
        self.ui.tab_menu.take().is_some()
            || self.ui.dialogs.mask.close_context()
            || self.ui.notation.list.close_transients()
    }
}
