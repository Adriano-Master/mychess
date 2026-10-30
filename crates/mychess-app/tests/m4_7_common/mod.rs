//! Shared builders for the M4-7 graph tests (public API only).
#![allow(dead_code, unreachable_pub)]

use mychess_core::{Game, NodeId, Position};

/// Knight shuffle: legal from the start position for any number of plies.
const SHUFFLE: [&str; 4] = ["g1f3", "g8f6", "f3g1", "f6g8"];

/// A game of `n` plies on the main line; `comment(i)` is the comment of the
/// move at (0-based) index `i`, when it returns `Some`.
pub fn shuffle_game(n: usize, mut comment: impl FnMut(usize) -> Option<String>) -> Game {
    let mut game = Game::new();
    let mut pos = Position::new();
    let mut node = game.root();
    for i in 0..n {
        let mv = pos.parse_uci(SHUFFLE[i % 4]).expect("legal move");
        pos.play(mv).expect("play");
        node = game.add_move(node, mv).expect("add move");
        if let Some(c) = comment(i) {
            game.set_comment(node, &c).expect("comment");
        }
    }
    game
}

/// A game with the given comments, one per ply (empty string = none).
pub fn game_with(comments: &[&str]) -> Game {
    shuffle_game(comments.len(), |i| {
        Some(comments[i].to_owned()).filter(|c| !c.is_empty())
    })
}

/// The main-line node after `ply` halfmoves (0 = root).
pub fn node_at(game: &Game, ply: usize) -> NodeId {
    if ply == 0 {
        game.root()
    } else {
        game.mainline().nth(ply - 1).expect("ply exists")
    }
}

/// `[%emt h:m:s]` for a number of seconds.
pub fn emt(seconds: u32) -> String {
    format!(
        "[%emt {}:{:02}:{:02}]",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// `[%clk h:m:s]` for a number of seconds.
pub fn clk(seconds: u32) -> String {
    format!(
        "[%clk {}:{:02}:{:02}]",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// A game with a `%clk` on every ply, starting from the given clocks.
pub fn clock_game(clocks: &[u32], time_control: Option<&str>) -> Game {
    let mut game = shuffle_game(clocks.len(), |i| Some(clk(clocks[i])));
    if let Some(tc) = time_control {
        game.tags_mut().set_extra("TimeControl", tc).expect("tag");
    }
    game
}
