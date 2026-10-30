//! The guided review of the key moments (spec M4-10c, D1435): the state
//! machine that walks the faults of the report and judges the user's
//! attempts at the right move. Pure; the cursor and the board are driven by
//! the state around it.

use mychess_core::Move;

use super::{GameReport, Moment, SideFilter, key_moments};

/// Wrong attempts after which the panel gives a hint (the piece to move).
pub const HINT_AFTER: u32 = 2;

/// Where the review is for the moment on show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewStep {
    /// The position before the move is shown with the verdict.
    Looking,
    /// The user is looking for the better move on the board.
    Trying,
    /// The user found the best move.
    Solved,
    /// The best move was shown.
    Revealed,
}

/// The answer to an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// It is the move the engine preferred.
    Correct,
    /// It is not; try again.
    Wrong,
}

/// A review in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// Whose moves are visited.
    pub side: SideFilter,
    /// The moments, in game order.
    pub moments: Vec<Moment>,
    /// The moment on show (index into `moments`).
    pub index: usize,
    /// Where the moment is.
    pub step: ReviewStep,
    /// Wrong attempts at the moment on show.
    pub attempts: u32,
}

impl Review {
    /// A review of `side` over `report`, with the brilliant and great moves
    /// too when `highlights`. `None` when there is nothing to review.
    pub fn new(report: &GameReport, side: SideFilter, highlights: bool) -> Option<Review> {
        let moments = key_moments(report, side, highlights);
        (!moments.is_empty()).then_some(Review {
            side,
            moments,
            index: 0,
            step: ReviewStep::Looking,
            attempts: 0,
        })
    }

    /// The moment on show.
    pub fn current(&self) -> Option<Moment> {
        self.moments.get(self.index).copied()
    }

    /// Goes to another moment; `false` at the end of the list.
    fn go(&mut self, index: Option<usize>) -> bool {
        match index.filter(|i| *i < self.moments.len()) {
            Some(index) => {
                self.index = index;
                self.step = ReviewStep::Looking;
                self.attempts = 0;
                true
            }
            None => false,
        }
    }

    /// The next moment; `false` when it is the last.
    pub fn advance(&mut self) -> bool {
        let next = self.index.checked_add(1);
        self.go(next)
    }

    /// The previous moment; `false` when it is the first.
    pub fn previous(&mut self) -> bool {
        let previous = self.index.checked_sub(1);
        self.go(previous)
    }

    /// The user starts looking for the better move.
    pub fn start_trying(&mut self) {
        self.step = ReviewStep::Trying;
        self.attempts = 0;
    }

    /// The user gives up and asks for the best move.
    pub fn reveal(&mut self) {
        self.step = ReviewStep::Revealed;
    }

    /// Judges `played` against the engine's `best`. Outside the trying
    /// step, or without a known best move, nothing is judged (`None`).
    pub fn judge(&mut self, played: Move, best: Option<Move>) -> Option<Attempt> {
        if self.step != ReviewStep::Trying {
            return None;
        }
        let best = best?;
        if played == best {
            self.step = ReviewStep::Solved;
            Some(Attempt::Correct)
        } else {
            self.attempts = self.attempts.saturating_add(1);
            Some(Attempt::Wrong)
        }
    }

    /// Whether the hint (the piece to move) is due.
    pub fn hint_due(&self) -> bool {
        self.step == ReviewStep::Trying && self.attempts >= HINT_AFTER
    }
}

#[cfg(test)]
mod tests {
    use mychess_core::{Color, Position};

    use super::*;
    use crate::report::{MoveClass, MoveReport, SideReport};

    fn mv(uci: &str) -> Move {
        Position::new().parse_uci(uci).expect("uci")
    }

    fn report_with(classes: &[MoveClass]) -> GameReport {
        let moves = classes
            .iter()
            .enumerate()
            .map(|(i, class)| MoveReport {
                ply: i + 1,
                mover: if i % 2 == 0 {
                    Color::White
                } else {
                    Color::Black
                },
                class: *class,
                loss: 0.0,
                accuracy: 100.0,
                played: mv("e2e4"),
                best: Some(mv("d2d4")),
            })
            .collect();
        let side = SideReport {
            accuracy: None,
            counts: [0; 10],
        };
        GameReport {
            moves,
            white: side.clone(),
            black: side,
        }
    }

    #[test]
    fn a_review_visits_only_the_faults_of_the_side() {
        let report = report_with(&[
            MoveClass::Blunder,
            MoveClass::Mistake,
            MoveClass::Best,
            MoveClass::Inaccuracy,
        ]);
        let white = Review::new(&report, SideFilter::White, false).expect("review");
        assert_eq!(white.moments.iter().map(|m| m.ply).collect::<Vec<_>>(), [1]);
        let both = Review::new(&report, SideFilter::Both, false).expect("review");
        assert_eq!(both.moments.len(), 3);
        assert!(Review::new(&report_with(&[MoveClass::Best]), SideFilter::Both, false).is_none());
    }

    #[test]
    fn next_and_previous_stop_at_the_ends_and_reset_the_moment() {
        let report = report_with(&[MoveClass::Blunder, MoveClass::Best, MoveClass::Mistake]);
        let mut review = Review::new(&report, SideFilter::Both, false).expect("review");
        assert!(!review.previous());
        review.start_trying();
        assert!(review.advance());
        assert_eq!(review.step, ReviewStep::Looking);
        assert_eq!(review.current().map(|m| m.ply), Some(3));
        assert!(!review.advance());
        assert!(review.previous());
        assert_eq!(review.index, 0);
    }

    #[test]
    fn only_the_best_move_is_accepted_and_the_hint_comes_after_two_misses() {
        let report = report_with(&[MoveClass::Blunder]);
        let mut review = Review::new(&report, SideFilter::Both, false).expect("review");
        let best = Some(mv("d2d4"));
        // Not trying yet: nothing is judged.
        assert_eq!(review.judge(mv("d2d4"), best), None);
        review.start_trying();
        assert_eq!(review.judge(mv("e2e4"), best), Some(Attempt::Wrong));
        assert!(!review.hint_due());
        assert_eq!(review.judge(mv("g1f3"), best), Some(Attempt::Wrong));
        assert!(review.hint_due());
        assert_eq!(review.judge(mv("d2d4"), best), Some(Attempt::Correct));
        assert_eq!(review.step, ReviewStep::Solved);
        assert_eq!(review.attempts, 2);
    }

    #[test]
    fn without_a_known_best_move_nothing_is_judged() {
        let report = report_with(&[MoveClass::Blunder]);
        let mut review = Review::new(&report, SideFilter::Both, false).expect("review");
        review.start_trying();
        assert_eq!(review.judge(mv("e2e4"), None), None);
        assert_eq!(review.attempts, 0);
    }
}
