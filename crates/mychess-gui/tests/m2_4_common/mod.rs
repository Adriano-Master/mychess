//! Shared helpers of the `m2_4_*` integration tests (spec
//! `docs/specs/m2-4-notacao-e-dialogos.md`).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::float_cmp,
    clippy::unnecessary_wraps
)]

use std::path::Path;

use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::{
    Effect, GameMessage, General, Key, KeyInput, LanguagePref, LoadedPreferences,
    Message as AppMessage, Modifiers, NamedKey, Nav, Preferences, ThemePref, perform, update,
};
use mychess_core::{Game, NodeId};
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::{Message, Shell, UiMessage};

/// Text shaping and rendering share one font system. Running several
/// simulators at once in the threads of the test harness crashed the process
/// (SIGSEGV, about one run in three for the heavier files), while the same
/// tests alone or one at a time never did. Every test that draws or shapes
/// text holds this guard.
pub fn render_lock() -> RenderGuard {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A test may hold several rigs at once: only the first guard of a
    // thread takes the lock.
    let nested = DEPTH.with(|depth| {
        let before = depth.get();
        depth.set(before + 1);
        before > 0
    });
    let lock = if nested {
        None
    } else {
        Some(
            LOCK.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    };
    RenderGuard { _lock: lock }
}

thread_local! {
    /// How many guards this thread holds.
    static DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Held while a test draws or shapes text; see [`render_lock`].
pub struct RenderGuard {
    _lock: Option<std::sync::MutexGuard<'static, ()>>,
}

impl Drop for RenderGuard {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Plays `uci` as a child of `parent`.
pub fn play(game: &mut Game, parent: NodeId, uci: &str) -> NodeId {
    let mv = game
        .position_at(parent)
        .expect("position")
        .parse_uci(uci)
        .expect("legal move");
    game.add_move(parent, mv).expect("added move")
}

/// A line of moves from `parent`.
pub fn line(game: &mut Game, parent: NodeId, ucis: &[&str]) -> Vec<NodeId> {
    let mut at = parent;
    ucis.iter()
        .map(|uci| {
            at = play(game, at, uci);
            at
        })
        .collect()
}

/// A shell with a fixed language and theme (`System` would depend on the
/// machine) and a current-thread runtime to run the effects of the
/// application, like the iced runtime does.
pub struct Rig {
    _render: RenderGuard,
    pub shell: Shell,
    /// Host effects emitted so far (file dialogs, clipboard, exit).
    pub host: Vec<Effect>,
    rt: tokio::runtime::Runtime,
}

impl Rig {
    pub fn new() -> Rig {
        Rig::with_language(LanguagePref::EnUs)
    }

    pub fn with_language(language: LanguagePref) -> Rig {
        let guard = render_lock();
        let mut shell = Shell::new(None);
        let prefs = Preferences {
            general: General {
                language,
                theme: ThemePref::Light,
                ..General::default()
            },
            ..Preferences::default()
        };
        drop(shell.update(Message::App(AppMessage::PreferencesLoaded(
            LoadedPreferences {
                prefs,
                issues: Vec::new(),
            },
        ))));
        Rig {
            _render: guard,
            shell,
            host: Vec::new(),
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
        }
    }

    pub fn with_game(game: Game) -> Rig {
        let mut rig = Rig::new();
        rig.set_game(game);
        rig
    }

    pub fn set_game(&mut self, game: Game) {
        self.shell.app.session.replace_game(game);
        self.shell.app.refresh_notation();
        self.sync();
    }

    /// Brings the visual state up to date after the application was
    /// changed directly.
    pub fn sync(&mut self) {
        drop(self.shell.update(Message::Ui(UiMessage::ShowAbout(false))));
    }

    /// Sends a shell message (effects of the application are dropped).
    pub fn msg(&mut self, message: Message) {
        drop(self.shell.update(message));
    }

    pub fn app_msg(&mut self, message: AppMessage) {
        self.msg(Message::App(message));
    }

    /// Sends several shell messages.
    pub fn feed(&mut self, messages: impl IntoIterator<Item = Message>) {
        for message in messages {
            self.msg(message);
        }
    }

    /// `mychess_app::update` with every non-host effect run to completion.
    pub fn send(&mut self, message: impl Into<AppMessage>) -> Vec<Effect> {
        let mut host = Vec::new();
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in update(&mut self.shell.app, message) {
                if effect.runs_on_host() {
                    host.push(effect);
                } else if matches!(effect, Effect::ScheduleAutoplayTick { .. }) {
                    // Would sleep for real.
                } else {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        self.host.extend(host.iter().cloned());
        self.sync();
        host
    }

    /// Answers the last file dialog the application asked for.
    pub fn pick(&mut self, path: Option<&Path>) {
        let request = self
            .host
            .iter()
            .rev()
            .find_map(|e| match e {
                Effect::PickFile(spec) => Some(spec.request),
                _ => None,
            })
            .expect("a PickFile effect was emitted");
        self.send(AppMessage::FilePicked(request, path.map(Path::to_path_buf)));
    }

    pub fn cursor(&self) -> NodeId {
        self.shell.app.session.cursor()
    }

    pub fn game(&self) -> &Game {
        self.shell.app.session.game()
    }

    pub fn goto(&mut self, node: NodeId) {
        self.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::GoToNode(node))));
    }

    pub fn sim(&self, size: Size) -> Simulator<'_, Message> {
        Simulator::with_size(Settings::default(), size, self.shell.view())
    }

    /// A key press as the subscription reports it.
    pub fn key(&mut self, input: KeyInput, text_focused: bool, typed: Option<char>) {
        self.msg(Message::Dialog(DialogMessage::Key {
            input,
            text_focused,
            typed,
        }));
    }

    pub fn press(&mut self, input: KeyInput) {
        let typed = match input.key {
            Key::Char(c) if input.modifiers == Modifiers::NONE => Some(c),
            _ => None,
        };
        self.key(input, false, typed);
    }
}

impl Default for Rig {
    fn default() -> Rig {
        Rig::new()
    }
}

pub fn plain(key: Key) -> KeyInput {
    KeyInput {
        key,
        modifiers: Modifiers::NONE,
    }
}

pub fn ch(c: char) -> KeyInput {
    plain(Key::Char(c))
}

pub fn named(key: NamedKey) -> KeyInput {
    plain(Key::Named(key))
}

pub fn command(key: Key) -> KeyInput {
    KeyInput {
        key,
        modifiers: Modifiers::COMMAND,
    }
}

pub fn command_shift(key: Key) -> KeyInput {
    KeyInput {
        key,
        modifiers: Modifiers::COMMAND_SHIFT,
    }
}

/// The messages a simulator collected, in order.
pub fn messages(sim: Simulator<'_, Message>) -> Vec<Message> {
    sim.into_messages().collect()
}

/// `Debug` text of a message, for matching without `PartialEq`.
pub fn dbg(message: &Message) -> String {
    format!("{message:?}")
}

/// Whether any message has this `Debug` text (exactly).
pub fn has(messages: &[Message], expected: &Message) -> bool {
    let wanted = dbg(expected);
    messages.iter().any(|m| dbg(m) == wanted)
}

/// 1.e4 e5 2.Nf3 Nc6 3.Bb5 with a variation 2.Nc3 (Nf6) at move 2 and
/// 1...c5 at move 1.
pub struct Sample {
    pub game: Game,
    pub e4: NodeId,
    pub e5: NodeId,
    pub c5: NodeId,
    pub nf3: NodeId,
    pub nc6: NodeId,
    pub bb5: NodeId,
    pub nc3: NodeId,
    pub nf6: NodeId,
}

pub fn sample() -> Sample {
    let mut game = Game::new();
    let root = game.root();
    let e4 = play(&mut game, root, "e2e4");
    let e5 = play(&mut game, e4, "e7e5");
    let c5 = play(&mut game, e4, "c7c5");
    let nf3 = play(&mut game, e5, "g1f3");
    let nc3 = play(&mut game, e5, "b1c3");
    let nf6 = play(&mut game, nc3, "g8f6");
    let nc6 = play(&mut game, nf3, "b8c6");
    let bb5 = play(&mut game, nc6, "f1b5");
    Sample {
        game,
        e4,
        e5,
        c5,
        nf3,
        nc6,
        bb5,
        nc3,
        nf6,
    }
}

/// The SHA-256 of the pixels of a snapshot, read back from the file that
/// `Snapshot::matches_hash` writes (the pixels themselves are not exposed).
/// Compare fingerprints within one run only.
pub fn fingerprint<M>(sim: &mut Simulator<'_, M>) -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "mychess-m2-4-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let snapshot = sim.snapshot(&iced::Theme::Light).expect("snapshot");
    assert!(
        snapshot
            .matches_hash(dir.join("scene"))
            .expect("write hash"),
        "the first run always matches"
    );
    let file = std::fs::read_dir(&dir)
        .expect("hash dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "sha256"))
        .expect("hash file");
    let hash = std::fs::read_to_string(file).expect("read hash");
    drop(std::fs::remove_dir_all(&dir));
    hash
}
