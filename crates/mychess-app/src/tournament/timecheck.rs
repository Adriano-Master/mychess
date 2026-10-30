//! Does each engine respect the clock? (spec M8-2c, D2514-D2516).
//!
//! [`TimeStats`] collects what the games measured about one participant: the
//! response time of every move (measured by the task that talks to the
//! engine, D1267), the excess over a fixed time per move, the lowest clock
//! and the games lost on time. Everything is pure; the runner feeds it when a
//! game is saved and the stats travel in the saved state.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::config::TimeControl;

/// The least a move may take over its time before it counts as an overrun,
/// in milliseconds (D2514).
pub const OVERRUN_SLACK_MS: u64 = 50;
/// The same slack in per cent of the time of the move.
pub const OVERRUN_SLACK_PERCENT: u64 = 10;
/// The numbers of the saved file never pass the signed 64-bit range.
const CAP: u64 = u64::MAX >> 1;

/// The limit of a move of `MoveTime { ms }` before it is an overrun.
pub fn overrun_limit_ms(ms: u64) -> u64 {
    ms.saturating_add((ms / 100 * OVERRUN_SLACK_PERCENT).max(OVERRUN_SLACK_MS))
}

/// A move of the participant, as the game measured it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedMove {
    /// The time between `go` and `bestmove`.
    pub spent: Duration,
    /// The clock of the side after the move (countdown only).
    pub left: Option<Duration>,
}

/// What the games measured about one participant (D2514).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeStats {
    /// Games with at least one timed move.
    pub games: u32,
    /// Timed moves.
    pub moves: u64,
    /// The sum of the response times, in milliseconds.
    pub total_ms: u64,
    /// The slowest move, in milliseconds.
    pub max_ms: u64,
    /// `MoveTime` moves that took more than the time.
    pub over_moves: u64,
    /// The sum of those excesses, in milliseconds.
    pub over_total_ms: u64,
    /// The largest excess, in milliseconds.
    pub over_max_ms: u64,
    /// `MoveTime` moves above the limit with slack ([`overrun_limit_ms`]).
    pub overruns: u32,
    /// The lowest clock left after a move, in milliseconds (clock games).
    pub min_left_ms: Option<u64>,
    /// Time spent in clock games, in milliseconds.
    pub clock_spent_ms: u64,
    /// The time the clock gave in those games (base plus increments), in
    /// milliseconds.
    pub clock_budget_ms: u64,
    /// Games lost on time.
    pub forfeits: u32,
}

fn add(value: &mut u64, by: u64) {
    *value = value.saturating_add(by).min(CAP);
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX).min(CAP)
}

impl TimeStats {
    /// Adds a game: the timed moves of the participant (book and opening
    /// moves are not part of them) and whether the flag fell on him.
    pub fn record_game(
        &mut self,
        time: TimeControl,
        moves: impl IntoIterator<Item = TimedMove>,
        lost_on_time: bool,
    ) {
        let mut count = 0u64;
        let mut spent_in_game = 0u64;
        for m in moves {
            count += 1;
            let ms = millis(m.spent);
            spent_in_game = spent_in_game.saturating_add(ms).min(CAP);
            add(&mut self.total_ms, ms);
            self.max_ms = self.max_ms.max(ms);
            if let TimeControl::MoveTime { ms: allowed } = time
                && ms > allowed
            {
                let over = ms - allowed;
                self.over_moves = self.over_moves.saturating_add(1);
                add(&mut self.over_total_ms, over);
                self.over_max_ms = self.over_max_ms.max(over);
                if ms > overrun_limit_ms(allowed) {
                    self.overruns = self.overruns.saturating_add(1);
                }
            }
            if let (TimeControl::Clock { .. }, Some(left)) = (time, m.left) {
                let left = millis(left);
                self.min_left_ms = Some(self.min_left_ms.map_or(left, |old| old.min(left)));
            }
        }
        if count > 0 {
            self.games = self.games.saturating_add(1);
            add(&mut self.moves, count);
        }
        if let TimeControl::Clock {
            base_ms,
            increment_ms,
        } = time
            && count > 0
        {
            add(&mut self.clock_spent_ms, spent_in_game);
            let budget = base_ms.saturating_add(increment_ms.saturating_mul(count));
            add(&mut self.clock_budget_ms, budget);
        }
        if lost_on_time {
            self.forfeits = self.forfeits.saturating_add(1);
        }
    }

    /// The mean response time in milliseconds.
    pub fn mean_ms(&self) -> Option<u64> {
        self.total_ms.checked_div(self.moves)
    }

    /// The mean excess over the time of a move, in milliseconds, among the
    /// moves that took longer (`MoveTime`): the overhead of the protocol and
    /// of the process (D2515).
    pub fn mean_over_ms(&self) -> Option<u64> {
        self.over_total_ms.checked_div(self.over_moves)
    }

    /// The share of the clock the engine used, in per cent (clock games).
    #[allow(clippy::cast_precision_loss)] // milliseconds of a tournament
    pub fn clock_used_percent(&self) -> Option<f64> {
        if self.clock_budget_ms == 0 {
            return None;
        }
        Some(self.clock_spent_ms as f64 * 100.0 / self.clock_budget_ms as f64)
    }

    /// The verdict (D2515).
    pub fn verdict(&self) -> TimeVerdict {
        if self.forfeits > 0 {
            TimeVerdict::Forfeits
        } else if self.overruns > 0 {
            TimeVerdict::Overruns
        } else if self.moves == 0 {
            TimeVerdict::NoData
        } else {
            TimeVerdict::Respects
        }
    }
}

/// Whether an engine respects the clock (D2515).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeVerdict {
    /// Nothing was measured.
    NoData,
    /// No game lost on time and no move over the limit.
    Respects,
    /// Some move took longer than the time plus the slack.
    Overruns,
    /// A game was lost on time.
    Forfeits,
}

impl TimeVerdict {
    /// The Fluent key of the label.
    pub fn label_key(self) -> &'static str {
        match self {
            TimeVerdict::NoData => "tournament-time-verdict-none",
            TimeVerdict::Respects => "tournament-time-verdict-ok",
            TimeVerdict::Overruns => "tournament-time-verdict-overruns",
            TimeVerdict::Forfeits => "tournament-time-verdict-forfeits",
        }
    }
}

/// A line of the report of time (D2516).
#[derive(Debug, Clone, PartialEq)]
pub struct TimeRow {
    /// The participant.
    pub participant: usize,
    /// What was measured.
    pub stats: TimeStats,
    /// The mean response time, in milliseconds.
    pub mean_ms: Option<u64>,
    /// The mean excess (overhead), in milliseconds.
    pub overhead_ms: Option<u64>,
    /// The share of the clock used, in per cent.
    pub clock_used_percent: Option<f64>,
    /// The verdict.
    pub verdict: TimeVerdict,
}

/// The report, one row per participant, in the order of registration.
pub fn time_report(stats: &[TimeStats]) -> Vec<TimeRow> {
    stats
        .iter()
        .enumerate()
        .map(|(participant, s)| TimeRow {
            participant,
            stats: s.clone(),
            mean_ms: s.mean_ms(),
            overhead_ms: s.mean_over_ms(),
            clock_used_percent: s.clock_used_percent(),
            verdict: s.verdict(),
        })
        .collect()
}

/// Milliseconds as text for the tables: `250 ms`, `1.25 s`, `-` for none.
pub fn duration_text(ms: Option<u64>) -> String {
    match ms {
        None => "-".to_owned(),
        Some(ms) if ms < 1000 => format!("{ms} ms"),
        Some(ms) => format!("{}.{:02} s", ms / 1000, ms % 1000 / 10),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(ms: u64, left: Option<u64>) -> TimedMove {
        TimedMove {
            spent: Duration::from_millis(ms),
            left: left.map(Duration::from_millis),
        }
    }

    const MOVE_TIME: TimeControl = TimeControl::MoveTime { ms: 1000 };
    const CLOCK: TimeControl = TimeControl::Clock {
        base_ms: 10_000,
        increment_ms: 100,
    };

    #[test]
    fn collects_mean_and_maximum() {
        let mut s = TimeStats::default();
        s.record_game(
            MOVE_TIME,
            [mv(900, None), mv(1000, None), mv(950, None)],
            false,
        );
        assert_eq!((s.games, s.moves, s.total_ms, s.max_ms), (1, 3, 2850, 1000));
        assert_eq!(s.mean_ms(), Some(950));
        assert_eq!(s.verdict(), TimeVerdict::Respects);
        assert_eq!(s.mean_over_ms(), None);
    }

    #[test]
    fn the_excess_is_the_overhead_and_slack_decides_overruns() {
        let mut s = TimeStats::default();
        // 1000 ms allowed: the limit is 1000 + max(50, 100) = 1100.
        assert_eq!(overrun_limit_ms(1000), 1100);
        s.record_game(
            MOVE_TIME,
            [mv(1040, None), mv(1100, None), mv(900, None)],
            false,
        );
        assert_eq!(
            (s.over_moves, s.over_total_ms, s.over_max_ms),
            (2, 140, 100)
        );
        assert_eq!(s.mean_over_ms(), Some(70));
        assert_eq!(s.overruns, 0, "1100 is not above the limit");
        assert_eq!(s.verdict(), TimeVerdict::Respects);
        s.record_game(MOVE_TIME, [mv(1101, None)], false);
        assert_eq!(s.overruns, 1);
        assert_eq!(s.verdict(), TimeVerdict::Overruns);
    }

    #[test]
    fn short_times_use_the_fixed_slack() {
        assert_eq!(overrun_limit_ms(100), 150);
        assert_eq!(overrun_limit_ms(0), 50);
        assert_eq!(overrun_limit_ms(u64::MAX), u64::MAX);
    }

    #[test]
    fn a_forfeit_beats_every_other_verdict() {
        let mut s = TimeStats::default();
        s.record_game(MOVE_TIME, [mv(5000, None)], true);
        assert_eq!(s.verdict(), TimeVerdict::Forfeits);
        assert_eq!(s.forfeits, 1);
        assert_eq!(s.overruns, 1);
    }

    #[test]
    fn clock_games_track_the_lowest_clock_and_the_share_used() {
        let mut s = TimeStats::default();
        s.record_game(
            CLOCK,
            [
                mv(1000, Some(9100)),
                mv(2000, Some(7200)),
                mv(500, Some(6800)),
            ],
            false,
        );
        assert_eq!(s.min_left_ms, Some(6800));
        // 3500 ms of 10_000 + 3 * 100.
        let used = s.clock_used_percent().expect("share");
        assert!((used - 3500.0 * 100.0 / 10_300.0).abs() < 1e-9, "{used}");
        // A fixed time does not touch the clock numbers.
        let mut t = TimeStats::default();
        t.record_game(MOVE_TIME, [mv(10, Some(1))], false);
        assert_eq!((t.min_left_ms, t.clock_used_percent()), (None, None));
    }

    #[test]
    fn a_game_without_timed_moves_adds_no_game() {
        let mut s = TimeStats::default();
        s.record_game(CLOCK, [], false);
        assert_eq!(s, TimeStats::default());
        assert_eq!(s.verdict(), TimeVerdict::NoData);
        assert_eq!((s.mean_ms(), s.mean_over_ms()), (None, None));
    }

    #[test]
    fn depth_games_only_collect_response_times() {
        let mut s = TimeStats::default();
        s.record_game(TimeControl::Depth(8), [mv(40, None), mv(60, None)], false);
        assert_eq!(s.mean_ms(), Some(50));
        assert_eq!((s.over_moves, s.overruns), (0, 0));
    }

    #[test]
    fn huge_numbers_saturate_without_overflow() {
        let mut s = TimeStats::default();
        let huge = TimedMove {
            spent: Duration::MAX,
            left: Some(Duration::MAX),
        };
        s.record_game(CLOCK, [huge, huge], false);
        s.record_game(MOVE_TIME, [huge], true);
        assert!(s.total_ms <= u64::MAX >> 1);
        assert!(s.clock_budget_ms <= u64::MAX >> 1);
    }

    #[test]
    fn the_report_has_one_row_per_participant() {
        let mut a = TimeStats::default();
        a.record_game(MOVE_TIME, [mv(500, None)], false);
        let rows = time_report(&[a, TimeStats::default()]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].mean_ms, Some(500));
        assert_eq!(rows[0].verdict, TimeVerdict::Respects);
        assert_eq!(rows[1].verdict, TimeVerdict::NoData);
        assert_eq!(rows[1].participant, 1);
    }

    #[test]
    fn stats_survive_a_toml_round_trip_and_old_files() {
        let mut s = TimeStats::default();
        s.record_game(CLOCK, [mv(1000, Some(9000))], true);
        let text = toml::to_string(&s).expect("write");
        let back: TimeStats = toml::from_str(&text).expect("read");
        assert_eq!(back, s);
        let old: TimeStats = toml::from_str("").expect("empty table");
        assert_eq!(old, TimeStats::default());
    }

    #[test]
    fn durations_read_well() {
        assert_eq!(duration_text(None), "-");
        assert_eq!(duration_text(Some(250)), "250 ms");
        assert_eq!(duration_text(Some(1250)), "1.25 s");
        assert_eq!(duration_text(Some(61_000)), "61.00 s");
    }
}
