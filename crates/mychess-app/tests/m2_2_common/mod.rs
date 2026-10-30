//! Shared helpers of the `m2_2_*` integration tests
//! (spec `m2-2-arquivos-e-preferencias.md`).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::path::{Path, PathBuf};

pub use mychess_app::{
    AppState, Command, Effect, GameMessage, GameOrigin, ImportMessage, Message, PickKind,
    RequestId, SetupMessage, perform, update,
};
pub use mychess_core::{Color, Game, NodeId, Piece, Position, Role, Square};

/// An `AppState` plus a current-thread tokio runtime that runs `perform`.
///
/// `send` runs `update` and feeds the answers of every non-host effect back
/// into `update`, like the GUI does. Host effects (dialogs, clipboard, exit)
/// are collected in `host` for the test to answer by hand.
pub struct Harness {
    pub state: AppState,
    /// Host effects emitted so far and not yet taken.
    pub host: Vec<Effect>,
    rt: tokio::runtime::Runtime,
}

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

impl Harness {
    pub fn new() -> Harness {
        Harness::with_state(AppState::default())
    }

    pub fn with_prefs_path(path: Option<PathBuf>) -> Harness {
        Harness::with_state(AppState::with_preferences_path(path))
    }

    pub fn with_state(state: AppState) -> Harness {
        Harness {
            state,
            host: Vec::new(),
            rt: runtime(),
        }
    }

    /// Runs `perform` for one effect.
    pub fn perform(&self, effect: Effect) -> Message {
        self.rt.block_on(perform(effect))
    }

    /// `update` only: returns the effects without running any of them.
    pub fn raw(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        update(&mut self.state, message.into())
    }

    /// `update` + run every non-host effect to completion. Returns the host
    /// effects produced by this call (also appended to `self.host`).
    pub fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        let mut host = Vec::new();
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in update(&mut self.state, message) {
                if effect.runs_on_host() {
                    host.push(effect);
                } else if matches!(effect, Effect::ScheduleAutoplayTick { .. }) {
                    // Would sleep for real; autoplay is not under test here.
                } else {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        self.host.extend(host.iter().cloned());
        host
    }

    /// Plays `uci` from the cursor.
    pub fn play(&mut self, uci: &str) {
        let mv = self.state.session.position().parse_uci(uci).unwrap();
        self.send(GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        });
    }

    pub fn play_all(&mut self, ucis: &[&str]) {
        for uci in ucis {
            self.play(uci);
        }
    }

    /// Answers the pending file dialog (the last `PickFile` host effect).
    pub fn pick(&mut self, path: Option<&Path>) -> Vec<Effect> {
        let spec = self
            .host
            .iter()
            .rev()
            .find_map(|e| match e {
                Effect::PickFile(spec) => Some(spec.clone()),
                _ => None,
            })
            .expect("a PickFile effect was emitted");
        self.send(Message::FilePicked(
            spec.request,
            path.map(Path::to_path_buf),
        ))
    }

    /// Like `pick` but returns the raw effects of `update` without running them.
    pub fn pick_raw(&mut self, path: &Path) -> Vec<Effect> {
        let request = self
            .host
            .iter()
            .rev()
            .find_map(|e| match e {
                Effect::PickFile(spec) => Some(spec.request),
                _ => None,
            })
            .expect("a PickFile effect was emitted");
        self.raw(Message::FilePicked(request, Some(path.to_path_buf())))
    }

    pub fn clipboard(&mut self, purpose: mychess_app::ClipboardPurpose, text: &str) -> Vec<Effect> {
        self.send(Message::ClipboardRead {
            purpose,
            result: Ok(text.to_owned()),
        })
    }

    pub fn tags(&self) -> &mychess_core::Tags {
        self.state.session.game().tags()
    }

    pub fn plies(&self) -> usize {
        self.state.session.game().mainline().count()
    }
}

impl Default for Harness {
    fn default() -> Harness {
        Harness::new()
    }
}

pub fn sq(name: &str) -> Square {
    name.parse().unwrap()
}

pub fn piece(c: char) -> Piece {
    Piece::from_char(c).unwrap()
}

pub fn fen(text: &str) -> Position {
    Position::from_fen(text).unwrap()
}

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// A game from SAN-free UCI moves, with the standard start.
pub fn game_from_ucis(ucis: &[&str]) -> Game {
    let mut game = Game::new();
    let mut cur = game.root();
    for uci in ucis {
        let mv = game.position_at(cur).unwrap().parse_uci(uci).unwrap();
        cur = game.add_move(cur, mv).unwrap();
    }
    game
}

pub fn pgn_of(game: &Game) -> String {
    mychess_pgn::write::game_to_string(game, &mychess_pgn::write::WriteOptions::default()).unwrap()
}

/// A tiny PGN with `n` games whose white players are `P0`, `P1`, ...
pub fn multi_pgn(n: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        out.push_str(&format!(
            "[Event \"Ev{i}\"]\n[Site \"S\"]\n[Date \"2024.01.{:02}\"]\n[Round \"1\"]\n[White \"P{i}\"]\n[Black \"Q{i}\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n",
            (i % 28) + 1
        ));
    }
    out
}

/// Writes `bytes` to `dir/name` and returns the path.
pub fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// The sorted names of the entries of `dir` (listing order is not stable).
pub fn dir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
