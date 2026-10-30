//! Shared helpers of the `m4_7_wire_*` contract tests (spec
//! `m4-7-graficos-avaliacao-tempo.md`, app wiring D870-D875).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp
)]

pub use mychess_app::graphs::{GraphKind, GraphMessage, GraphPrefs, Invert};
pub use mychess_app::{AppState, GameMessage, Message, Nav, update};
use mychess_core::Game;
use mychess_pgn::read::{ReadOptions, parse_pgn_text};

/// A Lichess-style game (`[%eval]` and `[%clk]` after every move).
pub const LICHESS: &str = "[Event \"Rated Blitz game\"]\n[White \"alice\"]\n[Black \"bob\"]\n\
[WhiteElo \"1500\"]\n[BlackElo \"1480\"]\n[TimeControl \"300+3\"]\n[Result \"1-0\"]\n\n\
1. e4 { [%eval 0.17] [%clk 0:05:00] } 1... e5 { [%eval 0.25] [%clk 0:05:00] } \
2. Nf3 { [%eval 0.3] [%clk 0:04:58] } 2... Nc6 { [%eval -0.4] [%clk 0:04:55] } \
3. Bb5 { [%eval 0.5] [%clk 0:04:50] } 3... a6 { [%eval #-3] [%clk 0:04:53] } 1-0\n";

/// Parses the first game of `pgn`.
pub fn game_of(pgn: &str) -> Game {
    let text = if pgn.starts_with('[') {
        pgn.to_owned()
    } else {
        format!("[Event \"t\"]\n\n{pgn}")
    };
    parse_pgn_text(&text, &ReadOptions::default())
        .into_iter()
        .next()
        .expect("one game")
        .game
}

pub fn state_with(pgn: &str) -> AppState {
    let mut state = AppState::default();
    state.session.replace_game(game_of(pgn));
    state
}

pub fn send(state: &mut AppState, message: impl Into<Message>) {
    drop(update(state, message.into()));
}

pub fn open(state: &mut AppState) {
    send(state, GraphMessage::SetOpen(true));
}

pub fn ply(state: &AppState) -> u32 {
    state.session.game().ply(state.session.cursor()).unwrap()
}

pub fn close_to(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

pub fn nav(state: &mut AppState, nav: Nav) {
    send(state, Message::Game(GameMessage::Navigate(nav)));
}

/// A game of `n` plies (knights shuffling) with `comment` on every move.
pub fn shuffle_game(n: usize, comment: &str) -> Game {
    let mut game = Game::new();
    let mut pos = mychess_core::Position::new();
    let mut node = game.root();
    for i in 0..n {
        let uci = ["g1f3", "g8f6", "f3g1", "f6g8"][i % 4];
        let mv = pos.parse_uci(uci).expect("legal");
        pos.play(mv).expect("play");
        node = game.add_move(node, mv).expect("add");
        if !comment.is_empty() {
            game.set_comment(node, comment).expect("comment");
        }
    }
    game
}
