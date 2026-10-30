//! Shared helpers of the `m7_2c_*` tests (spec M7-2c): the report database of
//! the spec as a PGN file open in a slot, a shell whose tasks are run to
//! completion (the generation and the files are real) and the window
//! rendered headless.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use iced::Task;
pub use iced_test::Simulator;
use iced_test::futures::futures::StreamExt;
use iced_test::runtime::{Action, task::into_stream};
pub use mychess_app::opening_report::{
    FavoriteMessage, OpeningReportMessage, OpeningReportNotice, ReportFormat, ReportPart,
    ReportPhase, ReportSelector,
};
pub use mychess_app::{
    AppNotice, BasesMessage, GameMessage, Key, KeyInput, Message as AppMessage, Modifiers,
    NamedKey, Nav, SlotId,
};
pub use mychess_gui::windows::opening_report::{
    OpRepForm, OpRepUiMessage, OptCount, OptFlag, target_id,
};
pub use mychess_gui::{Message, Shell, UiMessage};

fn game(
    white: &str,
    black: &str,
    result: &str,
    date: &str,
    site: &str,
    elos: (u16, u16),
    moves: &str,
) -> String {
    let mut s = format!(
        "[Event \"?\"]\n[Site \"{site}\"]\n[Date \"{date}\"]\n[Round \"?\"]\n[White \"{white}\"]\n[Black \"{black}\"]\n[Result \"{result}\"]\n"
    );
    if elos.0 > 0 {
        let _ = writeln!(s, "[WhiteElo \"{}\"]", elos.0);
    }
    if elos.1 > 0 {
        let _ = writeln!(s, "[BlackElo \"{}\"]", elos.1);
    }
    let _ = write!(s, "\n{moves} {result}\n\n");
    s
}

/// The seven games of the report database of the spec.
pub fn pgn() -> String {
    [
        game(
            "Alfa, A",
            "Beta, B",
            "1-0",
            "2020.01.10",
            "Rio",
            (2500, 2400),
            "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7",
        ),
        game(
            "Gama, G",
            "Alfa, A",
            "1/2-1/2",
            "2015.05.02",
            "Lima",
            (2300, 2500),
            "1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5",
        ),
        game(
            "Beta, B",
            "Gama, G",
            "0-1",
            "2018.07.07",
            "Rio",
            (2400, 2300),
            "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6",
        ),
        game(
            "Alfa, A",
            "Delta, D",
            "1-0",
            "????.??.??",
            "Lima",
            (2500, 0),
            "1. e4 e5 2. Nf3 d6 3. d4",
        ),
        game(
            "Delta, D",
            "Beta, B",
            "*",
            "2021.01.01",
            "Rio",
            (0, 2400),
            "1. e4 e5 2. Nf3",
        ),
        game(
            "Alfa, A",
            "Gama, G",
            "1-0",
            "2019.03.03",
            "Rio",
            (2500, 2300),
            "1. Nf3 e5 2. e4 Nc6 3. Bb5",
        ),
        game(
            "Gama, G",
            "Alfa, A",
            "0-1",
            "2017.06.06",
            "Rio",
            (2300, 2500),
            "1. d4 d5 2. c4",
        ),
    ]
    .concat()
}

/// Sixty games that all reach `1.e4 e5 2.Nf3` and then differ (four Black
/// replies, five White moves, three Black moves): the theory table needs
/// merged rows and footnotes.
pub fn wide_pgn() -> String {
    let mut out = String::new();
    let mut n = 0u32;
    for black in ["Nc6", "d6", "Nf6", "f5"] {
        for white in ["Bc4", "Bb5", "Nc3", "d4", "Be2"] {
            for last in ["a6", "h6", "g6"] {
                n += 1;
                let result = ["1-0", "1/2-1/2", "0-1"][(n % 3) as usize];
                let _ = &mut out;
                out.push_str(&game(
                    &format!("W{n}, X"),
                    &format!("B{n}, Y"),
                    result,
                    &format!("{}.01.01", 1990 + n % 30),
                    "Rio",
                    (2000 + u16::try_from(n % 40).unwrap_or(0) * 10, 2100),
                    &format!("1. e4 e5 2. Nf3 {black} 3. {white} {last}"),
                ));
            }
        }
    }
    out
}

/// A shell whose tasks run to completion.
pub struct Rig {
    pub shell: Shell,
    rt: tokio::runtime::Runtime,
    pub dir: tempfile::TempDir,
    /// The messages that came out of the tasks (the answers of `perform`
    /// and of the dialogs), most recent last.
    pub seen: Vec<Message>,
}

impl Rig {
    pub fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = Shell::new(Some(dir.path().join("preferences.toml")));
        shell.app.prefs.general.language = mychess_app::LanguagePref::EnUs;
        shell.i18n = mychess_gui::i18n::I18n::new(mychess_gui::i18n::Language::EnUs);
        shell.app.open_pgn_in_slots = true;
        shell
            .app
            .set_opening_report_today(Some("2026.10.29".parse().unwrap()));
        Rig {
            shell,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            dir,
            seen: Vec::new(),
        }
    }

    /// A rig with the report database open at the position of the report.
    pub fn sample() -> Rig {
        let mut rig = Rig::new();
        rig.open_pgn("report.pgn", &pgn(), 5);
        rig
    }

    /// A rig with the sixty games; the board is at `1.e4 e5 2.Nf3`.
    pub fn wide() -> Rig {
        let mut rig = Rig::new();
        rig.open_pgn("wide.pgn", &wide_pgn(), 1);
        rig
    }

    /// Opens `pgn` and loads game `number` at its third half move.
    pub fn open_pgn(&mut self, name: &str, pgn: &str, number: u32) -> SlotId {
        let path = self.dir.path().join(name);
        std::fs::write(&path, pgn).unwrap();
        self.app(BasesMessage::Open(path));
        let slot = self.shell.app.bases.current();
        self.app(BasesMessage::LoadGame { slot, number });
        self.app(GameMessage::Navigate(Nav::GoToMainlinePly(3)));
        slot
    }

    /// Runs the tasks until nothing is left; the answers go back to the
    /// shell.
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
                self.seen.push(message.clone());
                queue.push(self.shell.update(message));
            }
        }
    }

    /// A message of the shell and everything it starts.
    pub fn msg(&mut self, message: Message) {
        let task = self.shell.update(message);
        self.drain(task);
    }

    /// A message of the shell, without running what it starts; returns the
    /// task for the caller to look at.
    pub fn once(&mut self, message: Message) -> Task<Message> {
        self.shell.update(message)
    }

    pub fn app(&mut self, message: impl Into<AppMessage>) {
        self.msg(Message::App(message.into()));
    }

    pub fn orep(&mut self, message: OpeningReportMessage) {
        self.app(AppMessage::OpeningReport(message));
    }

    pub fn ui(&mut self, message: OpRepUiMessage) {
        self.msg(Message::OpRep(message));
    }

    /// Opens the window as the menu does, and waits for the report.
    pub fn open_window(&mut self) {
        self.ui(OpRepUiMessage::Open);
        assert_eq!(self.view_phase(), ReportPhase::Ready);
    }

    pub fn view_phase(&self) -> ReportPhase {
        self.shell.app.opening_report_view().phase
    }

    pub fn is_open(&self) -> bool {
        self.shell.app.opening_report().is_open()
    }

    pub fn model(&self) -> &mychess_gui::windows::opening_report::Model {
        self.shell.ui.dialogs.oprep.model().expect("a model")
    }

    /// The text of the sections of the model, in order.
    pub fn sections(&self) -> Vec<String> {
        use mychess_gui::windows::opening_report::model::Block;
        self.model()
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Section(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn theory(&self) -> mychess_gui::windows::opening_report::model::Theory {
        use mychess_gui::windows::opening_report::model::Block;
        self.model()
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Theory(t) => Some((**t).clone()),
                _ => None,
            })
            .expect("a theory table")
    }

    pub fn notice(&self) -> Option<OpeningReportNotice> {
        match self.shell.ui.status_notice.as_ref() {
            Some(AppNotice::OpeningReport(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn filtered(&self) -> u32 {
        let slot = self.shell.app.bases.current();
        self.shell.app.bases.slot(slot).unwrap().filtered()
    }

    pub fn sim(&self, width: f32, height: f32) -> Simulator<'_, Message> {
        let element = mychess_gui::windows::main_window::view(&self.shell);
        Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(width, height),
            element,
        )
    }

    /// Sets the language of the interface and lets the shell notice.
    pub fn set_language(&mut self, language: mychess_app::LanguagePref) {
        self.shell.app.prefs.general.language = language;
        self.msg(Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)));
    }

    /// A key as the subscription reports it (the character is the one
    /// typed, for plain keys).
    pub fn key(&mut self, key: Key, modifiers: Modifiers) -> Task<Message> {
        let typed = match key {
            Key::Char(c) if modifiers == Modifiers::NONE => Some(c),
            _ => None,
        };
        self.shell.update(Message::Dialog(
            mychess_gui::windows::dialog_ui::DialogMessage::Key {
                input: KeyInput { key, modifiers },
                text_focused: false,
                typed,
            },
        ))
    }

    /// How many widget operations (a focus, for example) a task asks for.
    pub fn widget_operations(&self, task: Task<Message>) -> usize {
        let Some(stream) = into_stream(task) else {
            return 0;
        };
        self.rt.block_on(async {
            let mut stream = Box::pin(stream);
            let mut n = 0;
            while let Some(action) = stream.next().await {
                if matches!(action, Action::Widget(_)) {
                    n += 1;
                }
            }
            n
        })
    }

    /// A tall window: the whole page is visible, so every link can be
    /// clicked.
    pub fn tall(&self) -> Simulator<'_, Message> {
        self.sim(1400.0, 6000.0)
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

/// Clicks the widget `name` and returns the messages it produced.
pub fn click_id(mut sim: Simulator<'_, Message>, name: &str) -> Vec<Message> {
    sim.click(id(name))
        .unwrap_or_else(|e| panic!("click {name}: {e:?}"));
    sim.into_messages().collect()
}

pub fn debug_of(messages: &[Message]) -> Vec<String> {
    messages.iter().map(|m| format!("{m:?}")).collect()
}
