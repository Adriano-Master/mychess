//! Shared helpers of the `m8_1c_*` contract tests (spec
//! `docs/specs/m8-1-jogo-contra-engine.md`, section "M8-1c", slice A):
//! the helpers of M8-1a plus a generator of long quiet games.
//!
//! Only the public API of `mychess-app` is used.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

#[path = "../m8_1_common/mod.rs"]
pub mod base;

use std::collections::HashSet;
use std::time::Instant;

pub use base::*;
use mychess_app::{PlayController, PlayOutput};
use mychess_core::{Color, Move, Position};
use mychess_engine::Score;

/// The evaluation, from White's point of view, that makes the engine of a
/// game where the player has `player` see `engine_cp` centipawns.
pub fn white_pov(player: Color, engine_cp: i32) -> Score {
    Score::Cp(match player {
        Color::White => -engine_cp,
        Color::Black => engine_cp,
    })
}

fn board_key(pos: &Position) -> String {
    pos.fen()
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A deterministic quiet move for `pos`: no capture, no check, a position
/// not in `seen`. `step` rotates the choice.
pub fn quiet_move(pos: &Position, seen: &HashSet<String>, step: usize) -> Move {
    let mut all = pos.legal_moves();
    all.sort_by_key(|m| pos.uci(*m));
    let ok: Vec<Move> = all
        .iter()
        .copied()
        .filter(|m| {
            let Ok(after) = pos.after(*m) else {
                return false;
            };
            m.to().is_none_or(|sq| pos.piece_at(sq).is_none())
                && !after.is_check()
                && !after.legal_moves().is_empty()
                && !seen.contains(&board_key(&after))
        })
        .collect();
    assert!(!ok.is_empty(), "no quiet move in {}", pos.fen());
    ok[(step * 5 + 1) % ok.len()]
}

/// `plies` quiet moves from the standard position, as UCI text.
pub fn quiet_line(plies: usize) -> Vec<Move> {
    let mut pos = Position::new();
    let mut seen = HashSet::from([board_key(&pos)]);
    let mut line = Vec::new();
    for step in 0..plies {
        let mv = quiet_move(&pos, &seen, step);
        pos.play(mv).expect("legal");
        seen.insert(board_key(&pos));
        line.push(mv);
    }
    line
}

/// The line as an opening text `Name: san...`.
pub fn quiet_opening(name: &str, plies: usize) -> String {
    let mut pos = Position::new();
    let mut sans = Vec::new();
    for mv in quiet_line(plies) {
        sans.push(pos.san(mv).expect("san"));
        pos.play(mv).expect("legal");
    }
    format!("{name}: {}", sans.join(" "))
}

/// Plays `plies` more quiet moves (the player's through `player_move`, the
/// engine's through `opponent_moved` with `score(engine_move_index)`).
/// Positions already in the game are avoided, so no repetition ends it.
/// Time does not advance.
pub fn play_quiet(
    c: &mut PlayController,
    plies: usize,
    t: Instant,
    mut score: impl FnMut(usize) -> Option<Score>,
) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut pos = c.game().start_position().clone();
    seen.insert(board_key(&pos));
    for mv in c.position_moves() {
        pos.play(mv).expect("legal");
        seen.insert(board_key(&pos));
    }
    let mut engine_moves = 0;
    for step in 0..plies {
        let mv = quiet_move(c.position(), &seen, step + 3);
        if c.position().turn() == c.player_color() {
            let out = c.player_move(mv, t);
            assert!(
                out.iter().any(|o| matches!(o, PlayOutput::Moved(_))),
                "player move refused: {out:?}"
            );
        } else {
            let out = c.opponent_moved(Some(mv), None, t, score(engine_moves));
            engine_moves += 1;
            assert!(
                out.iter().any(|o| matches!(o, PlayOutput::Moved(_))),
                "engine move refused: {out:?}"
            );
        }
        let after = c.position().clone();
        seen.insert(board_key(&after));
    }
}

/// Extension to read the moves of the game of a controller.
pub trait MovesOf {
    fn position_moves(&self) -> Vec<Move>;
}

impl MovesOf for PlayController {
    fn position_moves(&self) -> Vec<Move> {
        let game = self.game();
        game.mainline()
            .filter(|id| *id != game.root())
            .filter_map(|id| game.mv(id).ok())
            .collect()
    }
}
