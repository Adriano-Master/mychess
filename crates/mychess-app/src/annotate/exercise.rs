//! Tactical exercises (spec M4-6, D470, D471): `markExercise` of the Scid
//! (`analysis.tcl` 1312-1395) split in its pure parts.
//!
//! The Scid runs two tests with its internal engine: whether the move is
//! obvious (the best move at depths 1, 2 and 3) and how long the engine
//! needs to find it. The run asks the engine of the annotation for both
//! and gives the answers to the functions here.

use mychess_core::{Color, Move};

use super::options::{Informant, centipawns};
use super::score::PositionEval;

/// The times, in milliseconds, at which the engine is asked for its move
/// to measure the difficulty (`timer` in `markExercise`).
pub const DIFFICULTY_TIMES_MS: [u64; 8] = [1, 2, 5, 10, 50, 100, 200, 1000];

/// The depths of the "obvious move" test.
pub const OBVIOUS_DEPTHS: [u32; 3] = [1, 2, 3];

/// The gap, in centipawns, between the best line and the second one that a
/// tactic needs.
const MIN_GAP_CP: i64 = 150;

/// The first filters of `markExercise`, from the scores alone. `prev` and
/// `cur` are the evaluations before and after the move that lost ground;
/// `side_to_move` is who moves in the position after it (the one who has
/// the tactic). Returns the move to find (the first move of the line of
/// `cur`) when the position may be an exercise.
pub fn exercise_move(
    inf: &Informant,
    prev: &PositionEval,
    cur: &PositionEval,
    side_to_move: Color,
) -> Option<Move> {
    let (p, s) = (centipawns(prev.score), centipawns(cur.score));
    let (moderate, decisive, won) = (
        centipawns(inf.moderate),
        centipawns(inf.decisive),
        centipawns(inf.won),
    );
    // Only high gains.
    if (s - p).abs() < moderate {
        return None;
    }
    // The result is already clear and the play goes on the same way.
    if p * s >= 0 {
        if p.abs() > won {
            return None;
        }
        if p.abs() > decisive && s.abs() < 2 * p.abs() {
            return None;
        }
    }
    // The best move is much better than the others.
    let second = centipawns(cur.second?);
    if (s - second).abs() < MIN_GAP_CP {
        return None;
    }
    // No other winning move.
    if s * second > 0 && s.abs() > decisive && second.abs() > decisive {
        return None;
    }
    // The best move does not lose ground.
    match side_to_move {
        Color::White if s < -moderate => return None,
        Color::Black if s > moderate => return None,
        _ => {}
    }
    cur.pv.first().copied()
}

/// Whether the move is obvious: the engine finds it at every depth of
/// [`OBVIOUS_DEPTHS`].
pub fn is_obvious(best: Move, by_depth: &[Option<Move>]) -> bool {
    !by_depth.is_empty() && by_depth.iter().all(|m| *m == Some(best))
}

/// The difficulty, 1 to 9, from the moves the engine chose at the times of
/// [`DIFFICULTY_TIMES_MS`]: 2 plus the index of the last time at which it
/// still missed the move, 1 if it never missed (`difficulty` = `t + 2`).
pub fn difficulty(best: Move, by_time: &[Option<Move>]) -> u8 {
    for (t, chosen) in by_time.iter().enumerate().rev() {
        if *chosen != Some(best) {
            return u8::try_from(t + 2).unwrap_or(9).min(9);
        }
    }
    1
}

/// The comment of an exercise: `****D<n> <prev>-><cur>` with one decimal
/// (D471). The existing comment is kept after it by the caller.
///
/// The Scid formats a Tcl double (`format %.1f` of `cp / 100.0`), so 15 cp
/// prints `0.1` and 45 cp prints `0.5` (the doubles are 0.14999... and
/// 0.45000...). The score is brought back to whole centipawns first: a
/// single-precision pawn value would round those two the other way.
pub fn exercise_comment(difficulty: u8, prev: f32, cur: f32) -> String {
    format!(
        "****D{difficulty} {}->{}",
        one_decimal(prev),
        one_decimal(cur)
    )
}

fn one_decimal(pawns: f32) -> String {
    let cp = centipawns(pawns);
    // Whole centipawns up to 2^31 are exact in an f64.
    #[allow(clippy::cast_precision_loss)]
    let value = cp as f64 / 100.0;
    format!("{value:.1}")
}

#[cfg(test)]
mod tests {
    use mychess_core::Position;

    use super::*;

    fn moves() -> (Move, Move) {
        let p = Position::new();
        (
            p.parse_uci("e2e4").expect("uci"),
            p.parse_uci("d2d4").expect("uci"),
        )
    }

    fn eval(score: f32, second: Option<f32>, pv: &[Move]) -> PositionEval {
        PositionEval {
            score,
            mate: None,
            pv: pv.to_vec(),
            depth: 13,
            second,
        }
    }

    fn check(prev: f32, cur: f32, second: Option<f32>, side: Color) -> Option<Move> {
        let (e4, _) = moves();
        exercise_move(
            &Informant::default(),
            &eval(prev, None, &[]),
            &eval(cur, second, &[e4]),
            side,
        )
    }

    #[test]
    fn a_clear_shot_with_a_poor_second_line_is_a_candidate() {
        // Black blundered: White, to move, now has +3.0 against +0.5 for
        // the second best line.
        let (e4, _) = moves();
        assert_eq!(check(0.3, 3.0, Some(0.5), Color::White), Some(e4));
    }

    #[test]
    fn a_small_swing_is_not_an_exercise() {
        assert_eq!(check(0.3, 1.0, Some(-1.0), Color::White), None);
    }

    #[test]
    fn a_game_that_was_already_won_is_not_an_exercise() {
        assert_eq!(check(6.0, 9.0, Some(0.0), Color::White), None);
        // Decisive before and not much more after.
        assert_eq!(check(4.0, 5.5, Some(0.0), Color::White), None);
    }

    #[test]
    fn two_winning_moves_make_no_exercise() {
        // Opposite signs avoid the "already clear" filter.
        assert_eq!(check(-1.0, 4.0, Some(3.2), Color::White), None);
        assert_eq!(check(-1.0, 4.0, Some(1.0), Color::White), Some(moves().0));
    }

    #[test]
    fn the_second_line_must_be_at_least_one_and_a_half_pawns_worse() {
        assert_eq!(check(0.3, 3.0, Some(1.6), Color::White), None);
        assert_eq!(check(0.3, 3.0, Some(1.5), Color::White), Some(moves().0));
    }

    #[test]
    fn a_missing_second_line_means_no_exercise() {
        assert_eq!(check(0.3, 3.0, None, Color::White), None);
    }

    #[test]
    fn the_side_with_the_tactic_must_not_lose_ground() {
        assert_eq!(check(-0.3, -3.0, Some(-6.0), Color::White), None);
        assert_eq!(check(0.3, 3.0, Some(0.0), Color::Black), None);
    }

    #[test]
    fn an_empty_line_gives_no_move_to_find() {
        let r = exercise_move(
            &Informant::default(),
            &eval(0.3, None, &[]),
            &eval(3.0, Some(0.0), &[]),
            Color::White,
        );
        assert_eq!(r, None);
    }

    #[test]
    fn an_obvious_move_is_found_at_every_depth() {
        let (a, b) = moves();
        assert!(is_obvious(a, &[Some(a), Some(a), Some(a)]));
        assert!(!is_obvious(a, &[Some(a), Some(b), Some(a)]));
        assert!(!is_obvious(a, &[Some(a), None, Some(a)]));
        assert!(!is_obvious(a, &[]));
    }

    #[test]
    fn the_difficulty_is_two_plus_the_last_time_that_missed() {
        let (a, b) = moves();
        let hit = Some(a);
        let miss = Some(b);
        assert_eq!(difficulty(a, &[hit; 8]), 1);
        assert_eq!(difficulty(a, &[miss, hit, hit, hit, hit, hit, hit, hit]), 2);
        assert_eq!(
            difficulty(a, &[miss, miss, miss, hit, hit, hit, hit, hit]),
            4
        );
        assert_eq!(difficulty(a, &[miss; 8]), 9);
        assert_eq!(difficulty(a, &[hit, hit, hit, hit, hit, hit, hit, None]), 9);
        // A miss followed by hits: only the last miss counts.
        assert_eq!(difficulty(a, &[hit, miss, hit, hit, hit, hit, hit, hit]), 3);
    }

    #[test]
    fn the_comment_has_one_decimal() {
        assert_eq!(exercise_comment(4, 0.3, -1.84), "****D4 0.3->-1.8");
    }
}
