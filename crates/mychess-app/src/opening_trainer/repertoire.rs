//! The repertoire: a map from positions to the moves the user wants to play
//! (and the replies to expect), and the draw of a line from it (spec M8-5,
//! D2201-D2221).
//!
//! Scid builds the same thing as a list of `{fen {move nags ...}}` per game
//! (`tcl/tools/tactics.tcl`, `fillFen`) and scans every game at each move
//! (`getCM`). Here the games are merged once into a map keyed by the Zobrist
//! hash of the position, which already tells who moves.

use std::collections::HashMap;
use std::sync::Arc;

use mychess_core::{Color, Move, Position, ZobristHash};

/// Positions a repertoire may hold (D2210).
pub const MAX_POSITIONS: usize = 150_000;
/// Walks drawn to pick the line that needs training most (D2214).
pub const WALK_SAMPLES: usize = 6;
/// Walks drawn when lines are not repeated (D2220).
pub const WALK_SAMPLES_NO_REPEAT: usize = 24;
/// Moves listed for a position in the panel (D2217).
pub const MAX_CANDIDATES: usize = 12;

/// A move of the repertoire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepMove {
    /// The move.
    pub mv: Move,
    /// How often it is played: the number of games (or the weight of the
    /// book) that have it. Never zero.
    pub weight: u32,
    /// The move is dubious: every occurrence was marked so (D2205).
    pub dubious: bool,
}

/// The moves of one position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepNode {
    /// Who moves in the position.
    pub turn: Color,
    /// The moves, in the order they were first added.
    pub moves: Vec<RepMove>,
}

/// The moves a repertoire has, by position (D2201).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Repertoire {
    nodes: HashMap<ZobristHash, RepNode>,
    truncated: bool,
}

impl Repertoire {
    /// How many positions have a move.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether there is no position.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Whether the build stopped at a limit (D2210).
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// The node of a position.
    pub fn node(&self, position: &Position) -> Option<&RepNode> {
        self.nodes.get(&position.zobrist())
    }

    /// The moves of a position (empty when it is not in the repertoire).
    pub fn moves(&self, position: &Position) -> &[RepMove] {
        self.node(position).map_or(&[], |n| n.moves.as_slice())
    }

    /// The entry of `mv` in `position`.
    pub fn find(&self, position: &Position, mv: Move) -> Option<&RepMove> {
        self.moves(position).iter().find(|m| m.mv == mv)
    }

    /// The move to suggest: the heaviest that is not dubious, else the
    /// heaviest (the first of the ties, in insertion order).
    pub fn best(&self, position: &Position) -> Option<RepMove> {
        best_of(self.moves(position))
    }

    /// The positions where `color` moves, with their keys (D2224).
    pub fn positions_of(&self, color: Color) -> impl Iterator<Item = (ZobristHash, &RepNode)> {
        self.nodes
            .iter()
            .filter(move |(_, n)| n.turn == color && !n.moves.is_empty())
            .map(|(k, n)| (*k, n))
    }
}

fn best_of(moves: &[RepMove]) -> Option<RepMove> {
    let mut best: Option<RepMove> = None;
    for m in moves {
        let better = match best {
            None => true,
            Some(b) => (b.dubious && !m.dubious) || (b.dubious == m.dubious && m.weight > b.weight),
        };
        if better {
            best = Some(*m);
        }
    }
    best
}

/// Builds a [`Repertoire`] edge by edge (D2205).
#[derive(Debug, Default)]
pub struct RepBuilder {
    repertoire: Repertoire,
    /// Positions allowed; [`MAX_POSITIONS`] unless lowered (tests).
    limit: Option<usize>,
}

impl RepBuilder {
    /// An empty builder.
    pub fn new() -> RepBuilder {
        RepBuilder::default()
    }

    /// How many positions there are so far.
    pub fn len(&self) -> usize {
        self.repertoire.nodes.len()
    }

    /// Whether nothing was added.
    pub fn is_empty(&self) -> bool {
        self.repertoire.nodes.is_empty()
    }

    /// A builder that holds at most `limit` positions (at most
    /// [`MAX_POSITIONS`]).
    pub fn with_limit(limit: usize) -> RepBuilder {
        RepBuilder {
            limit: Some(limit.min(MAX_POSITIONS)),
            ..RepBuilder::default()
        }
    }

    /// Whether the limit of positions was reached.
    pub fn is_full(&self) -> bool {
        self.repertoire.nodes.len() >= self.limit.unwrap_or(MAX_POSITIONS)
    }

    /// Adds `weight` to the edge `position -> mv`. A new position beyond
    /// [`MAX_POSITIONS`] is refused (and the repertoire marked truncated).
    /// Returns whether the edge is in the repertoire. A move stays dubious
    /// only while every occurrence is.
    pub fn add(&mut self, position: &Position, mv: Move, weight: u32, dubious: bool) -> bool {
        let weight = weight.max(1);
        let key = position.zobrist();
        if !self.repertoire.nodes.contains_key(&key) && self.is_full() {
            self.repertoire.truncated = true;
            return false;
        }
        let node = self.repertoire.nodes.entry(key).or_insert_with(|| RepNode {
            turn: position.turn(),
            moves: Vec::new(),
        });
        if let Some(existing) = node.moves.iter_mut().find(|m| m.mv == mv) {
            existing.weight = existing.weight.saturating_add(weight);
            existing.dubious &= dubious;
        } else {
            node.moves.push(RepMove {
                mv,
                weight,
                dubious,
            });
        }
        true
    }

    /// Marks the repertoire as cut by a limit (games, depth of the walk...).
    pub fn mark_truncated(&mut self) {
        self.repertoire.truncated = true;
    }

    /// The repertoire.
    pub fn finish(self) -> Repertoire {
        self.repertoire
    }
}

/// The repertoires of the two colours (D2201, D2204). They may be the same.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepSet {
    /// For the lines where the user plays White.
    pub white: Option<Arc<Repertoire>>,
    /// For the lines where the user plays Black.
    pub black: Option<Arc<Repertoire>>,
}

impl RepSet {
    /// The repertoire for a user who plays `color`.
    pub fn for_color(&self, color: Color) -> Option<&Arc<Repertoire>> {
        match color {
            Color::White => self.white.as_ref(),
            Color::Black => self.black.as_ref(),
        }
    }
}

/// A small generator (xorshift64*): the draw depends only on the seed that
/// the front-end sends, never on the clock (D2214).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rng(u64);

impl Rng {
    /// A generator from a seed (zero is replaced, the state must not be 0).
    pub fn new(seed: u64) -> Rng {
        Rng(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    /// The next 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..n` (`0` when `n` is 0).
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }
}

/// Draws one of `moves` with probability proportional to the weight (D2214).
/// With `best_only` the dubious ones are left out unless they are all there
/// is (D2218).
pub fn pick_weighted(moves: &[RepMove], best_only: bool, rng: &mut Rng) -> Option<RepMove> {
    let any_good = moves.iter().any(|m| !m.dubious);
    let eligible: Vec<&RepMove> = moves
        .iter()
        .filter(|m| !(best_only && any_good && m.dubious))
        .collect();
    let total: u64 = eligible.iter().map(|m| u64::from(m.weight)).sum();
    if total == 0 {
        return None;
    }
    let mut ticket = rng.below(total);
    for m in &eligible {
        let w = u64::from(m.weight);
        if ticket < w {
            return Some(**m);
        }
        ticket -= w;
    }
    eligible.last().map(|m| **m)
}

/// A line drawn from the repertoire: the target of a session line (D2214).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk {
    /// The moves from the initial position, both sides.
    pub moves: Vec<Move>,
    /// The need of the positions of the user (D2223).
    pub need: u32,
    /// Identifies the line (the hash of the positions it goes through).
    pub id: u64,
}

/// Draws a walk from the initial position (D2214): the user follows the
/// heaviest move, the other side is drawn by weight. `need` gives the need
/// of a position of the user by its hash.
///
/// Limitation (D2260): the alternatives of the user himself are accepted when
/// he plays them but never proposed, because the walk only steers the
/// replies of the machine and the hint shows the heaviest move; lines that
/// start from a lighter move of the user are reached only when he chooses it.
pub fn sample_walk(
    rep: &Repertoire,
    user: Color,
    max_depth: usize,
    opponent_best: bool,
    rng: &mut Rng,
    need: &dyn Fn(u64) -> u32,
) -> Walk {
    let mut position = Position::new();
    let mut moves = Vec::new();
    let mut total_need = 0u32;
    let mut id = 0xCBF2_9CE4_8422_2325u64;
    while moves.len() < max_depth {
        let Some(node) = rep.node(&position) else {
            break;
        };
        let chosen = if position.turn() == user {
            total_need = total_need.saturating_add(need(position.zobrist().value()));
            best_of(&node.moves)
        } else {
            pick_weighted(&node.moves, opponent_best, rng)
        };
        let Some(chosen) = chosen else {
            break;
        };
        id = (id ^ position.zobrist().value()).wrapping_mul(0x0100_0000_01B3);
        if position.play(chosen.mv).is_err() {
            break;
        }
        moves.push(chosen.mv);
    }
    Walk {
        moves,
        need: total_need,
        id,
    }
}

/// Whether the walk has a move of the user (the repertoire is playable for
/// that colour, D2212).
pub fn has_user_move(walk: &Walk, user: Color) -> bool {
    let first_user_ply = usize::from(user == Color::Black);
    walk.moves.len() > first_user_ply
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(position: &Position, uci: &str) -> Move {
        position.parse_uci(uci).expect("legal uci")
    }

    fn small() -> Repertoire {
        // 1.e4 e5 (w 3) / 1.e4 c5 (w 1); 2.Nf3 after 1...e5; 1...a6?! dubious.
        let mut b = RepBuilder::new();
        let start = Position::new();
        b.add(&start, mv(&start, "e2e4"), 4, false);
        let after_e4 = start.after(mv(&start, "e2e4")).expect("e4");
        b.add(&after_e4, mv(&after_e4, "e7e5"), 3, false);
        b.add(&after_e4, mv(&after_e4, "c7c5"), 1, false);
        b.add(&after_e4, mv(&after_e4, "a7a6"), 2, true);
        let after_e5 = after_e4.after(mv(&after_e4, "e7e5")).expect("e5");
        b.add(&after_e5, mv(&after_e5, "g1f3"), 3, false);
        b.finish()
    }

    #[test]
    fn a_repeated_edge_adds_its_weight_and_is_dubious_only_if_all_are() {
        let mut b = RepBuilder::new();
        let start = Position::new();
        let e4 = mv(&start, "e2e4");
        b.add(&start, e4, 1, true);
        b.add(&start, e4, 2, true);
        assert!(b.add(&start, e4, 1, false));
        let rep = b.finish();
        let m = rep.find(&start, e4).expect("move");
        assert_eq!((m.weight, m.dubious), (4, false));
        assert_eq!(rep.moves(&start).len(), 1);
    }

    #[test]
    fn the_best_move_is_the_heaviest_that_is_not_dubious() {
        let rep = small();
        let start = Position::new();
        let after_e4 = start.after(mv(&start, "e2e4")).expect("e4");
        // a6 has weight 2 but is dubious; e5 (3) wins over c5 (1).
        assert_eq!(rep.best(&after_e4).map(|m| m.weight), Some(3));
        assert!(
            rep.moves(&Position::from_fen("8/8/8/8/8/8/8/K6k w - - 0 1").expect("fen"))
                .is_empty()
        );
    }

    #[test]
    fn weighted_pick_follows_the_weights_and_skips_dubious_moves() {
        let rep = small();
        let start = Position::new();
        let after_e4 = start.after(mv(&start, "e2e4")).expect("e4");
        let moves = rep.moves(&after_e4);
        let mut rng = Rng::new(7);
        let (mut e5, mut c5) = (0u32, 0u32);
        for _ in 0..4000 {
            let m = pick_weighted(moves, true, &mut rng).expect("pick");
            assert!(!m.dubious, "dubious moves are left out");
            if m.mv == mv(&after_e4, "e7e5") {
                e5 += 1;
            } else {
                c5 += 1;
            }
        }
        // 3:1 within a wide margin.
        assert!(e5 > 2 * c5 && c5 > 0, "e5 {e5} c5 {c5}");
        // With best_only off the dubious one shows up.
        let mut rng = Rng::new(7);
        assert!((0..400).any(|_| pick_weighted(moves, false, &mut rng).is_some_and(|m| m.dubious)));
    }

    #[test]
    fn all_dubious_moves_are_used_when_there_is_nothing_else() {
        let start = Position::new();
        let mut b = RepBuilder::new();
        b.add(&start, mv(&start, "a2a3"), 1, true);
        let rep = b.finish();
        let mut rng = Rng::new(1);
        assert!(pick_weighted(rep.moves(&start), true, &mut rng).is_some());
    }

    #[test]
    fn a_walk_is_deterministic_for_a_seed_and_stops_where_the_repertoire_ends() {
        let rep = small();
        let need = |_: u64| 1u32;
        let a = sample_walk(&rep, Color::White, 16, true, &mut Rng::new(3), &need);
        let b = sample_walk(&rep, Color::White, 16, true, &mut Rng::new(3), &need);
        assert_eq!(a, b);
        // e4, then a reply, then Nf3 only after e5.
        assert!(a.moves.len() == 2 || a.moves.len() == 3);
        assert_eq!(a.need, if a.moves.len() == 3 { 2 } else { 1 });
        let short = sample_walk(&rep, Color::White, 1, true, &mut Rng::new(3), &need);
        assert_eq!(short.moves.len(), 1, "the depth cuts the walk");
    }

    #[test]
    fn the_walk_of_a_user_with_black_starts_with_the_reply_of_the_machine() {
        let rep = small();
        let need = |_: u64| 5u32;
        let walk = sample_walk(&rep, Color::Black, 16, true, &mut Rng::new(9), &need);
        assert!(has_user_move(&walk, Color::Black));
        let none = Walk {
            moves: vec![],
            need: 0,
            id: 0,
        };
        assert!(!has_user_move(&none, Color::White));
    }

    #[test]
    fn the_builder_refuses_new_positions_beyond_its_limit_but_still_adds_moves_to_known_ones() {
        let mut b = RepBuilder::with_limit(1);
        let start = Position::new();
        assert!(b.add(&start, mv(&start, "e2e4"), 1, false));
        let after = start.after(mv(&start, "e2e4")).expect("e4");
        assert!(b.is_full());
        assert!(
            !b.add(&after, mv(&after, "e7e5"), 1, false),
            "a new position"
        );
        assert!(b.add(&start, mv(&start, "d2d4"), 1, false), "a known one");
        let rep = b.finish();
        assert!(rep.truncated());
        assert_eq!((rep.len(), rep.moves(&start).len()), (1, 2));
    }

    #[test]
    fn the_generator_never_sticks_at_zero() {
        let mut rng = Rng::new(0);
        assert_ne!(rng.next_u64(), 0);
        assert_eq!(rng.below(0), 0);
    }
}
