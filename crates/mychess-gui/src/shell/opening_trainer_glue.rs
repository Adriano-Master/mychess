//! The part of the shell that serves the dialog of the opening trainer: the
//! file dialog and the messages for the application (spec M8-5c).

use iced::Task;
use mychess_app::Message as AppMessage;
use mychess_app::opening_trainer::OpeningMessage;

use super::{Message, Shell};
use crate::windows::opening_trainer_dialog::OpeningUiMessage;

impl Shell {
    /// An event of the dialog of the opening trainer.
    pub(super) fn update_opening(&mut self, message: OpeningUiMessage) -> Task<Message> {
        match message {
            OpeningUiMessage::Open => {
                self.dispatch(AppMessage::OpeningTrainer(OpeningMessage::Open))
            }
            OpeningUiMessage::Edit(edit) => {
                let view = self.app.opening_view();
                self.ui.dialogs.opening.edit(&view, edit);
                Task::none()
            }
            OpeningUiMessage::PickFile => {
                let title = self.i18n.tr("opening-file-dialog-title");
                let filter = self.i18n.tr("opening-file-filter");
                let all = self.i18n.tr("file-filter-all");
                Task::perform(pick_repertoire(title, filter, all), |path| {
                    Message::OpeningUi(OpeningUiMessage::FilePicked(path))
                })
            }
            OpeningUiMessage::FilePicked(Some(path)) => {
                let view = self.app.opening_view();
                self.ui.dialogs.opening.set_file(&view, path);
                Task::none()
            }
            OpeningUiMessage::FilePicked(None) => Task::none(),
            OpeningUiMessage::Submit => {
                let message = self
                    .ui
                    .dialogs
                    .opening
                    .start_message(&self.app.opening_view());
                self.dispatch(message)
            }
            OpeningUiMessage::Cancel => {
                self.dispatch(crate::windows::opening_trainer_dialog::cancel_message())
            }
            OpeningUiMessage::Reset => {
                if self.ui.dialogs.opening.press_reset() {
                    let view = self.app.opening_view();
                    let config = self.ui.dialogs.opening.to_start(&view);
                    self.dispatch(AppMessage::OpeningTrainer(OpeningMessage::ResetStats(
                        Box::new(config),
                    )))
                } else {
                    Task::none()
                }
            }
        }
    }
}

/// The file dialog of the repertoire: books, masks, Scid databases and PGN
/// files (D2236).
async fn pick_repertoire(title: String, filter: String, all: String) -> Option<std::path::PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(title.as_str())
        .add_filter(
            filter.as_str(),
            &[
                "bin", "BIN", "Bin", "stm", "STM", "Stm", "si4", "SI4", "Si4", "sg4", "SG4", "sn4",
                "SN4", "pgn", "PGN", "Pgn", "gz", "GZ",
            ],
        )
        .add_filter(all.as_str(), &["*"])
        .pick_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}
