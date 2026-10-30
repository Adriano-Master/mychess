//! Automatic adjudication of a game (spec M8-2, D1276, D1277).
//!
//! The rules look at the last note each engine reported (`info score`), in
//! centipawns from the point of view of the side that moved. The engine layer
//! reports from the point of view of White; `TournamentGame::reply` converts
//! it once, before [`Adjudicator::record`] (D1302). A move without
//! a note breaks the streak of its side, so a mute engine never triggers a
//! rule. The Scid only has the manual adjudication (`comp.tcl` 1167-1180).

use std::fmt;

use mychess_core::{Color, GameResult, Position};

use super::config::Adjudication;

/// Why a game was decided without being played to the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// The result.
    pub result: GameResult,
    /// The text of the comment (`Adjudicated: resign +6.45`).
    pub reason: String,
}

fn index(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

/// A tablebase result, from the point of view of the side to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TbOutcome {
    /// The side to move wins.
    Win,
    /// The side to move wins, but not within the 50 moves rule: a draw.
    CursedWin,
    /// Draw.
    Draw,
    /// The side to move loses, but not within the 50 moves rule: a draw.
    BlessedLoss,
    /// The side to move loses.
    Loss,
}

/// The tablebase hook (D1277). There is no implementation until the
/// `mychess-tablebase` crate of M8 exists; the option is hidden without one.
pub trait TablebaseProbe: fmt::Debug + Send + Sync {
    /// The most pieces the probe knows.
    fn max_pieces(&self) -> u32;
    /// The result of `position`, `None` when it is not in the tables.
    fn probe(&self, position: &Position) -> Option<TbOutcome>;
}

/// Follows the notes of a game and says when a rule decides it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adjudicator {
    rules: Adjudication,
    /// The notes of each side, in order; `None` is a move without note.
    notes: [Vec<Option<i32>>; 2],
}

impl Adjudicator {
    /// An adjudicator with `rules`.
    pub fn new(rules: Adjudication) -> Adjudicator {
        Adjudicator {
            rules,
            notes: [Vec::new(), Vec::new()],
        }
    }

    /// Records the note of the move of `mover` (centipawns for `mover`).
    /// Free moves (book, opening) are not recorded.
    pub fn record(&mut self, mover: Color, centipawns: Option<i32>) {
        if let Some(list) = self.notes.get_mut(index(mover)) {
            list.push(centipawns);
        }
    }

    /// The last `n` notes of `side`, when all of them exist.
    fn streak(&self, side: Color, n: u32) -> Option<Vec<i32>> {
        let n = usize::try_from(n).ok()?.max(1);
        let list = self.notes.get(index(side))?;
        let start = list.len().checked_sub(n)?;
        list.get(start..)?.iter().copied().collect()
    }

    /// The verdict after the move that led to `plies` half-moves from the
    /// start of the game (free moves included); `None` when no rule fires.
    pub fn verdict(&self, plies: u32) -> Option<Verdict> {
        self.resign_verdict()
            .or_else(|| self.draw_verdict(plies))
            .or_else(|| self.limit_verdict(plies))
    }

    fn resign_verdict(&self) -> Option<Verdict> {
        // A zero rule would fire at once: it is the same as no rule (D1309).
        let rule = self
            .rules
            .resign
            .filter(|r| r.centipawns > 0 && r.moves > 0)?;
        let threshold = i32::try_from(rule.centipawns).unwrap_or(i32::MAX);
        for loser in [Color::White, Color::Black] {
            let Some(own) = self.streak(loser, rule.moves) else {
                continue;
            };
            if !own.iter().all(|&cp| cp <= -threshold) {
                continue;
            }
            if rule.require_both {
                // The engine of the winner agrees with its last note.
                let agrees = self
                    .notes
                    .get(index(!loser))
                    .and_then(|list| list.last().copied().flatten())
                    .is_some_and(|cp| cp >= threshold);
                if !agrees {
                    continue;
                }
            }
            let shown = own.last().copied().unwrap_or(0).saturating_neg();
            let result = match loser {
                Color::White => GameResult::Black,
                Color::Black => GameResult::White,
            };
            return Some(Verdict {
                result,
                reason: format!("Adjudicated: resign {}", signed_pawns(shown)),
            });
        }
        None
    }

    fn draw_verdict(&self, plies: u32) -> Option<Verdict> {
        let rule = self.rules.draw.filter(|r| r.moves > 0)?;
        if plies < rule.after_move.saturating_mul(2) {
            return None;
        }
        let bound = i32::try_from(rule.centipawns).unwrap_or(i32::MAX);
        let quiet = [Color::White, Color::Black].iter().all(|&side| {
            self.streak(side, rule.moves)
                .is_some_and(|notes| notes.iter().all(|cp| cp.abs() <= bound))
        });
        quiet.then(|| Verdict {
            result: GameResult::Draw,
            reason: "Adjudicated: draw".to_owned(),
        })
    }

    fn limit_verdict(&self, plies: u32) -> Option<Verdict> {
        let max = self.rules.max_plies.filter(|&m| m > 0)?;
        (plies >= max).then(|| Verdict {
            result: GameResult::Draw,
            reason: "Adjudicated: move limit".to_owned(),
        })
    }

    /// The verdict of a tablebase for `position` (D1277): decided when the
    /// rule is on, there is a probe and few enough pieces are left.
    pub fn tablebase_verdict(
        &self,
        probe: Option<&dyn TablebaseProbe>,
        position: &Position,
    ) -> Option<Verdict> {
        if !self.rules.tablebase {
            return None;
        }
        let probe = probe?;
        let limit = self.rules.tb_pieces.min(probe.max_pieces());
        let pieces = u32::try_from(position.pieces().count()).unwrap_or(u32::MAX);
        if pieces > limit {
            return None;
        }
        let mover = position.turn();
        let wins = |color: Color| match color {
            Color::White => GameResult::White,
            Color::Black => GameResult::Black,
        };
        let result = match probe.probe(position)? {
            TbOutcome::Win => wins(mover),
            TbOutcome::Loss => wins(!mover),
            TbOutcome::CursedWin | TbOutcome::BlessedLoss | TbOutcome::Draw => GameResult::Draw,
        };
        Some(Verdict {
            result,
            reason: "TB adjudication".to_owned(),
        })
    }
}

/// `+6.45` or `-0.30`, in pawns.
pub(crate) fn signed_pawns(centipawns: i32) -> String {
    let sign = if centipawns < 0 { '-' } else { '+' };
    let abs = centipawns.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tournament::config::{DrawRule, ResignRule};

    fn resign(require_both: bool) -> Adjudication {
        Adjudication {
            resign: Some(ResignRule {
                centipawns: 600,
                moves: 3,
                require_both,
            }),
            ..Adjudication::default()
        }
    }

    fn feed(a: &mut Adjudicator, white: &[Option<i32>], black: &[Option<i32>]) {
        for (w, b) in white.iter().zip(black) {
            a.record(Color::White, *w);
            a.record(Color::Black, *b);
        }
    }

    #[test]
    fn no_rule_means_no_verdict() {
        let mut a = Adjudicator::new(Adjudication::default());
        feed(&mut a, &[Some(-900); 10], &[Some(900); 10]);
        assert_eq!(a.verdict(20), None);
    }

    #[test]
    fn resign_needs_a_streak_of_the_loser_and_the_agreement_of_the_winner() {
        let mut a = Adjudicator::new(resign(true));
        feed(&mut a, &[Some(-700); 3], &[Some(650); 3]);
        let v = a.verdict(6).expect("verdict");
        assert_eq!(v.result, GameResult::Black);
        assert_eq!(v.reason, "Adjudicated: resign +7.00");

        // The winner does not agree: no verdict with `require_both`.
        let mut b = Adjudicator::new(resign(true));
        feed(&mut b, &[Some(-700); 3], &[Some(100); 3]);
        assert_eq!(b.verdict(6), None);
        // ... but one with only the loser's notes.
        let mut c = Adjudicator::new(resign(false));
        feed(&mut c, &[Some(-700); 3], &[Some(100); 3]);
        assert_eq!(c.verdict(6).map(|v| v.result), Some(GameResult::Black));
    }

    #[test]
    fn resign_works_for_black_losing_too() {
        let mut a = Adjudicator::new(resign(false));
        feed(&mut a, &[Some(800); 3], &[Some(-800); 3]);
        assert_eq!(a.verdict(6).map(|v| v.result), Some(GameResult::White));
    }

    #[test]
    fn a_short_or_broken_streak_does_not_fire() {
        let mut a = Adjudicator::new(resign(false));
        feed(&mut a, &[Some(-700), Some(-700)], &[Some(700), Some(700)]);
        assert_eq!(a.verdict(4), None, "two moves are not enough");
        a.record(Color::White, Some(-100));
        assert_eq!(a.verdict(5), None, "the last note is not lost");
        let mut b = Adjudicator::new(resign(false));
        feed(&mut b, &[Some(-700), None, Some(-700)], &[Some(0); 3]);
        assert_eq!(b.verdict(6), None, "a move without note breaks the streak");
    }

    #[test]
    fn draw_needs_the_move_number_and_quiet_notes_on_both_sides() {
        let rules = Adjudication {
            draw: Some(DrawRule {
                after_move: 3,
                centipawns: 10,
                moves: 2,
            }),
            ..Adjudication::default()
        };
        let mut a = Adjudicator::new(rules);
        feed(&mut a, &[Some(5), Some(-8)], &[Some(0), Some(10)]);
        assert_eq!(a.verdict(4), None, "too early");
        assert_eq!(
            a.verdict(6),
            Some(Verdict {
                result: GameResult::Draw,
                reason: "Adjudicated: draw".to_owned()
            })
        );
        a.record(Color::White, Some(40));
        a.record(Color::Black, Some(0));
        assert_eq!(a.verdict(8), None, "white left the band");
    }

    #[test]
    fn the_ply_limit_is_a_draw() {
        let rules = Adjudication {
            max_plies: Some(100),
            ..Adjudication::default()
        };
        let a = Adjudicator::new(rules);
        assert_eq!(a.verdict(99), None);
        assert_eq!(a.verdict(100).map(|v| v.result), Some(GameResult::Draw));
    }

    #[derive(Debug)]
    struct FakeProbe(Option<TbOutcome>);

    impl TablebaseProbe for FakeProbe {
        fn max_pieces(&self) -> u32 {
            5
        }
        fn probe(&self, _: &Position) -> Option<TbOutcome> {
            self.0
        }
    }

    #[test]
    fn the_tablebase_decides_small_positions_and_respects_the_fifty_moves_rule() {
        let rules = Adjudication {
            tablebase: true,
            ..Adjudication::default()
        };
        let a = Adjudicator::new(rules);
        let small = Position::from_fen("8/8/4k3/8/8/3K4/4R3/8 b - - 0 1").expect("fen");
        let win = FakeProbe(Some(TbOutcome::Win));
        // The side to move (Black) wins.
        assert_eq!(
            a.tablebase_verdict(Some(&win), &small),
            Some(Verdict {
                result: GameResult::Black,
                reason: "TB adjudication".to_owned()
            })
        );
        let cursed = FakeProbe(Some(TbOutcome::CursedWin));
        assert_eq!(
            a.tablebase_verdict(Some(&cursed), &small).map(|v| v.result),
            Some(GameResult::Draw)
        );
        assert_eq!(a.tablebase_verdict(None, &small), None, "no probe");
        let unknown = FakeProbe(None);
        assert_eq!(a.tablebase_verdict(Some(&unknown), &small), None);
        // Too many pieces.
        assert_eq!(a.tablebase_verdict(Some(&win), &Position::new()), None);
        // Rule off.
        let off = Adjudicator::new(Adjudication::default());
        assert_eq!(off.tablebase_verdict(Some(&win), &small), None);
    }

    #[test]
    fn formats_signed_pawns() {
        assert_eq!(signed_pawns(645), "+6.45");
        assert_eq!(signed_pawns(-30), "-0.30");
        assert_eq!(signed_pawns(0), "+0.00");
    }
}
