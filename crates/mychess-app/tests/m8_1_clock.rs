//! M8-1a contract tests of the clock (spec D1225, D1226, D1248): counting,
//! increment (first move included), time per move, flag, free moves and
//! externally measured times. The time is always injected.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1_common;

use std::time::{Duration, Instant};

use m8_1_common::*;
use mychess_app::{PlayClock, SideBudget};
use mychess_core::Color;
use proptest::prelude::*;
use rstest::rstest;

fn budget(secs: u64, inc: u64) -> SideBudget {
    SideBudget {
        time: Duration::from_secs(secs),
        increment: Duration::from_secs(inc),
    }
}

fn blitz_clock(t0: Instant) -> PlayClock {
    let mut c = PlayClock::countdown(budget(300, 3), budget(300, 3));
    c.start(Color::White, t0);
    c
}

#[test]
fn a_countdown_shows_the_time_left_of_the_running_side_only() {
    let t0 = Instant::now();
    let c = blitz_clock(t0);
    assert!(c.is_countdown());
    assert!(c.is_running());
    assert_eq!(c.turn(), Color::White);
    assert_eq!(c.shown(Color::White, t0 + 10 * SEC), 290 * SEC);
    assert_eq!(c.shown(Color::Black, t0 + 10 * SEC), 300 * SEC);
}

#[test]
fn a_count_up_clock_shows_the_time_used_and_never_flags() {
    let t0 = Instant::now();
    let mut c = PlayClock::count_up();
    c.start(Color::White, t0);
    assert!(!c.is_countdown());
    assert_eq!(c.shown(Color::White, t0 + 7 * SEC), 7 * SEC);
    assert_eq!(c.shown(Color::Black, t0 + 7 * SEC), Duration::ZERO);
    assert_eq!(c.flagged(t0 + 100_000 * SEC), None);
    let r = c.press(t0 + 100_000 * SEC);
    assert!(!r.flagged);
    assert_eq!(r.spent, 100_000 * SEC);
}

#[test]
fn nothing_runs_before_start() {
    let t0 = Instant::now();
    let c = PlayClock::countdown(budget(60, 0), budget(60, 0));
    assert!(!c.is_running());
    assert_eq!(c.shown(Color::White, t0 + 1000 * SEC), 60 * SEC);
    assert_eq!(c.flagged(t0 + 1000 * SEC), None);
}

#[test]
fn the_increment_is_added_after_the_move_of_each_side_first_moves_included() {
    // D1225: unlike the Scid, Black also gets the increment on move 1.
    let t0 = Instant::now();
    let mut c = blitz_clock(t0);
    let w = c.press(t0 + 4 * SEC);
    assert_eq!(w.side, Color::White);
    assert_eq!(w.spent, 4 * SEC);
    assert!(!w.flagged);
    assert_eq!(c.turn(), Color::Black);
    assert_eq!(c.shown(Color::White, t0 + 4 * SEC), 299 * SEC); // 300 - 4 + 3
    let b = c.press(t0 + 9 * SEC);
    assert_eq!(b.side, Color::Black);
    assert_eq!(b.spent, 5 * SEC);
    assert_eq!(c.shown(Color::Black, t0 + 9 * SEC), 298 * SEC); // 300 - 5 + 3
    assert_eq!(c.turn(), Color::White);
}

#[test]
fn each_side_has_its_own_increment() {
    let t0 = Instant::now();
    let mut c = PlayClock::countdown(budget(100, 10), budget(200, 0));
    c.start(Color::White, t0);
    c.press(t0);
    c.press(t0);
    assert_eq!(c.shown(Color::White, t0), 110 * SEC);
    assert_eq!(c.shown(Color::Black, t0), 200 * SEC);
}

#[test]
fn the_time_spent_of_every_move_is_recorded_in_order() {
    let t0 = Instant::now();
    let mut c = blitz_clock(t0);
    c.press(t0 + 4 * SEC);
    c.press(t0 + 6 * SEC);
    c.press(t0 + 13 * SEC);
    let spent: Vec<_> = c.move_times().iter().map(|m| (m.side, m.spent)).collect();
    assert_eq!(
        spent,
        [
            (Color::White, 4 * SEC),
            (Color::Black, 2 * SEC),
            (Color::White, 7 * SEC)
        ]
    );
}

#[test]
fn the_flag_falls_exactly_at_zero_and_not_a_millisecond_before() {
    let t0 = Instant::now();
    let c = blitz_clock(t0);
    assert_eq!(c.flagged(t0 + 299 * SEC + Duration::from_millis(999)), None);
    assert_eq!(c.flagged(t0 + 300 * SEC), Some(Color::White));
    assert_eq!(c.flagged(t0 + 9_999 * SEC), Some(Color::White));
    assert_eq!(c.shown(Color::White, t0 + 9_999 * SEC), Duration::ZERO);
}

#[test]
fn the_flag_of_black_falls_on_its_own_turn() {
    let t0 = Instant::now();
    let mut c = PlayClock::countdown(budget(60, 0), budget(10, 0));
    c.start(Color::White, t0);
    c.press(t0 + SEC);
    assert_eq!(c.flagged(t0 + 10 * SEC), None);
    assert_eq!(c.flagged(t0 + 12 * SEC), Some(Color::Black));
}

#[test]
fn a_move_that_arrives_after_the_flag_is_flagged_without_increment_and_stops_the_clock() {
    let t0 = Instant::now();
    let mut c = PlayClock::countdown(budget(10, 5), budget(10, 5));
    c.start(Color::White, t0);
    let r = c.press(t0 + 11 * SEC);
    assert!(r.flagged);
    assert_eq!(r.side, Color::White);
    assert!(!c.is_running());
    assert_eq!(c.shown(Color::White, t0 + 11 * SEC), Duration::ZERO);
    assert_eq!(c.flagged(t0 + 99 * SEC), None, "stopped clocks do not flag");
}

#[test]
fn a_move_that_uses_exactly_the_time_left_is_flagged() {
    let t0 = Instant::now();
    let mut c = PlayClock::countdown(budget(10, 5), budget(10, 5));
    c.start(Color::White, t0);
    assert!(c.press(t0 + 10 * SEC).flagged);
}

#[test]
fn a_move_one_millisecond_before_the_flag_is_not_flagged_and_gets_the_increment() {
    let t0 = Instant::now();
    let mut c = PlayClock::countdown(budget(10, 5), budget(10, 5));
    c.start(Color::White, t0);
    let used = 9 * SEC + Duration::from_millis(999);
    assert!(!c.press(t0 + used).flagged);
    assert_eq!(
        c.shown(Color::White, t0 + used),
        Duration::from_millis(1) + 5 * SEC
    );
}

#[test]
fn stop_accounts_the_time_and_freezes_the_display() {
    let t0 = Instant::now();
    let mut c = blitz_clock(t0);
    c.stop(t0 + 20 * SEC);
    assert!(!c.is_running());
    assert_eq!(c.shown(Color::White, t0 + 20 * SEC), 280 * SEC);
    assert_eq!(c.shown(Color::White, t0 + 500 * SEC), 280 * SEC);
    // Stopping twice does not charge twice.
    c.stop(t0 + 50 * SEC);
    assert_eq!(c.shown(Color::White, t0 + 50 * SEC), 280 * SEC);
}

#[test]
fn resume_restarts_the_clock_and_the_move_counts_the_time_before_and_after_the_pause_only() {
    let t0 = Instant::now();
    let mut c = blitz_clock(t0);
    c.stop(t0 + 20 * SEC);
    c.resume(t0 + 100 * SEC);
    assert!(c.is_running());
    assert_eq!(c.shown(Color::White, t0 + 105 * SEC), 275 * SEC);
    let m = c.press(t0 + 105 * SEC);
    // The move cost 20 s before the pause and 5 s after it; the 80 s of the
    // pause itself are not charged.
    assert_eq!(m.spent, 25 * SEC);
    assert_eq!(c.shown(Color::White, t0 + 105 * SEC), (300 - 25 + 3) * SEC);
}

#[test]
fn several_pauses_inside_one_move_add_up_in_the_time_of_the_move() {
    let t0 = Instant::now();
    let mut c = blitz_clock(t0);
    c.stop(t0 + 10 * SEC);
    c.resume(t0 + 100 * SEC);
    c.stop(t0 + 104 * SEC);
    c.resume(t0 + 200 * SEC);
    let m = c.press(t0 + 203 * SEC);
    assert_eq!(m.spent, 17 * SEC);
    assert_eq!(c.move_times()[0].spent, 17 * SEC);
}

#[test]
fn the_time_before_a_pause_does_not_leak_into_the_next_move() {
    let t0 = Instant::now();
    let mut c = blitz_clock(t0);
    c.stop(t0 + 10 * SEC);
    c.resume(t0 + 50 * SEC);
    c.press(t0 + 51 * SEC);
    let m = c.press(t0 + 55 * SEC);
    assert_eq!(m.side, Color::Black);
    assert_eq!(m.spent, 4 * SEC);
}

#[test]
fn a_free_move_after_a_pause_does_not_hand_its_carried_time_to_the_other_side() {
    let t0 = Instant::now();
    let mut c = blitz_clock(t0);
    c.stop(t0 + 10 * SEC);
    c.press_free(t0 + 10 * SEC);
    c.resume(t0 + 40 * SEC);
    let m = c.press(t0 + 43 * SEC);
    assert_eq!(m.side, Color::Black);
    assert_eq!(m.spent, 3 * SEC);
}

#[test]
fn stopping_a_count_up_clock_adds_the_time_used() {
    let t0 = Instant::now();
    let mut c = PlayClock::count_up();
    c.start(Color::White, t0);
    c.stop(t0 + 9 * SEC);
    assert_eq!(c.shown(Color::White, t0 + 99 * SEC), 9 * SEC);
}

#[test]
fn a_free_move_costs_no_time_and_no_increment_and_passes_the_turn() {
    let t0 = Instant::now();
    let mut c = blitz_clock(t0);
    let m = c.press_free(t0 + 30 * SEC);
    assert_eq!(m.side, Color::White);
    assert_eq!(m.spent, Duration::ZERO);
    assert!(!m.flagged);
    assert_eq!(c.turn(), Color::Black);
    // White keeps 300 (no increment, nothing charged) even though 30 s passed.
    assert_eq!(c.shown(Color::White, t0 + 30 * SEC), 300 * SEC);
    // Black's clock starts at the free move.
    assert_eq!(c.shown(Color::Black, t0 + 35 * SEC), 295 * SEC);
    assert_eq!(c.move_times().len(), 1);
}

#[test]
fn a_free_move_on_a_stopped_clock_leaves_it_stopped() {
    let t0 = Instant::now();
    let mut c = blitz_clock(t0);
    c.stop(t0);
    c.press_free(t0 + SEC);
    assert!(!c.is_running());
    assert_eq!(c.turn(), Color::Black);
}

#[test]
fn a_free_move_is_never_flagged_even_with_the_flag_already_down() {
    let t0 = Instant::now();
    let mut c = PlayClock::countdown(budget(5, 0), budget(5, 0));
    c.start(Color::White, t0);
    assert!(!c.press_free(t0 + 99 * SEC).flagged);
}

#[test]
fn press_with_uses_the_time_measured_outside_not_the_instants() {
    let t0 = Instant::now();
    let mut c = blitz_clock(t0);
    // The instants say 100 s; the outside measure says 2 s.
    let m = c.press_with(2 * SEC, t0 + 100 * SEC);
    assert_eq!(m.spent, 2 * SEC);
    assert!(!m.flagged);
    assert_eq!(c.shown(Color::White, t0 + 100 * SEC), 301 * SEC);
    // The clock of the other side restarts at `now`.
    assert_eq!(c.shown(Color::Black, t0 + 103 * SEC), 297 * SEC);
}

#[test]
fn press_with_a_time_beyond_the_budget_flags() {
    let t0 = Instant::now();
    let mut c = PlayClock::countdown(budget(10, 3), budget(10, 3));
    c.start(Color::White, t0);
    let m = c.press_with(10 * SEC, t0);
    assert!(m.flagged);
    assert!(!c.is_running());
}

#[test]
fn the_search_clock_carries_time_left_and_increments_in_the_go_fields() {
    let t0 = Instant::now();
    let mut c = PlayClock::countdown(budget(300, 3), budget(120, 1));
    c.start(Color::White, t0);
    let s = c.search_clock(t0 + 10 * SEC);
    assert_eq!(s.white_time, 290 * SEC);
    assert_eq!(s.black_time, 120 * SEC);
    assert_eq!(s.white_increment, 3 * SEC);
    assert_eq!(s.black_increment, SEC);
    assert_eq!(s.moves_to_go, None);
}

#[rstest]
#[case(Color::White)]
#[case(Color::Black)]
fn the_clock_can_start_on_either_side(#[case] side: Color) {
    let t0 = Instant::now();
    let mut c = PlayClock::countdown(budget(60, 0), budget(60, 0));
    c.start(side, t0);
    assert_eq!(c.turn(), side);
    assert_eq!(c.shown(side, t0 + 5 * SEC), 55 * SEC);
    assert_eq!(c.shown(!side, t0 + 5 * SEC), 60 * SEC);
}

proptest! {
    /// Without increment the total of time left plus time spent is constant.
    #[test]
    fn time_is_conserved_without_increment(steps in proptest::collection::vec(0u64..2_000, 1..40)) {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(100_000, 0), budget(100_000, 0));
        c.start(Color::White, t0);
        let mut now = t0;
        let mut spent = Duration::ZERO;
        for ms in steps {
            now += Duration::from_millis(ms);
            let m = c.press(now);
            prop_assert!(!m.flagged);
            spent += m.spent;
        }
        let total = c.shown(Color::White, now) + c.shown(Color::Black, now);
        prop_assert_eq!(total + spent, 200_000 * SEC);
    }

    /// The flag of a countdown never falls while the side still has time.
    #[test]
    fn the_flag_agrees_with_the_shown_time(secs in 0u64..400) {
        let t0 = Instant::now();
        let mut c = PlayClock::countdown(budget(300, 0), budget(300, 0));
        c.start(Color::White, t0);
        let at = t0 + Duration::from_secs(secs);
        prop_assert_eq!(c.flagged(at).is_some(), c.shown(Color::White, at).is_zero());
        prop_assert_eq!(c.flagged(at).is_some(), secs >= 300);
    }
}
