// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_tree_best`,
// `sc_tree_move`). Copyright (c) 1999-2002 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License (see
// its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The best games of the tree, and the choice of a move for training
//! (spec rules 20 to 22).

use mychess_core::{GameResult, Position};

use super::{TreeError, TreeResult, TreeState};
use crate::player_stats::PlayerStats;
use crate::si4::{ListPosition, RawGame, RawMove, bytes_to_string};
use crate::source::GameSource;

/// The most games [`best_games`] returns (Scid: 1000).
pub const MAX_BEST_GAMES: u32 = 1000;

/// How many half moves after the match each [`BestGame`] shows.
const NEXT_PLIES: usize = 6;

/// Which results [`best_games`] accepts (Scid: the `<results>` argument).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ResultFilter {
    /// Every result.
    #[default]
    All,
    /// White wins (`1-0`).
    White,
    /// Black wins (`0-1`).
    Black,
    /// White or Black wins (`1-0 0-1`).
    Decisive,
    /// Draws.
    Draw,
    /// Games without result (`*`).
    None,
}

impl ResultFilter {
    fn accepts(self, result: GameResult) -> bool {
        match self {
            ResultFilter::All => true,
            ResultFilter::White => result == GameResult::White,
            ResultFilter::Black => result == GameResult::Black,
            ResultFilter::Decisive => matches!(result, GameResult::White | GameResult::Black),
            ResultFilter::Draw => result == GameResult::Draw,
            ResultFilter::None => result == GameResult::Unknown,
        }
    }
}

/// The order of [`best_games`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BestOrder {
    /// By the average rating of the players, highest first; equal ratings
    /// stay in database order.
    #[default]
    Best,
    /// The last games of the filter, in database order.
    Order,
}

/// Options of [`best_games`].
#[derive(Debug, Clone, Copy, Default)]
pub struct BestOptions {
    /// How many games; limited to [`MAX_BEST_GAMES`]; 0 gives none.
    pub max: u32,
    /// Which results to accept.
    pub results: ResultFilter,
    /// The order.
    pub order: BestOrder,
    /// The ply of the current game in the interface, if it should replace the
    /// ply of the tree for a game that reaches the same position at that ply
    /// (a repetition), or for the current game itself.
    pub current_ply: Option<u8>,
    /// The number of the game currently loaded, if it is in this database.
    pub current_game: Option<u32>,
}

/// A game of the best-games list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BestGame {
    /// The game number (1-based).
    pub game: u32,
    /// The average rating that ranks the game (ratings missing in the index
    /// come from the name base, decision D946).
    pub avg_elo: u32,
    /// The ply of the position in the game.
    pub ply: u8,
    /// The SAN of up to 6 half moves from that position.
    pub next_moves: Vec<String>,
}

/// The best games of the last tree search (`sc_tree best`), from the games
/// of the tree filter of `state`. Without a previous search the list is empty.
///
/// # Errors
///
/// None at present (the `Result` is kept for the API). A game whose index
/// entry cannot be read is skipped. A record that cannot be replayed gives a game without
/// `next_moves`.
#[allow(clippy::unnecessary_wraps)] // the signature is part of the API (and of the tests)
pub fn best_games<D: GameSource + ?Sized>(
    db: &mut D,
    state: &TreeState,
    stats: &PlayerStats,
    opts: &BestOptions,
) -> Result<Vec<BestGame>, TreeError> {
    let max = opts.max.min(MAX_BEST_GAMES) as usize;
    let Some(filter) = state.tree_filter() else {
        return Ok(Vec::new());
    };
    if max == 0 {
        return Ok(Vec::new());
    }
    // (game, average rating) in the final order.
    let mut picked: Vec<(u32, u32)> = Vec::new();
    match opts.order {
        BestOrder::Best => {
            for n in filter.iter_games() {
                let Ok(entry) = db.entry(n) else {
                    continue;
                };
                if !opts.results.accepts(entry.result) {
                    continue;
                }
                let mut welo = u32::from(entry.white_elo);
                let mut belo = u32::from(entry.black_elo);
                if welo == 0 {
                    welo = u32::from(stats.max_elo(entry.white_id));
                }
                if belo == 0 {
                    belo = u32::from(stats.max_elo(entry.black_id));
                }
                let avg = u32::midpoint(welo, belo);
                // After every game that is at least as good.
                let at = picked.partition_point(|&(_, other)| other >= avg);
                if at < max {
                    picked.insert(at, (n, avg));
                    picked.truncate(max);
                }
            }
        }
        BestOrder::Order => {
            let mut n = filter.last();
            while let Some(game) = n {
                if picked.len() >= max {
                    break;
                }
                if db
                    .entry(game)
                    .is_ok_and(|entry| opts.results.accepts(entry.result))
                {
                    picked.push((game, 0));
                }
                n = filter.prev_before(game);
            }
            picked.reverse();
        }
    }
    let mut out = Vec::with_capacity(picked.len());
    for (game, avg_elo) in picked {
        let tree_ply = filter.ply(game);
        let raw = db.raw_game(game).ok();
        let ply = raw
            .as_ref()
            .map_or(tree_ply, |raw| display_ply(raw, game, tree_ply, opts));
        let next_moves = raw
            .as_ref()
            .map(|raw| next_moves(raw, usize::from(ply)))
            .unwrap_or_default();
        out.push(BestGame {
            game,
            avg_elo,
            ply,
            next_moves,
        });
    }
    Ok(out)
}

/// The ply shown for a game (Scid: "Cases to consider" in `sc_tree_best`).
fn display_ply(raw: &RawGame, game: u32, tree_ply: u8, opts: &BestOptions) -> u8 {
    let Some(current) = opts.current_ply else {
        return tree_ply;
    };
    if opts.current_game == Some(game) {
        return current;
    }
    if current == tree_ply || raw.start_fen.is_some() {
        return tree_ply;
    }
    // A repetition: the game is at the same position at the current ply.
    match (
        list_position_at(raw, usize::from(tree_ply)),
        list_position_at(raw, usize::from(current)),
    ) {
        (Some(a), Some(b)) if super::same_board(&a, &b) => current,
        _ => tree_ply,
    }
}

/// The position of the main line after `ply` half moves, when the game has
/// that many and they can be replayed.
fn list_position_at(raw: &RawGame, ply: usize) -> Option<ListPosition> {
    let mut pos = match &raw.start_fen {
        Some(fen) => ListPosition::from_fen(&bytes_to_string(fen)).ok()?,
        None => ListPosition::standard(),
    };
    for node in raw.mainline().get(..ply)? {
        pos.make(node.mv).ok()?;
    }
    Some(pos)
}

/// The SAN of up to [`NEXT_PLIES`] half moves of the main line from `ply`.
/// Stops at the first move that cannot be played.
fn next_moves(raw: &RawGame, ply: usize) -> Vec<String> {
    let mut pos = match &raw.start_fen {
        Some(fen) => match Position::from_fen(&bytes_to_string(fen)) {
            Ok(pos) => pos,
            Err(_) => return Vec::new(),
        },
        None => Position::new(),
    };
    let mut out = Vec::new();
    for (i, node) in raw.mainline().iter().enumerate() {
        if i >= ply + NEXT_PLIES {
            break;
        }
        let mv = if node.mv.is_null() {
            Some(mychess_core::Move::Null)
        } else {
            pos.find_move(node.mv.from, node.mv.to, node.mv.promotion)
        };
        let Some(mv) = mv else { break };
        if i >= ply {
            match pos.san(mv) {
                Ok(san) => out.push(san),
                Err(_) => break,
            }
        }
        if pos.play(mv).is_err() {
            break;
        }
    }
    out
}

/// Chooses a move at random, weighted by the number of games (training,
/// `sc_tree move random`). `rng` gives uniform 32-bit numbers.
///
/// The Scid picks `r` in `0..total` and takes the first row with
/// `r <= running sum`, which favours the first row by one game; here the test
/// is `r < running sum` (decision D945). The `[end]` row takes no part (D955).
/// `None` when no row has a move.
pub fn random_move(result: &TreeResult, rng: &mut impl FnMut() -> u32) -> Option<RawMove> {
    let total: u64 = result
        .rows
        .iter()
        .filter(|row| row.mv.is_some())
        .map(|row| u64::from(row.total))
        .sum();
    if total == 0 {
        return None;
    }
    let r = u64::from(rng()) % total;
    let mut sum = 0u64;
    for row in &result.rows {
        let Some(mv) = row.mv else { continue };
        sum += u64::from(row.total);
        if r < sum {
            return Some(mv);
        }
    }
    None
}

/// The move of row `n` (1-based, as the table is numbered). `None` for the
/// `[end]` row and for a number out of range.
pub fn move_by_index(result: &TreeResult, n: usize) -> Option<RawMove> {
    result.rows.get(n.checked_sub(1)?)?.mv
}
