//! The part of the shell that serves the dialog and the panel of the
//! calculation exercise: the messages for the application and the focus of
//! the field of moves (spec M8-6c).

use iced::Task;
use mychess_app::Message as AppMessage;
use mychess_app::calvar::CalvarMessage;

use super::{Message, Shell};
use crate::windows::calvar_dialog::{CALVAR_ENTRY_ID, CalvarUiMessage, cancel_message};

impl Shell {
    /// An event of the dialog or of the panel of the calculation exercise.
    pub(super) fn update_calvar(&mut self, message: CalvarUiMessage) -> Task<Message> {
        match message {
            CalvarUiMessage::Open => {
                let open = self.dispatch(AppMessage::Calvar(CalvarMessage::Open));
                // With an exercise on, the shortcut and the menu give the
                // focus back to the field of moves (D2362).
                if self.app.calvar_live() {
                    return Task::batch([open, iced::widget::operation::focus(CALVAR_ENTRY_ID)]);
                }
                open
            }
            CalvarUiMessage::New => {
                let close = self.dispatch(AppMessage::Calvar(CalvarMessage::Close));
                let open = self.dispatch(AppMessage::Calvar(CalvarMessage::Open));
                Task::batch([close, open])
            }
            CalvarUiMessage::Edit(edit) => {
                let view = self.app.calvar_view();
                self.ui.dialogs.calvar.edit(&view, edit);
                Task::none()
            }
            CalvarUiMessage::Submit => {
                let message = self
                    .ui
                    .dialogs
                    .calvar
                    .start_message(&self.app.calvar_view());
                let start = self.dispatch(message);
                // The field of moves takes the focus when the panel opens.
                Task::batch([start, iced::widget::operation::focus(CALVAR_ENTRY_ID)])
            }
            CalvarUiMessage::FromTactics => {
                self.dispatch(AppMessage::Calvar(CalvarMessage::StartFromTactics {
                    now: crate::windows::calvar_dialog::wall_clock(),
                }))
            }
            CalvarUiMessage::Cancel => self.dispatch(cancel_message()),
            CalvarUiMessage::Reset => {
                if self.ui.dialogs.calvar.press_reset() {
                    self.dispatch(AppMessage::Calvar(CalvarMessage::ResetStats))
                } else {
                    Task::none()
                }
            }
            CalvarUiMessage::EntryChanged(text) => {
                self.ui.dialogs.calvar.set_entry(text);
                Task::none()
            }
            CalvarUiMessage::EntrySubmit => {
                let text = self.ui.dialogs.calvar.take_entry();
                if text.trim().is_empty() {
                    return Task::none();
                }
                self.dispatch(AppMessage::Calvar(CalvarMessage::EnterText(text)))
            }
        }
    }
}
