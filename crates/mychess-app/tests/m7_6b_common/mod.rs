//! Shared helpers of the `m7_6b_*` contract tests (spec
//! `m7-6-mesclar-partida.md`, slice b).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::fmt::Write as _;
use std::path::Path;

pub use mychess_app::{
    AppNotice, AppState, BasesMessage, Effect, GameMessage, MergeLoadError, MergeMessage,
    MergeNotice, Message, SharedGame, SlotId, perform, update,
};

/// `(white, black, result, event, date, fen, movetext)`.
type Spec = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
);

/// Game numbers of the sample database.
pub const A: u32 = 1;
pub const B: u32 = 2;
pub const C: u32 = 3;
pub const D: u32 = 4;
pub const E: u32 = 5;
/// A copy of A (a different game with the same moves).
pub const A_COPY: u32 = 6;
/// A game that starts from another position and never meets A.
pub const OTHER_START: u32 = 7;

const GAMES: [Spec; 7] = [
    (
        "W1",
        "B1",
        "*",
        "Evt",
        "2019.01.01",
        None,
        "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 *",
    ),
    (
        "Alfa, A",
        "Beta, B",
        "1-0",
        "Aberto",
        "2020.05.05",
        None,
        "1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. c3 Nf6 1-0",
    ),
    (
        "W3",
        "B3",
        "*",
        "Evt",
        "2019.01.01",
        None,
        "1. Nf3 e5 2. e4 Nc6 3. Bc4 Bc5 *",
    ),
    (
        "W4",
        "B4",
        "*",
        "Evt",
        "2019.01.01",
        None,
        "1. d4 d5 2. c4 e6 *",
    ),
    ("W5", "B5", "*", "Evt", "2019.01.01", None, "1. e4 e5 *"),
    (
        "W6",
        "B6",
        "*",
        "Evt",
        "2019.01.01",
        None,
        "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 *",
    ),
    (
        "W7",
        "B7",
        "*",
        "Evt",
        "2019.01.01",
        Some("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1"),
        "1. e4 Kd7 *",
    ),
];

pub fn pgn() -> String {
    let mut out = String::new();
    for (white, black, result, event, date, fen, moves) in GAMES {
        let _ = write!(
            out,
            "[Event \"{event}\"]\n[Site \"?\"]\n[Date \"{date}\"]\n[Round \"1\"]\n\
             [White \"{white}\"]\n[Black \"{black}\"]\n[Result \"{result}\"]\n"
        );
        if let Some(fen) = fen {
            let _ = write!(out, "[SetUp \"1\"]\n[FEN \"{fen}\"]\n");
        }
        let _ = write!(out, "\n{moves}\n\n");
    }
    out
}

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

    /// `update` only.
    pub fn raw(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        update(&mut self.state, message.into())
    }

    pub fn perform(&self, effect: Effect) -> Message {
        self.rt.block_on(perform(effect))
    }

    /// `update` plus every effect that `perform` runs, answers fed back.
    pub fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        let mut host = Vec::new();
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in update(&mut self.state, message) {
                if effect.runs_on_host() {
                    host.push(effect);
                } else {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        host
    }

    pub fn open_sample(&mut self, dir: &Path) -> SlotId {
        let path = dir.join("s.pgn");
        std::fs::write(&path, pgn()).expect("write");
        self.send(BasesMessage::Open(path));
        self.state.bases.current()
    }

    pub fn load(&mut self, slot: SlotId, number: u32) {
        self.send(BasesMessage::LoadGame { slot, number });
    }

    pub fn open(&mut self, slot: SlotId, number: u32) {
        self.send(MergeMessage::Open { slot, number });
    }

    pub fn apply(&mut self) {
        self.send(MergeMessage::Apply { comment: None });
    }

    pub fn notice(&self) -> Option<MergeNotice> {
        match self.state.notice.as_ref() {
            Some(AppNotice::Merge(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn undo(&mut self) {
        self.send(GameMessage::Undo);
    }

    pub fn nodes(&self) -> usize {
        self.state.session.game().node_count()
    }
}

/// The sample open in a slot and game `number` in the editor.
pub fn sample(number: u32) -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = rig.open_sample(dir.path());
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 7);
    rig.load(slot, number);
    assert_eq!(
        rig.state.bases.slot(slot).expect("slot").current_game,
        Some(number)
    );
    (rig, slot, dir)
}
