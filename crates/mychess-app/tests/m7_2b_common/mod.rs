//! Shared helpers of the `m7_2b_*` contract tests (spec
//! `m7-2-relatorio-de-abertura.md`, slice b): the report database of the
//! spec as a PGN file open in a slot, and a rig that runs the effects.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::fmt::Write as _;
use std::path::Path;

pub use mychess_app::opening_report::{
    ComputeOutcome, FavoriteMessage, OpeningReportEffect, OpeningReportMessage,
    OpeningReportNotice, OpeningReportPrefs, ReportFormat, ReportPart, ReportPhase, ReportSelector,
};
pub use mychess_app::{
    AppNotice, AppState, BasesMessage, Effect, GameMessage, Message, SlotId, perform, update,
};

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
        state.set_opening_report_today(Some("2026.10.29".parse().unwrap()));
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
                    Effect::OpeningReport(OpeningReportEffect::OpenFile { .. })
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

    pub fn notice(&self) -> Option<OpeningReportNotice> {
        match self.state.notice.as_ref() {
            Some(AppNotice::OpeningReport(n)) => Some(*n),
            _ => None,
        }
    }

    /// The `Compute` effect among `effects`.
    pub fn compute_of(effects: &[Effect]) -> Option<Effect> {
        effects
            .iter()
            .find(|e| matches!(e, Effect::OpeningReport(OpeningReportEffect::Compute(_))))
            .cloned()
    }
}

/// The sample open in a slot, game 5 in the editor at the position after
/// `1.e4 e5 2.Nf3` (the reference position of the report).
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
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 7);
    rig.send(BasesMessage::LoadGame {
        slot,
        number: AT_POSITION,
    });
    rig.send(GameMessage::Navigate(mychess_app::Nav::GoToMainlinePly(3)));
    slot
}
