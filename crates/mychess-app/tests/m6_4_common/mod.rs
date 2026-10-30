//! Shared helpers of the `m6_4_*` contract tests (spec
//! `m6-4-tabela-estatisticas-rastreador.md`, controller of slices a to c).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty
)]

use std::fmt::Write as _;
use std::path::Path;

pub use mychess_app::charts::{ChartsEffect, ChartsMessage};
pub use mychess_app::crosstable::{CrosstableEffect, CrosstableMessage};
pub use mychess_app::piece_tracker::{PieceTrackerEffect, PieceTrackerMessage};
pub use mychess_app::{
    AppState, BasesMessage, Effect, ListMessage, Message, OpenControl, SlotId, perform, update,
};

/// One game of the sample: `(event, site, date, round, white, black, result,
/// white elo, black elo, movetext)`.
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

/// Games 1 to 3 are the tournament `Club`/`Rio` of the spec (A-B 1-0, A-C
/// draw, B-C 1-0); game 4 is another event; game 5 a Club game a year later.
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

pub struct Rig {
    pub state: AppState,
    /// The delays asked for and not fired yet.
    pub delays: Vec<Effect>,
    rt: tokio::runtime::Runtime,
}

impl Rig {
    pub fn new() -> Rig {
        let mut state = AppState::default();
        state.open_pgn_in_slots = true;
        Rig {
            state,
            delays: Vec::new(),
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

    /// `update` plus every non-host effect, answers fed back. The delays are
    /// kept in `self.delays` instead of waiting; the host effects are
    /// returned.
    pub fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        let mut host = Vec::new();
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in update(&mut self.state, message) {
                if effect.runs_on_host() {
                    host.push(effect);
                } else if matches!(
                    effect,
                    Effect::Crosstable(CrosstableEffect::Delay { .. })
                        | Effect::Charts(ChartsEffect::Delay { .. })
                        | Effect::PieceTracker(PieceTrackerEffect::Delay { .. })
                        | Effect::ScheduleAutoplayTick { .. }
                ) {
                    self.delays.push(effect);
                } else {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        host
    }

    /// The message a delay answers with, without waiting.
    pub fn elapsed(effect: &Effect) -> Option<Message> {
        match effect {
            Effect::Crosstable(CrosstableEffect::Delay { request, .. }) => Some(
                Message::Crosstable(CrosstableMessage::DelayElapsed(*request)),
            ),
            Effect::Charts(ChartsEffect::Delay { request, part, .. }) => {
                Some(Message::Charts(ChartsMessage::DelayElapsed {
                    request: *request,
                    part: *part,
                }))
            }
            Effect::PieceTracker(PieceTrackerEffect::Delay { request, .. }) => Some(
                Message::PieceTracker(PieceTrackerMessage::DelayElapsed(*request)),
            ),
            _ => None,
        }
    }

    /// Fires the delays asked for (the last ones are the live ones; older
    /// ones are dropped by the request).
    pub fn fire_delays(&mut self) {
        let delays = std::mem::take(&mut self.delays);
        for effect in delays {
            if let Some(message) = Rig::elapsed(&effect) {
                self.send(message);
            }
        }
    }

    pub fn open_games(&mut self, dir: &Path, name: &str, games: &[Spec]) -> SlotId {
        let path = dir.join(name);
        std::fs::write(&path, pgn(games)).expect("write");
        self.send(BasesMessage::Open(path));
        self.state.bases.current()
    }

    pub fn load(&mut self, slot: SlotId, number: u32) {
        self.send(BasesMessage::LoadGame { slot, number });
    }

    pub fn filter_numbers(&self, slot: SlotId) -> Vec<u32> {
        self.state
            .bases
            .slot(slot)
            .expect("slot")
            .filter()
            .iter_games()
            .collect()
    }

    pub fn negate_filter(&mut self, slot: SlotId) {
        self.send(BasesMessage::List(ListMessage::NegateFilter { slot }));
    }
}

/// A state with the sample games open in one slot and game 1 loaded.
pub fn sample() -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "s.pgn", &GAMES);
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 5);
    rig.load(slot, 1);
    assert_eq!(
        rig.state.bases.slot(slot).expect("slot").current_game,
        Some(1)
    );
    (rig, slot, dir)
}
