//! The part of the shell that serves the tablebase panel: the pane that
//! follows the application, the seed of the new positions and the fields of
//! the window (spec M8-8c).

use std::time::{SystemTime, UNIX_EPOCH};

use iced::Task;
use mychess_app::Message as AppMessage;
use mychess_app::tablebase::TablebaseMessage;

use super::{Message, Shell};
use crate::windows::layout::PaneKind;
use crate::windows::tablebase_panel::{TablebaseUiMessage, add_folder_message};

/// A seed from the clock (the positions are reproducible from it, D2577).
fn clock_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

impl Shell {
    /// An event of the window of the tablebase panel.
    pub(super) fn update_tablebase(&mut self, message: TablebaseUiMessage) -> Task<Message> {
        match message {
            TablebaseUiMessage::ToggleConfig => {
                let ui = &mut self.ui.dialogs.tablebase;
                ui.config_open = !ui.config_open;
                Task::none()
            }
            TablebaseUiMessage::FolderEdit(text) => {
                self.ui.dialogs.tablebase.folder = text;
                Task::none()
            }
            TablebaseUiMessage::AddFolder => {
                match add_folder_message(&mut self.ui.dialogs.tablebase) {
                    Some(message) => self.dispatch(AppMessage::Tablebase(message)),
                    None => Task::none(),
                }
            }
            TablebaseUiMessage::MaterialEdit(text) => {
                self.ui.dialogs.tablebase.material = text;
                Task::none()
            }
            TablebaseUiMessage::NewPosition(material) => {
                self.dispatch(AppMessage::Tablebase(TablebaseMessage::NewPosition {
                    material,
                    seed: clock_seed(),
                }))
            }
            TablebaseUiMessage::NewTyped => {
                let material = self.ui.dialogs.tablebase.material.trim().to_owned();
                if material.is_empty() {
                    return Task::none();
                }
                self.dispatch(AppMessage::Tablebase(TablebaseMessage::NewPosition {
                    material,
                    seed: clock_seed(),
                }))
            }
        }
    }

    /// Tells the application whether the tablebase panel is open, which
    /// starts or stops the queries (spec M8-8, D2569). Opening and closing
    /// produce effects, so this returns their tasks.
    pub(super) fn sync_tablebase(&mut self) -> Task<Message> {
        let mut wanted = self.ui.is_open(PaneKind::Tablebase);
        // Locked by an online game: the application ignores the opening
        // (D2570), so the panel closes instead of asking again at every
        // update.
        let mut closed = Task::none();
        if wanted && self.app.assist_locked() && !self.app.tablebase_open() {
            closed = self.change_panes(|ui| ui.close_pane(PaneKind::Tablebase));
            wanted = false;
        }
        if self.app.tablebase_open() == wanted {
            return closed;
        }
        let message = TablebaseMessage::SetOpen(wanted);
        Task::batch([closed, self.dispatch(AppMessage::Tablebase(message))])
    }
}
