//! Shared helpers of the `m9_5_*` tests (spec M9-5): a shell in a chosen
//! language whose tasks run to completion, the window rendered headless and
//! small helpers to click an id and to press a key as the subscription
//! reports it.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use iced::Task;
pub use iced_test::Simulator;
use iced_test::futures::futures::StreamExt;
use iced_test::runtime::{Action, task::into_stream};
pub use mychess_app::manual::{CHAPTER_FILES, HelpContext, ManualMessage};
pub use mychess_app::{Key, KeyInput, LanguagePref, Message as AppMessage, Modifiers, NamedKey};
pub use mychess_gui::i18n::{I18n, Language};
pub use mychess_gui::windows::manual::{
    MANUAL_FIND_ID, MANUAL_SCROLL_ID, MANUAL_TEXT_ID, MANUAL_WINDOW_ID, ManualUiMessage,
    chapter_entry_id, heading_id, result_entry_id,
};
pub use mychess_gui::{Message, Shell, UiMessage};

/// A shell whose tasks run to completion.
pub struct Rig {
    pub shell: Shell,
    rt: tokio::runtime::Runtime,
    pub dir: tempfile::TempDir,
}

impl Rig {
    pub fn new(language: Language) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = Shell::new(Some(dir.path().join("preferences.toml")));
        shell.app.prefs.general.language = match language {
            Language::EnUs => LanguagePref::EnUs,
            Language::PtBr => LanguagePref::PtBr,
        };
        shell.i18n = I18n::new(language);
        Rig {
            shell,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            dir,
        }
    }

    pub fn pt() -> Rig {
        Rig::new(Language::PtBr)
    }

    pub fn en() -> Rig {
        Rig::new(Language::EnUs)
    }

    /// Runs the tasks until nothing is left, except the ones that would
    /// reach the outside (the browser): the effect `OpenUrl` is never
    /// produced by the messages these tests drain.
    pub fn drain(&mut self, task: Task<Message>) {
        let mut queue = vec![task];
        let mut guard = 0;
        while let Some(task) = queue.pop() {
            guard += 1;
            assert!(guard < 500, "the tasks never end");
            let Some(stream) = into_stream(task) else {
                continue;
            };
            let outputs: Vec<Message> = self.rt.block_on(async {
                let mut stream = Box::pin(stream);
                let mut out = Vec::new();
                while let Some(action) = stream.next().await {
                    if let Action::Output(message) = action {
                        out.push(message);
                    }
                }
                out
            });
            for message in outputs {
                queue.push(self.shell.update(message));
            }
        }
    }

    pub fn msg(&mut self, message: Message) {
        let task = self.shell.update(message);
        self.drain(task);
    }

    pub fn ui(&mut self, message: ManualUiMessage) {
        self.msg(Message::ManualUi(message));
    }

    pub fn open(&mut self) {
        self.ui(ManualUiMessage::Open);
        assert!(self.shell.app.manual().is_open());
    }

    pub fn goto(&mut self, chapter: usize) {
        self.ui(ManualUiMessage::Nav(ManualMessage::Goto {
            chapter,
            anchor: None,
        }));
    }

    pub fn link(&mut self, uri: &str) {
        self.ui(ManualUiMessage::Nav(ManualMessage::Link(uri.to_owned())));
    }

    /// A key as the subscription reports it, with no text field focused.
    pub fn key(&mut self, key: Key, modifiers: Modifiers) {
        let typed = match key {
            Key::Char(c) if modifiers == Modifiers::NONE => Some(c),
            _ => None,
        };
        self.msg(Message::Dialog(
            mychess_gui::windows::dialog_ui::DialogMessage::Key {
                input: KeyInput { key, modifiers },
                text_focused: false,
                typed,
            },
        ));
    }

    pub fn named(&mut self, key: NamedKey) {
        self.key(Key::Named(key), Modifiers::NONE);
    }

    pub fn sim(&self, width: f32, height: f32) -> Simulator<'_, Message> {
        let element = mychess_gui::windows::main_window::view(&self.shell);
        Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(width, height),
            element,
        )
    }

    pub fn chapter(&self) -> usize {
        self.shell.app.manual().chapter()
    }
}

pub fn id(name: &str) -> iced::widget::Id {
    iced::widget::Id::from(name.to_owned())
}

/// Clicks the widget `name` and returns the messages it produced.
pub fn click_id(mut sim: Simulator<'_, Message>, name: &str) -> Vec<Message> {
    sim.click(id(name))
        .unwrap_or_else(|e| panic!("click {name}: {e:?}"));
    sim.into_messages().collect()
}

pub fn has_id(rig: &Rig, name: &str) -> bool {
    rig.sim(1300.0, 900.0).find(id(name)).is_ok()
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
