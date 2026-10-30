//! Shared helpers of the `m6_4d_*` tests (spec M6-4d): a shell with a real
//! database open, the effects of the application run to the end, and the
//! panels rendered headless.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

use std::fmt::Write as _;

pub use iced_test::Simulator;
pub use mychess_app::charts::{ChartsMessage, StatsTab};
pub use mychess_app::crosstable::CrosstableMessage;
pub use mychess_app::piece_tracker::PieceTrackerMessage;
pub use mychess_app::{
    Action, BasesMessage, Effect, Message as AppMessage, PaneKind, SlotId, perform, update,
};
pub use mychess_gui::windows::data_panels::DataPanelsMessage;
pub use mychess_gui::{Message, Shell, UiMessage};

/// One game: `(event, site, date, round, white, black, result, white elo,
/// black elo, movetext)`.
pub type Spec = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    u16,
    u16,
    &'static str,
);

/// The tournament `Club`/`Rio` (A-B 1-0, A-C draw, B-C 1-0) and two other games.
pub const GAMES: [Spec; 5] = [
    (
        "Club",
        "Rio",
        "2020.05.01",
        "1",
        "A",
        "B",
        "1-0",
        2400,
        2300,
        "1. e4 e5 2. Qh5 Nc6 3. Qxf7# 1-0",
    ),
    (
        "Club",
        "Rio",
        "2020.05.01",
        "2",
        "A",
        "C",
        "1/2-1/2",
        2410,
        2200,
        "1. d4 d5 2. c4 e6 1/2-1/2",
    ),
    (
        "Club",
        "Rio",
        "2020.05.01",
        "3",
        "B",
        "C",
        "1-0",
        2300,
        2200,
        "1. Nf3 d5 2. Ng1 Nf6 1-0",
    ),
    (
        "Open",
        "Lima",
        "2021.03.10",
        "1",
        "D",
        "E",
        "0-1",
        2100,
        2150,
        "1. e4 c5 2. Nf3 d6 0-1",
    ),
    (
        "Club",
        "Rio",
        "2021.07.01",
        "1",
        "A",
        "B",
        "1-0",
        2420,
        2310,
        "1. e4 e5 1-0",
    ),
];

pub fn pgn(games: &[Spec]) -> String {
    let mut out = String::new();
    for (event, site, date, round, white, black, result, we, be, moves) in games {
        let _ = write!(
            out,
            "[Event \"{event}\"]\n[Site \"{site}\"]\n[Date \"{date}\"]\n[Round \"{round}\"]\n\
             [White \"{white}\"]\n[Black \"{black}\"]\n[Result \"{result}\"]\n\
             [WhiteElo \"{we}\"]\n[BlackElo \"{be}\"]\n\n{moves}\n\n"
        );
    }
    out
}

/// A shell and a runtime that runs the effects of the application.
pub struct Rig {
    pub shell: Shell,
    rt: tokio::runtime::Runtime,
    pub dir: tempfile::TempDir,
}

impl Rig {
    pub fn new() -> Rig {
        let mut shell = Shell::new(None);
        shell.app.open_pgn_in_slots = true;
        // English whatever the locale of the machine that runs the tests.
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
    /// for the host (delays are dropped); returns the host effects.
    pub fn app(&mut self, message: impl Into<AppMessage>) -> Vec<Effect> {
        let mut host = Vec::new();
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in update(&mut self.shell.app, message) {
                if effect.runs_on_host() {
                    host.push(effect);
                } else if is_delay(&effect) {
                    // Not waited for: the first computation starts at once.
                } else {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        host
    }

    /// A message of the shell (the tasks it returns are dropped).
    pub fn ui(&mut self, message: Message) {
        drop(self.shell.update(message));
    }

    pub fn toggle(&mut self, kind: PaneKind) {
        self.ui(Message::Ui(UiMessage::TogglePane(kind)));
    }

    /// Opens the sample games in a slot.
    pub fn open_games(&mut self, games: &[Spec]) -> SlotId {
        let path = self.dir.path().join("s.pgn");
        std::fs::write(&path, pgn(games)).unwrap();
        self.app(BasesMessage::Open(path));
        self.shell.app.bases.current()
    }

    pub fn load(&mut self, slot: SlotId, number: u32) {
        self.app(BasesMessage::LoadGame { slot, number });
    }

    /// The window rendered at `width` x `height`.
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

pub fn is_delay(effect: &Effect) -> bool {
    use mychess_app::charts::ChartsEffect;
    use mychess_app::crosstable::CrosstableEffect;
    use mychess_app::piece_tracker::PieceTrackerEffect;
    matches!(
        effect,
        Effect::Crosstable(CrosstableEffect::Delay { .. })
            | Effect::Charts(ChartsEffect::Delay { .. })
            | Effect::PieceTracker(PieceTrackerEffect::Delay { .. })
            | Effect::ScheduleAutoplayTick { .. }
    )
}

/// A dark and a light theme, to draw the canvases with.
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
