//! M8-2a contract tests of the tournament state machine (spec D1269, D1270,
//! D1275, D1284, D1286, D1294-D1296, D1308, D1312, D1314): schedule played
//! to the end, failures, pause, stop, abort, skip, redo, cancellation by
//! epoch and resume. The tasks are simulated by `Sim`.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m8_2_common;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use m8_2_common::*;
use mychess_app::tournament::{
    Action, Decision, FailKind, GameStatus, LogEvent, OnTimeLoss, Participant, Phase, Reply,
    Scheduling, SearchOutcome, Termination, TimeControl, Tournament, TournamentConfig,
    TournamentNotice, parse_state, render_state,
};
use mychess_core::{Color, GameResult};
use mychess_engine::Score;

/// The participant with the lowest index wins every game it plays.
fn low_wins() -> Policy {
    decide_by(|c| {
        if c.white < c.black {
            GameResult::White
        } else {
            GameResult::Black
        }
    })
}

fn finished_phase(sim: &Sim) -> Phase {
    sim.t.phase()
}

#[test]
fn a_round_robin_plays_every_game_saves_each_one_and_names_the_winner() {
    let mut config = cfg(3);
    config.cycles = 2;
    let mut sim = Sim::new(config, low_wins());
    sim.start();
    assert_eq!(finished_phase(&sim), Phase::Finished);
    assert_eq!(sim.t.progress(), (6, 6));
    assert_eq!(sim.games.len(), 6);
    let numbers: Vec<u32> = sim.games.iter().map(|(n, _)| *n).collect();
    assert_eq!(numbers, [1, 2, 3, 4, 5, 6]);
    let rows = sim.t.standings();
    assert_eq!(rows[0].participant, 0);
    assert_eq!(rows[0].points_text(), "4");
    assert_eq!(rows[1].points_text(), "2");
    assert_eq!(rows[2].points_text(), "0");
    assert_eq!(sim.t.winners(), [0]);
    assert!(
        sim.notices
            .iter()
            .any(|n| matches!(n, TournamentNotice::Finished { winners } if winners == &[0]))
    );
    assert!(sim.shutdowns >= 1, "the engines are closed at the end");
    // The state is saved at the start and after each game; the last one is
    // marked finished.
    assert!(sim.states.len() >= 7);
    assert!(sim.states.last().unwrap().finished);
    assert!(!sim.states.first().unwrap().finished);
}

#[test]
fn the_state_revision_strictly_grows_with_every_saved_state() {
    let mut sim = Sim::new(cfg(3), low_wins());
    sim.start();
    let revs: Vec<u64> = sim.states.iter().map(|s| s.revision).collect();
    assert!(revs.windows(2).all(|w| w[0] < w[1]), "{revs:?}");
}

#[test]
fn every_saved_game_has_the_round_and_players_of_its_pairing() {
    let mut sim = Sim::new(cfg(3), low_wins());
    sim.start();
    let slots = sim.t.slots().to_vec();
    for ((number, game), slot) in sim.games.iter().zip(&slots) {
        assert_eq!(*number, slot.pairing.number);
        assert_eq!(game.tags().round, slot.pairing.round_label());
        assert_eq!(
            game.tags().white,
            format!("P{}", slot.pairing.white),
            "game {number}"
        );
        assert_eq!(game.tags().black, format!("P{}", slot.pairing.black));
    }
}

#[test]
fn a_gauntlet_plays_only_the_tested_participant() {
    let mut config = cfg(4);
    config.scheduling = Scheduling::Gauntlet;
    config.tested = 2;
    let mut sim = Sim::new(config, low_wins());
    sim.start();
    assert_eq!(sim.games.len(), 3);
    for slot in sim.t.slots() {
        assert!(slot.pairing.involves(2));
    }
}

#[test]
fn the_carousel_tournament_with_odd_players_reports_its_byes() {
    let mut config = cfg(5);
    config.scheduling = Scheduling::Carousel;
    let mut sim = Sim::new(config, low_wins());
    sim.start();
    assert_eq!(sim.t.byes().len(), 5);
    assert_eq!(sim.games.len(), 10);
}

// ---- epochs ----

#[test]
fn an_answer_with_a_stale_or_unknown_epoch_is_ignored_and_a_second_answer_too() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.hold = true;
    sim.start();
    let Some(Action::Search(job)) = sim.held.last().cloned() else {
        panic!("a search in flight")
    };
    for epoch in [job.epoch + 1, job.epoch.saturating_sub(1), 9_999] {
        let out = sim.t.searched(
            epoch,
            SearchOutcome::Moved(reply_with(&job, "e2e4", 1, Some(0))),
            sim.now,
        );
        assert!(out.is_empty(), "epoch {epoch} must be ignored");
    }
    assert_eq!(sim.t.live().unwrap().moves.len(), 0);
    let out = sim.t.searched(
        job.epoch,
        SearchOutcome::Moved(reply_with(&job, "f2f3", 1, Some(0))),
        sim.now,
    );
    assert!(out.iter().any(|a| matches!(a, Action::Search(_))));
    // The same epoch again: already answered.
    let again = sim.t.searched(
        job.epoch,
        SearchOutcome::Moved(reply_with(&job, "e2e4", 1, Some(0))),
        sim.now,
    );
    assert!(again.is_empty());
    assert_eq!(sim.t.live().unwrap().moves.len(), 1);
}

#[test]
fn a_prepared_answer_of_another_epoch_is_ignored() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.hold_prepare = true;
    sim.start();
    let Some(Action::Prepare { sides, epoch, .. }) = sim.held_prepare.last().cloned() else {
        panic!("prepare")
    };
    let ready = sides.map(|participant| mychess_app::tournament::SideReady {
        participant,
        engine_name: None,
        can_ponder: false,
    });
    assert!(
        sim.t
            .prepared(epoch + 3, Ok(ready.clone()), sim.now)
            .is_empty()
    );
    assert!(!sim.t.prepared(epoch, Ok(ready), sim.now).is_empty());
}

// ---- failures ----

#[test]
fn a_dead_engine_repeats_the_game_once_restarting_it_and_does_not_save_the_broken_try() {
    let calls = Rc::new(Cell::new(0));
    let c = Rc::clone(&calls);
    let mut sim = Sim::new(
        cfg(2),
        Box::new(move |job, ctx| {
            c.set(c.get() + 1);
            if c.get() == 1 {
                dead()
            } else {
                scripted_result(
                    job,
                    if ctx.white == 0 {
                        GameResult::White
                    } else {
                        GameResult::Black
                    },
                )
            }
        }),
    );
    sim.start();
    let started: Vec<u8> = sim
        .t
        .log()
        .iter()
        .filter_map(|e| match e {
            LogEvent::GameStarted { number: 1, attempt } => Some(*attempt),
            _ => None,
        })
        .collect();
    assert_eq!(started, [0, 1]);
    assert_eq!(sim.t.failures()[0], 1);
    // The second try restarts the engine that died.
    let second = &sim.prepares[1];
    assert!(second.1[0], "the dead engine restarts: {second:?}");
    // One game of the schedule saved for number 1: the broken try is not.
    assert_eq!(sim.games.iter().filter(|(n, _)| *n == 1).count(), 1);
    assert_eq!(sim.t.finished_games().len(), 1);
}

#[test]
fn an_engine_that_keeps_dying_loses_the_game_as_abandoned_after_the_retries() {
    let mut config = cfg(2);
    config.cycles = 2;
    config.failures.max_failures = 100;
    let mut sim = Sim::new(
        config,
        Box::new(|job, ctx| {
            if job.participant == 0 {
                dead()
            } else {
                scripted_result(job, GameResult::Draw)
            }
            .tap(ctx)
        }),
    );
    sim.start();
    // Game 1: P0 white dies twice -> loses; game 2: P0 black moves second:
    // P1 plays e.g. Nf3, then P0 dies.
    let first = &sim.t.slots()[0].status;
    let GameStatus::Played(o) = first else {
        panic!("{first:?}")
    };
    assert_eq!(
        (o.result, o.termination),
        (GameResult::Black, Termination::Abandoned)
    );
    assert_eq!(sim.t.failures()[0], 4);
}

trait Tap {
    fn tap(self, ctx: Ctx) -> Self;
}
impl Tap for SearchOutcome {
    fn tap(self, _: Ctx) -> Self {
        self
    }
}

#[test]
fn max_retries_zero_forfeits_at_the_first_failure() {
    let mut config = cfg(2);
    config.cycles = 2;
    config.failures.max_retries = 0;
    config.failures.max_failures = 100;
    let mut sim = Sim::new(
        config,
        Box::new(|job, _| {
            if job.participant == 1 {
                dead()
            } else {
                scripted_result(job, GameResult::Draw)
            }
        }),
    );
    sim.start();
    // P1 is black in game 1: it fails when it first moves.
    let GameStatus::Played(o) = &sim.t.slots()[0].status else {
        panic!("played")
    };
    assert_eq!(
        (o.result, o.termination),
        (GameResult::White, Termination::Abandoned)
    );
    assert_eq!(sim.games.len(), 2, "both games are saved");
}

#[test]
fn three_failures_pause_for_a_decision_and_withdrawing_drops_the_games_left() {
    let mut config = cfg(2);
    config.cycles = 5;
    config.failures.max_retries = 0;
    config.failures.max_failures = 3;
    let mut sim = Sim::new(
        config,
        Box::new(|job, _| {
            if job.participant == 0 {
                dead()
            } else {
                scripted_result(job, GameResult::Draw)
            }
        }),
    );
    sim.start();
    assert_eq!(sim.t.phase(), Phase::Decision(0));
    assert_eq!(sim.t.failures()[0], 3);
    assert_eq!(sim.games.len(), 3);
    assert!(
        sim.notices
            .iter()
            .any(|n| matches!(n, TournamentNotice::TooManyFailures { participant: 0 }))
    );
    // Nothing moves while the question is open.
    let before = sim.searches.len();
    let actions = sim.t.pause();
    sim.run(actions);
    assert_eq!(sim.searches.len(), before);

    let actions = sim.t.decide(Decision::Withdraw, sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.t.withdrawn(), [true, false]);
    let withdrawn = sim
        .t
        .slots()
        .iter()
        .filter(|s| s.status == GameStatus::Withdrawn)
        .count();
    assert_eq!(withdrawn, 2);
    let row = sim
        .t
        .standings()
        .into_iter()
        .find(|r| r.participant == 0)
        .unwrap();
    assert!(row.withdrawn);
    // The games left are neither won nor lost: only the 3 played count.
    assert_eq!(sim.t.progress().0, 3);
    assert!(sim.discarded.contains(&0));
}

#[test]
fn deciding_to_retry_clears_the_count_and_goes_on_and_replace_changes_the_participant() {
    let mk = || {
        let mut config = cfg(2);
        config.cycles = 5;
        config.failures.max_retries = 0;
        config.failures.max_failures = 3;
        let mut sim = Sim::new(
            config,
            Box::new(|job, _| {
                if job.participant == 0 {
                    dead()
                } else {
                    scripted_result(job, GameResult::Draw)
                }
            }),
        );
        sim.start();
        sim
    };
    let mut sim = mk();
    let actions = sim.t.decide(Decision::Retry, sim.now);
    sim.run(actions);
    // The count restarted from zero: 2 more games fail, ending the schedule.
    assert_eq!(sim.t.failures()[0], 2);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.games.len(), 5);

    let mut sim = mk();
    let mut new = Participant::new(77, "Newcomer");
    new.elo = Some(2000);
    sim.policy = Box::new(|job, _| scripted_result(job, GameResult::Draw));
    let actions = sim.t.decide(Decision::Replace(Box::new(new)), sim.now);
    sim.run(actions);
    assert_eq!(sim.t.config().participants[0].name, "Newcomer");
    assert!(sim.discarded.contains(&0));
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.t.failures()[0], 0);
}

#[test]
fn a_decision_without_a_question_changes_nothing() {
    let mut sim = Sim::new(cfg(2), low_wins());
    let actions = sim.t.decide(Decision::Withdraw, sim.now);
    assert!(matches!(
        actions.as_slice(),
        [Action::Notice(TournamentNotice::NotApplicable)]
    ));
    assert_eq!(sim.t.withdrawn(), [false, false]);
}

#[test]
fn an_engine_that_cannot_start_counts_as_a_failure_and_is_retried() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.hold_prepare = true;
    sim.start();
    sim.hold_prepare = false;
    sim.answer_prepare(Err(prepare_failure(0)));
    assert_eq!(sim.t.failures()[0], 1);
    assert!(sim.t.log().iter().any(|e| matches!(
        e,
        LogEvent::EngineFailed {
            participant: 0,
            kind: FailKind::StartFailed
        }
    )));
    assert_eq!(sim.t.phase(), Phase::Finished, "the retry went through");
}

fn clock_config() -> TournamentConfig {
    let mut c = cfg(2);
    c.time = TimeControl::Clock {
        base_ms: 10_000,
        increment_ms: 0,
    };
    c
}

#[test]
fn no_answer_with_a_clock_is_a_loss_on_time_by_default() {
    let mut sim = Sim::new(
        clock_config(),
        Box::new(|job, _| {
            if job.participant == 0 && job.think.moves.len() == 2 {
                SearchOutcome::NoAnswer { killed: true }
            } else {
                scripted_result(job, GameResult::Draw)
            }
        }),
    );
    sim.start();
    let GameStatus::Played(o) = &sim.t.slots()[0].status else {
        panic!("played")
    };
    assert_eq!(
        (o.result, o.termination),
        (GameResult::Black, Termination::TimeForfeit)
    );
    assert_eq!(
        sim.t
            .log()
            .iter()
            .filter(|e| matches!(e, LogEvent::GameStarted { number: 1, .. }))
            .count(),
        1
    );
}

#[test]
fn time_loss_with_replay_consumes_the_retries_and_then_the_loss_stands() {
    let mut config = clock_config();
    config.failures.on_time_loss = OnTimeLoss::Replay;
    config.failures.max_retries = 1;
    let mut sim = Sim::new(
        config,
        Box::new(|job, _| {
            if job.participant == 0 && job.think.moves.is_empty() {
                SearchOutcome::NoAnswer { killed: false }
            } else {
                scripted_result(job, GameResult::Draw)
            }
        }),
    );
    sim.start();
    let attempts: Vec<u8> = sim
        .t
        .log()
        .iter()
        .filter_map(|e| match e {
            LogEvent::GameStarted { number: 1, attempt } => Some(*attempt),
            _ => None,
        })
        .collect();
    assert_eq!(attempts, [0, 1]);
    let GameStatus::Played(o) = &sim.t.slots()[0].status else {
        panic!("played")
    };
    assert_eq!(o.termination, Termination::TimeForfeit);
    assert_eq!(o.result, GameResult::Black);
}

#[test]
fn no_answer_without_a_clock_is_an_engine_failure_not_a_time_loss() {
    let mut config = cfg(2);
    config.failures.max_retries = 0;
    let mut sim = Sim::new(
        config,
        Box::new(|job, _| {
            if job.participant == 0 {
                SearchOutcome::NoAnswer { killed: true }
            } else {
                scripted_result(job, GameResult::Draw)
            }
        }),
    );
    sim.start();
    let GameStatus::Played(o) = &sim.t.slots()[0].status else {
        panic!("played")
    };
    assert_eq!(o.termination, Termination::Abandoned);
}

#[test]
fn a_killed_engine_restarts_for_the_next_game_and_a_healthy_one_does_not() {
    let mut config = clock_config();
    config.cycles = 2;
    let done = Rc::new(Cell::new(false));
    let d = Rc::clone(&done);
    let mut sim = Sim::new(
        config,
        Box::new(move |job, _| {
            if job.participant == 0 && !d.get() {
                d.set(true);
                SearchOutcome::NoAnswer { killed: true }
            } else {
                scripted_result(job, GameResult::Draw)
            }
        }),
    );
    sim.start();
    // Game 1: P0 (White) was killed. Game 2 has P0 as Black: only it restarts.
    assert_eq!(sim.prepares[0].1, [false, false]);
    assert_eq!(sim.prepares[1].0, [1, 0]);
    assert_eq!(sim.prepares[1].1, [false, true]);
}

// ---- cancellation (D1308) ----

#[test]
fn abort_during_a_search_cancels_it_and_leaves_the_game_pending_at_the_front() {
    let mut sim = Sim::new(cfg(3), low_wins());
    sim.hold = true;
    sim.start();
    let epoch = sim.t.epoch();
    let actions = sim.t.abort(sim.now);
    assert!(matches!(actions.as_slice(), [Action::Cancel { epoch: e, .. }] if *e == epoch));
    sim.run(actions);
    // The task answers `Cancelled`.
    sim.hold = false;
    sim.answer_held(SearchOutcome::Cancelled);
    assert_eq!(sim.t.phase(), Phase::Aborted);
    assert!(sim.games.is_empty(), "the interrupted game is not saved");
    assert_eq!(sim.t.queue_numbers(), [1, 2, 3]);
    assert_eq!(sim.shutdowns, 1);
    assert!(sim.states.last().is_some());
    assert!(!sim.t.to_saved().finished);
}

#[test]
fn abort_while_the_engines_start_cancels_the_start() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.hold_prepare = true;
    sim.start();
    let epoch = sim.t.epoch();
    let actions = sim.t.abort(sim.now);
    assert!(matches!(actions.as_slice(), [Action::Cancel { epoch: e, .. }] if *e == epoch));
    sim.answer_prepare(Err(prepare_failure(0)));
    assert_eq!(sim.t.phase(), Phase::Aborted);
    assert_eq!(
        sim.t.failures(),
        [0, 0],
        "a cancelled start is not a failure"
    );
}

#[test]
fn skipping_while_the_engines_start_cancels_too_and_goes_to_the_next_game() {
    let mut sim = Sim::new(cfg(3), low_wins());
    sim.hold_prepare = true;
    sim.start();
    let epoch = sim.t.epoch();
    let actions = sim.t.skip_current(sim.now);
    assert!(matches!(actions.as_slice(), [Action::Cancel { epoch: e, .. }] if *e == epoch));
    sim.hold_prepare = false;
    sim.answer_prepare(Err(prepare_failure(0)));
    assert_eq!(sim.t.slots()[0].status, GameStatus::Skipped);
    assert_eq!(sim.t.failures(), [0, 0, 0]);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.games.len(), 2);
}

#[test]
fn adjudicating_before_the_game_exists_is_refused_without_harm() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.hold_prepare = true;
    sim.start();
    let actions = sim.t.adjudicate(GameResult::White, sim.now);
    assert!(matches!(
        actions.as_slice(),
        [Action::Notice(TournamentNotice::NotApplicable)]
    ));
    assert_eq!(sim.t.phase(), Phase::Running);
}

#[test]
fn manual_adjudication_ends_the_game_after_the_search_comes_back() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.hold = true;
    sim.start();
    let actions = sim.t.adjudicate(GameResult::Black, sim.now);
    assert!(matches!(actions.as_slice(), [Action::Cancel { .. }]));
    sim.hold = false;
    sim.answer_held(SearchOutcome::Cancelled);
    let GameStatus::Played(o) = &sim.t.slots()[0].status else {
        panic!("played")
    };
    assert_eq!(
        (o.result, o.termination),
        (GameResult::Black, Termination::Adjudication)
    );
    let pgn = &sim.pgn_texts()[0];
    assert!(pgn.contains("Manual adjudication"), "{pgn}");
    assert!(pgn.contains("[Termination \"adjudication\"]"), "{pgn}");
    assert!(pgn.contains("[Result \"0-1\"]"));
}

#[test]
fn adjudicating_with_an_unknown_result_is_refused() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.hold = true;
    sim.start();
    let a = sim.t.adjudicate(GameResult::Unknown, sim.now);
    assert!(matches!(
        a.as_slice(),
        [Action::Notice(TournamentNotice::NotApplicable)]
    ));
}

// ---- pause, stop ----

#[test]
fn pause_waits_for_the_move_and_resume_asks_again_from_the_same_position() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.hold = true;
    sim.start();
    let actions = sim.t.pause();
    assert!(actions.is_empty());
    assert_eq!(sim.t.phase(), Phase::Pausing);
    sim.hold = false;
    let before = sim.searches.len();
    sim.answer_held(SearchOutcome::Moved(reply_with(
        &sim.searches[0].clone(),
        "e2e4",
        5,
        Some(0),
    )));
    assert_eq!(sim.t.phase(), Phase::Paused);
    assert_eq!(sim.searches.len(), before, "no new search while paused");
    assert!(sim.stop_ponders >= 1);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.searches[before].think.moves.len(), 1);
}

#[test]
fn resuming_while_still_pausing_cancels_the_pause() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.hold = true;
    sim.start();
    sim.t.pause();
    assert!(sim.t.resume_play(sim.now).is_empty());
    assert_eq!(sim.t.phase(), Phase::Running);
    sim.hold = false;
    sim.answer_held(SearchOutcome::Moved(reply_with(
        &sim.searches[0].clone(),
        "e2e4",
        5,
        Some(0),
    )));
    assert_ne!(sim.t.phase(), Phase::Paused);
}

#[test]
fn a_game_decided_by_the_last_move_is_saved_even_if_a_pause_was_asked() {
    // D1314.
    let mut sim = Sim::new(
        cfg(2),
        Box::new(|job, _| scripted_result(job, GameResult::Black)),
    );
    sim.hold = true;
    sim.start();
    for text in ["f2f3", "e7e5", "g2g4"] {
        let job = sim.searches.last().unwrap().clone();
        sim.hold = true;
        sim.held.clear();
        let actions = sim.t.searched(
            job.epoch,
            SearchOutcome::Moved(reply_with(&job, text, 1, Some(0))),
            sim.now,
        );
        sim.run(actions);
    }
    sim.t.pause();
    assert_eq!(sim.t.phase(), Phase::Pausing);
    let job = sim.searches.last().unwrap().clone();
    let actions = sim.t.searched(
        job.epoch,
        SearchOutcome::Moved(reply_with(&job, "d8h4", 1, Some(0))),
        sim.now,
    );
    sim.run(actions);
    assert_eq!(sim.games.len(), 1, "the mated game is not left frozen");
    assert_eq!(sim.t.phase(), Phase::Paused);
}

#[test]
fn stop_after_this_game_finishes_it_saves_closes_the_engines_and_can_continue() {
    let mut sim = Sim::new(cfg(3), low_wins());
    sim.hold = true;
    sim.start();
    assert!(sim.t.stop_after_game(sim.now).is_empty());
    sim.hold = false;
    let job = sim.searches[0].clone();
    let actions = sim
        .t
        .searched(job.epoch, scripted_result(&job, GameResult::White), sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::StoppedAfterGame);
    assert_eq!(sim.games.len(), 1);
    assert!(sim.shutdowns >= 1);
    assert_eq!(sim.t.queue_numbers(), [2, 3]);
    assert!(!sim.states.last().unwrap().finished);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.games.len(), 3);
}

#[test]
fn controls_are_refused_after_the_end() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.start();
    assert_eq!(sim.t.phase(), Phase::Finished);
    for actions in [
        sim.t.abort(sim.now),
        sim.t.stop_after_game(sim.now),
        sim.t.pause(),
        sim.t.skip_current(sim.now),
        sim.t.skip_pending(1),
        sim.t.move_to_front(1),
    ] {
        assert!(
            matches!(
                actions.as_slice(),
                [Action::Notice(TournamentNotice::NotApplicable)]
            ),
            "{actions:?}"
        );
    }
}

// ---- queue edition (D1286) ----

#[test]
fn skip_move_and_redo_edit_the_queue() {
    let mut sim = Sim::new(cfg(3), low_wins());
    assert_eq!(sim.t.queue_numbers(), [1, 2, 3]);
    sim.t.move_to_front(3);
    assert_eq!(sim.t.queue_numbers(), [3, 1, 2]);
    sim.t.skip_pending(2);
    assert_eq!(sim.t.queue_numbers(), [3, 1]);
    assert_eq!(sim.t.slots()[1].status, GameStatus::Skipped);
    sim.start();
    let order: Vec<u32> = sim.games.iter().map(|(n, _)| *n).collect();
    assert_eq!(order, [3, 1]);
    assert_eq!(sim.t.progress(), (2, 2), "skipped games do not count");
    // Unknown or non-pending numbers are refused.
    assert!(matches!(
        sim.t.skip_pending(2).as_slice(),
        [Action::Notice(TournamentNotice::NotApplicable)]
    ));
}

#[test]
fn a_redone_game_replaces_the_old_result_and_says_so_in_its_round_tag() {
    let flip = Rc::new(Cell::new(false));
    let f = Rc::clone(&flip);
    let mut sim = Sim::new(
        cfg(2),
        Box::new(move |job, ctx| {
            let white_wins = !f.get();
            let result = if ctx.number == 1 && white_wins {
                GameResult::White
            } else if ctx.number == 1 {
                GameResult::Black
            } else {
                GameResult::Draw
            };
            scripted_result(job, result)
        }),
    );
    sim.start();
    assert_eq!(sim.results()[0], GameResult::White);
    let before = sim.games.len();
    flip.set(true);
    let actions = sim.t.redo(1);
    sim.run(actions);
    assert_eq!(
        sim.t.phase(),
        Phase::Paused,
        "a finished tournament waits for go on"
    );
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.games.len(), before + 1);
    assert_eq!(sim.games.last().unwrap().1.tags().round, "1.1 (redo 1)");
    // Game 1 counts once, with the new result.
    let finished = sim.t.finished_games();
    assert_eq!(finished.iter().filter(|g| g.number == 1).count(), 1);
    assert_eq!(
        finished.iter().find(|g| g.number == 1).unwrap().result,
        GameResult::Black
    );
    assert!(matches!(sim.t.slots()[0].status, GameStatus::Replaced(_)));
    // A second redo counts as redo 2.
    let actions = sim.t.redo(1);
    sim.run(actions);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(sim.games.last().unwrap().1.tags().round, "1.1 (redo 2)");
}

#[test]
fn redo_of_a_game_that_was_not_played_is_refused() {
    let mut sim = Sim::new(cfg(2), low_wins());
    assert!(matches!(
        sim.t.redo(1).as_slice(),
        [Action::Notice(TournamentNotice::NotApplicable)]
    ));
}

// ---- ponder ----

/// Plays `g1f3 g8f6 f3g1 f6g8 ...`, each reply announcing the next move as
/// the move to ponder on.
fn ponder_policy() -> Policy {
    Box::new(|job, _| {
        let ply = job.think.moves.len();
        let text = SHUFFLE[ply % 8];
        let position = position_of(job);
        let mut r = reply_with(job, text, 1, Some(0));
        let after = position.after(r.mv.unwrap()).unwrap();
        r.ponder = after.parse_uci(SHUFFLE[(ply + 1) % 8]).ok();
        SearchOutcome::Moved(r)
    })
}

#[test]
fn ponder_job_is_sent_only_with_the_option_on_and_a_capable_engine() {
    for (option, capable, expect) in [
        (true, true, true),
        (false, true, false),
        (true, false, false),
    ] {
        let mut config = cfg(2);
        config.ponder = option;
        config.claim_draws = false;
        config.adjudication.max_plies = Some(4);
        let mut sim = Sim::new(config, ponder_policy());
        sim.can_ponder = capable;
        sim.start();
        let any = sim.searches.iter().any(|s| s.ponder.is_some());
        assert_eq!(any, expect, "option={option} capable={capable}");
    }
}

#[test]
fn the_ponder_job_belongs_to_the_engine_that_just_moved_and_expects_the_scripted_reply() {
    let mut config = cfg(2);
    config.ponder = true;
    config.claim_draws = false;
    config.adjudication.max_plies = Some(4);
    let mut sim = Sim::new(config, ponder_policy());
    sim.can_ponder = true;
    sim.start();
    assert!(sim.searches[0].ponder.is_none(), "nobody moved yet");
    let second = &sim.searches[1];
    let job = second.ponder.as_ref().expect("ponder");
    assert_eq!(
        job.participant, 0,
        "the white engine ponders while black thinks"
    );
    assert_eq!(second.participant, 1);
    // The expected move is the one in the ponder position.
    assert_eq!(job.think.moves.len(), 2);
    assert_eq!(job.think.side, Color::White);
}

#[test]
fn a_pause_forgets_the_move_the_engines_were_pondering_on() {
    // D1295 / D1314.
    let mut config = cfg(2);
    config.ponder = true;
    config.claim_draws = false;
    let mut sim = Sim::new(config, ponder_policy());
    sim.can_ponder = true;
    sim.hold = true;
    sim.start();
    sim.hold = false;
    // Answer ply 1 with ponder; then ask for a pause while ply 2 is in
    // flight.
    let job = sim.searches[0].clone();
    sim.hold = true;
    let actions = sim.t.searched(
        job.epoch,
        ponder_policy()(
            &job,
            Ctx {
                number: 1,
                white: 0,
                black: 1,
            },
        ),
        sim.now,
    );
    sim.run(actions);
    let second = sim.searches.last().unwrap().clone();
    assert!(second.ponder.is_some());
    sim.t.pause();
    let actions = sim.t.searched(
        second.epoch,
        ponder_policy()(
            &second,
            Ctx {
                number: 1,
                white: 0,
                black: 1,
            },
        ),
        sim.now,
    );
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Paused);
    assert!(sim.stop_ponders >= 1);
    sim.hold = true;
    let n = sim.searches.len();
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(sim.searches.len(), n + 1);
    assert!(
        sim.searches[n].ponder.is_none(),
        "the old guess is forgotten"
    );
}

// ---- resume ----

#[test]
fn a_saved_state_round_trips_and_resumes_paused_with_the_interrupted_game_first() {
    let mut sim = Sim::new(cfg(3), low_wins());
    sim.hold = true;
    sim.start();
    // Game 1 is in flight: the state saved now has it pending, first.
    let saved = sim.t.to_saved();
    let text = render_state(&saved).expect("render");
    let back = parse_state(&text).expect("parse");
    assert_eq!(back.queue.first(), Some(&0));
    let resumed = Tournament::resume(back, None, None).expect("resume");
    assert_eq!(resumed.phase(), Phase::Paused);
    assert_eq!(resumed.queue_numbers(), [1, 2, 3]);
    let mut sim2 = Sim::with(resumed, low_wins());
    let actions = sim2.t.resume_play(sim2.now);
    sim2.run(actions);
    assert_eq!(sim2.t.phase(), Phase::Finished);
    assert_eq!(sim2.games.len(), 3, "the interrupted game was replayed");
}

#[test]
fn resuming_after_two_games_plays_only_the_rest() {
    let mut sim = Sim::new(cfg(3), low_wins());
    sim.start();
    let mut state = sim.t.to_saved();
    // Make it look like the app was killed after game 2.
    for g in state.games.iter_mut().skip(2) {
        g.status = mychess_app::tournament::SavedStatus::Pending;
        g.result = None;
        g.termination = None;
        g.plies = 0;
    }
    state.finished = false;
    state.queue = vec![2];
    let resumed = Tournament::resume(
        parse_state(&render_state(&state).unwrap()).unwrap(),
        None,
        None,
    )
    .unwrap();
    assert_eq!(resumed.finished_games().len(), 2);
    let mut sim2 = Sim::with(resumed, low_wins());
    let actions = sim2.t.resume_play(sim2.now);
    sim2.run(actions);
    assert_eq!(sim2.games.len(), 1);
    assert_eq!(sim2.t.finished_games().len(), 3);
    assert_eq!(sim2.t.phase(), Phase::Finished);
}

#[test]
fn a_finished_tournament_resumes_as_finished() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.start();
    let resumed = Tournament::resume(sim.t.to_saved(), None, None).unwrap();
    assert_eq!(resumed.phase(), Phase::Finished);
    assert_eq!(resumed.winners(), sim.t.winners());
}

#[test]
fn all_engines_restart_after_a_resume() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.hold = true;
    sim.start();
    let resumed = Tournament::resume(sim.t.to_saved(), None, None).unwrap();
    let mut sim2 = Sim::with(resumed, low_wins());
    let actions = sim2.t.resume_play(sim2.now);
    sim2.run(actions);
    assert_eq!(sim2.prepares[0].1, [true, true]);
}

#[test]
fn live_view_follows_the_game_in_progress() {
    let mut sim = Sim::new(cfg(2), low_wins());
    sim.hold = true;
    sim.start();
    let live = sim.t.live().expect("live");
    assert_eq!(
        (live.number, live.white, live.black, live.attempt),
        (1, 0, 1, 0)
    );
    assert_eq!(live.moves.len(), 0);
    let job = sim.searches[0].clone();
    sim.hold = true;
    let actions = sim.t.searched(
        job.epoch,
        SearchOutcome::Moved(reply_with(&job, "e2e4", 1, Some(10))),
        sim.now,
    );
    sim.run(actions);
    assert_eq!(sim.t.live().unwrap().moves.len(), 1);
    assert!(sim.t.live().unwrap().position.is_some());
}

#[test]
fn invalid_configuration_is_refused_by_new() {
    let mut config = cfg(2);
    config.cycles = 0;
    assert!(Tournament::new(config, "t", today(), None, None).is_err());
    assert!(Tournament::new(cfg(1), "t", today(), None, None).is_err());
}

#[test]
fn only_an_engine_reply_with_a_score_feeds_the_resign_rule_in_a_whole_tournament() {
    // 2 players, resign rule 600/2: P0 always reports +800 for itself, P1
    // always -800 for itself. Every game ends by adjudication in 4 plies.
    let mut config = cfg(2);
    config.cycles = 2;
    config.adjudication.resign = Some(mychess_app::tournament::ResignRule {
        centipawns: 600,
        moves: 2,
        require_both: true,
    });
    let mut sim = Sim::new(
        config,
        Box::new(|job, ctx| {
            let position = position_of(job);
            let ply = job.think.moves.len();
            let text = ["e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6"][ply];
            let mover_cp = if job.participant == 0 { 800 } else { -800 };
            let white_cp = if job.think.side == Color::White {
                mover_cp
            } else {
                -mover_cp
            };
            let mut r = reply_with(job, text, 1, Some(white_cp));
            r.score = Some(Score::Cp(white_cp));
            let _ = (position, ctx);
            SearchOutcome::Moved(r)
        }),
    );
    sim.start();
    assert_eq!(sim.results(), [GameResult::White, GameResult::Black]);
    assert!(
        sim.pgn_texts()
            .iter()
            .all(|t| t.contains("Adjudicated: resign +8.00"))
    );
    let _ = Duration::ZERO;
    let _ = Reply {
        mv: None,
        spent: Duration::ZERO,
        score: None,
        depth: None,
        ponder: None,
    };
}
