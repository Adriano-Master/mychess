//! Judging a move of the user (spec M8-4, D1910-D1913, D1923).
//!
//! Scid: `tcl/tools/tactics.tcl`, `foundBestLine` (same move, mate in the
//! same number of moves, "shorter mate exists"). The judge needs no engine:
//! the solver of `mychess_core::mate` answers.

use mychess_core::mate::{DefenceResult, best_defence, principal_line};
use mychess_core::{Move, Position};

use super::source::LINE_NODES;

/// Nodes the solver may use to judge one move (D1909).
pub const JUDGE_NODES: u64 = 2_000_000;

/// How far past the shortest mate a longer mate is still recognised as a
/// mate (D1911).
pub const LONGER_MARGIN: u8 = 2;

/// What decides if a move is right.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum JudgeRule {
    /// Mate in N: the position has a forced mate in `remaining` moves of
    /// the side to move (D1910).
    Mate {
        /// The shortest mate that the position has.
        remaining: u8,
        /// Accept a mate up to [`LONGER_MARGIN`] moves longer (D1911).
        accept_longer: bool,
    },
    /// Best move: the line of the game (D1913).
    Best {
        /// The solution of the game, from the position.
        solution: Vec<Move>,
    },
}

/// A move to judge, with what is needed to judge it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgeJob {
    /// The position before the move.
    pub position: Position,
    /// The move of the user (legal in `position`).
    pub mv: Move,
    /// The rule.
    pub rule: JudgeRule,
}

/// Why a move is not the move (D1910, D1913).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Wrong {
    /// It is not the move of the solution.
    NotBest,
    /// The forced mate is gone.
    NoMate,
    /// A mate remains, but a shorter one existed ("shorter mate exists").
    LongerMate,
}

/// The judgement of a move.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    /// The move is right.
    Correct {
        /// The move mates: the problem is solved.
        mated: bool,
        /// The reply of the opponent to play (Mate in N, not mated).
        reply: Option<Move>,
        /// The mate that is left, in moves of the user, after the reply
        /// (Mate in N, not mated).
        remaining: u8,
        /// The line to follow from there: the solver's (Mate in N) or the
        /// rest of the solution (best move).
        line: Vec<Move>,
    },
    /// The move is not right.
    Wrong(Wrong),
    /// The solver could not tell within its budget (D1923).
    Inconclusive,
}

/// The judgement when it needs no solver; `None` when the solver must run.
/// Cheap enough for `update` (D1936).
pub fn quick_verdict(job: &JudgeJob) -> Option<Verdict> {
    let Ok(after) = job.position.after(job.mv) else {
        return Some(Verdict::Wrong(match job.rule {
            JudgeRule::Mate { .. } => Wrong::NoMate,
            JudgeRule::Best { .. } => Wrong::NotBest,
        }));
    };
    let mated = |line: Vec<Move>| Verdict::Correct {
        mated: true,
        reply: None,
        remaining: 0,
        line,
    };
    match &job.rule {
        JudgeRule::Mate { .. } => {
            if after.is_checkmate() {
                return Some(mated(Vec::new()));
            }
            // Stalemate and bare kings end any mate.
            (after.is_stalemate() || after.is_insufficient_material())
                .then_some(Verdict::Wrong(Wrong::NoMate))
        }
        JudgeRule::Best { solution } => {
            if solution.first() == Some(&job.mv) {
                return Some(Verdict::Correct {
                    mated: after.is_checkmate(),
                    reply: None,
                    remaining: 0,
                    line: solution.iter().skip(1).copied().collect(),
                });
            }
            // A move that mates is never worse than the solution.
            if after.is_checkmate() {
                return Some(mated(Vec::new()));
            }
            // Only a solution that ends in mate can be matched by another
            // mate: anything else is wrong without asking the solver.
            ends_in_mate(&job.position, solution)
                .is_none()
                .then_some(Verdict::Wrong(Wrong::NotBest))
        }
    }
}

/// The number of moves of the side to move that the solution of the game
/// takes to give mate, if it ends in checkmate **by the side to move**
/// (D1913, D1948).
fn ends_in_mate(position: &Position, solution: &[Move]) -> Option<u8> {
    // The attacker moves first and last: an even number of plies ends with
    // a move of the defender, which is no mate "in N" (D1948).
    if solution.is_empty() || solution.len().is_multiple_of(2) {
        return None;
    }
    let mut pos = position.clone();
    for m in solution {
        pos.play(*m).ok()?;
    }
    pos.is_checkmate()
        .then(|| u8::try_from(solution.len().div_ceil(2)).ok())
        .flatten()
}

/// Judges the move. Blocking: the solver may run for as long as
/// [`JUDGE_NODES`] positions (D1909).
pub fn judge(job: &JudgeJob) -> Verdict {
    if let Some(verdict) = quick_verdict(job) {
        return verdict;
    }
    let Ok(after) = job.position.after(job.mv) else {
        return Verdict::Inconclusive;
    };
    match &job.rule {
        JudgeRule::Mate {
            remaining,
            accept_longer,
        } => judge_mate(&after, *remaining, *accept_longer),
        JudgeRule::Best { solution } => {
            let Some(length) = ends_in_mate(&job.position, solution) else {
                return Verdict::Wrong(Wrong::NotBest);
            };
            // Another mate that is as short as the solution is as good.
            match best_defence(&after, length.saturating_sub(1), JUDGE_NODES) {
                DefenceResult::Mated => Verdict::Correct {
                    mated: true,
                    reply: None,
                    remaining: 0,
                    line: Vec::new(),
                },
                DefenceResult::Forced { .. } => Verdict::Correct {
                    mated: false,
                    reply: None,
                    remaining: 0,
                    line: Vec::new(),
                },
                DefenceResult::Escapes => Verdict::Wrong(Wrong::NotBest),
                // Unknown, or an answer this version does not know.
                _ => Verdict::Inconclusive,
            }
        }
    }
}

fn judge_mate(after: &Position, remaining: u8, accept_longer: bool) -> Verdict {
    // The mate that the move keeps, looking a little past the shortest.
    let bound = remaining.saturating_add(LONGER_MARGIN.saturating_sub(1));
    match best_defence(after, bound, JUDGE_NODES) {
        DefenceResult::Mated => Verdict::Correct {
            mated: true,
            reply: None,
            remaining: 0,
            line: Vec::new(),
        },
        DefenceResult::Forced { moves, reply } => {
            // The move itself is one move of the user; `moves` more follow.
            let total = moves.saturating_add(1);
            let allowed = remaining.saturating_add(if accept_longer { LONGER_MARGIN } else { 0 });
            if total > allowed {
                return Verdict::Wrong(Wrong::LongerMate);
            }
            let line = after
                .after(reply)
                .ok()
                .and_then(|next| principal_line(&next, moves, LINE_NODES))
                .unwrap_or_default();
            Verdict::Correct {
                mated: false,
                reply: Some(reply),
                remaining: moves,
                line,
            }
        }
        DefenceResult::Escapes => Verdict::Wrong(Wrong::NoMate),
        _ => Verdict::Inconclusive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATE1: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
    const MATE2: &str = "kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1";

    fn job(fen: &str, uci: &str, rule: JudgeRule) -> JudgeJob {
        let position = Position::from_fen(fen).expect("fen");
        let mv = position.parse_uci(uci).expect("uci");
        JudgeJob { position, mv, rule }
    }

    fn mate(remaining: u8, accept_longer: bool) -> JudgeRule {
        JudgeRule::Mate {
            remaining,
            accept_longer,
        }
    }

    #[test]
    fn mate_delivered_is_correct_without_the_solver() {
        let j = job(MATE1, "a1a8", mate(1, false));
        match quick_verdict(&j) {
            Some(Verdict::Correct { mated: true, .. }) => {}
            other => panic!("expected a quick mate, got {other:?}"),
        }
    }

    #[test]
    fn a_stalemating_move_is_wrong_without_the_solver() {
        let j = job("7k/8/6K1/8/8/8/8/5Q2 w - - 0 1", "f1f7", mate(1, false));
        assert_eq!(quick_verdict(&j), Some(Verdict::Wrong(Wrong::NoMate)));
    }

    #[test]
    fn the_right_first_move_of_a_mate_in_two_gets_the_best_reply() {
        let j = job(MATE2, "a1a6", mate(2, false));
        match judge(&j) {
            Verdict::Correct {
                mated: false,
                reply: Some(_),
                remaining: 1,
                line,
            } => assert_eq!(line.len(), 1),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_move_that_drops_the_mate_is_wrong() {
        let j = job(MATE2, "a1a2", mate(2, false));
        assert_eq!(judge(&j), Verdict::Wrong(Wrong::NoMate));
    }

    #[test]
    fn a_longer_mate_is_refused_unless_it_is_accepted() {
        // King and queen: Qf8 mates in one, but a waiting move keeps a mate
        // in two. Some legal move must tell the two rules apart.
        let fen = "7k/8/6K1/8/8/8/8/5Q2 w - - 0 1";
        let position = Position::from_fen(fen).expect("fen");
        let mut told_apart = 0;
        for mv in position.legal_moves() {
            let strict = judge(&JudgeJob {
                position: position.clone(),
                mv,
                rule: mate(1, false),
            });
            let relaxed = judge(&JudgeJob {
                position: position.clone(),
                mv,
                rule: mate(1, true),
            });
            if strict == Verdict::Wrong(Wrong::LongerMate) {
                assert!(
                    matches!(relaxed, Verdict::Correct { mated: false, .. }),
                    "{}: {relaxed:?}",
                    position.uci(mv)
                );
                told_apart += 1;
            }
        }
        assert!(told_apart > 0, "no move keeps a longer mate");
    }

    #[test]
    fn a_solution_that_ends_with_a_mate_by_the_defender_is_not_a_mate_in_n() {
        // White plays b4 and Black answers Ra1#: the line ends in checkmate,
        // but of the side that moved first (D1948).
        let position = Position::from_fen("r6k/8/8/8/8/1P6/5PPP/6K1 w - - 0 1").expect("fen");
        let solution = [position.parse_uci("b3b4").expect("uci"), {
            let after = position
                .after(position.parse_uci("b3b4").expect("uci"))
                .expect("legal");
            after.parse_uci("a8a1").expect("uci")
        }];
        assert_eq!(ends_in_mate(&position, &solution), None);
        // The rule of the game agrees: no other move can match "that" mate.
        let verdict = quick_verdict(&job(
            "r6k/8/8/8/8/1P6/5PPP/6K1 w - - 0 1",
            "g1f1",
            JudgeRule::Best {
                solution: solution.to_vec(),
            },
        ));
        assert_eq!(verdict, Some(Verdict::Wrong(Wrong::NotBest)));
    }

    #[test]
    fn a_mate_in_one_by_the_side_to_move_counts_one() {
        let position = Position::from_fen(MATE1).expect("fen");
        let mate_move = position.parse_uci("a1a8").expect("uci");
        assert_eq!(ends_in_mate(&position, &[mate_move]), Some(1));
    }

    #[test]
    fn a_tiny_budget_cannot_decide_so_the_verdict_is_inconclusive_not_wrong() {
        // The judge itself has a fixed budget; the solver below it is the
        // part under test: an unlimited-depth question returns Unknown.
        let position = Position::from_fen(MATE2).expect("fen");
        let after = position
            .after(position.parse_uci("a1a6").expect("uci"))
            .expect("legal");
        assert_eq!(
            best_defence(&after, 3, 1),
            DefenceResult::Unknown,
            "the solver must not invent an answer"
        );
    }

    #[test]
    fn best_move_accepts_the_move_of_the_solution_and_hands_back_the_rest() {
        let position = Position::from_fen(MATE1).expect("fen");
        let first = position.parse_uci("a1a8").expect("uci");
        let j = JudgeJob {
            position,
            mv: first,
            rule: JudgeRule::Best {
                solution: vec![first],
            },
        };
        assert!(matches!(
            quick_verdict(&j),
            Some(Verdict::Correct { mated: true, .. })
        ));
    }

    #[test]
    fn best_move_without_a_mate_solution_rejects_another_move_at_once() {
        let position = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").expect("fen");
        let best = position.parse_uci("e2e4").expect("uci");
        let other = position.parse_uci("e2e3").expect("uci");
        let j = JudgeJob {
            position,
            mv: other,
            rule: JudgeRule::Best {
                solution: vec![best],
            },
        };
        assert_eq!(quick_verdict(&j), Some(Verdict::Wrong(Wrong::NotBest)));
    }

    #[test]
    fn best_move_accepts_another_mate_as_short_as_the_solution() {
        // Two rooks: Ra8# and Rb8# both mate; the game recorded only one.
        let fen = "6k1/5ppp/8/8/8/8/1R6/R5K1 w - - 0 1";
        let position = Position::from_fen(fen).expect("fen");
        let recorded = position.parse_uci("a1a8").expect("uci");
        let alternative = position.parse_uci("b2b8").expect("uci");
        let j = JudgeJob {
            position,
            mv: alternative,
            rule: JudgeRule::Best {
                solution: vec![recorded],
            },
        };
        assert!(matches!(judge(&j), Verdict::Correct { mated: true, .. }));
    }

    #[test]
    fn best_move_rejects_a_move_that_does_not_mate_as_fast() {
        let fen = "6k1/5ppp/8/8/8/8/1R6/R5K1 w - - 0 1";
        let position = Position::from_fen(fen).expect("fen");
        let recorded = position.parse_uci("a1a8").expect("uci");
        let slow = position.parse_uci("g1f1").expect("uci");
        let j = JudgeJob {
            position,
            mv: slow,
            rule: JudgeRule::Best {
                solution: vec![recorded],
            },
        };
        assert_eq!(judge(&j), Verdict::Wrong(Wrong::NotBest));
    }

    #[test]
    fn an_illegal_move_is_wrong_not_a_panic() {
        let position = Position::from_fen(MATE1).expect("fen");
        let other = Position::new();
        let mv = other.parse_uci("e2e4").expect("uci");
        let j = JudgeJob {
            position,
            mv,
            rule: mate(1, false),
        };
        assert_eq!(judge(&j), Verdict::Wrong(Wrong::NoMate));
    }
}
