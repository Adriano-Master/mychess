//! Shared helpers of the `m3_4c_*` contract tests (spec
//! `m3-4-slots-clipbase-lista-de-partidas.md`, slice 4c).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub use mychess_app::{
    AppNotice, AppState, BaseError, BaseKind, BasesMessage, BasesNotice, Effect, ListColumn,
    ListJobKind, ListMessage, Message, PickMode, SearchKind, SearchOutcome, SlotId, SortDir,
    perform, update,
};

/// One game of a synthetic PGN, described by the header values only.
#[derive(Clone, Debug)]
pub struct G {
    pub white: String,
    pub black: String,
    pub event: String,
    pub site: String,
    pub date: String,
    pub round: String,
    pub result: &'static str,
    pub moves: String,
    pub welo: u16,
    pub belo: u16,
    pub eco: String,
}

impl G {
    pub fn new(white: &str, black: &str) -> G {
        G {
            white: white.into(),
            black: black.into(),
            event: "Ev".into(),
            site: "Rome".into(),
            date: "2020.01.01".into(),
            round: "1".into(),
            result: "*",
            moves: "1. e4 *".into(),
            welo: 0,
            belo: 0,
            eco: String::new(),
        }
    }
    pub fn eco(mut self, v: &str) -> G {
        self.eco = v.into();
        self
    }
    pub fn event(mut self, v: &str) -> G {
        self.event = v.into();
        self
    }
    pub fn site(mut self, v: &str) -> G {
        self.site = v.into();
        self
    }
    pub fn date(mut self, v: &str) -> G {
        self.date = v.into();
        self
    }
    pub fn round(mut self, v: &str) -> G {
        self.round = v.into();
        self
    }
    pub fn result(mut self, v: &'static str) -> G {
        self.result = v;
        self
    }
    pub fn moves(mut self, v: &str) -> G {
        self.moves = v.into();
        self
    }
    pub fn elo(mut self, w: u16, b: u16) -> G {
        self.welo = w;
        self.belo = b;
        self
    }
}

/// PGN text of `games`, one per entry, in order.
pub fn pgn(games: &[G]) -> String {
    let mut out = String::new();
    for g in games {
        let _ = write!(
            out,
            "[Event \"{}\"]\n[Site \"{}\"]\n[Date \"{}\"]\n[Round \"{}\"]\n[White \"{}\"]\n\
             [Black \"{}\"]\n[Result \"{}\"]\n",
            g.event, g.site, g.date, g.round, g.white, g.black, g.result
        );
        if g.welo > 0 {
            let _ = writeln!(out, "[WhiteElo \"{}\"]", g.welo);
        }
        if g.belo > 0 {
            let _ = writeln!(out, "[BlackElo \"{}\"]", g.belo);
        }
        if !g.eco.is_empty() {
            let _ = writeln!(out, "[ECO \"{}\"]", g.eco);
        }
        // The movetext ends with the result of the game, as in a real PGN.
        let moves = g
            .moves
            .strip_suffix('*')
            .map_or_else(|| g.moves.clone(), |head| format!("{head}{}", g.result));
        let _ = write!(out, "\n{moves}\n\n");
    }
    out
}

pub fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    path
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

pub fn sel(n: usize) -> SlotId {
    SlotId::from_number(n).expect("slot number 1..=9")
}

pub fn l(message: ListMessage) -> BasesMessage {
    BasesMessage::List(message)
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

    /// `update` plus every non-host effect, answers fed back.
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

    pub fn list(&mut self, message: ListMessage) {
        self.send(BasesMessage::List(message));
    }

    pub fn open(&mut self, path: &Path) -> SlotId {
        self.send(BasesMessage::Open(path.to_path_buf()));
        self.state.bases.current()
    }

    /// Opens a PGN with these games (in a fresh file of `dir`).
    pub fn open_games(&mut self, dir: &Path, name: &str, games: &[G]) -> SlotId {
        let path = write(dir, name, &pgn(games));
        self.open(&path)
    }

    pub fn pick(&mut self, slot: SlotId, number: u32, mode: PickMode) {
        self.send(l(ListMessage::Pick { slot, number, mode }));
    }

    pub fn select_all(&mut self, slot: SlotId, numbers: &[u32]) {
        self.send(l(ListMessage::SelectNumbers {
            slot,
            numbers: numbers.to_vec(),
        }));
    }

    pub fn set_search(&mut self, slot: SlotId, text: &str) {
        self.send(BasesMessage::SetSearchText {
            slot,
            text: text.into(),
        });
    }

    /// The numbers the list shows, in view order.
    pub fn view(&self, slot: SlotId) -> Vec<u32> {
        self.state
            .bases
            .list_page(slot, 1, u32::MAX / 2)
            .iter()
            .map(|r| r.number)
            .collect()
    }

    /// The numbers in the filter, in database order.
    pub fn filter_numbers(&self, slot: SlotId) -> Vec<u32> {
        self.state
            .bases
            .slot(slot)
            .expect("slot")
            .filter()
            .iter_games()
            .collect()
    }

    pub fn selection(&self, slot: SlotId) -> Vec<u32> {
        self.state
            .bases
            .slot(slot)
            .expect("slot")
            .list
            .selection
            .iter()
            .copied()
            .collect()
    }

    pub fn anchor(&self, slot: SlotId) -> Option<u32> {
        self.state.bases.slot(slot).expect("slot").list.selected
    }

    pub fn notice(&self) -> Option<&BasesNotice> {
        match &self.state.notice {
            Some(AppNotice::Bases(n)) => Some(n),
            _ => None,
        }
    }

    pub fn clear_notice(&mut self) {
        self.state.notice = None;
    }

    pub fn first(&self, slot: SlotId) -> u32 {
        self.state.bases.slot(slot).expect("slot").list.first
    }
}

/// `n` games `Pn` vs `Qn`, all in the filter.
pub fn numbered(n: usize) -> Vec<G> {
    (1..=n)
        .map(|i| G::new(&format!("P{i}"), &format!("Q{i}")))
        .collect()
}
