//! Shared helpers of the `m3_4a_*` contract tests (spec
//! `m3-4-slots-clipbase-lista-de-partidas.md`, slice 4a).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::path::{Path, PathBuf};

pub use mychess_app::{
    AppNotice, AppState, BaseError, BaseKind, BasesMessage, BasesNotice, DiscardChoice, Effect,
    GameMessage, GameOrigin, Message, Nav, PlayOptions, SlotId, perform, update,
};

pub const TWO: &str = "[Event \"E1\"]\n[Site \"Paris FRA\"]\n[White \"A\"]\n[Black \"B\"]\n\
    [Result \"1-0\"]\n\n1. e4 e5 1-0\n\n\
    [Event \"E2\"]\n[White \"C\"]\n[Black \"D\"]\n[Result \"*\"]\n\n1. d4 *\n";
pub const ONE: &str = "[White \"Solo\"]\n[Black \"Only\"]\n\n1. c4 c5 *\n";

pub fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    path
}

/// A state with the slot flow switched on and a runtime for `perform`.
pub struct Rig {
    pub state: AppState,
    rt: tokio::runtime::Runtime,
}

impl Rig {
    pub fn new() -> Rig {
        let mut state = AppState::default();
        state.open_pgn_in_slots = true;
        Rig {
            state,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
        }
    }

    /// `update` only; the caller answers the effects.
    pub fn raw(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        update(&mut self.state, message.into())
    }

    pub fn perform(&self, effect: Effect) -> Message {
        self.rt.block_on(perform(effect))
    }

    /// `update` plus every non-host effect, answers fed back. Returns the
    /// host effects.
    pub fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        let mut host = Vec::new();
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in update(&mut self.state, message) {
                if effect.runs_on_host() {
                    host.push(effect);
                } else if matches!(effect, Effect::ScheduleAutoplayTick { .. }) {
                } else {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        host
    }

    pub fn open(&mut self, path: &Path) {
        self.send(BasesMessage::Open(path.to_path_buf()));
    }

    pub fn select(&mut self, n: usize) {
        self.send(BasesMessage::Select(sel(n)));
    }

    /// Plays `uci` at the end of the main line.
    pub fn play(&mut self, uci: &str) {
        self.send(GameMessage::Navigate(Nav::GameEnd));
        let mv = self.state.session.position().parse_uci(uci).expect("uci");
        self.send(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        });
    }

    pub fn current(&self) -> SlotId {
        self.state.bases.current()
    }

    pub fn open_count(&self) -> usize {
        self.state.bases.iter().count()
    }

    pub fn notice(&self) -> Option<&BasesNotice> {
        match &self.state.notice {
            Some(AppNotice::Bases(n)) => Some(n),
            _ => None,
        }
    }

    pub fn white(&self) -> String {
        self.state.session.game().tags().white.clone()
    }

    pub fn plies(&self) -> usize {
        self.state.session.game().mainline().count()
    }
}

pub fn sel(n: usize) -> SlotId {
    SlotId::from_number(n).expect("slot number 1..=9")
}

/// Builds an SI4 database `<dir>/<name>.si4` from PGN text and returns the
/// path of the `.si4`.
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
