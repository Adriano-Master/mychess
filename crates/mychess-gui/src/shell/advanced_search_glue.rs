//! The part of the shell that serves the window of the advanced searches:
//! the numeric fields, the name of a new saved search and the file dialog of
//! the `.sso` import (spec M6-1c).

use iced::Task;
use mychess_app::{AdvancedSearchMessage, Message as AppMessage};

use super::{Message, Shell};
use crate::host::SsoFileDialog;
use crate::windows::advanced_search_dialog::{AdvancedUiMessage, number_message};

impl Shell {
    /// An event of the window of the advanced searches.
    pub(super) fn update_advanced_search(&mut self, message: AdvancedUiMessage) -> Task<Message> {
        match message {
            AdvancedUiMessage::Send(message) => {
                // A preset, a tab or a load changes values under the drafts.
                self.ui.dialogs.advanced.clear_drafts();
                self.dispatch(AppMessage::AdvancedSearch(message))
            }
            AdvancedUiMessage::Number(field, typed) => {
                let Some(view) = self.app.advanced_search_view() else {
                    return Task::none();
                };
                match self.ui.dialogs.advanced.type_into(field, &typed) {
                    Some(number) => self.dispatch(AppMessage::AdvancedSearch(number_message(
                        field, number, &view,
                    ))),
                    None => Task::none(),
                }
            }
            AdvancedUiMessage::Name(name) => {
                self.ui.dialogs.advanced.set_name(name);
                Task::none()
            }
            AdvancedUiMessage::SaveNamed => {
                let name = self.ui.dialogs.advanced.name().trim().to_owned();
                if name.is_empty() {
                    return Task::none();
                }
                self.dispatch(AppMessage::AdvancedSearch(AdvancedSearchMessage::SaveAs(
                    name,
                )))
            }
            AdvancedUiMessage::PickSso => {
                let dialog = SsoFileDialog::new(&self.i18n);
                Task::perform(dialog.run(), |path| {
                    Message::AdvancedUi(AdvancedUiMessage::SsoPicked(path))
                })
            }
            AdvancedUiMessage::SsoPicked(Some(path)) => self.dispatch(AppMessage::AdvancedSearch(
                AdvancedSearchMessage::ImportSso(path),
            )),
            AdvancedUiMessage::SsoPicked(None) => Task::none(),
        }
    }
}
