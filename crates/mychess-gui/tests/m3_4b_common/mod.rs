//! Shared helpers of the `m3_4b_*` contract tests (spec
//! `docs/specs/m3-4-slots-clipbase-lista-de-partidas.md`, slice 4b).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

#[path = "../m2_4_common/mod.rs"]
pub mod common;

use std::path::{Path, PathBuf};

pub use common::{Rig, command, named, plain};
use iced::{Size, widget};
use mychess_app::{BasesMessage, Effect, Message as AppMessage, SlotId, perform};
use mychess_gui::Message;
use mychess_gui::windows::game_list::GameListMessage;

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// One game of a synthetic PGN.
pub struct G {
    pub white: String,
    pub black: String,
    pub result: &'static str,
    pub extra: String,
    pub moves: &'static str,
}

impl G {
    pub fn new(white: &str, black: &str, result: &'static str) -> G {
        G {
            white: white.to_owned(),
            black: black.to_owned(),
            result,
            extra: String::new(),
            moves: "1. e4 e5 2. Nf3 Nc6",
        }
    }

    pub fn tag(mut self, name: &str, value: &str) -> G {
        self.extra.push_str(&format!("[{name} \"{value}\"]\n"));
        self
    }
}

pub fn write_games(dir: &Path, name: &str, games: &[G]) -> PathBuf {
    let mut text = String::new();
    for g in games {
        text.push_str(&format!(
            "[Event \"Ev\"]\n[White \"{}\"]\n[Black \"{}\"]\n[Result \"{}\"]\n{}\n{} {}\n\n",
            g.white, g.black, g.result, g.extra, g.moves, g.result
        ));
    }
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write the pgn");
    path
}

/// `count` games, game `n` played by `<prefix> n` as White.
pub fn numbered(dir: &Path, name: &str, prefix: &str, count: u32) -> PathBuf {
    let mut text = String::with_capacity(count as usize * 90);
    for n in 1..=count {
        text.push_str(&format!(
            "[White \"{prefix} {n}\"]\n[Black \"Opp\"]\n[Result \"*\"]\n\n1. e4 *\n\n"
        ));
    }
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write the pgn");
    path
}

/// Opens `path` as the shell would (request to the application, answer back
/// through the shell).
pub fn open(rig: &mut Rig, path: &Path) {
    let rt = runtime();
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::Open(path.to_path_buf())),
    );
    for effect in effects {
        assert!(matches!(effect, Effect::OpenBase { .. }), "{effect:?}");
        let answer = rt.block_on(perform(effect));
        rig.app_msg(answer);
    }
}

/// The messages a list message turns into for the application, run with
/// their effects (loading a game included).
pub fn list(rig: &mut Rig, message: GameListMessage) {
    let out = rig.shell.ui.games.update(message, &rig.shell.app);
    for message in out {
        rig.send(message);
    }
}

pub fn viewport(rig: &mut Rig, width: f32, height: f32) {
    rig.msg(Message::Games(GameListMessage::Viewport(Size::new(
        width, height,
    ))));
}

pub fn slot(n: usize) -> SlotId {
    SlotId::from_number(n).expect("slot")
}

/// Which of `ids` (game numbers) have a row widget in the current view.
pub fn rows_built(rig: &Rig, ids: impl IntoIterator<Item = u32>) -> Vec<u32> {
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    ids.into_iter()
        .filter(|n| sim.find(widget::Id::from(format!("games-row-{n}"))).is_ok())
        .collect()
}

pub fn has_id(rig: &Rig, id: &'static str) -> bool {
    rig.sim(Size::new(1280.0, 800.0))
        .find(widget::Id::new(id))
        .is_ok()
}

pub fn has_text(rig: &Rig, text: &str) -> bool {
    rig.sim(Size::new(1280.0, 800.0)).find(text).is_ok()
}

pub fn current_first(rig: &Rig) -> u32 {
    rig.shell.app.bases.current_slot().unwrap().list.first
}

pub fn selected(rig: &Rig) -> Option<u32> {
    rig.shell.app.bases.current_slot().unwrap().list.selected
}
