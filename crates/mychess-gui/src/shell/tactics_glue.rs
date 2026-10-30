//! The part of the shell that serves the dialog of the tactics training:
//! the file dialog and the messages for the application (spec M8-4c).

use iced::Task;
use mychess_app::Message as AppMessage;
use mychess_app::tactics::TacticsMessage;

use super::{Message, Shell};
use crate::windows::tactics_dialog::TacticsUiMessage;

impl Shell {
    /// An event of the dialog of the tactics training.
    pub(super) fn update_tactics(&mut self, message: TacticsUiMessage) -> Task<Message> {
        match message {
            TacticsUiMessage::Open => self.dispatch(AppMessage::Tactics(TacticsMessage::Open)),
            TacticsUiMessage::Edit(edit) => {
                let view = self.app.tactics_view();
                self.ui.dialogs.tactics.edit(&view, edit);
                Task::none()
            }
            TacticsUiMessage::PickFile => {
                let title = self.i18n.tr("tactics-file-dialog-title");
                let filter = self.i18n.tr("tactics-file-filter");
                let all = self.i18n.tr("file-filter-all");
                Task::perform(pick_problems(title, filter, all), |path| {
                    Message::TacticsUi(TacticsUiMessage::FilePicked(path))
                })
            }
            TacticsUiMessage::FilePicked(Some(path)) => {
                let view = self.app.tactics_view();
                self.ui.dialogs.tactics.set_file(&view, path);
                Task::none()
            }
            TacticsUiMessage::FilePicked(None) => Task::none(),
            TacticsUiMessage::Submit => {
                let message = self
                    .ui
                    .dialogs
                    .tactics
                    .start_message(&self.app.tactics_view());
                self.dispatch(message)
            }
            TacticsUiMessage::Cancel => {
                self.dispatch(crate::windows::tactics_dialog::cancel_message())
            }
            TacticsUiMessage::Reset => {
                if self.ui.dialogs.tactics.press_reset() {
                    let view = self.app.tactics_view();
                    let config = self.ui.dialogs.tactics.to_start(&view);
                    self.dispatch(AppMessage::Tactics(TacticsMessage::ResetStats(Box::new(
                        config,
                    ))))
                } else {
                    Task::none()
                }
            }
        }
    }
}

/// The file dialog of the source of the problems: Scid databases and PGN
/// files (D1971).
pub(super) async fn pick_problems(
    title: String,
    filter: String,
    all: String,
) -> Option<std::path::PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(title.as_str())
        .add_filter(
            filter.as_str(),
            &[
                "si4", "SI4", "Si4", "sg4", "SG4", "sn4", "SN4", "pgn", "PGN", "Pgn", "gz", "GZ",
            ],
        )
        .add_filter(all.as_str(), &["*"])
        .pick_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}
