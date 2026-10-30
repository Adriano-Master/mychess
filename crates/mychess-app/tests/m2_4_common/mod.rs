//! Shared helpers of the `m2_4_*` integration tests (spec
//! `m2-4-notacao-e-dialogos.md`).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

pub use mychess_app::*;
pub use mychess_core::{Game, GameResult, Nag, NodeId, Position, SanMode};

/// Plays the SAN `san` below `parent` of `game`.
pub fn san(game: &mut Game, parent: NodeId, san: &str) -> NodeId {
    let mv = game
        .position_at(parent)
        .unwrap()
        .parse_san(san, SanMode::Strict)
        .unwrap()
        .mv;
    game.add_move(parent, mv).unwrap()
}

/// Plays the whitespace-separated SANs one after the other; the nodes.
pub fn sans(game: &mut Game, parent: NodeId, text: &str) -> Vec<NodeId> {
    let mut at = parent;
    text.split_whitespace()
        .map(|s| {
            at = san(game, at, s);
            at
        })
        .collect()
}

pub fn nag(n: u16) -> Nag {
    Nag::new(n).unwrap()
}

/// Options of the Scid with the knobs of the test applied.
pub fn opts(f: impl FnOnce(&mut NotationOptions)) -> NotationOptions {
    let mut o = NotationOptions::default();
    f(&mut o);
    o
}

/// Inline layout, `$n` NAGs (the shape of plain PGN movetext).
pub fn plain() -> NotationOptions {
    opts(|o| {
        o.layout = NotationLayout::Inline;
        o.symbolic_nags = false;
        // M2-5 D249 hid the header by default; these tests predate it.
        o.header = HeaderMode::Full;
    })
}

pub fn flat(game: &Game, o: &NotationOptions) -> String {
    build_notation(game, o, 1).flat_text()
}

/// A state whose session holds `game`, with the cursor at the end.
pub fn state_with(game: Game) -> AppState {
    let mut state = AppState::default();
    state.session.replace_game(game);
    update(
        &mut state,
        Message::Game(GameMessage::Navigate(Nav::GameEnd)),
    );
    state
}

pub fn send(state: &mut AppState, message: GameMessage) -> Vec<Effect> {
    update(state, Message::Game(message))
}

/// Plays the SANs from the root.
pub fn main_line(game: &mut Game, text: &str) -> Vec<NodeId> {
    let root = game.root();
    sans(game, root, text)
}
