//! Helpers shared by the tests of the `eco` modules.

use std::path::{Path, PathBuf};

use mychess_core::{Game, SanMode};

use crate::si4::{GameFlags, Si4Writer};

/// A game from SAN moves.
pub(super) fn game_from_sans(moves: &[&str]) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    for san in moves {
        let position = game.position_at(node).unwrap().clone();
        let parsed = position.parse_san(san, SanMode::Strict).unwrap();
        node = game.add_move(node, parsed.mv).unwrap();
    }
    game
}

/// A small deterministic generator (xorshift64*).
pub(super) struct Rng(pub(super) u64);

impl Rng {
    pub(super) fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let x = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        usize::try_from(x >> 33).unwrap() % n.max(1)
    }
}

/// A random legal game of up to `max_plies` half-moves that picks among the
/// first `branching` legal moves, so that different games often share
/// positions.
pub(super) fn random_game(rng: &mut Rng, max_plies: usize, branching: usize) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    let plies = rng.below(max_plies + 1);
    for _ in 0..plies {
        let position = game.position_at(node).unwrap().clone();
        let moves = position.legal_moves();
        if moves.is_empty() {
            break;
        }
        let mv = moves[rng.below(moves.len().min(branching))];
        node = game.add_move(node, mv).unwrap();
    }
    game
}

/// Writes `games` to a new database in `dir`; returns the base path.
pub(super) fn make_db(dir: &Path, games: &[Game]) -> PathBuf {
    let base = dir.join("db");
    let mut writer = Si4Writer::create(&base, 1).unwrap();
    for game in games {
        writer.add_game(game, GameFlags::empty()).unwrap();
    }
    base
}
