//! M8-1d contract tests of the pure coach (spec D2418-D2423): the two
//! searches, the grades by loss, the decided and mate cases, the queue, the
//! failures and the cancellation. Only `CoachTracker`'s public API.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

use mychess_app::engines::EngineId;
use mychess_app::{
    BLUNDER_CP, COACH_DEPTH_RANGE, COACH_QUEUE_LIMIT, COACH_SECONDS_RANGE, CoachConfig, CoachGrade,
    CoachOutput, CoachSearch, CoachTracker, CoachVerdict, DECIDED_CP, INACCURACY_CP, MISTAKE_CP,
};
use mychess_core::{Color, Move, Position};
use mychess_engine::Score;

fn mv(line: &[&str], uci: &str) -> Move {
    let mut pos = Position::new();
    for t in line {
        let m = pos.parse_uci(t).unwrap();
        pos.play(m).unwrap();
    }
    pos.parse_uci(uci).unwrap()
}

fn tracker(color: Color, hint: bool) -> CoachTracker {
    let mut c = CoachConfig::new(EngineId(2));
    c.hint = hint;
    let mut t = CoachTracker::new(c, color);
    assert!(t.set_ready(true).is_empty());
    t
}

fn search(out: &[CoachOutput]) -> CoachSearch {
    match out {
        [CoachOutput::Search(s)] => s.clone(),
        other => panic!("one search expected: {other:?}"),
    }
}

fn verdict(out: &[CoachOutput]) -> CoachVerdict {
    match out {
        [CoachOutput::Verdict(v)] => v.clone(),
        other => panic!("one verdict expected: {other:?}"),
    }
}

/// White plays `played` from the start; the coach thinks `best` is worth
/// `best_cp` and the move `played_cp` (White's point of view).
fn judge(played: &str, best: &str, best_cp: i32, played_cp: i32) -> CoachVerdict {
    let mut t = tracker(Color::White, false);
    let s = search(&t.player_moved(&Position::new(), &[], mv(&[], played), 1));
    let out = t.search_done(s.token, Some(mv(&[], best)), Some(Score::Cp(best_cp)), None);
    if played == best {
        return verdict(&out);
    }
    let s2 = search(&out);
    verdict(&t.search_done(
        s2.token,
        Some(mv(&[], played)),
        Some(Score::Cp(played_cp)),
        None,
    ))
}

#[test]
fn the_defaults_and_ranges_follow_the_spec() {
    let c = CoachConfig::new(EngineId(1));
    assert_eq!(
        (c.depth, c.seconds, c.hint, c.numbers),
        (12, 3, false, true)
    );
    let wild = CoachConfig {
        depth: 0,
        seconds: 999,
        ..c
    }
    .normalized();
    assert_eq!(wild.depth, COACH_DEPTH_RANGE.0);
    assert_eq!(wild.seconds, COACH_SECONDS_RANGE.1);
}

#[test]
fn the_thresholds_are_the_informant_of_the_scid() {
    assert_eq!(
        (INACCURACY_CP, MISTAKE_CP, BLUNDER_CP, DECIDED_CP),
        (50, 150, 300, 550)
    );
    for (loss, grade) in [
        (0, CoachGrade::Good),
        (49, CoachGrade::Good),
        (50, CoachGrade::Inaccuracy),
        (149, CoachGrade::Inaccuracy),
        (150, CoachGrade::Mistake),
        (299, CoachGrade::Mistake),
        (300, CoachGrade::Blunder),
        (5000, CoachGrade::Blunder),
    ] {
        assert_eq!(CoachGrade::from_loss(loss), grade, "{loss}");
    }
    assert!(!CoachGrade::Good.is_weak() && !CoachGrade::Best.is_weak());
    assert!(CoachGrade::Inaccuracy.is_weak() && CoachGrade::Blunder.is_weak());
}

#[test]
fn the_loss_is_the_difference_and_the_grade_follows_it() {
    assert_eq!(judge("a2a3", "e2e4", 40, -45).grade, CoachGrade::Inaccuracy);
    assert_eq!(judge("a2a3", "e2e4", 40, -45).loss_cp, 85);
    assert_eq!(judge("a2a3", "e2e4", 40, 10).grade, CoachGrade::Good);
    assert_eq!(judge("a2a3", "e2e4", 100, -100).grade, CoachGrade::Mistake);
    assert_eq!(judge("a2a3", "e2e4", 0, -400).grade, CoachGrade::Blunder);
    // A move the coach likes better than its own best (shallower noise)
    // loses nothing.
    let v = judge("a2a3", "e2e4", 10, 30);
    assert_eq!((v.loss_cp, v.grade), (0, CoachGrade::Good));
}

#[test]
fn the_best_move_costs_one_search_and_grades_best() {
    let v = judge("e2e4", "e2e4", 25, 25);
    assert_eq!((v.grade, v.loss_cp), (CoachGrade::Best, 0));
}

#[test]
fn a_decided_game_is_not_warned_about() {
    // Winning by a lot before and after: good.
    let v = judge("a2a3", "e2e4", 900, 600);
    assert_eq!(v.grade, CoachGrade::Good);
    // Losing by a lot before and after: good (nothing to say).
    let v = judge("a2a3", "e2e4", -600, -1500);
    assert_eq!(v.grade, CoachGrade::Good);
    // But throwing the win away is warned about.
    let v = judge("a2a3", "e2e4", 900, 100);
    assert_eq!(v.grade, CoachGrade::Blunder);
}

#[test]
fn mates_are_decisive_and_missing_one_is_a_blunder() {
    let mut t = tracker(Color::White, false);
    let s = search(&t.player_moved(&Position::new(), &[], mv(&[], "a2a3"), 1));
    let out = t.search_done(s.token, Some(mv(&[], "e2e4")), Some(Score::Mate(3)), None);
    let s2 = search(&out);
    let v = verdict(&t.search_done(s2.token, Some(mv(&[], "a2a3")), Some(Score::Cp(200)), None));
    assert_eq!(v.grade, CoachGrade::Blunder);
    assert!(v.decisive, "no number is shown for a mate");
}

#[test]
fn black_is_judged_from_its_own_side() {
    let mut t = tracker(Color::Black, true);
    let line = ["e2e4"];
    let s = search(&t.player_moved(
        &{ Position::new() },
        &[mv(&[], "e2e4")],
        mv(&line, "e7e5"),
        2,
    ));
    // c5 is -20 for White = +20 for Black.
    let s2 = search(&t.search_done(s.token, Some(mv(&line, "c7c5")), Some(Score::Cp(-20)), None));
    assert_eq!(s2.only, Some(mv(&line, "e7e5")));
    let v = verdict(&t.search_done(s2.token, Some(mv(&line, "e7e5")), Some(Score::Cp(30)), None));
    assert_eq!(v.loss_cp, 50);
    assert_eq!(v.best_san.as_deref(), Some("c5"));
    assert_eq!(v.played_san, "e5");
    assert_eq!(v.ply, 2);
}

#[test]
fn the_hint_only_comes_with_a_weak_grade() {
    let mut t = tracker(Color::White, true);
    let s = search(&t.player_moved(&Position::new(), &[], mv(&[], "a2a3"), 1));
    let s2 = search(&t.search_done(s.token, Some(mv(&[], "e2e4")), Some(Score::Cp(30)), None));
    let v = verdict(&t.search_done(s2.token, Some(mv(&[], "a2a3")), Some(Score::Cp(20)), None));
    assert_eq!(v.grade, CoachGrade::Good);
    assert_eq!(v.best_san, None, "a good move needs no hint");
}

#[test]
fn a_late_or_unknown_token_changes_nothing() {
    let mut t = tracker(Color::White, false);
    let s = search(&t.player_moved(&Position::new(), &[], mv(&[], "e2e4"), 1));
    assert!(
        t.search_done(s.token + 1, Some(mv(&[], "e2e4")), Some(Score::Cp(0)), None)
            .is_empty()
    );
    assert!(t.search_done(0, None, None, None).is_empty());
    assert_eq!(t.running(), Some(s.token));
    // The right one still works, once.
    verdict(&t.search_done(s.token, Some(mv(&[], "e2e4")), Some(Score::Cp(0)), None));
    assert!(
        t.search_done(s.token, Some(mv(&[], "e2e4")), Some(Score::Cp(0)), None)
            .is_empty()
    );
}

#[test]
fn a_failed_search_drops_the_move_and_goes_on_with_the_next() {
    let mut t = tracker(Color::White, false);
    let first = search(&t.player_moved(&Position::new(), &[], mv(&[], "e2e4"), 1));
    // Black replied meanwhile and White moved again: queued.
    let line = [mv(&[], "e2e4"), mv(&["e2e4"], "e7e5")];
    assert!(
        t.player_moved(&Position::new(), &line, mv(&["e2e4", "e7e5"], "g1f3"), 3)
            .is_empty()
    );
    assert!(t.busy());
    let out = t.search_done(first.token, None, None, None);
    let next = search(&out);
    assert_eq!(next.moves, line, "the second move's position");
    assert!(t.last().is_none(), "the first was left unjudged");
    // A search with a move but no score also fails the move.
    let out = t.search_done(next.token, Some(mv(&["e2e4", "e7e5"], "g1f3")), None, None);
    assert!(out.is_empty());
    assert!(!t.busy());
}

#[test]
fn moves_wait_for_the_coach_to_be_ready() {
    let mut t = CoachTracker::new(CoachConfig::new(EngineId(2)), Color::White);
    assert!(
        t.player_moved(&Position::new(), &[], mv(&[], "e2e4"), 1)
            .is_empty()
    );
    assert!(t.busy() && t.running().is_none());
    let s = search(&t.set_ready(true));
    assert_eq!(s.moves.len(), 0);
}

#[test]
fn the_queue_is_bounded_and_keeps_the_search_in_flight() {
    let mut t = tracker(Color::White, false);
    let first = search(&t.player_moved(&Position::new(), &[], mv(&[], "e2e4"), 1));
    let line = [mv(&[], "e2e4"), mv(&["e2e4"], "e7e5")];
    for _ in 0..(COACH_QUEUE_LIMIT + 3) {
        t.player_moved(&Position::new(), &line, mv(&["e2e4", "e7e5"], "g1f3"), 3);
    }
    assert_eq!(t.running(), Some(first.token), "the one in flight stays");
    // Count what is left by finishing everything.
    let mut done = 0;
    let mut out = t.search_done(first.token, Some(mv(&[], "e2e4")), Some(Score::Cp(0)), None);
    loop {
        done += 1;
        match out.as_slice() {
            [CoachOutput::Verdict(_), CoachOutput::Search(s)] => {
                let best = mv(&["e2e4", "e7e5"], "g1f3");
                out = t.search_done(s.token, Some(best), Some(Score::Cp(0)), None);
            }
            _ => break,
        }
    }
    assert_eq!(
        done, COACH_QUEUE_LIMIT,
        "in-flight job plus the bounded rest"
    );
}

#[test]
fn moves_that_do_not_belong_to_the_player_or_are_illegal_are_ignored() {
    let mut t = tracker(Color::Black, false);
    // It is White to move at the start: not the player's move.
    assert!(
        t.player_moved(&Position::new(), &[], mv(&[], "e2e4"), 1)
            .is_empty()
    );
    let mut t = tracker(Color::White, false);
    // e7e5 is not legal for White at the start.
    assert!(
        t.player_moved(&Position::new(), &[], mv(&["e2e4"], "e7e5"), 1)
            .is_empty()
    );
    assert!(!t.busy());
}

#[test]
fn cancel_clears_everything_and_stops_the_search_in_flight_once() {
    let mut t = tracker(Color::White, false);
    drop(t.player_moved(&Position::new(), &[], mv(&[], "e2e4"), 1));
    assert_eq!(t.cancel(), [CoachOutput::Stop]);
    assert!(!t.busy());
    assert!(t.cancel().is_empty());
    // Not ready any more: nothing starts.
    assert!(t.set_ready(false).is_empty());
    assert!(
        t.player_moved(&Position::new(), &[], mv(&[], "e2e4"), 1)
            .is_empty()
    );
}

#[test]
fn the_last_remark_is_kept_for_the_panel() {
    let mut t = tracker(Color::White, false);
    let s = search(&t.player_moved(&Position::new(), &[], mv(&[], "e2e4"), 1));
    verdict(&t.search_done(s.token, Some(mv(&[], "e2e4")), Some(Score::Cp(0)), None));
    assert_eq!(t.last().map(|v| v.grade), Some(CoachGrade::Best));
    drop(t.cancel());
    assert!(t.last().is_some(), "cancel keeps the last remark");
}

#[test]
fn searches_that_ended_at_distant_depths_leave_the_move_unjudged() {
    for (d1, d2, judged) in [
        (12, 8, false),
        (8, 12, false),
        (9, 8, true),
        (8, 9, true),
        (8, 8, true),
    ] {
        let mut t = tracker(Color::White, false);
        let s = search(&t.player_moved(&Position::new(), &[], mv(&[], "a2a3"), 1));
        let out = t.search_done(
            s.token,
            Some(mv(&[], "e2e4")),
            Some(Score::Cp(300)),
            Some(d1),
        );
        let s2 = search(&out);
        let out = t.search_done(
            s2.token,
            Some(mv(&[], "a2a3")),
            Some(Score::Cp(-200)),
            Some(d2),
        );
        assert_eq!(!out.is_empty() && t.last().is_some(), judged, "{d1} {d2}");
        assert!(!t.busy(), "the queue is free either way");
    }
    // A depth the engine did not report is not compared.
    let mut t = tracker(Color::White, false);
    let s = search(&t.player_moved(&Position::new(), &[], mv(&[], "a2a3"), 1));
    let s2 = search(&t.search_done(
        s.token,
        Some(mv(&[], "e2e4")),
        Some(Score::Cp(300)),
        Some(30),
    ));
    let out = t.search_done(s2.token, Some(mv(&[], "a2a3")), Some(Score::Cp(-200)), None);
    assert_eq!(verdict(&out).grade, CoachGrade::Blunder);
    assert_eq!(mychess_app::DEPTH_TOLERANCE, 1);
}
