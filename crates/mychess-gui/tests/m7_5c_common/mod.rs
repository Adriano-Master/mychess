//! Shared helpers of the `m7_5c_*` tests (spec M7-5c): a shell, a runtime
//! that runs the effects of the application, EPD files on disk and the
//! window rendered headless.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

use std::path::{Path, PathBuf};

pub use iced_test::Simulator;
pub use mychess_app::epd_window::{EpdMessage, EpdTabView, EpdView};
pub use mychess_app::{Action, Effect, Message as AppMessage, PaneKind, perform, update};
pub use mychess_gui::windows::epd::{EpdForm, EpdUiMessage};
pub use mychess_gui::{Message, Shell, UiMessage};

pub const CINCO: &str = "\
rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - bm e5; id \"T.1\";
rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq - am e5; id \"T.2\"; ce 25;
rnbqkbnr/pppppppp/8/8/8/5N2/PPPPPPPP/RNBQKB1R b KQkq - id \"T.3\";
rnbqkbnr/pppppppp/8/8/2P5/8/PP1PPPPP/RNBQKBNR b KQkq -
rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - id \"T.5\";
";

/// The first four records of `CINCO`: the starting position is not in them.
pub fn cuatro() -> String {
    let mut out = String::new();
    for line in CINCO.lines().take(4) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

pub fn write_epd(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

pub struct Rig {
    pub shell: Shell,
    rt: tokio::runtime::Runtime,
    pub dir: tempfile::TempDir,
}

impl Rig {
    pub fn new() -> Rig {
        let mut shell = Shell::new(None);
        shell.app.prefs.general.language = mychess_app::LanguagePref::EnUs;
        shell.i18n = mychess_gui::i18n::I18n::new(mychess_gui::i18n::Language::EnUs);
        Rig {
            shell,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            dir: tempfile::tempdir().unwrap(),
        }
    }

    /// Sends a message to the application and runs every effect that is not
    /// for the host; returns the host effects.
    pub fn app(&mut self, message: impl Into<AppMessage>) -> Vec<Effect> {
        let mut host = Vec::new();
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in update(&mut self.shell.app, message) {
                if effect.runs_on_host() {
                    host.push(effect);
                } else if matches!(
                    effect,
                    Effect::Epd(mychess_app::epd_window::EpdEffect::Tick { .. })
                ) {
                    // timers are not waited for
                } else {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        host
    }

    /// A message of the shell; the app effects it asks for are run.
    pub fn ui(&mut self, message: Message) {
        drop(self.shell.update(message));
    }

    /// A message for the application sent through the shell (so the shell
    /// follows: panes, editor, notices), then the effects are run.
    pub fn send(&mut self, message: EpdMessage) {
        let mut queue = vec![AppMessage::Epd(message)];
        while let Some(message) = queue.pop() {
            let effects = update(&mut self.shell.app, message);
            for effect in effects {
                if effect.runs_on_host() {
                    continue;
                }
                queue.push(self.rt.block_on(perform(effect)));
            }
        }
        // Lets the shell catch up (opens the pane, loads the editor).
        self.ui(Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)));
    }

    pub fn open(&mut self, name: &str, text: &str) -> PathBuf {
        let path = write_epd(self.dir.path(), name, text);
        self.send(EpdMessage::OpenPath(path.clone()));
        path
    }

    /// Opens a file as the application does, but with the answer of the job
    /// changed to "cannot be written": the portable way to get a read-only
    /// file, since permissions mean nothing to root and Windows differs.
    pub fn open_read_only(&mut self, name: &str, text: &str) -> PathBuf {
        let path = write_epd(self.dir.path(), name, text);
        let mut queue = vec![AppMessage::Epd(EpdMessage::OpenPath(path.clone()))];
        while let Some(mut message) = queue.pop() {
            if let AppMessage::Epd(EpdMessage::Opened {
                result: Ok(opened), ..
            }) = &mut message
            {
                opened.read_only = true;
            }
            for effect in update(&mut self.shell.app, message) {
                if !effect.runs_on_host() {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        self.ui(Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)));
        path
    }

    pub fn view(&self) -> EpdView {
        self.shell.app.epd_view()
    }

    pub fn tab(&self) -> EpdTabView {
        let view = self.view();
        view.tabs
            .iter()
            .find(|t| Some(t.id) == view.active)
            .or(view.tabs.first())
            .cloned()
            .expect("an open file")
    }

    pub fn sim(&self, width: f32, height: f32) -> Simulator<'_, Message> {
        let element = mychess_gui::windows::main_window::view(&self.shell);
        Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(width, height),
            element,
        )
    }

    pub fn tr(&self, key: &str) -> String {
        self.shell.i18n.tr(key)
    }
}

pub fn id(name: &str) -> iced::widget::Id {
    iced::widget::Id::from(name.to_owned())
}

/// A dark and a light theme.
pub fn themes() -> [iced::Theme; 2] {
    use mychess_gui::theme::{AccentId, ThemeMode, resolve};
    [
        resolve(
            ThemeMode::Dark,
            iced::theme::Mode::Dark,
            AccentId::default(),
        ),
        resolve(
            ThemeMode::Light,
            iced::theme::Mode::Light,
            AccentId::default(),
        ),
    ]
}
