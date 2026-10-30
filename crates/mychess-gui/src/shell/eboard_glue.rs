//! The part of the shell that serves the window of the electronic board:
//! the settings of the form, the messages for the application and the file
//! dialog of the driver (spec M8-10c).

use iced::Task;
use mychess_app::Message as AppMessage;
use mychess_app::eboard::{EboardMessage, UserCommand};

use super::{Message, Shell};
use crate::windows::eboard_dialog::{EBOARD_LINE_ID, EboardUi, EboardUiMessage};

impl Shell {
    /// An event of the window of the electronic board.
    pub(super) fn update_eboard(&mut self, message: EboardUiMessage) -> Task<Message> {
        if message == EboardUiMessage::Open {
            if self.ui.dialogs.eboard.is_none() {
                self.ui.dialogs.eboard = Some(EboardUi::open(&self.app));
            }
            return self.dispatch(AppMessage::Eboard(EboardMessage::RefreshPorts));
        }
        if message == EboardUiMessage::Close {
            self.ui.dialogs.eboard = None;
            return Task::none();
        }
        let Some(ui) = self.ui.dialogs.eboard.as_mut() else {
            return Task::none();
        };
        let app = |m: EboardMessage| AppMessage::Eboard(m);
        match message {
            EboardUiMessage::Kind(kind) => ui.draft.kind = kind,
            EboardUiMessage::Port(text) => ui.draft.port = text,
            EboardUiMessage::Command(text) => ui.draft.engine_command = text,
            EboardUiMessage::Params(text) => ui.draft.engine_params = text,
            EboardUiMessage::Browse => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .pick_file()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    |path| Message::EboardUi(EboardUiMessage::Browsed(path)),
                );
            }
            EboardUiMessage::Browsed(path) => {
                if let Some(path) = path {
                    ui.draft.engine_command = path.display().to_string();
                }
            }
            EboardUiMessage::Filter(filter) => {
                ui.draft.send_filter = filter;
                return self.dispatch(app(EboardMessage::Command(UserCommand::Filter(filter))));
            }
            EboardUiMessage::Line(text) => ui.line = text,
            EboardUiMessage::Send => {
                let line = std::mem::take(&mut ui.line);
                if line.trim().is_empty() {
                    return Task::none();
                }
                let send = self.dispatch(app(EboardMessage::Command(UserCommand::Raw(line))));
                return Task::batch([send, iced::widget::operation::focus(EBOARD_LINE_ID)]);
            }
            EboardUiMessage::Connect => {
                let prefs = ui.draft.clone();
                return self.dispatch(app(EboardMessage::Connect(Box::new(prefs))));
            }
            EboardUiMessage::Disconnect => return self.dispatch(app(EboardMessage::Disconnect)),
            EboardUiMessage::Refresh => return self.dispatch(app(EboardMessage::RefreshPorts)),
            EboardUiMessage::Info => {
                return self.dispatch(app(EboardMessage::Command(UserCommand::Info)));
            }
            EboardUiMessage::Rotate => {
                return self.dispatch(app(EboardMessage::Command(UserCommand::Rotate)));
            }
            EboardUiMessage::Synchronise => {
                return self.dispatch(app(EboardMessage::Command(UserCommand::Synchronise)));
            }
            EboardUiMessage::Clear => return self.dispatch(app(EboardMessage::ClearConsole)),
            EboardUiMessage::Open | EboardUiMessage::Close => {}
        }
        Task::none()
    }
}
