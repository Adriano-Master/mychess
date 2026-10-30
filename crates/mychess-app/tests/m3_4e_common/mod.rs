//! Shared helpers of the `m3_4e_*` contract tests (spec
//! `m3-4-slots-clipbase-lista-de-partidas.md`, slice 4e: the game browser,
//! D595-D606).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

#[path = "../m3_4c_common/mod.rs"]
pub mod c;

use std::path::{Path, PathBuf};

pub use c::{G, Rig, l, make_si4, numbered, pgn, sel, write};
pub use mychess_app::{
    AppNotice, BaseError, BaseKind, BasesMessage, BasesNotice, BrowseMessage, BrowseMode,
    BrowserContent, BrowserState, Effect, GameMessage, GameOrigin, GameStep, ListMessage,
    MAX_BROWSER_GAMES, Message, Nav, PlayOptions, SlotId, grid_columns,
};

/// A game of six plies (`1. e4 e5 2. Nf3 Nc6 3. Bb5 a6`) played by `Wn`
/// against `Bn`.
pub fn six(n: usize) -> G {
    G::new(&format!("W{n}"), &format!("B{n}"))
        .event(&format!("Ev{n}"))
        .result("1-0")
        .moves("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 *")
}

/// `count` games of six plies.
pub fn sixes(count: usize) -> Vec<G> {
    (1..=count).map(six).collect()
}

pub fn browse(message: BrowseMessage) -> BasesMessage {
    BasesMessage::Browse(message)
}

pub fn open_msg(slot: SlotId, numbers: &[u32]) -> BasesMessage {
    browse(BrowseMessage::Open {
        slot,
        numbers: numbers.to_vec(),
    })
}

/// A PGN slot with `count` games of six plies.
pub fn setup(count: usize) -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "g.pgn", &sixes(count));
    (rig, slot, dir)
}

/// Opens the browser and lets the reading finish.
pub fn open_browser(rig: &mut Rig, slot: SlotId, numbers: &[u32]) {
    rig.send(open_msg(slot, numbers));
}

pub fn send(rig: &mut Rig, message: BrowseMessage) {
    rig.send(browse(message));
}

pub fn br(rig: &Rig) -> &BrowserState {
    rig.state.browser.as_ref().expect("the browser is open")
}

/// The ply of every board (`usize::MAX` for a board without a game).
pub fn plies(rig: &Rig) -> Vec<usize> {
    br(rig)
        .games
        .iter()
        .map(|g| g.ready().map_or(usize::MAX, mychess_app::ReadyGame::ply))
        .collect()
}

pub fn numbers_shown(rig: &Rig) -> Vec<u32> {
    br(rig).games.iter().map(|g| g.number).collect()
}

pub fn fen_of(rig: &Rig, board: usize) -> String {
    br(rig).games[board]
        .ready()
        .expect("ready")
        .position()
        .expect("position")
        .fen()
}

/// Plays `uci` at the end of the main line of the editor.
pub fn play(rig: &mut Rig, uci: &str) {
    rig.send(GameMessage::Navigate(Nav::GameEnd));
    let mv = rig.state.session.position().parse_uci(uci).expect("uci");
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
}

pub fn editor_white(rig: &Rig) -> String {
    rig.state.session.game().tags().white.clone()
}

pub fn editor_ply(rig: &Rig) -> usize {
    let ply = rig
        .state
        .session
        .game()
        .ply(rig.state.session.cursor())
        .expect("ply");
    usize::try_from(ply).unwrap()
}

pub fn has_notice(rig: &Rig, notice: &BasesNotice) -> bool {
    rig.notice() == Some(notice)
}
