//! The score of the tactical games (spec M8-7, D2463).

use serde::Serialize;

use super::strength::Outcome;

/// The simple score of the games against the engine, from the point of view
/// of the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct TacticalStats {
    /// Games that ended with a result.
    pub games: u32,
    /// Wins of the user.
    pub wins: u32,
    /// Draws.
    pub draws: u32,
    /// Losses of the user (resigning counts).
    pub losses: u32,
    /// Wins in a row up to now.
    pub streak: u32,
    /// The longest run of wins.
    pub best_streak: u32,
    /// The highest Elo of an engine that the user beat; 0 for none.
    pub best_win_elo: u32,
}

impl TacticalStats {
    /// Counts a game. `engine_elo` is the Elo the engine played at.
    pub fn record(&mut self, outcome: Outcome, engine_elo: u32) {
        self.games = self.games.saturating_add(1);
        match outcome {
            Outcome::Win => {
                self.wins = self.wins.saturating_add(1);
                self.streak = self.streak.saturating_add(1);
                self.best_streak = self.best_streak.max(self.streak);
                self.best_win_elo = self.best_win_elo.max(engine_elo);
            }
            Outcome::Draw => {
                self.draws = self.draws.saturating_add(1);
                self.streak = 0;
            }
            Outcome::Loss => {
                self.losses = self.losses.saturating_add(1);
                self.streak = 0;
            }
        }
    }

    /// Clears the score.
    pub fn reset(&mut self) {
        *self = TacticalStats::default();
    }

    /// The score in half points over the games, in percent (0 without
    /// games): `(wins + draws / 2) / games`.
    #[must_use]
    pub fn score_percent(&self) -> u32 {
        if self.games == 0 {
            return 0;
        }
        let halves = u64::from(self.wins) * 2 + u64::from(self.draws);
        let percent = halves * 100 / (u64::from(self.games) * 2);
        u32::try_from(percent.min(100)).unwrap_or(100)
    }

    /// The same score with the counts made consistent: no count above the
    /// number of games, and the games at least the sum of the parts (a
    /// file edited by hand cannot make the percentage absurd).
    #[must_use]
    pub fn sanitized(mut self) -> TacticalStats {
        let sum = self
            .wins
            .saturating_add(self.draws)
            .saturating_add(self.losses);
        self.games = self.games.max(sum);
        self.best_streak = self.best_streak.max(self.streak).min(self.games);
        self.streak = self.streak.min(self.best_streak);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_results_streaks_and_the_best_win() {
        let mut s = TacticalStats::default();
        s.record(Outcome::Win, 1500);
        s.record(Outcome::Win, 1600);
        s.record(Outcome::Draw, 1900);
        s.record(Outcome::Win, 1550);
        s.record(Outcome::Loss, 2000);
        assert_eq!((s.games, s.wins, s.draws, s.losses), (5, 3, 1, 1));
        assert_eq!(s.streak, 0);
        assert_eq!(s.best_streak, 2);
        assert_eq!(s.best_win_elo, 1600, "a draw or a loss is not a win");
    }

    #[test]
    fn the_score_is_wins_plus_half_the_draws() {
        let mut s = TacticalStats::default();
        assert_eq!(s.score_percent(), 0);
        s.record(Outcome::Win, 1500);
        s.record(Outcome::Draw, 1500);
        s.record(Outcome::Loss, 1500);
        s.record(Outcome::Loss, 1500);
        assert_eq!(s.score_percent(), 37);
    }

    #[test]
    fn counts_saturate_and_reset_clears() {
        let mut s = TacticalStats {
            games: u32::MAX,
            wins: u32::MAX,
            streak: u32::MAX,
            ..TacticalStats::default()
        };
        s.record(Outcome::Win, 1500);
        assert_eq!(s.games, u32::MAX);
        assert!(s.score_percent() <= 100);
        s.reset();
        assert_eq!(s, TacticalStats::default());
    }

    #[test]
    fn a_file_with_absurd_counts_is_made_consistent() {
        let s = TacticalStats {
            games: 1,
            wins: 5,
            draws: 2,
            losses: 1,
            streak: 9,
            best_streak: 3,
            best_win_elo: 1800,
        }
        .sanitized();
        assert_eq!(s.games, 8);
        assert!(s.best_streak <= s.games);
        assert!(s.streak <= s.best_streak);
        assert!(s.score_percent() <= 100);
    }
}
