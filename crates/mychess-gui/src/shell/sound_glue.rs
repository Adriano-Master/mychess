//! The part of the shell that serves the sounds window: the folder dialog
//! (spec M8-11, D2814).

use iced::Task;
use mychess_app::Message as AppMessage;
use mychess_app::sound::SoundMessage;

use super::{Message, Shell};
use crate::windows::sound_dialog::SoundUiMessage;

impl Shell {
    /// What the sounds window needs besides the application: opening the
    /// window rescans the packs of the user, and the answer to "Open sounds
    /// folder" opens the folder in the file manager (D2907, D2914).
    pub(super) fn sound_extra(&mut self, message: &AppMessage) -> Option<Task<Message>> {
        match message {
            AppMessage::Sound(SoundMessage::Open) => {
                Some(self.dispatch(AppMessage::Sound(SoundMessage::ScanPacks)))
            }
            AppMessage::Sound(SoundMessage::UserFolderReady(Ok(path))) => {
                let (opener, path) = (self.folder_opener, path.clone());
                // On a thread of its own: starting the file manager and
                // waiting for the launcher must not stall the interface. A
                // failure only means the window keeps showing the path.
                std::thread::spawn(move || {
                    if let Err(error) = opener(&path) {
                        tracing::warn!(%error, "could not open the sounds folder");
                    }
                });
                None
            }
            _ => None,
        }
    }

    /// An event of the sounds window that is not for the application.
    pub(super) fn update_sound(&mut self, message: SoundUiMessage) -> Task<Message> {
        match message {
            SoundUiMessage::PickFolder => {
                let title = self.i18n.tr("sound-folder");
                Task::perform(pick_folder(title), |path| {
                    Message::SoundUi(SoundUiMessage::FolderPicked(path))
                })
            }
            SoundUiMessage::FolderPicked(Some(path)) => {
                // A folder whose name is not text cannot be kept in the file.
                let Some(text) = path.to_str() else {
                    return Task::none();
                };
                let mut prefs = self.app.sound_prefs().clone();
                text.clone_into(&mut prefs.folder);
                self.dispatch(AppMessage::Sound(SoundMessage::Set(Box::new(prefs))))
            }
            SoundUiMessage::FolderPicked(None) | SoundUiMessage::PathTyped => Task::none(),
        }
    }
}

/// The folder dialog.
async fn pick_folder(title: String) -> Option<std::path::PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(title.as_str())
        .pick_folder()
        .await
        .map(|handle| handle.path().to_path_buf())
}

/// Opens `path` in the file manager of the system: the launcher is a program
/// with the path as a separate argument (never a shell line).
pub(super) fn open_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    let mut command = launcher(path);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let status = command.spawn()?.wait()?;
    // `explorer` answers 1 even when it worked.
    if status.success() || cfg!(windows) {
        Ok(())
    } else {
        Err(std::io::Error::other("the launcher failed"))
    }
}

#[cfg(target_os = "linux")]
fn launcher(path: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(path);
    command
}

#[cfg(target_os = "macos")]
fn launcher(path: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new("open");
    command.arg(path);
    command
}

#[cfg(windows)]
fn launcher(path: &std::path::Path) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW
    let mut command = std::process::Command::new("explorer");
    command.arg(path).creation_flags(0x0800_0000);
    command
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn launcher(path: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(path);
    command
}
