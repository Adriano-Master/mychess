//! Forced-mate search: "mate in N" problems without an engine (spec M8-4,
//! D1906-D1909).
//!
//! The search is exhaustive and deterministic: the attacker tries every
//! move and the defender every reply (alternating, with iterative
//! deepening), so the first depth that succeeds is the **shortest** forced
//! mate. A node budget bounds the work: when it runs out the answer is
//! [`MateResult::Unknown`] (or [`DefenceResult::Unknown`]), never an error and
//! never a panic.
//!
//! Counting: "mate in N" is `N` moves of the attacker, so a mate that the
//! attacker delivers with the move he is about to play is a mate in 1.
//!
//! ```
//! use mychess_core::Position;
//! use mychess_core::mate::{MateLimits, MateResult, find_mate};
//!
//! let pos = Position::from_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1")?;
//! let result = find_mate(&pos, MateLimits { max_moves: 3, max_nodes: 100_000 });
//! assert!(matches!(result, MateResult::Mate { moves: 1, .. }));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::HashMap;

use crate::{Move, Position};

/// The longest mate the search looks for, whatever the caller asks.
pub const MAX_MATE_MOVES: u8 = 8;

/// Most entries kept in the table of transpositions.
const TABLE_CAP: usize = 1_000_000;

/// How far a search goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MateLimits {
    /// Longest mate looked for, in moves of the attacker (at most
    /// [`MAX_MATE_MOVES`]).
    pub max_moves: u8,
    /// Positions the search may generate before it gives up.
    pub max_nodes: u64,
}

/// The answer of [`find_mate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MateResult {
    /// The side to move forces mate in `moves` moves (the shortest), and
    /// `first` starts it.
    Mate {
        /// Length of the mate, in moves of the attacker.
        moves: u8,
        /// A first move that forces it.
        first: Move,
    },
    /// Proved: there is no forced mate within the limit.
    NoMate,
    /// The node budget ran out before the question was settled.
    Unknown,
}

/// The answer of [`best_defence`]: the defender is the side to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DefenceResult {
    /// The side to move is already checkmated.
    Mated,
    /// Every reply loses to a mate in `moves` more moves of the attacker;
    /// `reply` is the one that resists longest (ties: the first in the
    /// alphabetical order of the UCI notation).
    Forced {
        /// The mate that is left after the reply, in moves of the attacker.
        moves: u8,
        /// The best defence.
        reply: Move,
    },
    /// Proved: the defender has a reply that avoids a mate within the limit
    /// (or is stalemated).
    Escapes,
    /// The node budget ran out.
    Unknown,
}

/// The node budget is spent.
struct OutOfNodes;

/// A cancellation hook: returns `true` when the search must stop.
pub type Interrupt<'a> = &'a dyn Fn() -> bool;

/// Nodes between two consultations of the [`Interrupt`] hook.
const INTERRUPT_STRIDE: u64 = 4096;

struct Solver<'a> {
    nodes_left: u64,
    table: HashMap<(u64, u8), bool>,
    interrupt: Option<Interrupt<'a>>,
    since_check: u64,
}

/// The children of a position, ordered for the search: checks, captures,
/// then the rest (stable inside each class).
fn children(solver: &mut Solver<'_>, pos: &Position) -> Result<Vec<(Move, Position)>, OutOfNodes> {
    let moves = pos.legal_moves();
    let count = u64::try_from(moves.len()).unwrap_or(u64::MAX);
    if solver.nodes_left < count {
        solver.nodes_left = 0;
        return Err(OutOfNodes);
    }
    solver.nodes_left -= count;
    if let Some(stop) = solver.interrupt {
        solver.since_check += count;
        if solver.since_check >= INTERRUPT_STRIDE {
            solver.since_check = 0;
            if stop() {
                solver.nodes_left = 0;
                return Err(OutOfNodes);
            }
        }
    }
    let mut kids: Vec<(u8, Move, Position)> = moves
        .into_iter()
        .filter_map(|m| {
            let next = pos.after(m).ok()?;
            let class = if next.is_check() {
                0
            } else {
                u8::from(!m.is_capture()) + 1
            };
            Some((class, m, next))
        })
        .collect();
    kids.sort_by_key(|(class, ..)| *class);
    Ok(kids.into_iter().map(|(_, m, p)| (m, p)).collect())
}

impl<'a> Solver<'a> {
    fn new(max_nodes: u64, interrupt: Option<Interrupt<'a>>) -> Solver<'a> {
        Solver {
            nodes_left: max_nodes,
            table: HashMap::new(),
            interrupt,
            since_check: 0,
        }
    }

    /// Whether the side to move mates in at most `n` moves (memoised).
    fn attack(&mut self, pos: &Position, n: u8) -> Result<bool, OutOfNodes> {
        if n == 0 {
            return Ok(false);
        }
        let key = (u64::from(pos.zobrist()), n);
        if let Some(known) = self.table.get(&key) {
            return Ok(*known);
        }
        let won = self.mating_move(pos, n)?.is_some();
        if self.table.len() < TABLE_CAP {
            self.table.insert(key, won);
        }
        Ok(won)
    }

    /// A move of the side to move that forces mate in at most `n` moves.
    fn mating_move(&mut self, pos: &Position, n: u8) -> Result<Option<Move>, OutOfNodes> {
        // Without mating material nothing is forced (K+B or K+N alone).
        if n == 0 || pos.has_insufficient_material(pos.turn()) {
            return Ok(None);
        }
        for (m, next) in children(self, pos)? {
            if next.is_check() && next.is_checkmate() {
                return Ok(Some(m));
            }
            if n == 1 || next.is_stalemate() {
                continue;
            }
            if self.defend(&next, n - 1)? {
                return Ok(Some(m));
            }
        }
        Ok(None)
    }

    /// Whether every reply of the side to move loses to a mate in at most
    /// `n` moves of the opponent.
    fn defend(&mut self, pos: &Position, n: u8) -> Result<bool, OutOfNodes> {
        let replies = children(self, pos)?;
        if replies.is_empty() {
            // Mate or stalemate: the caller screened both out.
            return Ok(false);
        }
        for (_, next) in replies {
            if !self.attack(&next, n)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The reply that resists longest in a position that `defend(pos, n)`
    /// proved lost: the first (by UCI text) whose position is not a mate in
    /// `n - 1`.
    fn longest_reply(&mut self, pos: &Position, n: u8) -> Result<Option<Move>, OutOfNodes> {
        let mode = pos.castling_mode();
        let mut replies = children(self, pos)?;
        replies.sort_by_cached_key(|(m, _)| m.to_uci(mode));
        for (m, next) in replies {
            if !self.attack(&next, n.saturating_sub(1))? {
                return Ok(Some(m));
            }
        }
        Ok(None)
    }
}

/// Looks for the shortest forced mate for the side to move.
///
/// Iterative deepening from 1 to `limits.max_moves` (at most
/// [`MAX_MATE_MOVES`]): the first depth that succeeds is the answer, so the
/// mate is the shortest one. A position without legal moves has no mate.
pub fn find_mate(pos: &Position, limits: MateLimits) -> MateResult {
    find_mate_with(pos, limits, None)
}

/// [`find_mate`] with a cancellation hook, consulted every 4096 nodes; when
/// it answers `true` the result is [`MateResult::Unknown`]. Without a hook
/// (or while it answers `false`) the result is exactly that of
/// [`find_mate`].
pub fn find_mate_with(
    pos: &Position,
    limits: MateLimits,
    interrupt: Option<Interrupt<'_>>,
) -> MateResult {
    let max = limits.max_moves.min(MAX_MATE_MOVES);
    let mut solver = Solver::new(limits.max_nodes, interrupt);
    for n in 1..=max {
        match solver.mating_move(pos, n) {
            Ok(Some(first)) => return MateResult::Mate { moves: n, first },
            Ok(None) => {}
            Err(OutOfNodes) => return MateResult::Unknown,
        }
    }
    MateResult::NoMate
}

/// The line of the shortest forced mate: the moves of the attacker
/// alternate with the best defence (the reply that resists longest), and the
/// last move gives mate. `None` when there is no mate within `max_moves` or
/// the budget runs out.
pub fn principal_line(pos: &Position, max_moves: u8, max_nodes: u64) -> Option<Vec<Move>> {
    let limits = MateLimits {
        max_moves,
        max_nodes,
    };
    let MateResult::Mate { moves, .. } = find_mate(pos, limits) else {
        return None;
    };
    let mut solver = Solver::new(max_nodes, None);
    let mut line = Vec::new();
    let mut current = pos.clone();
    let mut n = moves;
    loop {
        let m = solver.mating_move(&current, n).ok()??;
        line.push(m);
        let next = current.after(m).ok()?;
        if next.is_checkmate() {
            return Some(line);
        }
        let reply = solver.longest_reply(&next, n - 1).ok()??;
        line.push(reply);
        current = next.after(reply).ok()?;
        n -= 1;
    }
}

/// Judges a position in which the **defender** is to move: whether every
/// reply loses to a forced mate within `max_moves` moves of the attacker,
/// and which reply resists longest.
pub fn best_defence(pos: &Position, max_moves: u8, max_nodes: u64) -> DefenceResult {
    best_defence_with(pos, max_moves, max_nodes, None)
}

/// [`best_defence`] with a cancellation hook (see [`find_mate_with`]); a
/// cancelled search answers [`DefenceResult::Unknown`].
pub fn best_defence_with(
    pos: &Position,
    max_moves: u8,
    max_nodes: u64,
    interrupt: Option<Interrupt<'_>>,
) -> DefenceResult {
    if pos.is_checkmate() {
        return DefenceResult::Mated;
    }
    if pos.is_stalemate() {
        return DefenceResult::Escapes;
    }
    let mut solver = Solver::new(max_nodes, interrupt);
    for n in 1..=max_moves.min(MAX_MATE_MOVES) {
        match solver.defend(pos, n) {
            Ok(true) => {
                return match solver.longest_reply(pos, n) {
                    Ok(Some(reply)) => DefenceResult::Forced { moves: n, reply },
                    Ok(None) => DefenceResult::Escapes,
                    Err(OutOfNodes) => DefenceResult::Unknown,
                };
            }
            Ok(false) => {}
            Err(OutOfNodes) => return DefenceResult::Unknown,
        }
    }
    DefenceResult::Escapes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("test fen")
    }

    fn limits(max_moves: u8) -> MateLimits {
        MateLimits {
            max_moves,
            max_nodes: 2_000_000,
        }
    }

    #[test]
    fn finds_a_back_rank_mate_in_one() {
        let p = pos("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
        match find_mate(&p, limits(3)) {
            MateResult::Mate { moves, first } => {
                assert_eq!(moves, 1);
                assert_eq!(p.uci(first), "a1a8");
            }
            other => panic!("expected a mate in one, got {other:?}"),
        }
    }

    #[test]
    fn finds_the_loyd_mate_in_two() {
        let p = pos("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1");
        match find_mate(&p, limits(3)) {
            MateResult::Mate { moves, first } => {
                assert_eq!(moves, 2);
                assert_eq!(p.uci(first), "a1a6");
            }
            other => panic!("expected a mate in two, got {other:?}"),
        }
    }

    #[test]
    fn the_stalemate_trap_is_not_the_mating_move() {
        // Qf7 would stalemate; the move found must mate.
        let p = pos("7k/8/6K1/8/8/8/8/5Q2 w - - 0 1");
        match find_mate(&p, limits(2)) {
            MateResult::Mate { moves, first } => {
                assert_eq!(moves, 1);
                let after = p.after(first).expect("legal");
                assert!(after.is_checkmate(), "{} must mate", p.uci(first));
            }
            other => panic!("expected a mate in one, got {other:?}"),
        }
    }

    #[test]
    fn reports_no_mate_when_none_exists_within_the_limit() {
        assert_eq!(find_mate(&Position::new(), limits(2)), MateResult::NoMate);
    }

    #[test]
    fn a_position_without_legal_moves_has_no_mate() {
        // Black is already mated: nothing to play for the side to move.
        let p = pos("R5k1/5ppp/8/8/8/8/8/6K1 b - - 0 1");
        assert!(p.is_checkmate());
        assert_eq!(find_mate(&p, limits(3)), MateResult::NoMate);
    }

    #[test]
    fn a_tiny_budget_gives_unknown_instead_of_a_wrong_answer() {
        let p = pos("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1");
        let result = find_mate(
            &p,
            MateLimits {
                max_moves: 3,
                max_nodes: 3,
            },
        );
        assert_eq!(result, MateResult::Unknown);
    }

    #[test]
    fn insufficient_mating_material_has_no_mate() {
        // K+B against K cannot mate.
        let p = pos("7k/8/6K1/8/8/8/8/5B2 w - - 0 1");
        assert_eq!(find_mate(&p, limits(3)), MateResult::NoMate);
    }

    #[test]
    fn the_principal_line_is_legal_and_ends_in_mate() {
        let p = pos("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1");
        let line = principal_line(&p, 3, 2_000_000).expect("a mate in two");
        assert_eq!(line.len(), 3);
        let mut cur = p;
        for m in &line {
            cur.play(*m).expect("legal");
        }
        assert!(cur.is_checkmate());
    }

    #[test]
    fn the_principal_line_of_a_mate_in_one_is_one_move() {
        let p = pos("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
        let line = principal_line(&p, 3, 100_000).expect("mate");
        assert_eq!(line.len(), 1);
    }

    #[test]
    fn the_best_defence_resists_longest_and_is_deterministic() {
        let p = pos("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1");
        let after = p.after(p.parse_uci("a1a6").expect("uci")).expect("legal");
        let first = best_defence(&after, 3, 1_000_000);
        let again = best_defence(&after, 3, 1_000_000);
        assert_eq!(first, again);
        match first {
            DefenceResult::Forced { moves, .. } => assert_eq!(moves, 1),
            other => panic!("expected a forced mate, got {other:?}"),
        }
    }

    #[test]
    fn the_defence_escapes_when_no_forced_mate_exists() {
        let after = pos("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1")
            .after(
                Position::from_fen("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1")
                    .expect("fen")
                    .parse_uci("a1a2")
                    .expect("uci"),
            )
            .expect("legal");
        assert_eq!(best_defence(&after, 2, 1_000_000), DefenceResult::Escapes);
    }

    #[test]
    fn a_mated_defender_is_reported() {
        let p = pos("R5k1/5ppp/8/8/8/8/8/6K1 b - - 0 1");
        assert_eq!(best_defence(&p, 2, 1000), DefenceResult::Mated);
    }

    #[test]
    fn a_stalemated_defender_escapes() {
        let p = pos("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
        assert!(p.is_stalemate());
        assert_eq!(best_defence(&p, 2, 1000), DefenceResult::Escapes);
    }

    #[test]
    fn max_moves_above_the_hard_cap_is_clamped() {
        let p = pos("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
        let result = find_mate(
            &p,
            MateLimits {
                max_moves: 200,
                max_nodes: 100_000,
            },
        );
        assert!(matches!(result, MateResult::Mate { moves: 1, .. }));
    }
}
