//! Shared helpers of the `m7_3b_*` contract tests (spec
//! `m7-3-relatorio-de-jogador.md`, slice b): the report database of the M7-2
//! spec plus game 8 as a PGN file open in a slot, and a rig that runs the
//! effects.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::path::Path;

#[path = "../m7_2b_common/mod.rs"]
mod base;

pub use mychess_app::player_report::{
    NameMatching, PlayerColor, PlayerFailure, PlayerPhase, PlayerReportEffect, PlayerReportMessage,
    PlayerReportNotice, PlayerReportPrefs, PlayerRequest, PlayerStart, ReportFormat, ReportPart,
};
pub use mychess_app::{
    AppNotice, AppState, BasesMessage, Effect, GameMessage, Message, SlotId, perform, update,
};

/// The seven games of the M7-2 report database and the eighth of the M7-3
/// spec: `Alfa, Z` (1000) x `Beta, B` (2400), `1-0`, 2022.01.01, Rio,
/// `1.e4 c5`.
pub fn pgn() -> String {
    let mut s = base::pgn();
    s.push_str(
        "[Event \"?\"]\n[Site \"Rio\"]\n[Date \"2022.01.01\"]\n[Round \"?\"]\n\
         [White \"Alfa, Z\"]\n[Black \"Beta, B\"]\n[Result \"1-0\"]\n\
         [WhiteElo \"1000\"]\n[BlackElo \"2400\"]\n\n1. e4 c5 1-0\n\n",
    );
    s
}

/// The game that ends at the position after `1.e4 e5 2.Nf3`.
pub const AT_POSITION: u32 = 5;

pub struct Rig {
    pub state: AppState,
    pub rt: tokio::runtime::Runtime,
    /// Every effect `update` returned, in order, before being run.
    pub seen: Vec<Effect>,
}

impl Rig {
    pub fn new() -> Rig {
        let mut state = AppState::default();
        state.open_pgn_in_slots = true;
        state.set_player_report_today(Some("2026.10.29".parse().unwrap()));
        Rig::with(state)
    }

    pub fn with(state: AppState) -> Rig {
        Rig {
            state,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
            seen: Vec::new(),
        }
    }

    pub fn raw(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        update(&mut self.state, message.into())
    }

    pub fn perform(&self, effect: Effect) -> Message {
        self.rt.block_on(perform(effect))
    }

    /// `update` plus every effect `perform` runs, answers fed back. The
    /// effects that open a file in the system are not run.
    pub fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        let mut host = Vec::new();
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in update(&mut self.state, message) {
                self.seen.push(effect.clone());
                if matches!(
                    effect,
                    Effect::PlayerReport(PlayerReportEffect::OpenFile { .. })
                ) {
                    continue;
                }
                if effect.runs_on_host() {
                    host.push(effect);
                } else {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        host
    }

    pub fn notice(&self) -> Option<PlayerReportNotice> {
        match self.state.notice.as_ref() {
            Some(AppNotice::PlayerReport(n)) => Some(*n),
            _ => None,
        }
    }

    /// The `Compute` effect among `effects`.
    pub fn compute_of(effects: &[Effect]) -> Option<Effect> {
        effects
            .iter()
            .find(|e| matches!(e, Effect::PlayerReport(PlayerReportEffect::Compute(_))))
            .cloned()
    }

    /// Generates the report of `name` and runs it to the end.
    pub fn report(&mut self, name: &str, color: PlayerColor, start: PlayerStart) {
        self.send(PlayerReportMessage::Generate(Box::new(request(
            name, color, start,
        ))));
    }

    pub fn lines(&self) -> Vec<u32> {
        let mut games: Vec<u32> = self
            .state
            .player_report_view()
            .report
            .expect("a report")
            .core
            .lines
            .iter()
            .map(|l| l.game)
            .collect();
        games.sort_unstable();
        games
    }
}

pub fn request(name: &str, color: PlayerColor, start: PlayerStart) -> PlayerRequest {
    PlayerRequest {
        name: name.to_owned(),
        color,
        matching: NameMatching::Exact,
        start,
        copy_to_clipbase: false,
    }
}

/// The sample open in a slot, game 5 in the editor at the position after
/// `1.e4 e5 2.Nf3`.
pub fn sample() -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = open_in(&mut rig, dir.path(), "report.pgn");
    (rig, slot, dir)
}

pub fn open_in(rig: &mut Rig, dir: &Path, name: &str) -> SlotId {
    let path = dir.join(name);
    std::fs::write(&path, pgn()).expect("write");
    rig.send(BasesMessage::Open(path));
    let slot = rig.state.bases.current();
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 8);
    rig.send(BasesMessage::LoadGame {
        slot,
        number: AT_POSITION,
    });
    rig.send(GameMessage::Navigate(mychess_app::Nav::GoToMainlinePly(3)));
    slot
}
