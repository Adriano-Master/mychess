//! Shared helpers for the unit tests of the graphs module.

use mychess_core::{Game, Position};

/// A game with `moves` (UCI) on the main line; `comments[i]` (when present
/// and not empty) is the comment of move `i`.
pub(super) fn game_with(moves: &[&str], comments: &[&str]) -> Game {
    let mut game = Game::new();
    let mut pos = Position::new();
    let mut node = game.root();
    for (i, uci) in moves.iter().enumerate() {
        let mv = pos.parse_uci(uci).expect("legal test move");
        pos.play(mv).expect("play test move");
        node = game.add_move(node, mv).expect("add test move");
        if let Some(c) = comments.get(i).filter(|c| !c.is_empty()) {
            game.set_comment(node, c).expect("set comment");
        }
    }
    game
}

/// Twelve plies of a legal game (Ruy Lopez shuffle).
pub(super) const TWELVE: [&str; 12] = [
    "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6", "e1g1", "f8e7", "f1e1", "b7b5",
];
