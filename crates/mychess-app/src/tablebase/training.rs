//! The rules of the endgame training, free of any state (spec M8-8, D2574,
//! D2575): the grade of a position, the slips of the user and the verdict.

use mychess_core::{Color, GameEnd};
use mychess_tablebase::Category;

/// The theoretical result for the user, reduced to three values: a cursed win
/// and a blessed loss are draws under the 50-move rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Grade {
    /// The user loses.
    Loss,
    /// The game is drawn.
    Draw,
    /// The user wins.
    Win,
}

/// The grade of a category, or `None` when it is not known.
#[must_use]
pub fn grade_of(category: Category) -> Option<Grade> {
    match category {
        Category::Win | Category::MaybeWin => Some(Grade::Win),
        Category::CursedWin | Category::Draw | Category::BlessedLoss => Some(Grade::Draw),
        Category::Loss | Category::MaybeLoss => Some(Grade::Loss),
        _ => None,
    }
}

/// How a training ended, when it shows a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EndReason {
    /// The game was checkmated.
    Checkmate,
    /// Stalemate.
    Stalemate,
    /// No material to mate with.
    InsufficientMaterial,
    /// The 50-move rule.
    FiftyMoves,
    /// The same position three times.
    Repetition,
    /// The position left what the tablebase covers.
    OutOfRange,
}

/// What the user achieved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    /// Started with a win and won.
    Converted,
    /// Started with a draw and drew.
    Held,
    /// Got more than the position was worth.
    Escaped,
    /// Started lost and lost, as the tablebase says.
    LostAsExpected,
    /// Got less than the position was worth.
    Failed,
    /// The training did not reach an end.
    Inconclusive,
}

/// The result shown when a training ends by itself (D2575).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrainingOutcome {
    /// The side the user played.
    pub user: Color,
    /// What the position was worth for the user at the start.
    pub start: Grade,
    /// What the game ended with, if it ended.
    pub end: Option<Grade>,
    /// How it ended.
    pub reason: EndReason,
    /// What the user achieved.
    pub verdict: Verdict,
    /// Moves of the user.
    pub user_moves: u16,
    /// Times the user made the position worth less.
    pub slips: u16,
}

/// The verdict of a training that started worth `start` and ended with `end`.
#[must_use]
pub fn verdict(start: Grade, end: Option<Grade>) -> Verdict {
    let Some(end) = end else {
        return Verdict::Inconclusive;
    };
    match (start, end) {
        (Grade::Win, Grade::Win) => Verdict::Converted,
        (Grade::Draw, Grade::Draw) => Verdict::Held,
        (Grade::Loss, Grade::Loss) => Verdict::LostAsExpected,
        (s, e) if e > s => Verdict::Escaped,
        _ => Verdict::Failed,
    }
}

/// The grade for `user` of a finished game, with the reason.
#[must_use]
pub(crate) fn grade_of_end(end: GameEnd, user: Color) -> (Grade, EndReason) {
    match end {
        GameEnd::Checkmate { winner } => (
            if winner == user {
                Grade::Win
            } else {
                Grade::Loss
            },
            EndReason::Checkmate,
        ),
        GameEnd::Stalemate => (Grade::Draw, EndReason::Stalemate),
        GameEnd::InsufficientMaterial => (Grade::Draw, EndReason::InsufficientMaterial),
        GameEnd::FiftyMoves => (Grade::Draw, EndReason::FiftyMoves),
        GameEnd::ThreefoldRepetition => (Grade::Draw, EndReason::Repetition),
        // A rule the core learns later: a draw, by a reason not named here.
        _ => (Grade::Draw, EndReason::OutOfRange),
    }
}

/// A training in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Training {
    /// The side of the user.
    pub(crate) user: Color,
    /// The game being played (`GameSession::generation`).
    pub(crate) generation: u64,
    /// What the position was worth at the start; `None` until it is probed.
    pub(crate) start: Option<Grade>,
    /// What the position is worth now (never raised by the tablebase).
    pub(crate) current: Option<Grade>,
    /// Slips so far.
    pub(crate) slips: u16,
    /// Moves of the user so far.
    pub(crate) user_moves: u16,
    /// The key of the position in which the last answer was counted.
    pub(crate) counted: Option<String>,
    /// The answer of the tablebase waits for its delay: request and the key
    /// of the position.
    pub(crate) reply: Option<(crate::RequestId, String)>,
}

impl Training {
    pub(crate) fn new(user: Color, generation: u64) -> Training {
        Training {
            user,
            generation,
            start: None,
            current: None,
            slips: 0,
            user_moves: 0,
            counted: None,
            reply: None,
        }
    }

    /// Counts a probe of the position in which the **tablebase** is to move:
    /// the user made a move and the grade (for the user) may have dropped.
    pub(crate) fn count_user_move(&mut self, key: &str, grade_for_user: Option<Grade>) {
        // Without the value of the starting position there is nothing to
        // compare with: the flow ends the training instead (D2575).
        if self.start.is_none() || self.counted.as_deref() == Some(key) {
            return;
        }
        self.counted = Some(key.to_owned());
        self.user_moves = self.user_moves.saturating_add(1);
        if let (Some(now), Some(before)) = (grade_for_user, self.current)
            && now < before
        {
            self.slips = self.slips.saturating_add(1);
            self.current = Some(now);
        }
    }

    /// The outcome when the game ended with `end`.
    pub(crate) fn outcome(
        &self,
        end: Option<(Grade, EndReason)>,
        reason: EndReason,
    ) -> TrainingOutcome {
        let start = self.start.unwrap_or(Grade::Draw);
        let (grade, reason) = match end {
            Some((grade, reason)) => (Some(grade), reason),
            None => (None, reason),
        };
        TrainingOutcome {
            user: self.user,
            start,
            end: grade,
            reason,
            verdict: if self.start.is_some() {
                verdict(start, grade)
            } else {
                Verdict::Inconclusive
            },
            user_moves: self.user_moves,
            slips: self.slips,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grades_follow_the_fifty_move_rule() {
        assert_eq!(grade_of(Category::CursedWin), Some(Grade::Draw));
        assert_eq!(grade_of(Category::BlessedLoss), Some(Grade::Draw));
        assert_eq!(grade_of(Category::MaybeWin), Some(Grade::Win));
        assert_eq!(grade_of(Category::Unknown), None);
    }

    #[test]
    fn verdicts() {
        use Grade::{Draw, Loss, Win};
        assert_eq!(verdict(Win, Some(Win)), Verdict::Converted);
        assert_eq!(verdict(Win, Some(Draw)), Verdict::Failed);
        assert_eq!(verdict(Draw, Some(Draw)), Verdict::Held);
        assert_eq!(verdict(Draw, Some(Loss)), Verdict::Failed);
        assert_eq!(verdict(Loss, Some(Draw)), Verdict::Escaped);
        assert_eq!(verdict(Loss, Some(Loss)), Verdict::LostAsExpected);
        assert_eq!(verdict(Win, None), Verdict::Inconclusive);
    }

    #[test]
    fn a_drop_counts_once_per_position() {
        let mut t = Training::new(Color::White, 1);
        t.start = Some(Grade::Win);
        t.current = Some(Grade::Win);
        t.count_user_move("a", Some(Grade::Win));
        assert_eq!((t.user_moves, t.slips), (1, 0));
        t.count_user_move("a", Some(Grade::Draw));
        assert_eq!((t.user_moves, t.slips), (1, 0));
        t.count_user_move("b", Some(Grade::Draw));
        assert_eq!((t.user_moves, t.slips), (2, 1));
        assert_eq!(t.current, Some(Grade::Draw));
    }
}
