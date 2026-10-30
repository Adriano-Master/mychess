//! Shared helpers of the `m5_2_gui_*` contract tests (spec
//! `docs/specs/m5-2-janela-da-arvore.md`, M5-2a, D960-D985: the opening tree
//! panel), headless.
//!
//! Unlike the older rigs, [`TreeRig`] runs the tasks the shell returns (the
//! searches of the tree are `Effect`s turned into tasks), so the whole path
//! is real: a base on disk, the shortcut or the menu, the search in the
//! blocking pool, the answer and the table drawn from it.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::float_cmp
)]

#[path = "../m2_4_common/mod.rs"]
pub mod common;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub use common::{Rig, ch, command, command_shift, named, plain};
use iced::{Settings, Size, Task};
use iced_test::Simulator;
use iced_test::futures::futures::StreamExt;
use iced_test::runtime::{Action, task::into_stream};
pub use mychess_app::{
    Action as AppAction, BasesMessage, GameMessage, Key, KeyInput, Message as AppMessage,
    Modifiers, NamedKey, Nav, PlayOptions, TreeBody, TreeFastMode, TreeIssue, TreeMessage,
    TreeSort, TreeView, bar_segments,
};
use mychess_app::{LanguagePref, TreeRowView};
use mychess_gui::i18n::{I18n, Language};
pub use mychess_gui::windows::layout::PaneKind;
pub use mychess_gui::{Message, Shell, UiMessage};

pub const SIZE: Size = Size::new(1400.0, 900.0);

/// The standard five-game base: White to move at the start position sees
/// `e4` (3 games: 1-0, 1/2, no result), `d4` (0-1) and `Nf3` (1-0).
pub const STD: &str = "[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n\
    [Result \"0-1\"]\n\n1. d4 d5 0-1\n\n\
    [Result \"1/2-1/2\"]\n\n1. e4 c5 1/2-1/2\n\n\
    [Result \"1-0\"]\n\n1. Nf3 d5 1-0\n\n\
    [Result \"*\"]\n\n1. e4 *\n\n";

pub fn en() -> I18n {
    I18n::new(Language::EnUs)
}

pub fn pt() -> I18n {
    I18n::new(Language::PtBr)
}

pub fn ftl_text(language: &str, file: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// One game of a synthetic PGN.
pub fn game_pgn(moves: &str, result: &str, fen: Option<&str>) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "[Event \"T\"]\n[Site \"S\"]\n[Date \"2000.01.01\"]\n[Round \"1\"]\n[White \"W\"]\n[Black \"B\"]\n[Result \"{result}\"]\n"
    );
    if let Some(fen) = fen {
        let _ = writeln!(out, "[SetUp \"1\"]\n[FEN \"{fen}\"]");
    }
    let _ = write!(out, "\n{moves} {result}\n\n");
    out
}

/// Builds `<dir>/<name>.si4` from PGN text and returns the `.si4` path.
pub fn make_si4(dir: &Path, name: &str, pgn: &str) -> PathBuf {
    use mychess_db::si4::Si4Writer;
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let base = dir.join(name);
    let mut writer = Si4Writer::create(&base, 0).expect("create si4");
    import_pgn(
        pgn.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("import");
    drop(writer);
    base.with_extension("si4")
}

/// A shell whose tasks are run to completion.
pub struct TreeRig {
    pub rig: Rig,
    rt: tokio::runtime::Runtime,
}

impl TreeRig {
    pub fn new() -> TreeRig {
        TreeRig::with_language(LanguagePref::EnUs)
    }

    pub fn with_language(language: LanguagePref) -> TreeRig {
        let mut rig = Rig::with_language(language);
        rig.shell.app.open_pgn_in_slots = true;
        TreeRig {
            rig,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
        }
    }

    /// A rig with the std base open (and in the current slot).
    pub fn with_std(dir: &Path) -> TreeRig {
        let mut rig = TreeRig::new();
        rig.open_pgn(dir, "std.pgn", STD);
        rig
    }

    pub fn app(&self) -> &mychess_app::AppState {
        &self.rig.shell.app
    }

    /// Runs the tasks until nothing is left (answers go back to `update`).
    pub fn drain(&mut self, task: Task<Message>) {
        let mut queue = vec![task];
        let mut guard = 0;
        while let Some(task) = queue.pop() {
            guard += 1;
            assert!(guard < 500, "the tasks never end");
            let Some(stream) = into_stream(task) else {
                continue;
            };
            // A widget operation (a focus probe, `unfocus_all`) is dropped as
            // soon as it comes: it holds the sender its task waits on, and
            // there is no widget tree here to run it.
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
                queue.push(self.rig.shell.update(message));
            }
        }
    }

    /// Sends a shell message and runs what it starts.
    pub fn msg(&mut self, message: Message) {
        let task = self.rig.shell.update(message);
        self.drain(task);
    }

    pub fn app_msg(&mut self, message: impl Into<AppMessage>) {
        self.msg(Message::App(message.into()));
    }

    pub fn ui(&mut self, message: UiMessage) {
        self.msg(Message::Ui(message));
    }

    /// A key press as the subscription reports it.
    pub fn key(&mut self, input: KeyInput) {
        let typed = match input.key {
            Key::Char(c) if input.modifiers == Modifiers::NONE => Some(c),
            _ => None,
        };
        self.msg(Message::Dialog(
            mychess_gui::windows::dialog_ui::DialogMessage::Key {
                input,
                text_focused: false,
                typed,
            },
        ));
    }

    /// Opens a PGN text as a base (written to `dir`).
    pub fn open_pgn(&mut self, dir: &Path, name: &str, text: &str) {
        let path = dir.join(name);
        std::fs::write(&path, text).expect("write");
        self.app_msg(BasesMessage::Open(path));
    }

    pub fn open_si4(&mut self, dir: &Path, name: &str, text: &str) {
        let path = make_si4(dir, name, text);
        self.app_msg(BasesMessage::Open(path));
    }

    /// Opens the panel with the shortcut.
    pub fn open_panel(&mut self) {
        self.key(command_shift(Key::Char('t')));
    }

    /// Plays `uci` at the cursor.
    pub fn play(&mut self, uci: &str) {
        let mv = self.app().session.position().parse_uci(uci).expect("uci");
        self.app_msg(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        });
    }

    pub fn nav(&mut self, nav: Nav) {
        self.app_msg(GameMessage::Navigate(nav));
    }

    pub fn view(&self) -> TreeView {
        self.app().tree_view()
    }

    pub fn sans(&self) -> Vec<String> {
        self.view().rows.iter().map(|r| r.san.clone()).collect()
    }

    pub fn row(&self, san: &str) -> TreeRowView {
        self.view()
            .rows
            .into_iter()
            .find(|r| r.san == san)
            .unwrap_or_else(|| panic!("no row {san}"))
    }

    pub fn ply(&self) -> u32 {
        let s = &self.app().session;
        s.game().ply(s.cursor()).expect("ply")
    }

    pub fn nodes(&self) -> usize {
        self.app().session.game().node_count()
    }

    pub fn panel_open(&self) -> bool {
        self.rig.shell.ui.is_open(PaneKind::Tree)
    }

    pub fn sim(&self) -> Simulator<'_, Message> {
        self.rig.sim(SIZE)
    }

    pub fn sim_at(&self, size: Size) -> Simulator<'_, Message> {
        self.rig.sim(size)
    }

    pub fn shows(&self, text: &str) -> bool {
        self.sim().find(text).is_ok()
    }

    /// Clicks `text` in the window and runs the messages it produced.
    pub fn click(&mut self, text: &str) {
        let messages = {
            let mut sim = self.sim();
            sim.click(text)
                .unwrap_or_else(|e| panic!("click {text}: {e:?}"));
            sim.into_messages().collect::<Vec<_>>()
        };
        for message in messages {
            self.msg(message);
        }
    }

    /// The messages a click on `text` would send, without sending them.
    pub fn click_messages(&self, text: &str) -> Vec<Message> {
        let mut sim = self.sim();
        sim.click(text)
            .unwrap_or_else(|e| panic!("click {text}: {e:?}"));
        sim.into_messages().collect()
    }
}

/// `Debug` text of a message, for matching without `PartialEq`.
pub fn dbg(message: &Message) -> String {
    format!("{message:?}")
}

/// Whether `messages` has one with exactly this `Debug` text.
pub fn sent(messages: &[Message], expected: &Message) -> bool {
    let wanted = dbg(expected);
    messages.iter().any(|m| dbg(m) == wanted)
}

pub fn tree_msg(message: TreeMessage) -> Message {
    Message::App(AppMessage::Tree(message))
}

/// A game with ratings and a date, from the standard position.
pub fn rated_game(moves: &str, result: &str, welo: u16, belo: u16, date: &str) -> String {
    format!(
        "[Event \"T\"]\n[Site \"S\"]\n[Date \"{date}\"]\n[Round \"1\"]\n[White \"W\"]\n\
         [Black \"B\"]\n[Result \"{result}\"]\n[WhiteElo \"{welo}\"]\n[BlackElo \"{belo}\"]\n\
         \n{moves} {result}\n\n"
    )
}

/// The tree panel alone (what `windows::tree::view` draws) at `size`.
pub fn pane_sim_at(rig: &TreeRig, size: Size) -> Simulator<'_, Message> {
    Simulator::with_size(
        Settings::default(),
        size,
        mychess_gui::windows::tree::view(&rig.rig.shell),
    )
}

pub fn pane_sim(rig: &TreeRig) -> Simulator<'_, Message> {
    pane_sim_at(rig, SIZE)
}

/// Whether the panel alone shows `text`.
pub fn pane_shows(rig: &TreeRig, text: &str) -> bool {
    pane_sim(rig).find(text).is_ok()
}

/// The top of the first widget showing `text`.
pub fn y_of(sim: &mut Simulator<'_, Message>, text: &str) -> f32 {
    sim.find(text)
        .unwrap_or_else(|e| panic!("{text}: {e:?}"))
        .bounds()
        .y
}

/// The bounds of the row `index` (its container id).
pub fn row_bounds(sim: &mut Simulator<'_, Message>, id: &str) -> iced::Rectangle {
    sim.find(iced::widget::Id::from(id.to_owned()))
        .unwrap_or_else(|e| panic!("{id}: {e:?}"))
        .bounds()
}

/// `app` message without running its tasks (the searches stay "in flight").
pub fn send_without_running(rig: &mut TreeRig, message: impl Into<AppMessage>) {
    drop(rig.rig.shell.update(Message::App(message.into())));
}

/// A key press whose tasks are not run (a search stays "in flight").
pub fn key_without_running(rig: &mut TreeRig, input: KeyInput) {
    drop(rig.rig.shell.update(Message::Dialog(
        mychess_gui::windows::dialog_ui::DialogMessage::Key {
            input,
            text_focused: false,
            typed: None,
        },
    )));
}

/// Clicks `text` in the panel alone and runs the messages it sent. Returns
/// them.
pub fn click_pane(rig: &mut TreeRig, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let mut sim = pane_sim(rig);
        sim.click(text)
            .unwrap_or_else(|e| panic!("click {text}: {e:?}"));
        sim.into_messages().collect()
    };
    for message in messages.clone() {
        rig.msg(message);
    }
    messages
}

pub fn row_clicks(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| dbg(m).contains("RowClicked"))
        .count()
}
