//! Comparing the lines of the user with the engine, and the score of the
//! exercise (spec M8-6, D2312-D2316).
//!
//! Pure: no engine, no clock. The Scid only added the lines the user missed
//! (`positionDone`, `calvar.tcl` 441-471) and had no score; the rule of
//! "lines that come before the first one considered" is its.

use mychess_core::{Color, Move, Position};

use super::eval::{Eval, LineProbes, RootLine};
use super::line::{Assessment, UserLine, san_line};

/// A move is good when it is within this many centipawns of the best
/// (D2312).
pub const GOOD_MARGIN_CP: i32 = 50;
/// Above this, every move that keeps the advantage is good (D2312).
pub const DECISIVE_CP: i32 = 500;
/// A line is sound when its end is not worse than this many centipawns
/// below the evaluation after its first move (D2314).
pub const SOUND_TOLERANCE_CP: i32 = 80;
/// A line with this many moves has full depth credit (D2314).
pub const FULL_DEPTH_PLIES: usize = 5;
/// Weight of the candidates in the score (D2315).
pub const CANDIDATES_POINTS: f32 = 50.0;
/// Weight of the accuracy of the evaluations.
pub const ACCURACY_POINTS: f32 = 25.0;
/// Weight of the quality of the lines.
pub const QUALITY_POINTS: f32 = 25.0;
/// Points taken away when a crucial move was missed.
pub const CRUCIAL_PENALTY: f32 = 15.0;

/// A good move of the start position (D2312).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoodMove {
    /// The move.
    pub mv: Move,
    /// It in SAN.
    pub san: String,
    /// Its evaluation, White's point of view.
    pub eval: Eval,
    /// Its evaluation for the side that moves.
    pub mover_cp: i32,
    /// The depth reached.
    pub depth: u32,
    /// The engine's line, starting with the move.
    pub pv: Vec<Move>,
    /// The user listed it as the first move of a line.
    pub found: bool,
    /// Missed and better than the best candidate of the user (D2312).
    pub crucial: bool,
}

/// What became of one line of the user (D2314).
#[derive(Debug, Clone, PartialEq)]
pub struct LineReport {
    /// The line in SAN.
    pub san: String,
    /// The moves.
    pub moves: Vec<Move>,
    /// The user's evaluation.
    pub assessment: Assessment,
    /// The engine's evaluation after the first move.
    pub first: Option<Eval>,
    /// The engine's evaluation at the end of the line.
    pub end: Option<Eval>,
    /// The class of `end`.
    pub engine_assessment: Option<Assessment>,
    /// The first move is one of the good moves.
    pub good_candidate: bool,
    /// Plies of the line that follow the engine's play (D2314).
    pub agree_plies: usize,
    /// How close the evaluation is, 0 to 1.
    pub accuracy: f32,
    /// The line did not drop the evaluation.
    pub sound: bool,
    /// The credit for how deep the line goes, 0 to 1.
    pub depth_credit: f32,
    /// `sound` times `depth_credit`.
    pub quality: f32,
}

/// The parts of the score (D2315).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScoreParts {
    /// Good moves found over good moves, 0 to 1.
    pub candidates: f32,
    /// Mean accuracy of the lines.
    pub accuracy: f32,
    /// Mean quality of the lines.
    pub quality: f32,
    /// Points taken away.
    pub penalty: f32,
}

/// The result of the exercise.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The good moves, best first.
    pub good: Vec<GoodMove>,
    /// One entry per line of the user, in the order given.
    pub lines: Vec<LineReport>,
    /// How many good moves the user listed.
    pub found: u8,
    /// How many good moves there are.
    pub good_total: u8,
    /// The score, 0 to 100.
    pub score: u8,
    /// How the score came about.
    pub parts: ScoreParts,
}

impl Report {
    /// Whether a crucial move was missed.
    pub fn has_crucial_miss(&self) -> bool {
        self.good.iter().any(|g| g.crucial)
    }

    /// The best of the missed moves, if any (for the arrow of the board).
    pub fn best_missed(&self) -> Option<&GoodMove> {
        self.good.iter().find(|g| !g.found)
    }
}

/// The good moves of the start position, best first (D2312). Lines without
/// a variation are ignored; two lines with the same first move count once.
pub fn good_moves(root: &Position, lines: &[RootLine]) -> Vec<GoodMove> {
    let mover = root.turn();
    let mut all: Vec<GoodMove> = Vec::new();
    for line in lines {
        let Some(first) = line.pv.first().copied() else {
            continue;
        };
        if all.iter().any(|g| g.mv == first) {
            continue;
        }
        let Ok(san) = root.san(first) else {
            continue;
        };
        all.push(GoodMove {
            mv: first,
            san,
            eval: line.eval,
            mover_cp: line.eval.for_mover(mover),
            depth: line.depth,
            pv: line.pv.clone(),
            found: false,
            crucial: false,
        });
    }
    all.sort_by_key(|g| std::cmp::Reverse(g.mover_cp));
    let Some(best) = all.first().map(|g| g.mover_cp) else {
        return all;
    };
    all.retain(|g| {
        g.mover_cp >= best.saturating_sub(GOOD_MARGIN_CP)
            || (best > DECISIVE_CP && g.mover_cp > DECISIVE_CP)
    });
    all
}

/// Whether the end of a line kept the evaluation of its first move (D2314).
fn is_sound(mover: Color, first: Eval, end: Eval) -> bool {
    if first.is_mate() && end.is_mate() {
        return (first.for_mover(mover) > 0) == (end.for_mover(mover) > 0);
    }
    end.for_mover(mover) >= first.for_mover(mover).saturating_sub(SOUND_TOLERANCE_CP)
}

fn common_prefix(a: &[Move], b: &[Move]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// Whether the position after all the moves has no legal move.
fn ends_the_game(root: &Position, moves: &[Move]) -> bool {
    let mut pos = root.clone();
    for mv in moves {
        match pos.after(*mv) {
            Ok(next) => pos = next,
            Err(_) => return false,
        }
    }
    pos.legal_moves().is_empty()
}

fn mean(values: impl Iterator<Item = f32>) -> f32 {
    let (sum, n) = values.fold((0.0_f32, 0_u16), |(s, n), v| (s + v, n.saturating_add(1)));
    if n == 0 { 0.0 } else { sum / f32::from(n) }
}

/// The evaluation, for the side that moves, of the best first move among the
/// lines of the user, by the first stage (D2352): the good moves come from
/// there, so the two sides of the comparison are in the same scale. A move
/// outside the MultiPV of the first stage falls back to the probe of the
/// second, which is never taken above the worst line of the first (the move
/// was not among them).
fn best_candidate_cp(
    mover: Color,
    user: &[UserLine],
    lines: &[LineReport],
    root_lines: &[RootLine],
) -> Option<i32> {
    let worst_root = root_lines.iter().map(|r| r.eval.for_mover(mover)).min();
    user.iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let mv = *l.moves.first()?;
            root_lines
                .iter()
                .find(|r| r.pv.first() == Some(&mv))
                .map(|r| r.eval.for_mover(mover))
                .or_else(|| {
                    let probed = lines.get(i)?.first.map(|e| e.for_mover(mover))?;
                    Some(worst_root.map_or(probed, |worst| probed.min(worst)))
                })
        })
        .max()
}

/// Compares the lines of the user with the engine (D2312-D2316).
///
/// `probes` has one entry per line, in the same order; a missing or empty
/// entry means the engine could not search that line (it counts as an
/// unknown: no accuracy, not sound).
pub fn compare(
    root: &Position,
    root_lines: &[RootLine],
    user: &[UserLine],
    probes: &[LineProbes],
) -> Report {
    let mover = root.turn();
    let mut good = good_moves(root, root_lines);
    let firsts: Vec<Move> = user
        .iter()
        .filter_map(|l| l.moves.first().copied())
        .collect();
    for g in &mut good {
        g.found = firsts.contains(&g.mv);
    }

    let mut lines = Vec::with_capacity(user.len());
    for (index, line) in user.iter().enumerate() {
        let probe = probes.get(index);
        let first_probe = probe.and_then(|p| p.first.as_ref());
        let end_probe = if line.moves.len() <= 1 {
            first_probe
        } else {
            probe.and_then(|p| p.end.as_ref())
        };
        let first = first_probe.map(|p| p.eval);
        let end = end_probe.map(|p| p.eval);
        let engine_assessment = end.map(Eval::assessment);
        let accuracy = engine_assessment.map_or(0.0, |a| a.closeness(line.assessment));
        let sound = match (first, end) {
            (Some(f), Some(e)) => is_sound(mover, f, e),
            _ => false,
        };
        let agree_plies = match first_probe {
            Some(p) if !line.moves.is_empty() => {
                1 + common_prefix(line.moves.get(1..).unwrap_or(&[]), &p.pv)
            }
            _ => usize::from(!line.moves.is_empty()),
        };
        let depth_credit = if ends_the_game(root, &line.moves) {
            1.0
        } else {
            let plies = u8::try_from(line.moves.len().min(FULL_DEPTH_PLIES)).unwrap_or(0);
            f32::from(plies) / f32::from(u8::try_from(FULL_DEPTH_PLIES).unwrap_or(5))
        };
        let quality = if sound { depth_credit } else { 0.0 };
        let good_candidate = line
            .moves
            .first()
            .is_some_and(|mv| good.iter().any(|g| g.mv == *mv));
        lines.push(LineReport {
            san: san_line(root, &line.moves),
            moves: line.moves.clone(),
            assessment: line.assessment,
            first,
            end,
            engine_assessment,
            good_candidate,
            agree_plies,
            accuracy,
            sound,
            depth_credit,
            quality,
        });
    }

    let best_user = best_candidate_cp(mover, user, &lines, root_lines);
    for g in &mut good {
        g.crucial = !g.found && best_user.is_none_or(|best| g.mover_cp > best);
    }

    let good_total = u8::try_from(good.len()).unwrap_or(u8::MAX);
    let found = u8::try_from(good.iter().filter(|g| g.found).count()).unwrap_or(u8::MAX);
    let parts = if lines.is_empty() {
        ScoreParts::default()
    } else {
        ScoreParts {
            candidates: if good_total == 0 {
                0.0
            } else {
                f32::from(found) / f32::from(good_total)
            },
            accuracy: mean(lines.iter().map(|l| l.accuracy)),
            quality: mean(lines.iter().map(|l| l.quality)),
            penalty: if good.iter().any(|g| g.crucial) {
                CRUCIAL_PENALTY
            } else {
                0.0
            },
        }
    };
    let score = score_of(&parts, lines.is_empty());
    Report {
        good,
        lines,
        found,
        good_total,
        score,
        parts,
    }
}

/// The score of D2315 from its parts: 0 to 100.
pub fn score_of(parts: &ScoreParts, no_lines: bool) -> u8 {
    if no_lines {
        return 0;
    }
    let raw = CANDIDATES_POINTS * parts.candidates
        + ACCURACY_POINTS * parts.accuracy
        + QUALITY_POINTS * parts.quality
        - parts.penalty;
    let clamped = raw.round().clamp(0.0, 100.0);
    // `clamped` is a whole number from 0 to 100.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let score = clamped as u8;
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calvar::eval::Probe;

    fn pos() -> Position {
        Position::new()
    }

    fn mv(p: &Position, uci: &str) -> Move {
        p.parse_uci(uci).expect("uci")
    }

    fn root_line(p: &Position, n: u8, uci: &str, cp: i32) -> RootLine {
        RootLine {
            multipv: n,
            depth: 12,
            eval: Eval::centipawns(cp),
            pv: vec![mv(p, uci)],
        }
    }

    #[test]
    fn good_moves_are_within_the_margin_of_the_best() {
        let p = pos();
        let lines = [
            root_line(&p, 1, "e2e4", 40),
            root_line(&p, 2, "d2d4", 20),
            root_line(&p, 3, "g1f3", -30),
        ];
        let good = good_moves(&p, &lines);
        assert_eq!(good.len(), 2);
        assert_eq!(good[0].san, "e4");
    }

    #[test]
    fn when_the_position_is_won_every_winning_move_is_good() {
        let p = pos();
        let lines = [
            root_line(&p, 1, "e2e4", 900),
            root_line(&p, 2, "d2d4", 600),
            root_line(&p, 3, "g1f3", 100),
        ];
        assert_eq!(good_moves(&p, &lines).len(), 2);
    }

    #[test]
    fn black_to_move_reads_the_evaluation_from_its_side() {
        let p = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
            .expect("fen");
        let lines = [root_line(&p, 1, "e7e5", -30), root_line(&p, 2, "a7a6", 60)];
        let good = good_moves(&p, &lines);
        assert_eq!(good.len(), 1, "e5 is best for Black; a6 is 90 worse");
        assert_eq!(good[0].san, "e5");
    }

    fn probe(cp: i32) -> Probe {
        Probe {
            eval: Eval::centipawns(cp),
            depth: 10,
            pv: Vec::new(),
        }
    }

    #[test]
    fn a_missed_better_move_is_crucial_and_costs_points() {
        let p = pos();
        let lines = [root_line(&p, 1, "e2e4", 40), root_line(&p, 2, "d2d4", 30)];
        let user = [UserLine {
            moves: vec![mv(&p, "d2d4")],
            assessment: Assessment::Equal,
        }];
        let probes = [LineProbes {
            first: Some(probe(30)),
            end: None,
        }];
        let report = compare(&p, &lines, &user, &probes);
        assert_eq!(report.found, 1);
        assert_eq!(report.good_total, 2);
        assert!(report.has_crucial_miss(), "e4 is better than d4");
        assert_eq!(report.best_missed().map(|g| g.san.as_str()), Some("e4"));
        assert!((report.parts.penalty - CRUCIAL_PENALTY).abs() < 1e-6);
    }

    #[test]
    fn a_missed_move_that_is_worse_than_the_best_candidate_is_not_crucial() {
        let p = pos();
        let lines = [root_line(&p, 1, "e2e4", 40), root_line(&p, 2, "d2d4", 30)];
        let user = [UserLine {
            moves: vec![mv(&p, "e2e4")],
            assessment: Assessment::WhiteSlightly,
        }];
        let probes = [LineProbes {
            first: Some(probe(40)),
            end: None,
        }];
        let report = compare(&p, &lines, &user, &probes);
        assert!(!report.has_crucial_miss());
        assert_eq!(report.found, 1);
        assert!(report.best_missed().is_some(), "d4 is still missed");
    }

    #[test]
    fn a_perfect_exercise_scores_one_hundred() {
        let p = pos();
        let lines = [root_line(&p, 1, "e2e4", 30)];
        let user = [UserLine {
            moves: crate::calvar::line::read_moves(&p, "e4 e5 Nf3 Nc6 Bb5").moves,
            assessment: Assessment::WhiteSlightly,
        }];
        let probes = [LineProbes {
            first: Some(probe(30)),
            end: Some(probe(35)),
        }];
        let report = compare(&p, &lines, &user, &probes);
        assert_eq!(report.score, 100);
        assert!(report.lines[0].sound);
        assert!((report.lines[0].accuracy - 1.0).abs() < 1e-6);
    }

    fn mv_after(p: &Position, played: &[Move], uci: &str) -> Move {
        let mut pos = p.clone();
        for m in played {
            pos = pos.after(*m).expect("after");
        }
        pos.parse_uci(uci).expect("uci")
    }

    #[test]
    fn no_lines_scores_zero() {
        let p = pos();
        let lines = [root_line(&p, 1, "e2e4", 30)];
        let report = compare(&p, &lines, &[], &[]);
        assert_eq!(report.score, 0);
        assert_eq!(report.found, 0);
        assert!(report.has_crucial_miss());
    }

    #[test]
    fn a_line_that_drops_the_evaluation_is_not_sound() {
        let p = pos();
        let e4 = mv(&p, "e2e4");
        let e5 = mv_after(&p, &[e4], "e7e5");
        let lines = [root_line(&p, 1, "e2e4", 30)];
        let user = [UserLine {
            moves: vec![e4, e5],
            assessment: Assessment::Equal,
        }];
        let probes = [LineProbes {
            first: Some(probe(30)),
            end: Some(probe(-200)),
        }];
        let line = &compare(&p, &lines, &user, &probes).lines[0];
        assert!(!line.sound);
        assert!(line.quality.abs() < 1e-6);
        assert_eq!(line.engine_assessment, Some(Assessment::BlackBetter));
    }

    #[test]
    fn the_score_is_clamped_between_zero_and_one_hundred() {
        let big = ScoreParts {
            candidates: 1.0,
            accuracy: 1.0,
            quality: 1.0,
            penalty: 0.0,
        };
        assert_eq!(score_of(&big, false), 100);
        let bad = ScoreParts {
            penalty: CRUCIAL_PENALTY,
            ..ScoreParts::default()
        };
        assert_eq!(score_of(&bad, false), 0);
    }
}
