//! Mate in N as a tactical detector, on top of [`crate::mate`] (D3267).

use crate::Position;
use crate::mate::{
    DefenceResult, Interrupt, MAX_MATE_MOVES, MateLimits, MateResult, best_defence_with,
    find_mate_with,
};

/// What a mate query found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MateVerdict {
    /// The side to move has a forced mate whose **shortest** length is in
    /// the asked range.
    Mate,
    /// Proved: there is not (the shortest mate is outside the range, or
    /// there is none).
    NoMate,
    /// The node budget ran out before the question was settled.
    Unknown,
}

/// The question of a mate search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MateQuery {
    /// Shortest acceptable mate, in moves of the attacker (1..=8).
    pub min_moves: u8,
    /// Longest acceptable mate (1..=8, at least `min_moves`).
    pub max_moves: u8,
    /// `available` only: the first move of the attacker must give check.
    pub forcing_only: bool,
    /// Node budget **per position**.
    pub max_nodes: u32,
}

impl MateQuery {
    fn max(self) -> u8 {
        self.max_moves.min(MAX_MATE_MOVES)
    }
}

/// "Played" mode: `pos` has the attacker to move and the game that was
/// played delivers checkmate after `plies_to_mate` more plies. Only that
/// distance is tested: it must be odd, with `n = plies_to_mate.div_ceil(2)`
/// inside the query range, and then one [`crate::mate::find_mate`] with `max_moves = n`
/// decides whether the shortest forced mate is at least `min_moves`.
pub fn mate_in_played(pos: &Position, plies_to_mate: u32, query: MateQuery) -> MateVerdict {
    mate_in_played_with(pos, plies_to_mate, query, None)
}

/// [`mate_in_played`] with a cancellation hook, consulted every 4096 nodes
/// of the solver; a cancelled search answers [`MateVerdict::Unknown`]. The
/// hook is a separate argument (not a field of [`MateQuery`]) so the query
/// stays `Copy` and comparable. Without cancellation the answer is the same
/// as [`mate_in_played`].
pub fn mate_in_played_with(
    pos: &Position,
    plies_to_mate: u32,
    query: MateQuery,
    interrupt: Option<Interrupt<'_>>,
) -> MateVerdict {
    if plies_to_mate.is_multiple_of(2) {
        return MateVerdict::NoMate;
    }
    let n = plies_to_mate.div_ceil(2);
    if n < u32::from(query.min_moves) || n > u32::from(query.max()) {
        return MateVerdict::NoMate;
    }
    let Ok(n) = u8::try_from(n) else {
        return MateVerdict::NoMate;
    };
    let limits = MateLimits {
        max_moves: n,
        max_nodes: u64::from(query.max_nodes),
    };
    match find_mate_with(pos, limits, interrupt) {
        MateResult::Mate { moves, .. } if moves >= query.min_moves => MateVerdict::Mate,
        MateResult::Unknown => MateVerdict::Unknown,
        _ => MateVerdict::NoMate,
    }
}

/// "Available" mode: does the side to move have a forced mate whose
/// shortest length is in the query range? Pruned by insufficient material.
///
/// With `forcing_only` only first moves that give check are tried; each one
/// is judged by [`crate::mate::best_defence`] with an equal share of `max_nodes`
/// (`max_nodes / number of checks`), so the total work is bounded and the
/// answer is deterministic. Without it, [`crate::mate::find_mate`] runs with the whole
/// budget.
pub fn mate_in_available(pos: &Position, query: MateQuery) -> MateVerdict {
    mate_in_available_with(pos, query, None)
}

/// [`mate_in_available`] with a cancellation hook (see
/// [`mate_in_played_with`]): consulted every 4096 nodes and before each
/// first move of the `forcing_only` search; cancelled means
/// [`MateVerdict::Unknown`].
pub fn mate_in_available_with(
    pos: &Position,
    query: MateQuery,
    interrupt: Option<Interrupt<'_>>,
) -> MateVerdict {
    let attacker = pos.turn();
    if pos.has_insufficient_material(attacker) || pos.is_checkmate() || pos.is_stalemate() {
        return MateVerdict::NoMate;
    }
    let max = query.max();
    if !query.forcing_only {
        let limits = MateLimits {
            max_moves: max,
            max_nodes: u64::from(query.max_nodes),
        };
        return match find_mate_with(pos, limits, interrupt) {
            MateResult::Mate { moves, .. } if moves >= query.min_moves => MateVerdict::Mate,
            MateResult::Unknown => MateVerdict::Unknown,
            _ => MateVerdict::NoMate,
        };
    }
    let mut checks = Vec::new();
    for mv in pos.legal_moves() {
        if let Ok(next) = pos.after(mv)
            && next.is_check()
        {
            checks.push(next);
        }
    }
    if checks.is_empty() {
        return MateVerdict::NoMate;
    }
    let share = (u64::from(query.max_nodes) / checks.len() as u64).max(1);
    let mut shortest: Option<u8> = None;
    let mut unknown = false;
    for next in &checks {
        if interrupt.is_some_and(|stop| stop()) {
            return MateVerdict::Unknown;
        }
        let length = if next.is_checkmate() {
            Some(1)
        } else if max < 2 {
            None
        } else {
            match best_defence_with(next, max - 1, share, interrupt) {
                DefenceResult::Mated => Some(1),
                DefenceResult::Forced { moves, .. } => Some(moves + 1),
                DefenceResult::Escapes => None,
                _ => {
                    unknown = true;
                    None
                }
            }
        };
        if let Some(l) = length {
            shortest = Some(shortest.map_or(l, |s| s.min(l)));
        }
    }
    forcing_verdict(shortest, unknown, query)
}

/// Final verdict of the `forcing_only` search from the shortest mate found
/// (if any) and whether some reply ran out of budget.
fn forcing_verdict(shortest: Option<u8>, unknown: bool, query: MateQuery) -> MateVerdict {
    match shortest {
        // A mate in one is certain; anything else may still be beaten by a
        // reply that ran out of budget, but never below 2.
        Some(1) => in_range(1, query),
        // A shorter mate than `min_moves` was found but an unfinished reply
        // may hide one inside the range: the answer is unknown, not NoMate.
        Some(l) if unknown && l >= query.min_moves && query.min_moves > 2 => MateVerdict::Unknown,
        Some(l) => in_range(l, query),
        None if unknown => MateVerdict::Unknown,
        None => MateVerdict::NoMate,
    }
}

fn in_range(length: u8, query: MateQuery) -> MateVerdict {
    if length >= query.min_moves && length <= query.max() {
        MateVerdict::Mate
    } else {
        MateVerdict::NoMate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("test fen")
    }

    fn query(min: u8, max: u8, forcing: bool) -> MateQuery {
        MateQuery {
            min_moves: min,
            max_moves: max,
            forcing_only: forcing,
            max_nodes: 200_000,
        }
    }

    #[test]
    fn mate_in_one_available() {
        let p = pos("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
        for forcing in [false, true] {
            assert_eq!(
                mate_in_available(&p, query(1, 1, forcing)),
                MateVerdict::Mate
            );
            // The shortest mate is 1, so a "mate in 2" query does not match.
            assert_eq!(
                mate_in_available(&p, query(2, 2, forcing)),
                MateVerdict::NoMate
            );
        }
    }

    #[test]
    fn scholars_mate_position() {
        let p = pos("r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4");
        assert_eq!(mate_in_available(&p, query(1, 1, true)), MateVerdict::Mate);
    }

    #[test]
    fn mate_in_two_with_a_forcing_start() {
        let p = pos("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1");
        // The key move Ra6 is quiet: found without `forcing_only` only.
        assert_eq!(mate_in_available(&p, query(2, 2, false)), MateVerdict::Mate);
        assert_eq!(
            mate_in_available(&p, query(2, 2, true)),
            MateVerdict::NoMate
        );
    }

    #[test]
    fn tiny_budgets_give_unknown_not_a_wrong_answer() {
        let p = pos("kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1");
        let q = MateQuery {
            max_nodes: 1,
            ..query(2, 2, false)
        };
        assert_eq!(mate_in_available(&p, q), MateVerdict::Unknown);
    }

    #[test]
    fn played_mode_checks_the_distance() {
        let p = pos("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
        assert_eq!(mate_in_played(&p, 1, query(1, 1, true)), MateVerdict::Mate);
        // Even distances and distances outside the range are no mate.
        assert_eq!(
            mate_in_played(&p, 2, query(1, 3, true)),
            MateVerdict::NoMate
        );
        assert_eq!(
            mate_in_played(&p, 3, query(1, 1, true)),
            MateVerdict::NoMate
        );
    }

    #[test]
    fn shorter_mate_below_min_moves_is_no_mate_even_with_an_unfinished_reply() {
        // Range 3..3: a check mating in 2 was found and another check ran out
        // of nodes. The shortest mate is outside the range: NoMate.
        let q = query(3, 3, true);
        assert_eq!(forcing_verdict(Some(2), true, q), MateVerdict::NoMate);
        // Without a shorter mate in hand the unfinished reply may hide one.
        assert_eq!(forcing_verdict(None, true, q), MateVerdict::Unknown);
        // A mate of exactly 3 plus an unfinished reply could still be shorter.
        assert_eq!(forcing_verdict(Some(3), true, q), MateVerdict::Unknown);
        assert_eq!(forcing_verdict(Some(1), true, q), MateVerdict::NoMate);
    }
}
