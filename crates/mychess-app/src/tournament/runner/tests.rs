//! Tests of the tournament state machine, driven by a simulator that
//! answers the actions the way the tasks of the application would.

use mychess_book::Book;
use mychess_core::Color;
use mychess_engine::Score as EngineScore;

use super::*;
use crate::engines::{EngineFailure, FailureKind};
use crate::play::BookSelection;
use crate::tournament::adjudicate::signed_pawns;
use crate::tournament::config::{
    Adjudication, BookConfig, BookPick, FailurePolicy, OnTimeLoss, ResignRule,
};
use crate::tournament::game::Reply;
use crate::tournament::store::{parse_state, render_state};

type Policy = Box<dyn FnMut(&SearchJob) -> SearchOutcome>;

/// Answers every action like the application would.
struct Sim {
    t: Tournament,
    now: Instant,
    games: Vec<Game>,
    states: Vec<SavedTournament>,
    notices: Vec<TournamentNotice>,
    prepares: Vec<([usize; 2], [bool; 2])>,
    searches: Vec<SearchJob>,
    cancels: Vec<u64>,
    shutdowns: usize,
    discarded: Vec<usize>,
    stop_ponders: usize,
    policy: Policy,
    can_ponder: bool,
    /// Searches are not answered (the engine is thinking).
    hold: bool,
    /// Searches are held once this many games were saved.
    hold_after: Option<usize>,
    held: Vec<Action>,
}

fn names(n: usize) -> Vec<Participant> {
    (0..n)
        .map(|i| Participant::new(i as u64 + 1, &format!("P{i}")))
        .collect()
}

fn base_config(n: usize) -> TournamentConfig {
    TournamentConfig {
        participants: names(n),
        cycles: 1,
        time: TimeControl::Depth(5),
        // Participant 0 always reports +8.00 and the others -8.00: the
        // resignation rule ends every game it plays in after 4 moves each.
        adjudication: Adjudication {
            resign: Some(ResignRule {
                centipawns: 600,
                moves: 4,
                require_both: true,
            }),
            ..Adjudication::default()
        },
        ..TournamentConfig::default()
    }
}

/// The first legal move by UCI text in the position of the search.
fn legal_reply(job: &SearchJob, strong: usize) -> Reply {
    let mut position = job.think.start.clone();
    for &mv in &job.think.moves {
        position.play(mv).expect("legal history");
    }
    let mut moves: Vec<_> = position
        .legal_moves()
        .into_iter()
        .map(|m| (position.uci(m), m))
        .collect();
    moves.sort_by(|a, b| a.0.cmp(&b.0));
    // The engine layer reports from the point of view of White (D1302).
    let mover_cp = if job.participant == strong { 800 } else { -800 };
    let cp = if job.think.side == Color::White {
        mover_cp
    } else {
        -mover_cp
    };
    Reply {
        mv: moves.first().map(|(_, m)| *m),
        spent: Duration::from_millis(100),
        score: Some(EngineScore::Cp(cp)),
        depth: Some(10),
        ponder: None,
    }
}

impl Sim {
    fn new(config: TournamentConfig) -> Sim {
        Sim::with(
            Tournament::new(config, "t", PartialDate::default(), None, None).expect("tournament"),
        )
    }

    fn with(t: Tournament) -> Sim {
        Sim {
            t,
            now: Instant::now(),
            games: Vec::new(),
            states: Vec::new(),
            notices: Vec::new(),
            prepares: Vec::new(),
            searches: Vec::new(),
            cancels: Vec::new(),
            shutdowns: 0,
            discarded: Vec::new(),
            stop_ponders: 0,
            policy: Box::new(|job| SearchOutcome::Moved(legal_reply(job, 0))),
            can_ponder: false,
            hold: false,
            hold_after: None,
            held: Vec::new(),
        }
    }

    fn ready(&self, sides: [usize; 2]) -> [SideReady; 2] {
        sides.map(|participant| SideReady {
            participant,
            engine_name: Some(format!("Engine {participant}")),
            can_ponder: self.can_ponder,
        })
    }

    /// Processes the actions and everything they cause.
    fn run(&mut self, first: Vec<Action>) {
        let mut queue: VecDeque<Action> = first.into();
        while let Some(action) = queue.pop_front() {
            let more = match action {
                Action::Prepare {
                    sides,
                    restart,
                    epoch,
                    ..
                } => {
                    self.prepares.push((sides, restart));
                    let ready = self.ready(sides);
                    self.t.prepared(epoch, Ok(ready), self.now)
                }
                Action::Search(job) => {
                    self.searches.push((*job).clone());
                    if self.hold || self.hold_after.is_some_and(|n| self.games.len() >= n) {
                        self.held.push(Action::Search(job));
                        Vec::new()
                    } else {
                        let outcome = (self.policy)(&job);
                        self.t.searched(job.epoch, outcome, self.now)
                    }
                }
                Action::Cancel { epoch, .. } => {
                    self.cancels.push(epoch);
                    Vec::new()
                }
                Action::StopPonder { .. } => {
                    self.stop_ponders += 1;
                    Vec::new()
                }
                Action::GameOver { .. } => Vec::new(),
                Action::SaveGame { game, .. } => {
                    self.games.push(*game);
                    Vec::new()
                }
                Action::SaveState(state) => {
                    self.states.push(*state);
                    Vec::new()
                }
                Action::Discard(p) => {
                    self.discarded.push(p);
                    Vec::new()
                }
                Action::Shutdown => {
                    self.shutdowns += 1;
                    Vec::new()
                }
                Action::Notice(n) => {
                    self.notices.push(n);
                    Vec::new()
                }
            };
            queue.extend(more);
        }
    }

    fn start(&mut self) {
        let actions = self.t.begin(self.now);
        self.run(actions);
    }

    /// Answers the search that was held.
    fn answer_held(&mut self, outcome: SearchOutcome) {
        let held = self.held.pop().expect("a held search");
        let Action::Search(job) = held else {
            panic!("not a search")
        };
        let actions = self.t.searched(job.epoch, outcome, self.now);
        self.run(actions);
    }
}

fn results(sim: &Sim) -> Vec<GameResult> {
    sim.games.iter().map(|g| g.tags().result).collect()
}

fn dead() -> SearchOutcome {
    SearchOutcome::Dead(EngineFailure {
        kind: FailureKind::Crashed,
        exit_code: None,
        stderr_tail: Vec::new(),
    })
}

#[test]
fn a_round_robin_runs_to_the_end_and_ranks_the_winner_first() {
    let mut sim = Sim::new(base_config(3));
    sim.start();
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.games.len(), 3);
    assert_eq!(sim.t.progress(), (3, 3));
    let rows = sim.t.standings();
    assert_eq!(rows[0].participant, 0);
    assert_eq!(rows[0].half_points, 4);
    assert_eq!(sim.t.winners(), [0]);
    assert!(sim.shutdowns >= 1);
    assert!(sim.notices.contains(&TournamentNotice::Started));
    assert!(
        sim.notices
            .contains(&TournamentNotice::Finished { winners: vec![0] })
    );
    // The state is saved at the start and after every game.
    assert!(sim.states.len() >= 4, "{}", sim.states.len());
    assert!(sim.states.last().expect("state").finished);
    // The games carry the tags of the spec.
    let first = &sim.games[0];
    assert_eq!(first.tags().event, "MyChess Tournament");
    assert_eq!(first.tags().round, "1.1");
    assert_eq!(first.tags().get_extra("Termination"), Some("adjudication"));
    assert_eq!(first.tags().get_extra("WhiteEngine"), Some("Engine 0"));
    assert!(matches!(sim.t.log().last(), Some(LogEvent::Finished)));
}

#[test]
fn the_adjudication_comment_says_why() {
    let mut sim = Sim::new(base_config(2));
    sim.start();
    let game = &sim.games[0];
    let last = game.mainline().last().expect("moves");
    let comment = game.comment(last).expect("node").expect("comment");
    assert!(
        comment.ends_with(&format!("Adjudicated: resign {}", signed_pawns(800))),
        "{comment}"
    );
}

#[test]
fn engines_restart_only_when_asked_or_after_a_failure() {
    let mut config = base_config(2);
    config.cycles = 2;
    config.restart_each_game = true;
    let mut sim = Sim::new(config);
    sim.start();
    assert!(
        sim.prepares
            .iter()
            .all(|(_, restart)| *restart == [true, true])
    );
    let mut sim = Sim::new(base_config(2));
    sim.start();
    assert_eq!(sim.prepares[0].1, [false, false]);
}

#[test]
fn a_search_in_flight_is_answered_once_and_stale_answers_are_ignored() {
    let mut sim = Sim::new(base_config(2));
    sim.hold = true;
    sim.start();
    let Some(Action::Search(job)) = sim.held.pop() else {
        panic!("a search is held");
    };
    // An answer of another epoch changes nothing.
    assert!(sim.t.searched(job.epoch + 7, dead(), sim.now).is_empty());
    assert!(
        sim.t
            .prepared(
                job.epoch,
                Err(PrepareFailure {
                    participant: 0,
                    failure: EngineFailure {
                        kind: FailureKind::Crashed,
                        exit_code: None,
                        stderr_tail: vec![]
                    },
                }),
                sim.now
            )
            .is_empty()
    );
    sim.hold = false;
    let outcome = SearchOutcome::Moved(legal_reply(&job, 0));
    let actions = sim.t.searched(job.epoch, outcome.clone(), sim.now);
    assert!(!actions.is_empty());
    sim.run(actions);
    // The same answer again is stale.
    assert!(sim.t.searched(job.epoch, outcome, sim.now).is_empty());
}

#[test]
fn pause_waits_for_the_move_and_resume_asks_again() {
    let mut sim = Sim::new(base_config(2));
    sim.hold = true;
    sim.start();
    let actions = sim.t.pause();
    assert!(actions.is_empty());
    assert_eq!(sim.t.phase(), Phase::Pausing);
    sim.answer_held(SearchOutcome::Moved(legal_reply(
        &sim.searches[0].clone(),
        0,
    )));
    assert_eq!(sim.t.phase(), Phase::Paused);
    assert_eq!(sim.stop_ponders, 1);
    assert_eq!(sim.searches.len(), 1, "nothing is asked while paused");
    sim.hold = false;
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.games.len(), 1);
    // The moves of the first search were kept: the game was not restarted.
    assert_eq!(sim.prepares.len(), 1);
}

#[test]
fn pause_between_games_waits_for_the_next_resume() {
    let mut sim = Sim::new(base_config(3));
    sim.hold = true;
    sim.start();
    sim.t.pause();
    sim.hold = false;
    let held = sim.held.pop().expect("held");
    let Action::Search(job) = held else {
        panic!("search")
    };
    let actions = sim.t.searched(
        job.epoch,
        SearchOutcome::Moved(legal_reply(&job, 0)),
        sim.now,
    );
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Paused);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.games.len(), 3);
}

#[test]
fn stop_after_this_game_saves_and_can_be_resumed() {
    let mut sim = Sim::new(base_config(3));
    sim.hold = true;
    sim.start();
    assert!(sim.t.stop_after_game(sim.now).is_empty());
    sim.hold = false;
    let Action::Search(job) = sim.held.pop().expect("held") else {
        panic!("search")
    };
    let actions = sim.t.searched(
        job.epoch,
        SearchOutcome::Moved(legal_reply(&job, 0)),
        sim.now,
    );
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::StoppedAfterGame);
    assert_eq!(
        sim.games.len(),
        1,
        "the game in progress was played to the end"
    );
    assert!(sim.notices.contains(&TournamentNotice::StoppedAfterGame));
    assert_eq!(sim.shutdowns, 1);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.games.len(), 3);
}

#[test]
fn abort_cancels_the_search_and_leaves_the_game_unplayed() {
    let mut sim = Sim::new(base_config(3));
    sim.hold = true;
    sim.start();
    let actions = sim.t.abort(sim.now);
    sim.run(actions);
    assert_eq!(sim.cancels.len(), 1);
    assert_eq!(
        sim.t.phase(),
        Phase::Running,
        "waits for the engine to stop"
    );
    sim.answer_held(SearchOutcome::Cancelled);
    assert_eq!(sim.t.phase(), Phase::Aborted);
    assert!(sim.games.is_empty(), "the game in progress is not saved");
    assert!(sim.notices.contains(&TournamentNotice::Aborted));
    let state = sim.states.last().expect("state");
    assert_eq!(state.queue.first(), Some(&0), "the game comes back first");
    assert!(state.games.iter().all(|g| g.status == SavedStatus::Pending));
    assert_eq!(sim.shutdowns, 1);
    // Aborting twice is not applicable.
    assert!(matches!(
        sim.t.abort(sim.now).first(),
        Some(Action::Notice(TournamentNotice::NotApplicable))
    ));
}

#[test]
fn manual_adjudication_ends_the_game_with_the_chosen_result() {
    let mut config = base_config(2);
    config.adjudication = Adjudication::default();
    let mut sim = Sim::new(config);
    sim.hold = true;
    sim.start();
    let actions = sim.t.adjudicate(GameResult::Black, sim.now);
    sim.run(actions);
    assert_eq!(sim.cancels.len(), 1);
    sim.answer_held(SearchOutcome::Cancelled);
    assert_eq!(results(&sim), [GameResult::Black]);
    let game = &sim.games[0];
    assert_eq!(game.tags().get_extra("Termination"), Some("adjudication"));
    assert_eq!(game.initial_comment(), Some("Manual adjudication"));
    assert_eq!(sim.t.phase(), Phase::Finished);
    // Outside a game the buttons do nothing.
    assert!(matches!(
        sim.t.adjudicate(GameResult::Draw, sim.now).first(),
        Some(Action::Notice(TournamentNotice::NotApplicable))
    ));
}

#[test]
fn an_engine_that_dies_gets_a_second_try_with_a_restart() {
    let mut config = base_config(2);
    config.failures.max_retries = 1;
    let mut sim = Sim::new(config);
    let mut deaths = 0;
    sim.policy = Box::new(move |job| {
        if deaths == 0 {
            deaths += 1;
            return dead();
        }
        SearchOutcome::Moved(legal_reply(job, 0))
    });
    sim.start();
    assert_eq!(sim.prepares.len(), 2, "the game started again");
    assert_eq!(sim.prepares[0].1, [false, false]);
    assert_eq!(
        sim.prepares[1].1,
        [true, false],
        "the engine that died restarts"
    );
    assert_eq!(sim.games.len(), 1, "the broken attempt is not saved");
    assert_eq!(sim.t.failures(), [1, 0]);
    assert!(sim.t.log().iter().any(|e| matches!(
        e,
        LogEvent::EngineFailed {
            participant: 0,
            kind: FailKind::Died
        }
    )));
    assert_eq!(sim.t.phase(), Phase::Finished);
}

#[test]
fn a_game_whose_engine_keeps_dying_is_lost_by_it_and_saved() {
    let mut config = base_config(2);
    config.failures.max_retries = 1;
    config.failures.max_failures = 10;
    let mut sim = Sim::new(config);
    sim.policy = Box::new(|job| {
        if job.participant == 0 && job.think.moves.len() >= 2 {
            return dead();
        }
        SearchOutcome::Moved(legal_reply(job, 1))
    });
    sim.start();
    // Two attempts, both ending with the death of participant 0 (White).
    assert_eq!(sim.prepares.len(), 2);
    assert_eq!(results(&sim), [GameResult::Black]);
    let game = &sim.games[0];
    assert_eq!(game.tags().get_extra("Termination"), Some("abandoned"));
    assert_eq!(sim.t.failures(), [2, 0]);
    assert!(
        sim.t
            .log()
            .iter()
            .any(|e| matches!(e, LogEvent::Discarded { number: 1, .. }))
    );
}

#[test]
fn three_failures_pause_and_withdrawing_drops_the_games_left() {
    let mut config = base_config(3);
    config.failures.max_retries = 0;
    config.failures.max_failures = 2;
    config.cycles = 2;
    let mut sim = Sim::new(config);
    sim.policy = Box::new(|job| {
        if job.participant == 2 {
            return dead();
        }
        SearchOutcome::Moved(legal_reply(job, 0))
    });
    sim.start();
    assert_eq!(sim.t.phase(), Phase::Decision(2));
    assert!(
        sim.notices
            .contains(&TournamentNotice::TooManyFailures { participant: 2 })
    );
    let played = sim.games.len();
    let actions = sim.t.decide(Decision::Withdraw, sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert!(sim.t.withdrawn()[2]);
    assert_eq!(sim.discarded, [2]);
    assert!(sim.games.len() >= played);
    let rows = sim.t.standings();
    assert!(
        rows.iter()
            .find(|r| r.participant == 2)
            .expect("row")
            .withdrawn
    );
    // The games of participant 2 that did not happen are dropped.
    assert!(
        sim.t
            .slots()
            .iter()
            .any(|s| s.status == GameStatus::Withdrawn)
    );
    assert!(
        sim.t
            .slots()
            .iter()
            .all(|s| s.status != GameStatus::Pending)
    );
}

#[test]
fn deciding_to_retry_clears_the_failures_and_goes_on() {
    let mut config = base_config(2);
    config.failures.max_retries = 0;
    config.failures.max_failures = 1;
    let mut sim = Sim::new(config);
    let mut healthy = false;
    sim.policy = Box::new(move |job| {
        if !healthy && job.participant == 0 {
            healthy = true;
            return dead();
        }
        SearchOutcome::Moved(legal_reply(job, 1))
    });
    sim.start();
    assert_eq!(sim.t.phase(), Phase::Decision(0));
    assert_eq!(sim.games.len(), 1, "the lost game was saved");
    let actions = sim.t.decide(Decision::Retry, sim.now);
    sim.run(actions);
    assert_eq!(sim.t.failures(), [0, 0]);
    assert_eq!(sim.t.phase(), Phase::Finished);
    // A decision nobody asked for does nothing.
    assert!(matches!(
        sim.t.decide(Decision::Retry, sim.now).first(),
        Some(Action::Notice(TournamentNotice::NotApplicable))
    ));
}

#[test]
fn replacing_a_participant_discards_its_engine() {
    let mut config = base_config(2);
    config.failures.max_retries = 0;
    config.failures.max_failures = 1;
    let mut sim = Sim::new(config);
    let mut first = true;
    sim.policy = Box::new(move |job| {
        if first && job.participant == 1 {
            first = false;
            return dead();
        }
        SearchOutcome::Moved(legal_reply(job, 0))
    });
    let new = Participant::new(99, "Other");
    sim.start();
    assert_eq!(sim.t.phase(), Phase::Decision(1));
    let actions = sim.t.decide(Decision::Replace(Box::new(new)), sim.now);
    sim.run(actions);
    assert_eq!(sim.t.config().participants[1].name, "Other");
    assert_eq!(sim.discarded, [1]);
}

#[test]
fn losing_on_time_is_a_loss_or_a_replay_by_policy() {
    let timeout = |_: &SearchJob| SearchOutcome::NoAnswer { killed: false };
    let mut config = base_config(2);
    config.time = TimeControl::Clock {
        base_ms: 1_000,
        increment_ms: 0,
    };
    let mut sim = Sim::new(config.clone());
    sim.policy = Box::new(timeout);
    sim.start();
    assert_eq!(results(&sim), [GameResult::Black]);
    assert_eq!(
        sim.games[0].tags().get_extra("Termination"),
        Some("time forfeit")
    );
    assert_eq!(sim.prepares.len(), 1);

    config.failures = FailurePolicy {
        on_time_loss: OnTimeLoss::Replay,
        max_retries: 1,
        max_failures: 9,
    };
    let mut sim = Sim::new(config);
    sim.policy = Box::new(timeout);
    sim.start();
    assert_eq!(sim.prepares.len(), 2, "replayed once");
    assert_eq!(results(&sim), [GameResult::Black], "then the loss stands");
}

#[test]
fn a_hung_engine_outside_a_clock_is_a_failure_to_replay() {
    let mut sim = Sim::new(base_config(2));
    let mut once = true;
    sim.policy = Box::new(move |job| {
        if once {
            once = false;
            return SearchOutcome::NoAnswer { killed: true };
        }
        SearchOutcome::Moved(legal_reply(job, 0))
    });
    sim.start();
    assert_eq!(sim.prepares.len(), 2);
    assert_eq!(sim.prepares[1].1, [true, false]);
}

#[test]
fn an_engine_that_cannot_start_counts_as_a_failure() {
    let mut config = base_config(2);
    config.failures.max_retries = 0;
    config.failures.max_failures = 9;
    let mut t = Tournament::new(config, "t", PartialDate::default(), None, None).expect("t");
    let now = Instant::now();
    let mut actions = t.begin(now);
    let Some(Action::Prepare { epoch, .. }) = actions
        .drain(..)
        .find(|a| matches!(a, Action::Prepare { .. }))
    else {
        panic!("prepare")
    };
    let failure = PrepareFailure {
        participant: 1,
        failure: EngineFailure {
            kind: FailureKind::CannotStart,
            exit_code: None,
            stderr_tail: Vec::new(),
        },
    };
    let actions = t.prepared(epoch, Err(failure), now);
    assert!(
        actions.iter().any(|a| matches!(a, Action::SaveGame { .. })),
        "the forfeit is saved"
    );
    assert_eq!(t.failures(), [0, 1]);
    assert_eq!(t.phase(), Phase::Finished);
    let rows = t.standings();
    assert_eq!(
        rows[0].participant, 0,
        "the other participant gets the point"
    );
}

#[test]
fn skip_move_and_redo_edit_the_queue() {
    let mut sim = Sim::new(base_config(3));
    sim.hold = true;
    sim.start();
    assert_eq!(sim.t.queue_numbers(), [2, 3]);
    sim.t.move_to_front(3);
    assert_eq!(sim.t.queue_numbers(), [3, 2]);
    sim.t.skip_pending(2);
    assert_eq!(sim.t.queue_numbers(), [3]);
    assert_eq!(sim.t.slots()[1].status, GameStatus::Skipped);
    assert!(matches!(
        sim.t.skip_pending(2).first(),
        Some(Action::Notice(TournamentNotice::NotApplicable))
    ));
    // Finish game 1 and 3.
    sim.hold = false;
    let Action::Search(job) = sim.held.pop().expect("held") else {
        panic!("search")
    };
    let actions = sim.t.searched(
        job.epoch,
        SearchOutcome::Moved(legal_reply(&job, 0)),
        sim.now,
    );
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.t.progress(), (2, 2));
    // Redo game 1.
    let actions = sim.t.redo(1);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Paused, "the user goes on");
    assert!(matches!(sim.t.slots()[0].status, GameStatus::Replaced(_)));
    assert_eq!(sim.t.progress(), (1, 2), "the replaced game stops counting");
    assert_eq!(sim.t.standings().iter().map(|r| r.played).sum::<u32>(), 2);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.t.progress(), (2, 2));
    assert_eq!(sim.games.len(), 3);
    // Same participants and colours.
    let a = &sim.games[0];
    let c = &sim.games[2];
    assert_eq!(
        (&a.tags().white, &a.tags().black),
        (&c.tags().white, &c.tags().black)
    );
}

#[test]
fn skipping_the_game_in_progress_marks_it_skipped_and_goes_on() {
    let mut sim = Sim::new(base_config(3));
    sim.hold = true;
    sim.start();
    let actions = sim.t.skip_current(sim.now);
    sim.run(actions);
    sim.hold = false;
    sim.answer_held(SearchOutcome::Cancelled);
    assert_eq!(sim.t.slots()[0].status, GameStatus::Skipped);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.games.len(), 2);
}

#[test]
fn a_saved_tournament_resumes_and_replays_the_interrupted_game() {
    let mut sim = Sim::new(base_config(3));
    sim.hold_after = Some(1);
    sim.start();
    // Game 1 is over, game 2 is waiting for its engine.
    assert_eq!(sim.games.len(), 1);
    let saved = sim.t.to_saved();
    let text = render_state(&saved).expect("render");
    let saved = parse_state(&text).expect("parse");
    assert_eq!(
        saved.queue.first(),
        Some(&1),
        "the game in progress is first"
    );
    let t = Tournament::resume(saved, None, None).expect("resume");
    assert_eq!(t.phase(), Phase::Paused);
    assert_eq!(t.progress(), (1, 3));
    assert_eq!(t.standings()[0].participant, 0);
    let mut sim = Sim::with(t);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Finished);
    assert_eq!(sim.games.len(), 2, "games 2 and 3 were played");
    assert_eq!(sim.games[0].tags().round, "1.2");
    // The engines are started again after a resume.
    assert_eq!(sim.prepares[0].1, [true, true]);
}

#[test]
fn a_finished_tournament_resumes_as_finished() {
    let mut sim = Sim::new(base_config(2));
    sim.start();
    let saved = sim.t.to_saved();
    let t = Tournament::resume(saved, None, None).expect("resume");
    assert_eq!(t.phase(), Phase::Finished);
}

fn book_for_two_moves() -> PlayBook {
    let start = Position::new();
    let e4 = start.parse_uci("e2e4").expect("uci");
    let after = start.after(e4).expect("after");
    let e5 = after.parse_uci("e7e5").expect("uci");
    let mut book = Book::new();
    book.set_position_moves(&start, &[(e4, 10)]).expect("set");
    book.set_position_moves(&after, &[(e5, 10)]).expect("set");
    PlayBook::new(book, BookSelection::Best)
}

#[test]
fn the_second_game_of_a_pair_repeats_the_book_line() {
    let mut config = base_config(2);
    config.cycles = 2;
    config.opening = OpeningSource::Book;
    config.book = Some(BookConfig {
        path: "book.bin".into(),
        pick: BookPick::Best,
    });
    config.pair_openings = true;
    let t = Tournament::new(
        config,
        "t",
        PartialDate::default(),
        Some(book_for_two_moves()),
        None,
    )
    .expect("t");
    let mut sim = Sim::with(t);
    sim.start();
    assert_eq!(sim.games.len(), 2);
    for game in &sim.games {
        let nodes: Vec<_> = game.mainline().collect();
        assert!(
            game.comment(nodes[1])
                .expect("node")
                .is_some_and(|c| c.contains("Last book move")),
            "both games comment the last book move"
        );
        assert_eq!(game.comment(nodes[0]).expect("node"), None);
    }
    assert_eq!(
        sim.games[0]
            .mainline()
            .take(2)
            .filter_map(|n| sim.games[0].mv(n).ok())
            .collect::<Vec<_>>(),
        sim.games[1]
            .mainline()
            .take(2)
            .filter_map(|n| sim.games[1].mv(n).ok())
            .collect::<Vec<_>>(),
    );
    // The line survives a save.
    assert_eq!(sim.t.to_saved().book_lines[0].moves, ["e2e4", "e7e5"]);
}

#[test]
fn a_suite_gives_each_pair_an_opening_and_logs_the_wrap() {
    let mut config = base_config(3);
    config.cycles = 2;
    config.opening = OpeningSource::Suite;
    config.suite = Some(crate::tournament::config::SuiteConfig {
        path: "s.epd".into(),
        order: crate::tournament::config::SuiteOrder::Sequential,
        plies: 4,
    });
    let suite = OpeningSuite::new(vec![
        Opening {
            name: Some("Alpha".into()),
            start: Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
                .expect("fen"),
            moves: Vec::new(),
        },
        Opening {
            name: Some("Beta".into()),
            start: Position::from_fen("rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq - 0 1")
                .expect("fen"),
            moves: Vec::new(),
        },
    ]);
    let t = Tournament::new(config, "t", PartialDate::default(), None, Some(suite)).expect("t");
    let mut sim = Sim::with(t);
    sim.start();
    assert_eq!(sim.games.len(), 6);
    let openings: Vec<_> = sim
        .games
        .iter()
        .map(|g| g.tags().get_extra("Opening").unwrap_or("").to_owned())
        .collect();
    assert_eq!(
        openings,
        ["Alpha", "Alpha", "Beta", "Beta", "Alpha", "Alpha"]
    );
    assert!(
        sim.t
            .log()
            .iter()
            .any(|e| matches!(e, LogEvent::SuiteWrapped { number: 5 }))
    );
    assert!(
        sim.games.iter().all(
            |g| g.tags().get_extra("FEN").is_none() || g.start_position().fen().contains(" b ")
        )
    );
}

#[test]
fn ponder_is_offered_only_to_engines_that_declare_it() {
    let mut config = base_config(2);
    config.ponder = true;
    config.adjudication = Adjudication::default();
    config.adjudication.max_plies = Some(4);
    let ponders = |job: &SearchJob| {
        let mut reply = legal_reply(job, 0);
        // Expect the first legal reply of the opponent.
        let mut position = job.think.start.clone();
        for &mv in &job.think.moves {
            position.play(mv).expect("legal");
        }
        if let Some(mv) = reply.mv
            && let Ok(after) = position.after(mv)
        {
            let mut replies: Vec<_> = after
                .legal_moves()
                .into_iter()
                .map(|m| (after.uci(m), m))
                .collect();
            replies.sort_by(|a, b| a.0.cmp(&b.0));
            reply.ponder = replies.first().map(|(_, m)| *m);
        }
        SearchOutcome::Moved(reply)
    };
    let mut with = Sim::new(config.clone());
    with.can_ponder = true;
    with.policy = Box::new(ponders);
    with.start();
    assert!(with.searches[0].ponder.is_none());
    let ponder = with.searches[1].ponder.as_ref().expect("white ponders");
    assert_eq!(ponder.participant, 0);
    assert_eq!(ponder.think.moves.len(), 2);

    let mut without = Sim::new(config);
    without.policy = Box::new(ponders);
    without.start();
    assert!(without.searches.iter().all(|s| s.ponder.is_none()));
}

#[test]
fn the_state_is_saved_after_each_game() {
    let mut sim = Sim::new(base_config(3));
    sim.start();
    let played: Vec<usize> = sim.states.iter().map(SavedTournament::played).collect();
    assert_eq!(played.first(), Some(&0));
    assert!(played.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(played.last(), Some(&3));
    assert!(sim.states.iter().any(|s| s.played() == 1 && !s.finished));
}

#[test]
fn new_refuses_an_invalid_configuration() {
    let mut config = base_config(2);
    config.participants.pop();
    assert!(matches!(
        Tournament::new(config, "t", PartialDate::default(), None, None),
        Err(ConfigError::TooFewParticipants)
    ));
}

#[test]
fn the_live_view_follows_the_game() {
    let mut sim = Sim::new(base_config(2));
    sim.hold = true;
    sim.start();
    let live = sim.t.live().expect("live");
    assert_eq!((live.number, live.white, live.black), (1, 0, 1));
    assert!(live.position.is_some());
    assert!(live.moves.is_empty());
    sim.answer_held(SearchOutcome::Moved(legal_reply(
        &sim.searches[0].clone(),
        0,
    )));
    assert_eq!(sim.t.live().expect("live").moves.len(), 1);
    assert_eq!(sim.t.engine_name(0), Some("Engine 0"));
}

#[test]
fn abort_while_the_engines_start_cancels_the_start() {
    // Regression (D1308): only a search was cancelled, so `Abort` waited for a
    // slow engine to finish starting.
    let mut sim = Sim::new(base_config(2));
    let actions = sim.t.begin(sim.now);
    let Some(Action::Prepare { epoch, sides, .. }) = actions
        .iter()
        .find(|a| matches!(a, Action::Prepare { .. }))
        .cloned()
    else {
        panic!("the first game asks to start the engines");
    };
    let actions = sim.t.abort(sim.now);
    assert!(
        matches!(actions.as_slice(), [Action::Cancel { epoch: e, .. }] if *e == epoch),
        "{actions:?}"
    );
    // The pool answers the cancelled start with a failure; it is ignored.
    let ready = sim.ready(sides);
    let actions = sim.t.prepared(epoch, Ok(ready), sim.now);
    sim.run(actions);
    assert_eq!(sim.t.phase(), Phase::Aborted);
    assert_eq!(sim.t.failures(), [0, 0]);
}

#[test]
fn skipping_while_the_engines_start_cancels_the_start_too() {
    let mut sim = Sim::new(base_config(3));
    let actions = sim.t.begin(sim.now);
    let epoch = actions
        .iter()
        .find_map(|a| match a {
            Action::Prepare { epoch, .. } => Some(*epoch),
            _ => None,
        })
        .expect("prepare");
    let actions = sim.t.skip_current(sim.now);
    assert!(matches!(actions.as_slice(), [Action::Cancel { .. }]));
    assert_eq!(epoch, sim.t.epoch());
}

#[test]
fn a_game_decided_by_the_last_move_is_saved_even_when_a_pause_was_asked() {
    // Regression (D1314): the pause came first and the decided game stayed
    // frozen until "Continue".
    let mut config = base_config(2);
    config.adjudication = Adjudication::default();
    config.adjudication.max_plies = Some(2);
    let mut sim = Sim::new(config);
    sim.hold = true;
    sim.start();
    let first = sim.searches[0].clone();
    sim.answer_held(SearchOutcome::Moved(legal_reply(&first, 0)));
    assert!(sim.t.pause().is_empty());
    assert_eq!(sim.t.phase(), Phase::Pausing);
    let second = sim.searches[1].clone();
    sim.answer_held(SearchOutcome::Moved(legal_reply(&second, 0)));
    assert_eq!(sim.games.len(), 1, "the finished game was written");
    assert_eq!(sim.games[0].tags().result, GameResult::Draw);
    assert_eq!(sim.t.progress().0, 1);
    assert_eq!(sim.t.phase(), Phase::Paused);
    assert!(sim.t.live().is_none());
}

#[test]
fn a_pause_forgets_the_move_the_engines_were_pondering_on() {
    // Regression (D1314): after `StopPonder` the next search was still given
    // a ponder job for the old guess.
    let mut config = base_config(2);
    config.ponder = true;
    config.adjudication = Adjudication::default();
    config.adjudication.max_plies = Some(6);
    let mut sim = Sim::new(config);
    sim.can_ponder = true;
    sim.hold = true;
    sim.start();
    let first = sim.searches[0].clone();
    let mut reply = legal_reply(&first, 0);
    let mv = reply.mv.expect("move");
    let after = first.think.start.after(mv).expect("after");
    reply.ponder = after.legal_moves().into_iter().next();
    assert!(reply.ponder.is_some());
    assert!(sim.t.pause().is_empty());
    sim.answer_held(SearchOutcome::Moved(reply));
    assert_eq!(sim.t.phase(), Phase::Paused);
    assert_eq!(sim.stop_ponders, 1);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    let after_pause = sim.searches.last().expect("search").clone();
    assert!(
        after_pause.ponder.is_none(),
        "the old guess is gone: {after_pause:?}"
    );
}

#[test]
fn a_resumed_tournament_queues_every_waiting_game() {
    // Regression (D1312): a pending game missing from the saved queue was
    // lost, and a repeated entry would have played it twice.
    let mut sim = Sim::new(base_config(3));
    sim.hold_after = Some(1);
    sim.start();
    let mut saved = sim.t.to_saved();
    saved.queue.clear();
    let t = Tournament::resume(saved.clone(), None, None).expect("resume");
    assert_eq!(t.queue_numbers(), [2, 3]);
    saved.queue = vec![2, 2, 1, 0];
    let t = Tournament::resume(saved, None, None).expect("resume");
    assert_eq!(t.queue_numbers(), [3, 2], "no repeats, played game dropped");
}

#[test]
fn a_redone_game_says_so_in_its_round_tag() {
    // The replaced game and its redo have the same pairing; the PGN keeps
    // both and the tag tells them apart (D1312).
    let mut config = base_config(2);
    config.cycles = 2;
    let mut sim = Sim::new(config);
    sim.start();
    assert_eq!(sim.games.len(), 2);
    let actions = sim.t.redo(1);
    sim.run(actions);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(sim.games.len(), 3);
    let rounds: Vec<&str> = sim.games.iter().map(|g| g.tags().round.as_str()).collect();
    assert_eq!(rounds, ["1.1", "2.1", "1.1 (redo 1)"]);
    let actions = sim.t.redo(1);
    sim.run(actions);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(
        sim.games.last().map(|g| g.tags().round.as_str()),
        Some("1.1 (redo 2)")
    );
}

#[test]
fn the_state_revision_grows_with_every_state_and_survives_a_resume() {
    let mut sim = Sim::new(base_config(2));
    sim.start();
    let revisions: Vec<u64> = sim.states.iter().map(|s| s.revision).collect();
    assert!(revisions.windows(2).all(|w| w[0] < w[1]), "{revisions:?}");
    let last = *revisions.last().expect("states");
    let t = Tournament::resume(sim.t.to_saved(), None, None).expect("resume");
    assert!(t.to_saved().revision > last);
}

#[test]
fn the_suite_wrap_is_reported_once_per_game() {
    // Regression (D1316): a retry (or a redo) of a game reported it again.
    let mut config = base_config(2);
    config.cycles = 4;
    config.opening = OpeningSource::Suite;
    config.suite = Some(crate::tournament::config::SuiteConfig {
        path: "s.epd".into(),
        order: crate::tournament::config::SuiteOrder::Sequential,
        plies: 4,
    });
    let suite = OpeningSuite::new(vec![Opening::initial()]);
    let t = Tournament::new(config, "t", PartialDate::default(), None, Some(suite)).expect("t");
    let mut sim = Sim::with(t);
    sim.start();
    let wraps = |sim: &Sim, number: u32| {
        sim.t
            .log()
            .iter()
            .filter(|e| matches!(e, LogEvent::SuiteWrapped { number: n } if *n == number))
            .count()
    };
    assert_eq!(wraps(&sim, 3), 1);
    let actions = sim.t.redo(3);
    sim.run(actions);
    let actions = sim.t.resume_play(sim.now);
    sim.run(actions);
    assert_eq!(wraps(&sim, 3), 1, "the redo does not report it again");
}
