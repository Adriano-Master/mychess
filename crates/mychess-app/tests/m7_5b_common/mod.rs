//! Shared helpers of the `m7_5b_*` contract tests (spec
//! `m7-5-janela-epd.md`, slice b).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::path::{Path, PathBuf};

pub use mychess_app::epd_window::{
    EpdCloseChoice, EpdEffect, EpdFileId, EpdMessage, EpdNotice, EpdPrefs, EpdSort, EpdTabView,
    EpdView,
};
pub use mychess_app::{
    AppNotice, AppState, Command, DiscardChoice, Effect, GameMessage, Message, PickKind,
    PrefChange, PrefsMessage, RequestId, perform, update,
};
use mychess_core::{Position, SanMode};

pub struct Rig {
    pub state: AppState,
    rt: tokio::runtime::Runtime,
}

impl Rig {
    pub fn new() -> Rig {
        Rig {
            state: AppState::default(),
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
    /// Returns the effects for the host.
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

    pub fn epd(&mut self, message: EpdMessage) -> Vec<Effect> {
        self.send(Message::Epd(message))
    }

    /// Opens `path` in the window and returns the id of its tab.
    pub fn open(&mut self, path: &Path) -> EpdFileId {
        self.epd(EpdMessage::OpenPath(path.to_path_buf()));
        let view = self.state.epd_view();
        view.active.expect("a file is open")
    }

    pub fn view(&self) -> EpdView {
        self.state.epd_view()
    }

    pub fn tab(&self) -> EpdTabView {
        let view = self.view();
        let id = view.active.expect("an active tab");
        view.tabs.into_iter().find(|t| t.id == id).expect("tab")
    }

    pub fn notice(&self) -> Option<EpdNotice> {
        match self.state.notice.as_ref() {
            Some(AppNotice::Epd(n)) => Some(n.clone()),
            _ => None,
        }
    }

    /// Plays `san` on the board of the editor.
    pub fn play(&mut self, san: &str) {
        self.send(GameMessage::EnterMoveText(san.to_owned()));
    }

    /// Selects the record of the standard position and the others in the
    /// order of the file.
    pub fn rows(&self) -> Vec<mychess_app::epd_window::EpdRowView> {
        let tab = self.tab();
        self.state.epd_rows(tab.id, 0..tab.rows)
    }
}

pub fn mv(position: &Position, san: &str) -> mychess_core::Move {
    position
        .parse_san(san, SanMode::Tolerant)
        .expect("legal move")
        .mv
}

/// The five positions of `cinco.epd`: the start and after 1.e4, 1.d4, 1.c4
/// and 1.Nf3.
pub fn five_positions() -> Vec<Position> {
    let start = Position::new();
    let mut list = vec![start.clone()];
    for san in ["e4", "d4", "c4", "Nf3"] {
        list.push(start.after(mv(&start, san)).expect("after"));
    }
    list
}

pub fn four_fields(position: &Position) -> String {
    position
        .fen()
        .split_ascii_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A synthetic EPD with five legal positions: one with `bm`, one with `am`,
/// one with an `id` that has a `;` inside quotes, one with `ce` and `pv`,
/// one without operations.
pub fn cinco_text() -> String {
    let p = five_positions();
    let ops = [
        "bm e4; id \"first\";",
        "am e5;",
        "id \"X;Y\";",
        "ce 25; pv Nf6 g3;",
        "",
    ];
    let mut out = String::new();
    for (position, ops) in p.iter().zip(ops) {
        out.push_str(&four_fields(position));
        if !ops.is_empty() {
            out.push(' ');
            out.push_str(ops);
        }
        out.push('\n');
    }
    out
}

pub fn write_cinco(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, cinco_text()).expect("write");
    path
}

/// The record at `row` of the list, as `(fen, id)`.
pub fn row_fens(rows: &[mychess_app::epd_window::EpdRowView]) -> Vec<String> {
    rows.iter().map(|r| r.fen.clone()).collect()
}
