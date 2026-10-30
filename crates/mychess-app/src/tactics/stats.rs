//! The score of the training (spec M8-4, D1921, D1922, D1924).

use super::config::TacticsMode;

/// Why a problem was lost (D1921).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FailReason {
    /// The user asked to see the solution.
    GaveUp,
    /// Every attempt was used.
    NoAttempts,
    /// The time ran out.
    TimeUp,
}

/// How a problem ended (D1921).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProblemOutcome {
    /// Solved without a mistake and without a hint.
    Perfect,
    /// Solved after one or more mistakes (no hint).
    WithMistakes,
    /// Solved after using a hint.
    WithHelp,
    /// Lost.
    Failed(FailReason),
}

impl ProblemOutcome {
    /// Whether the problem was solved.
    pub fn solved(self) -> bool {
        !matches!(self, ProblemOutcome::Failed(_))
    }
}

/// The counters of one mode (D1924).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModeStats {
    /// Problems shown.
    pub shown: u32,
    /// Solved at the first try.
    pub perfect: u32,
    /// Solved after mistakes.
    pub with_mistakes: u32,
    /// Solved with a hint.
    pub with_help: u32,
    /// Lost.
    pub failed: u32,
    /// Skipped with "Next".
    pub skipped: u32,
    /// Time spent on the problems that were solved, in milliseconds.
    pub total_ms: u64,
    /// Solved in a row, without a failure ("Next" does not break it).
    pub streak: u32,
    /// The longest streak.
    pub best_streak: u32,
}

impl ModeStats {
    /// A problem was put on the board.
    pub fn record_shown(&mut self) {
        self.shown = self.shown.saturating_add(1);
    }

    /// A problem ended with `outcome` after `ms` milliseconds.
    pub fn record(&mut self, outcome: ProblemOutcome, ms: u64) {
        match outcome {
            ProblemOutcome::Perfect => self.perfect = self.perfect.saturating_add(1),
            ProblemOutcome::WithMistakes => {
                self.with_mistakes = self.with_mistakes.saturating_add(1);
            }
            ProblemOutcome::WithHelp => self.with_help = self.with_help.saturating_add(1),
            ProblemOutcome::Failed(_) => {
                self.failed = self.failed.saturating_add(1);
                self.streak = 0;
                return;
            }
        }
        self.total_ms = self.total_ms.saturating_add(ms);
        self.streak = self.streak.saturating_add(1);
        self.best_streak = self.best_streak.max(self.streak);
    }

    /// The problem was skipped (D1922).
    pub fn record_skipped(&mut self) {
        self.skipped = self.skipped.saturating_add(1);
    }

    /// Problems solved.
    pub fn solved(&self) -> u32 {
        self.perfect
            .saturating_add(self.with_mistakes)
            .saturating_add(self.with_help)
    }

    /// Solved over the problems that ended (solved plus lost), in percent;
    /// `None` before any ended.
    pub fn success_percent(&self) -> Option<u8> {
        let done = u64::from(self.solved()) + u64::from(self.failed);
        if done == 0 {
            return None;
        }
        let percent = u64::from(self.solved()) * 100 / done;
        Some(u8::try_from(percent).unwrap_or(100))
    }

    /// The average time of a solved problem, in milliseconds.
    pub fn average_ms(&self) -> Option<u64> {
        let solved = u64::from(self.solved());
        (solved > 0).then(|| self.total_ms / solved)
    }
}

/// The counters of both modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TacticsStats {
    /// Mate in N.
    pub mate: ModeStats,
    /// Best move.
    pub best: ModeStats,
}

impl TacticsStats {
    /// The counters of `mode`.
    pub fn of(&self, mode: TacticsMode) -> &ModeStats {
        match mode {
            TacticsMode::BestMove => &self.best,
            _ => &self.mate,
        }
    }

    /// The counters of `mode`, to change.
    pub fn of_mut(&mut self, mode: TacticsMode) -> &mut ModeStats {
        match mode {
            TacticsMode::BestMove => &mut self.best,
            _ => &mut self.mate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_breaks_the_streak_and_a_skip_does_not() {
        let mut s = ModeStats::default();
        s.record(ProblemOutcome::Perfect, 1000);
        s.record(ProblemOutcome::WithMistakes, 3000);
        s.record_skipped();
        assert_eq!((s.streak, s.best_streak), (2, 2));
        s.record(ProblemOutcome::Failed(FailReason::GaveUp), 9999);
        assert_eq!((s.streak, s.best_streak), (0, 2));
        s.record(ProblemOutcome::WithHelp, 2000);
        assert_eq!((s.streak, s.best_streak), (1, 2));
    }

    #[test]
    fn time_counts_only_for_solved_problems() {
        let mut s = ModeStats::default();
        s.record(ProblemOutcome::Perfect, 1000);
        s.record(ProblemOutcome::Failed(FailReason::TimeUp), 50_000);
        s.record(ProblemOutcome::Perfect, 3000);
        assert_eq!(s.total_ms, 4000);
        assert_eq!(s.average_ms(), Some(2000));
    }

    #[test]
    fn the_percentage_counts_solved_over_ended() {
        let mut s = ModeStats::default();
        assert_eq!(s.success_percent(), None);
        s.record(ProblemOutcome::Perfect, 1);
        s.record(ProblemOutcome::Failed(FailReason::NoAttempts), 1);
        s.record_skipped();
        assert_eq!(s.success_percent(), Some(50));
        assert_eq!(s.average_ms(), Some(1));
    }

    #[test]
    fn counters_saturate_instead_of_overflowing() {
        let mut s = ModeStats {
            perfect: u32::MAX,
            streak: u32::MAX,
            total_ms: u64::MAX,
            ..ModeStats::default()
        };
        s.record(ProblemOutcome::Perfect, 10);
        assert_eq!(s.perfect, u32::MAX);
        assert_eq!(s.total_ms, u64::MAX);
    }

    #[test]
    fn each_mode_has_its_own_counters() {
        let mut stats = TacticsStats::default();
        stats.of_mut(TacticsMode::BestMove).record_shown();
        assert_eq!(stats.of(TacticsMode::BestMove).shown, 1);
        assert_eq!(stats.of(TacticsMode::Mate).shown, 0);
    }
}
