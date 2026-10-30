//! The clock of a game against an opponent (spec M8-1, D1225-D1226).
//!
//! A pure model: it never reads the system clock, every call that depends on
//! time receives an [`Instant`]. The front-end ticks every 100 ms and the
//! model answers from the instants it is given.
//!
//! Two kinds of clock exist. A *countdown* (`timebonus` mode) has a budget
//! per side and a flag that falls when it reaches zero; a *count-up* (the
//! other modes) only measures the time each side has used. The increment is
//! added to the side that has just moved, the first move included (D1225,
//! a deliberate difference from the Scid, which does not add Black's on
//! move 1).

use std::time::{Duration, Instant};

use mychess_core::Color;
use mychess_engine::Clock as SearchClock;

/// The time budget of one side of a countdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideBudget {
    /// The initial time.
    pub time: Duration,
    /// Added after every move of the side.
    pub increment: Duration,
}

/// What a move cost on the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveTime {
    /// The side that moved.
    pub side: Color,
    /// The time between the start of its turn and the move.
    pub spent: Duration,
    /// The flag of the side had fallen before the move arrived (countdown
    /// only): the move does not count.
    pub flagged: bool,
}

fn index(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

/// The clock of a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayClock {
    /// Countdown: time left. Count-up: time used.
    time: [Duration; 2],
    increment: [Duration; 2],
    countdown: bool,
    turn: Color,
    /// When the running side's turn began; `None` while stopped.
    since: Option<Instant>,
    /// Time of the current move already charged by [`PlayClock::stop`]
    /// (a question or a pause in the middle of the move).
    carried: Duration,
    spent: Vec<MoveTime>,
    /// The increment each move of `spent` added to its side (zero for a
    /// free move or a count-up clock), to take the moves back (M8-1c).
    credits: Vec<Duration>,
}

impl PlayClock {
    /// A countdown with a budget per side (`White`, `Black`).
    pub fn countdown(white: SideBudget, black: SideBudget) -> PlayClock {
        PlayClock {
            time: [white.time, black.time],
            increment: [white.increment, black.increment],
            countdown: true,
            turn: Color::White,
            since: None,
            carried: Duration::ZERO,
            spent: Vec::new(),
            credits: Vec::new(),
        }
    }

    /// A clock that only measures the time used by each side.
    pub fn count_up() -> PlayClock {
        PlayClock {
            time: [Duration::ZERO; 2],
            increment: [Duration::ZERO; 2],
            countdown: false,
            turn: Color::White,
            since: None,
            carried: Duration::ZERO,
            spent: Vec::new(),
            credits: Vec::new(),
        }
    }

    /// A countdown whose times come from the server (online play, spec M8-3
    /// D1319): no local increment, no local result. Feed it with
    /// [`PlayClock::sync`]; the flag of [`PlayClock::flagged`] is only
    /// something to paint red, the end of the game comes from the server.
    pub fn authoritative() -> PlayClock {
        PlayClock::countdown(
            SideBudget {
                time: Duration::ZERO,
                increment: Duration::ZERO,
            },
            SideBudget {
                time: Duration::ZERO,
                increment: Duration::ZERO,
            },
        )
    }

    /// Replaces the times with the ones the server reported at `at` (they
    /// already include the increment). The side `turn` runs from `at` on
    /// when `running`, otherwise the clock stops (game over, game not
    /// started). Moves already recorded are kept.
    pub fn sync(
        &mut self,
        white: Duration,
        black: Duration,
        turn: Color,
        running: bool,
        at: Instant,
    ) {
        self.time = [white, black];
        self.turn = turn;
        self.carried = Duration::ZERO;
        self.since = running.then_some(at);
    }

    /// Whether the flag can fall (a countdown).
    pub fn is_countdown(&self) -> bool {
        self.countdown
    }

    /// The side whose clock runs (or would run).
    pub fn turn(&self) -> Color {
        self.turn
    }

    /// Whether the clock is running.
    pub fn is_running(&self) -> bool {
        self.since.is_some()
    }

    /// Starts the clock for `turn` at `now`.
    pub fn start(&mut self, turn: Color, now: Instant) {
        self.turn = turn;
        self.since = Some(now);
        self.carried = Duration::ZERO;
    }

    /// The time the running side has used since its turn began.
    fn running(&self, now: Instant) -> Duration {
        self.since
            .map_or(Duration::ZERO, |s| now.saturating_duration_since(s))
    }

    /// The shown time of `side` at `now`: time left (countdown, never below
    /// zero) or time used (count-up).
    pub fn shown(&self, side: Color, now: Instant) -> Duration {
        let base = self.time[index(side)];
        let extra = if side == self.turn {
            self.running(now)
        } else {
            Duration::ZERO
        };
        if self.countdown {
            base.saturating_sub(extra)
        } else {
            base.saturating_add(extra)
        }
    }

    /// The side whose flag has fallen at `now`, in a countdown.
    pub fn flagged(&self, now: Instant) -> Option<Color> {
        if self.countdown && self.is_running() && self.shown(self.turn, now).is_zero() {
            Some(self.turn)
        } else {
            None
        }
    }

    /// The running side moved at `now`: the time it used is accounted, the
    /// increment is added and the turn passes to the other side.
    ///
    /// If the flag had already fallen the move is reported as `flagged`, no
    /// increment is added and the clock stops.
    pub fn press(&mut self, now: Instant) -> MoveTime {
        let running = self.running(now);
        // What the pauses of this move already cost was charged to the
        // budget by `stop`; it still counts as time spent on the move.
        let total = self.carried.saturating_add(running);
        self.account(running, total, now)
    }

    /// Like [`PlayClock::press`], but the time used is given from outside
    /// (measured by the task that talks to the opponent, M8-2 D1266) instead
    /// of read from the instants of the model. `now` only restarts the
    /// clock of the other side.
    pub fn press_with(&mut self, used: Duration, now: Instant) -> MoveTime {
        self.account(used, used, now)
    }

    /// Charges `used` to the budget of the mover and reports `reported` as
    /// the time of the move.
    fn account(&mut self, used: Duration, reported: Duration, now: Instant) -> MoveTime {
        self.carried = Duration::ZERO;
        let side = self.turn;
        let i = index(side);
        let flagged = self.countdown && used >= self.time[i];
        let mut credit = Duration::ZERO;
        if self.countdown {
            self.time[i] = self.time[i].saturating_sub(used);
            if !flagged {
                credit = self.increment[i];
                self.time[i] = self.time[i].saturating_add(credit);
            }
        } else {
            self.time[i] = self.time[i].saturating_add(used);
        }
        let record = MoveTime {
            side,
            spent: reported,
            flagged,
        };
        self.spent.push(record);
        self.credits.push(credit);
        self.turn = !side;
        self.since = if flagged { None } else { Some(now) };
        record
    }

    /// A free move (a book move or a pre-played opening move): it costs no
    /// time, adds no increment and the turn passes to the other side, whose
    /// clock starts at `now` if the clock was running.
    pub fn press_free(&mut self, now: Instant) -> MoveTime {
        let side = self.turn;
        self.carried = Duration::ZERO;
        let record = MoveTime {
            side,
            spent: Duration::ZERO,
            flagged: false,
        };
        self.spent.push(record);
        self.credits.push(Duration::ZERO);
        self.turn = !side;
        if self.since.is_some() {
            self.since = Some(now);
        }
        record
    }

    /// Takes back the last `count` moves (M8-1c, D1501): each side gets back
    /// the time the moves cost and loses the increment they paid. The side
    /// that made the first of them is to move again and its clock runs from
    /// `now`. Returns how many moves were taken back (fewer than `count`
    /// when the clock has not seen that many).
    pub fn undo(&mut self, count: usize, now: Instant) -> usize {
        let mut undone = 0;
        while undone < count {
            let (Some(record), Some(credit)) = (self.spent.pop(), self.credits.pop()) else {
                break;
            };
            let i = index(record.side);
            self.time[i] = if self.countdown {
                self.time[i]
                    .saturating_add(record.spent)
                    .saturating_sub(credit)
            } else {
                self.time[i].saturating_sub(record.spent)
            };
            self.turn = record.side;
            undone += 1;
        }
        if undone > 0 {
            self.carried = Duration::ZERO;
            self.since = Some(now);
        }
        undone
    }

    /// Stops the clock at `now`, accounting the time of the running side.
    pub fn stop(&mut self, now: Instant) {
        if self.since.is_none() {
            return;
        }
        let used = self.running(now);
        self.carried = self.carried.saturating_add(used);
        let i = index(self.turn);
        self.time[i] = if self.countdown {
            self.time[i].saturating_sub(used)
        } else {
            self.time[i].saturating_add(used)
        };
        self.since = None;
    }

    /// Starts again for the side whose turn it is (after [`PlayClock::stop`]).
    pub fn resume(&mut self, now: Instant) {
        self.since = Some(now);
    }

    /// The time of every move so far, in order.
    pub fn move_times(&self) -> &[MoveTime] {
        &self.spent
    }

    /// The clock for `go wtime ... btime ...` (D1238), at `now`.
    pub fn search_clock(&self, now: Instant) -> SearchClock {
        SearchClock {
            white_time: self.shown(Color::White, now),
            black_time: self.shown(Color::Black, now),
            white_increment: self.increment[0],
            black_increment: self.increment[1],
            moves_to_go: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: Duration = Duration::from_secs(1);

    #[test]
    fn undo_gives_back_time_and_takes_the_increment_away() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(60, 5), budget(60, 5));
        c.start(Color::White, t0);
        c.press(t0 + 10 * SEC);
        c.press(t0 + 14 * SEC);
        assert_eq!(c.shown(Color::White, t0 + 14 * SEC), 55 * SEC);
        assert_eq!(c.shown(Color::Black, t0 + 14 * SEC), 61 * SEC);
        assert_eq!(c.undo(2, t0 + 20 * SEC), 2);
        assert_eq!(c.turn(), Color::White);
        assert_eq!(c.shown(Color::White, t0 + 20 * SEC), 60 * SEC);
        assert_eq!(c.shown(Color::Black, t0 + 20 * SEC), 60 * SEC);
        assert!(c.move_times().is_empty());
        assert!(c.is_running());
        assert_eq!(c.shown(Color::White, t0 + 23 * SEC), 57 * SEC);
    }

    #[test]
    fn undo_of_a_free_move_changes_no_time_and_count_up_subtracts() {
        let t0 = Instant::now();
        let mut c = PlayClock::count_up();
        c.start(Color::White, t0);
        c.press_free(t0);
        c.press(t0 + 7 * SEC);
        assert_eq!(c.shown(Color::Black, t0 + 7 * SEC), 7 * SEC);
        assert_eq!(c.undo(1, t0 + 8 * SEC), 1);
        assert_eq!(c.shown(Color::Black, t0 + 8 * SEC), Duration::ZERO);
        assert_eq!(c.turn(), Color::Black);
        assert_eq!(c.undo(5, t0 + 8 * SEC), 1);
        assert_eq!(c.turn(), Color::White);
    }

    fn budget(secs: u64, inc: u64) -> SideBudget {
        SideBudget {
            time: Duration::from_secs(secs),
            increment: Duration::from_secs(inc),
        }
    }

    #[test]
    fn a_synced_clock_interpolates_only_the_side_to_move_and_adds_no_increment() {
        let t0 = Instant::now();
        let mut c = PlayClock::authoritative();
        c.sync(60 * SEC, 45 * SEC, Color::Black, true, t0);
        assert_eq!(c.shown(Color::Black, t0 + 5 * SEC), 40 * SEC);
        assert_eq!(c.shown(Color::White, t0 + 5 * SEC), 60 * SEC);
        // A new report replaces everything, whatever was interpolated.
        c.sync(63 * SEC, 41 * SEC, Color::White, true, t0 + 6 * SEC);
        assert_eq!(c.shown(Color::White, t0 + 8 * SEC), 61 * SEC);
        assert_eq!(c.shown(Color::Black, t0 + 8 * SEC), 41 * SEC);
        // Stopped: nothing runs.
        c.sync(10 * SEC, 20 * SEC, Color::White, false, t0 + 9 * SEC);
        assert_eq!(c.shown(Color::White, t0 + 99 * SEC), 10 * SEC);
        assert!(!c.is_running());
    }

    #[test]
    fn a_synced_clock_never_goes_below_zero_and_only_reports_the_flag() {
        let t0 = Instant::now();
        let mut c = PlayClock::authoritative();
        c.sync(2 * SEC, 30 * SEC, Color::White, true, t0);
        assert_eq!(c.shown(Color::White, t0 + 10 * SEC), Duration::ZERO);
        assert_eq!(c.flagged(t0 + 10 * SEC), Some(Color::White));
        assert_eq!(c.flagged(t0 + SEC), None);
    }

    #[test]
    fn counts_down_only_the_side_to_move() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(60, 0), budget(60, 0));
        c.start(Color::White, t0);
        assert_eq!(c.shown(Color::White, t0 + 5 * SEC), 55 * SEC);
        assert_eq!(c.shown(Color::Black, t0 + 5 * SEC), 60 * SEC);
    }

    #[test]
    fn the_increment_goes_to_the_mover_including_the_first_move_of_black() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(60, 3), budget(60, 3));
        c.start(Color::White, t0);
        let w = c.press(t0 + 10 * SEC);
        assert_eq!(
            (w.side, w.spent, w.flagged),
            (Color::White, 10 * SEC, false)
        );
        assert_eq!(c.shown(Color::White, t0 + 10 * SEC), 53 * SEC);
        let b = c.press(t0 + 14 * SEC);
        assert_eq!((b.side, b.spent), (Color::Black, 4 * SEC));
        // 60 - 4 + 3: Black gets the bonus on its very first move (D1225).
        assert_eq!(c.shown(Color::Black, t0 + 14 * SEC), 59 * SEC);
        assert_eq!(c.turn(), Color::White);
    }

    #[test]
    fn sides_have_independent_budgets() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(300, 0), budget(60, 2));
        c.start(Color::White, t0);
        c.press(t0 + SEC);
        c.press(t0 + 2 * SEC);
        assert_eq!(c.shown(Color::White, t0 + 2 * SEC), 299 * SEC);
        assert_eq!(c.shown(Color::Black, t0 + 2 * SEC), 61 * SEC);
        let sc = c.search_clock(t0 + 2 * SEC);
        assert_eq!(sc.black_increment, 2 * SEC);
        assert_eq!(sc.white_increment, Duration::ZERO);
    }

    #[test]
    fn the_flag_falls_when_time_runs_out() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(10, 0), budget(10, 0));
        c.start(Color::White, t0);
        assert_eq!(c.flagged(t0 + 9 * SEC), None);
        assert_eq!(c.flagged(t0 + 10 * SEC), Some(Color::White));
        assert_eq!(c.shown(Color::White, t0 + 99 * SEC), Duration::ZERO);
    }

    #[test]
    fn a_move_after_the_flag_is_reported_and_gets_no_increment() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(10, 5), budget(10, 5));
        c.start(Color::White, t0);
        let m = c.press(t0 + 12 * SEC);
        assert!(m.flagged);
        assert!(!c.is_running());
        assert_eq!(c.shown(Color::White, t0 + 12 * SEC), Duration::ZERO);
    }

    #[test]
    fn count_up_never_flags_and_measures_use() {
        let t0 = Instant::now();
        let mut c = PlayClock::count_up();
        c.start(Color::White, t0);
        assert_eq!(c.flagged(t0 + 1000 * SEC), None);
        c.press(t0 + 7 * SEC);
        c.press(t0 + 9 * SEC);
        assert_eq!(c.shown(Color::White, t0 + 9 * SEC), 7 * SEC);
        assert_eq!(c.shown(Color::Black, t0 + 9 * SEC), 2 * SEC);
        let times: Vec<Duration> = c.move_times().iter().map(|m| m.spent).collect();
        assert_eq!(times, [7 * SEC, 2 * SEC]);
    }

    #[test]
    fn stop_and_resume_do_not_charge_the_pause() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(60, 0), budget(60, 0));
        c.start(Color::White, t0);
        c.stop(t0 + 5 * SEC);
        assert!(!c.is_running());
        assert_eq!(c.shown(Color::White, t0 + 500 * SEC), 55 * SEC);
        c.resume(t0 + 500 * SEC);
        assert_eq!(c.shown(Color::White, t0 + 503 * SEC), 52 * SEC);
    }

    #[test]
    fn a_stopped_clock_has_no_flag() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(1, 0), budget(1, 0));
        c.start(Color::White, t0);
        c.stop(t0 + 100 * SEC);
        assert_eq!(c.flagged(t0 + 200 * SEC), None);
    }

    #[test]
    fn a_free_move_costs_no_time_and_adds_no_increment() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(60, 3), budget(60, 3));
        c.start(Color::White, t0);
        let m = c.press_free(t0 + 20 * SEC);
        assert_eq!(
            (m.side, m.spent, m.flagged),
            (Color::White, Duration::ZERO, false)
        );
        assert_eq!(c.turn(), Color::Black);
        // White keeps 60 s (no increment); Black's clock starts at the
        // moment of the free move, not at the start.
        assert_eq!(c.shown(Color::White, t0 + 20 * SEC), 60 * SEC);
        assert_eq!(c.shown(Color::Black, t0 + 25 * SEC), 55 * SEC);
        assert_eq!(c.move_times().len(), 1);
    }

    #[test]
    fn a_free_move_does_not_start_a_stopped_clock() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(60, 0), budget(60, 0));
        c.press_free(t0);
        assert!(!c.is_running());
        assert_eq!(c.turn(), Color::Black);
    }

    #[test]
    fn the_time_used_can_come_from_outside() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(60, 2), budget(60, 2));
        c.start(Color::White, t0);
        // The model's own instants say 1 s; the task measured 7 s.
        let m = c.press_with(7 * SEC, t0 + SEC);
        assert_eq!(m.spent, 7 * SEC);
        assert_eq!(c.shown(Color::White, t0 + SEC), 55 * SEC);
        let late = c.press_with(70 * SEC, t0 + 2 * SEC);
        assert!(late.flagged);
        assert!(!c.is_running());
    }

    #[test]
    fn the_time_before_a_pause_counts_for_the_move() {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(60, 0), budget(60, 0));
        c.start(Color::White, t0);
        c.stop(t0 + 5 * SEC);
        c.resume(t0 + 100 * SEC);
        let m = c.press(t0 + 103 * SEC);
        assert_eq!(m.spent, 8 * SEC);
        // The budget was charged once: 60 - 5 - 3.
        assert_eq!(c.shown(Color::White, t0 + 103 * SEC), 52 * SEC);
        // The carry does not leak into the next move.
        let b = c.press(t0 + 104 * SEC);
        assert_eq!(b.spent, SEC);
        assert_eq!(c.move_times()[0].spent, 8 * SEC);
    }

    #[test]
    fn the_carry_works_in_count_up_and_resets_on_a_free_move() {
        let t0 = Instant::now();
        let mut c = PlayClock::count_up();
        c.start(Color::White, t0);
        c.stop(t0 + 4 * SEC);
        c.resume(t0 + 50 * SEC);
        assert_eq!(c.press(t0 + 51 * SEC).spent, 5 * SEC);
        assert_eq!(c.shown(Color::White, t0 + 51 * SEC), 5 * SEC);
        c.stop(t0 + 53 * SEC);
        c.press_free(t0 + 60 * SEC);
        c.resume(t0 + 60 * SEC);
        assert_eq!(c.press(t0 + 61 * SEC).spent, SEC);
    }

    #[test]
    fn an_instant_in_the_past_costs_nothing() {
        let t0 = Instant::now();
        let mut c = PlayClock::count_up();
        c.start(Color::White, t0 + 10 * SEC);
        assert_eq!(c.press(t0).spent, Duration::ZERO);
    }
}
