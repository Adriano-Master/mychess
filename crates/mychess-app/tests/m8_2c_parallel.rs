//! M8-2c contract tests of the games at once (spec D2500, D2504, D2505,
//! D2507, D2509): the pure state machine with several lanes, driven by hand
//! so the order of the answers is under the control of the test.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::needless_pass_by_value
)]

mod m8_2_common;

use std::time::Instant;

use m8_2_common::*;
use mychess_app::tournament::{
    Action, BookConfig, BookPick, ConfigError, Decision, OpeningSource, Phase, SearchJob,
    SearchOutcome, SideReady, Tournament, TournamentConfig, TournamentNotice,
};
use mychess_core::{Game, GameResult};

/// Answers the actions of the tournament the way the application does, but
/// keeps the searches until the test answers them.
struct Lanes {
    t: Tournament,
    now: Instant,
    inflight: Vec<SearchJob>,
    prepares: Vec<(usize, [usize; 2], u64)>,
    restarts: Vec<(usize, [bool; 2])>,
    cancels: Vec<(usize, u64)>,
    games: Vec<(u32, Game)>,
    notices: Vec<TournamentNotice>,
    shutdowns: usize,
    stop_ponders: Vec<usize>,
    game_overs: Vec<(usize, [usize; 2], GameResult)>,
    discarded: Vec<usize>,
}

impl Lanes {
    fn new(config: TournamentConfig) -> Lanes {
        let t = tournament(config);
        Lanes::with(t)
    }

    fn with(t: Tournament) -> Lanes {
        Lanes {
            t,
            now: Instant::now(),
            inflight: Vec::new(),
            prepares: Vec::new(),
            restarts: Vec::new(),
            cancels: Vec::new(),
            games: Vec::new(),
            notices: Vec::new(),
            shutdowns: 0,
            stop_ponders: Vec::new(),
            game_overs: Vec::new(),
            discarded: Vec::new(),
        }
    }

    fn feed(&mut self, first: Vec<Action>) {
        let mut queue: std::collections::VecDeque<Action> = first.into();
        while let Some(action) = queue.pop_front() {
            let more = match action {
                Action::Prepare {
                    lane,
                    sides,
                    restart,
                    epoch,
                    ..
                } => {
                    self.prepares.push((lane, sides, epoch));
                    self.restarts.push((lane, restart));
                    let ready = sides.map(|participant| SideReady {
                        participant,
                        engine_name: None,
                        can_ponder: false,
                    });
                    self.t.prepared(epoch, Ok(ready), self.now)
                }
                Action::Search(job) => {
                    self.inflight.push(*job);
                    Vec::new()
                }
                Action::Cancel { lane, epoch } => {
                    self.cancels.push((lane, epoch));
                    Vec::new()
                }
                Action::StopPonder { lane } => {
                    self.stop_ponders.push(lane);
                    Vec::new()
                }
                Action::GameOver {
                    lane,
                    participants,
                    result,
                    ..
                } => {
                    self.game_overs.push((lane, participants, result));
                    Vec::new()
                }
                Action::SaveGame { number, game } => {
                    self.games.push((number, *game));
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
                Action::SaveState(_) => Vec::new(),
            };
            queue.extend(more);
        }
    }

    fn begin(&mut self) {
        let actions = self.t.begin(self.now);
        self.feed(actions);
    }

    fn lane_number(&self, lane: usize) -> u32 {
        self.t
            .lanes()
            .iter()
            .find(|l| l.lane == lane)
            .map(|l| l.number)
            .expect("a game in that lane")
    }

    fn job_of(&self, lane: usize) -> SearchJob {
        self.inflight
            .iter()
            .find(|j| j.lane == lane)
            .cloned()
            .expect("a search in that lane")
    }

    fn answer_outcome(&mut self, lane: usize, outcome: SearchOutcome) {
        let at = self
            .inflight
            .iter()
            .position(|j| j.lane == lane)
            .expect("a search in that lane");
        let job = self.inflight.remove(at);
        let actions = self.t.searched(job.epoch, outcome, self.now);
        self.feed(actions);
    }

    /// The lane answers its search with the script of `result`.
    fn answer(&mut self, lane: usize, result: GameResult) {
        let outcome = scripted_result(&self.job_of(lane), result);
        self.answer_outcome(lane, outcome);
    }

    /// Answers every search until nothing is left, choosing the lane with
    /// `pick` among the busy ones.
    fn finish(&mut self, decide: impl Fn(u32) -> GameResult, pick: impl Fn(&[usize]) -> usize) {
        let mut guard = 0;
        while !self.inflight.is_empty() {
            guard += 1;
            assert!(guard < 10_000, "the tournament does not end");
            let busy: Vec<usize> = self.inflight.iter().map(|j| j.lane).collect();
            let lane = pick(&busy);
            let result = decide(self.lane_number(lane));
            self.answer(lane, result);
        }
    }
}

fn parallel(n: usize, lanes: u32) -> TournamentConfig {
    TournamentConfig {
        concurrency: lanes,
        ..cfg(n)
    }
}

/// A result that depends only on the number of the game.
fn by_number(number: u32) -> GameResult {
    match number % 3 {
        0 => GameResult::Black,
        1 => GameResult::White,
        _ => GameResult::Draw,
    }
}

fn first_lane(busy: &[usize]) -> usize {
    *busy.iter().min().unwrap()
}

fn last_lane(busy: &[usize]) -> usize {
    *busy.iter().max().unwrap()
}

#[test]
fn free_lanes_take_the_games_of_the_queue_in_order() {
    let mut l = Lanes::new(parallel(4, 3));
    l.begin();
    let started: Vec<(usize, u32)> = l.t.lanes().iter().map(|x| (x.lane, x.number)).collect();
    assert_eq!(started, [(0, 1), (1, 2), (2, 3)]);
    let lanes: Vec<usize> = l.prepares.iter().map(|p| p.0).collect();
    assert_eq!(lanes, [0, 1, 2]);
    // Every request has an epoch of its own.
    let mut epochs: Vec<u64> = l.prepares.iter().map(|p| p.2).collect();
    epochs.dedup();
    assert_eq!(epochs.len(), 3);
    assert_eq!(l.t.lane_count(), 3);
    // Each lane asks its engine for a move.
    let mut asked: Vec<usize> = l.inflight.iter().map(|j| j.lane).collect();
    asked.sort_unstable();
    assert_eq!(asked, [0, 1, 2]);
}

#[test]
fn a_finished_game_frees_its_lane_for_the_next_game() {
    let mut l = Lanes::new(parallel(4, 2));
    l.begin();
    assert_eq!(l.t.lanes().len(), 2);
    // Game 2 (lane 1) ends first: lane 1 takes game 3.
    let result = GameResult::White;
    while l.t.lanes().iter().any(|x| x.number == 2) {
        l.answer(1, result);
    }
    let now: Vec<(usize, u32)> = l.t.lanes().iter().map(|x| (x.lane, x.number)).collect();
    assert_eq!(now, [(0, 1), (1, 3)]);
    assert_eq!(l.game_overs.len(), 1);
    assert_eq!(l.game_overs[0].0, 1, "the engines of that lane were told");
}

#[test]
fn the_table_does_not_depend_on_the_order_the_games_end_in() {
    let run = |lanes: u32, pick: fn(&[usize]) -> usize| {
        let mut l = Lanes::new(TournamentConfig {
            cycles: 2,
            ..parallel(4, lanes)
        });
        l.begin();
        l.finish(by_number, pick);
        assert_eq!(l.t.phase(), Phase::Finished);
        (l.t.finished_games(), l.t.standings(), l.t.ratings().len())
    };
    let sequential = run(1, first_lane);
    for lanes in [2, 3, 6] {
        for pick in [first_lane as fn(&[usize]) -> usize, last_lane] {
            assert_eq!(run(lanes, pick), sequential, "{lanes} lanes");
        }
    }
    assert_eq!(sequential.0.len(), 12);
}

#[test]
fn every_game_of_the_schedule_is_played_exactly_once() {
    let mut l = Lanes::new(TournamentConfig {
        cycles: 2,
        ..parallel(5, 4)
    });
    l.begin();
    l.finish(by_number, last_lane);
    let mut numbers: Vec<u32> = l.games.iter().map(|(n, _)| *n).collect();
    numbers.sort_unstable();
    let expected: Vec<u32> = (1..=20).collect();
    assert_eq!(numbers, expected);
    assert_eq!(l.shutdowns, 1);
}

#[test]
fn an_answer_for_an_unknown_epoch_changes_nothing() {
    let mut l = Lanes::new(parallel(3, 2));
    l.begin();
    let job = l.job_of(0);
    let before = l.t.lanes();
    let outcome = scripted_result(&job, GameResult::White);
    assert!(
        l.t.searched(job.epoch + 100, outcome.clone(), l.now)
            .is_empty()
    );
    assert!(l.t.searched(0, outcome, l.now).is_empty());
    assert_eq!(l.t.lanes(), before);
    // The right epoch still works, once.
    let outcome = scripted_result(&job, GameResult::White);
    assert!(!l.t.searched(job.epoch, outcome.clone(), l.now).is_empty());
    assert!(l.t.searched(job.epoch, outcome, l.now).is_empty());
}

#[test]
fn the_games_of_a_pair_with_a_book_line_never_run_together() {
    // D2504: the second game repeats the book line of the first.
    let serial = TournamentConfig {
        opening: OpeningSource::Book,
        book: Some(BookConfig {
            path: "any.bin".into(),
            pick: BookPick::Weighted,
        }),
        pair_openings: true,
        cycles: 2,
        ..parallel(2, 2)
    };
    let mut l = Lanes::new(serial);
    l.begin();
    assert_eq!(l.t.lanes().len(), 1, "game 2 waits for game 1");
    l.finish(|_| GameResult::White, first_lane);
    assert_eq!(l.t.phase(), Phase::Finished);

    // Without a book the same pair can play both games at once.
    let free = TournamentConfig {
        cycles: 2,
        ..parallel(2, 2)
    };
    let mut l = Lanes::new(free);
    l.begin();
    assert_eq!(l.t.lanes().len(), 2);
}

#[test]
fn a_pause_is_complete_when_every_lane_finished_its_move() {
    let mut l = Lanes::new(parallel(4, 2));
    l.begin();
    drop(l.t.pause());
    assert_eq!(l.t.phase(), Phase::Pausing);
    // Lane 0 answers: it freezes, the other lane still thinks.
    l.answer(0, GameResult::White);
    assert_eq!(l.t.phase(), Phase::Pausing);
    assert_eq!(l.stop_ponders, [0]);
    l.answer(1, GameResult::White);
    assert_eq!(l.t.phase(), Phase::Paused);
    assert_eq!(l.inflight.len(), 0);
    assert!(l.notices.contains(&TournamentNotice::Paused));
    // Going on asks both lanes for a move again, and starts nothing new.
    let actions = l.t.resume_play(l.now);
    l.feed(actions);
    let mut lanes: Vec<usize> = l.inflight.iter().map(|j| j.lane).collect();
    lanes.sort_unstable();
    assert_eq!(lanes, [0, 1]);
    assert_eq!(l.t.phase(), Phase::Running);
}

#[test]
fn a_pause_asked_between_games_starts_no_new_game() {
    let mut l = Lanes::new(parallel(3, 2));
    l.begin();
    l.t.pause();
    // Both games end on their next answer (the scripts are short enough when
    // the answers come one after the other); nothing new starts after a pause.
    for _ in 0..16 {
        let Some(lane) = l.inflight.first().map(|j| j.lane) else {
            break;
        };
        l.answer(lane, GameResult::White);
        if l.t.phase() == Phase::Paused {
            break;
        }
    }
    assert_eq!(l.t.phase(), Phase::Paused);
    let played = l.t.progress().0;
    assert!(played <= 2, "no game began during the pause");
}

#[test]
fn stopping_after_the_game_waits_for_every_game_in_progress() {
    let mut l = Lanes::new(parallel(4, 2));
    l.begin();
    l.t.stop_after_game(l.now);
    // Lane 0 finishes its game: lane 1 still plays, so nothing stops yet and
    // nothing new starts.
    while l.t.lanes().iter().any(|x| x.lane == 0) {
        l.answer(0, GameResult::White);
    }
    assert_eq!(l.t.lanes().len(), 1);
    assert_eq!(l.shutdowns, 0);
    while !l.t.lanes().is_empty() {
        l.answer(1, GameResult::White);
    }
    assert_eq!(l.t.phase(), Phase::StoppedAfterGame);
    assert_eq!(l.shutdowns, 1);
    assert_eq!(l.t.progress().0, 2);
    // Going on starts two games again.
    let actions = l.t.resume_play(l.now);
    l.feed(actions);
    assert_eq!(l.t.lanes().len(), 2);
}

#[test]
fn aborting_cancels_every_lane_and_gives_the_games_back_in_order() {
    let mut l = Lanes::new(parallel(4, 3));
    l.begin();
    let actions = l.t.abort(l.now);
    l.feed(actions);
    let mut cancelled: Vec<usize> = l.cancels.iter().map(|c| c.0).collect();
    cancelled.sort_unstable();
    assert_eq!(cancelled, [0, 1, 2]);
    assert_ne!(l.t.phase(), Phase::Aborted, "the engines have not answered");
    // The cancelled searches come back.
    for lane in [2, 0, 1] {
        l.answer_outcome(lane, SearchOutcome::Cancelled);
    }
    assert_eq!(l.t.phase(), Phase::Aborted);
    assert_eq!(l.shutdowns, 1);
    assert_eq!(
        l.t.queue_numbers()[..3],
        [1, 2, 3],
        "in the order of the schedule"
    );
    assert_eq!(l.t.progress().0, 0);
    assert!(l.games.is_empty(), "an interrupted game is not saved");
}

#[test]
fn adjudication_and_skipping_act_on_the_watched_game() {
    let mut l = Lanes::new(parallel(4, 2));
    l.begin();
    l.t.watch(1);
    let live = l.t.live().expect("live");
    assert_eq!((live.lane, live.number), (1, 2));
    let actions = l.t.adjudicate(GameResult::Black, l.now);
    l.feed(actions);
    assert_eq!(l.cancels.len(), 1);
    assert_eq!(l.cancels[0].0, 1);
    l.answer_outcome(1, SearchOutcome::Cancelled);
    // Game 2 ended 0-1; game 1 is still on.
    let played: Vec<_> =
        l.t.finished_games()
            .into_iter()
            .map(|g| (g.number, g.result))
            .collect();
    assert_eq!(played, [(2, GameResult::Black)]);
    assert!(l.t.lanes().iter().any(|x| x.number == 1));
    // A lane that does not exist is ignored.
    l.t.watch(9);
    assert!(l.t.live().is_some());
}

#[test]
fn a_participant_that_fails_too_often_stops_new_games_only() {
    let mut config = parallel(4, 2);
    config.failures.max_failures = 1;
    config.failures.max_retries = 1;
    let mut l = Lanes::new(config);
    l.begin();
    // Lane 1 loses its engine: the question is asked.
    l.answer_outcome(1, dead());
    assert!(matches!(l.t.phase(), Phase::Decision(_)));
    assert!(
        l.notices
            .iter()
            .any(|n| matches!(n, TournamentNotice::TooManyFailures { .. }))
    );
    // Lane 0 plays on to the end of its game and nothing else begins.
    let before = l.prepares.len();
    for _ in 0..16 {
        if !l.inflight.iter().any(|j| j.lane == 0) {
            break;
        }
        l.answer(0, GameResult::White);
    }
    assert_eq!(
        l.prepares.len(),
        before,
        "no engines were prepared meanwhile"
    );
    assert!(matches!(l.t.phase(), Phase::Decision(_)));
    assert_eq!(l.t.progress().0, 1, "the game of lane 0 was saved");
    // Trying again repeats the broken game and fills the other lane.
    let actions = l.t.decide(Decision::Retry, l.now);
    l.feed(actions);
    assert_eq!(l.t.lanes().len(), 2);
    assert_eq!(l.t.phase(), Phase::Running);
}

#[test]
fn withdrawing_a_participant_ends_its_games_in_every_lane() {
    let mut config = parallel(4, 3);
    config.failures.max_failures = 1;
    config.failures.max_retries = 1;
    let mut l = Lanes::new(config);
    l.begin();
    // Lane 0 plays the first game, with participant 0 as White.
    let mut involved: Vec<usize> =
        l.t.lanes()
            .iter()
            .filter(|x| x.lane != 0 && (x.white == 0 || x.black == 0))
            .map(|x| x.lane)
            .collect();
    involved.sort_unstable();
    assert!(!involved.is_empty(), "another game has participant 0");
    assert_eq!(l.t.live().map(|v| v.white), Some(0));
    l.answer_outcome(0, dead());
    assert!(matches!(l.t.phase(), Phase::Decision(0)));
    let actions = l.t.decide(Decision::Withdraw, l.now);
    l.feed(actions);
    // The searches in flight of the other two lanes were cancelled.
    let mut cancelled: Vec<usize> = l.cancels.iter().map(|c| c.0).collect();
    cancelled.sort_unstable();
    assert_eq!(cancelled, involved);
    assert_eq!(l.discarded, [0]);
    // The cancelled searches come back and are ignored.
    for lane in involved {
        l.answer_outcome(lane, SearchOutcome::Cancelled);
    }
    l.finish(by_number, first_lane);
    assert_eq!(l.t.phase(), Phase::Finished);
    assert!(
        l.t.finished_games()
            .iter()
            .all(|g| g.white != 0 && g.black != 0)
    );
}

#[test]
fn games_in_progress_are_saved_as_waiting_and_the_lanes_survive_a_resume() {
    let mut l = Lanes::new(parallel(4, 3));
    l.begin();
    let saved = l.t.to_saved();
    assert_eq!(saved.config.concurrency, 3);
    assert_eq!(
        saved.queue[..3],
        [0, 1, 2],
        "in-flight games come first, by number"
    );
    let resumed = Tournament::resume(saved, None, None).expect("resume");
    assert_eq!(resumed.lane_count(), 3);
    let mut l = Lanes::with(resumed);
    // A resumed tournament goes on by itself, three games at once.
    let actions = l.t.resume_play(l.now);
    l.feed(actions);
    assert_eq!(l.t.lanes().len(), 3);
    l.finish(by_number, first_lane);
    assert_eq!(l.t.phase(), Phase::Finished);
}

#[test]
fn the_failures_of_a_lane_restart_only_its_own_engine() {
    // D2506: the restart flag is per lane and participant.
    let mut config = parallel(3, 2);
    config.failures.max_retries = 1;
    let mut l = Lanes::new(config);
    l.begin();
    l.answer_outcome(1, dead());
    // Lane 1 played again at once; its prepare asks for a restart, the
    // other lane's does not.
    let again: Vec<_> = l.restarts.iter().filter(|p| p.0 == 1).collect();
    assert_eq!(again.len(), 2);
    assert_eq!(again[0].1, [false, false]);
    assert!(again[1].1.contains(&true), "the broken engine starts again");
    let other: Vec<_> = l.restarts.iter().filter(|p| p.0 == 0).collect();
    assert_eq!(other.len(), 1);
    assert_eq!(
        other[0].1,
        [false, false],
        "the other lane keeps its engines"
    );
}

#[test]
fn the_configuration_validates_the_games_at_once() {
    let mut c = cfg(3);
    assert_eq!(c.concurrency, 1, "the default is one game");
    assert!(c.validate().is_ok());
    c.concurrency = 0;
    assert_eq!(c.validate(), Err(ConfigError::BadConcurrency));
    c.concurrency = 17;
    assert_eq!(c.validate(), Err(ConfigError::BadConcurrency));
    c.concurrency = 16;
    assert!(c.validate().is_ok());
    assert_eq!(c.clone().clamped().concurrency, 16);
    c.concurrency = 999;
    assert_eq!(c.clamped().concurrency, 16);
}

#[test]
fn the_estimate_divides_the_time_by_the_lanes() {
    let mut c = cfg(4);
    c.time = mychess_app::tournament::TimeControl::MoveTime { ms: 1000 };
    let one = c.estimate();
    c.concurrency = 4;
    let four = c.estimate();
    assert_eq!(four.lanes, 4);
    assert_eq!(one.games, four.games);
    assert_eq!(four.max_duration, one.max_duration / 4);
}

#[test]
fn an_old_configuration_without_the_field_means_one_game_at_once() {
    let text = toml::to_string(&cfg(2)).expect("write");
    let without: String = text
        .lines()
        .filter(|l| !l.starts_with("concurrency"))
        .collect::<Vec<_>>()
        .join("\n");
    let back: TournamentConfig = toml::from_str(&without).expect("read");
    assert_eq!(back.concurrency, 1);
}

// ---- corrections of the review ----

/// Three participants, two lanes: game 1 (0,1) in lane 0, game 2 (0,2) in
/// lane 1; participant 0 is in both and a failure raises the question.
fn two_lanes_with_a_question() -> Lanes {
    let mut config = parallel(3, 2);
    config.failures.max_failures = 1;
    config.failures.max_retries = 1;
    let mut l = Lanes::new(config);
    l.begin();
    assert_eq!(l.t.live().map(|v| v.white), Some(0));
    l.answer_outcome(0, dead());
    assert!(matches!(l.t.phase(), Phase::Decision(0)));
    l
}

fn new_engine() -> mychess_app::tournament::Participant {
    mychess_app::tournament::Participant::new(99, "New")
}

#[test]
fn replacing_a_participant_restarts_it_in_every_lane_and_replays_its_other_games() {
    let mut l = two_lanes_with_a_question();
    let before = l.restarts.len();
    let actions = l.t.decide(Decision::Replace(Box::new(new_engine())), l.now);
    l.feed(actions);
    // The search of the other lane, played by the old engine, is cancelled
    // and the old engine is discarded in the pool.
    assert!(l.cancels.iter().any(|c| c.0 == 1), "{:?}", l.cancels);
    assert_eq!(l.discarded, [0]);
    // Both lanes prepare a game with participant 0 again, and both ask to
    // start its engine from scratch.
    let after: Vec<_> = l.restarts[before..].to_vec();
    let prepares: Vec<_> = l.prepares[before..].to_vec();
    assert_eq!(after.len(), 2, "{after:?}");
    for ((lane, restart), (plane, sides, _)) in after.iter().zip(&prepares) {
        assert_eq!(lane, plane);
        let at = sides.iter().position(|p| *p == 0).expect("participant 0");
        assert!(
            restart[at],
            "lane {lane} restarts the new engine: {restart:?}"
        );
    }
    let mut lanes: Vec<usize> = after.iter().map(|r| r.0).collect();
    lanes.sort_unstable();
    assert_eq!(lanes, [0, 1]);
    // The cancelled answer of the old search is ignored.
    let old = l.inflight.iter().position(|j| j.lane == 1).unwrap();
    let stale = l.inflight.remove(old);
    assert!(
        l.t.searched(stale.epoch, SearchOutcome::Cancelled, l.now)
            .is_empty()
    );
    assert_eq!(l.t.phase(), Phase::Running);
}

#[test]
fn the_epochs_of_two_tournaments_never_meet() {
    let mut a = Lanes::new(cfg(2));
    a.begin();
    let old_epoch = a.prepares[0].2;
    let mut b = Lanes::new(cfg(2));
    b.begin();
    let new_epoch = b.prepares[0].2;
    assert_ne!(old_epoch, new_epoch);
    // The late answer of a task of the first tournament does nothing in the
    // second.
    let ready = [0, 1].map(|participant| SideReady {
        participant,
        engine_name: None,
        can_ponder: false,
    });
    assert!(b.t.prepared(old_epoch, Ok(ready), b.now).is_empty());
    let job = a.job_of(0);
    assert!(
        b.t.searched(job.epoch, SearchOutcome::Cancelled, b.now)
            .is_empty()
    );
    assert_eq!(b.t.lanes().len(), 1, "the second tournament is untouched");
}

#[test]
fn a_decision_cannot_bring_back_an_aborted_tournament() {
    let mut l = two_lanes_with_a_question();
    let actions = l.t.abort(l.now);
    l.feed(actions);
    // The answer of the question comes after the user ended the tournament.
    for decision in [
        Decision::Retry,
        Decision::Withdraw,
        Decision::Replace(Box::new(new_engine())),
    ] {
        let actions = l.t.decide(decision, l.now);
        assert!(
            matches!(
                actions.as_slice(),
                [Action::Notice(TournamentNotice::NotApplicable)]
            ),
            "{actions:?}"
        );
    }
    l.answer_outcome(1, SearchOutcome::Cancelled);
    assert_eq!(l.t.phase(), Phase::Aborted);
    assert_eq!(l.t.lanes().len(), 0);
    // And later, too.
    let actions = l.t.decide(Decision::Retry, l.now);
    assert!(matches!(
        actions.as_slice(),
        [Action::Notice(TournamentNotice::NotApplicable)]
    ));
    assert_eq!(l.t.phase(), Phase::Aborted);
    assert_eq!(l.t.progress(), (0, 3), "every game still waits");
}

#[test]
fn withdrawing_does_not_override_what_the_user_asked_for_a_game() {
    // Lane 1 is being skipped (its search is cancelled); the question about
    // participant 0 is answered "withdraw" meanwhile.
    let mut config = parallel(4, 3);
    config.failures.max_failures = 1;
    config.failures.max_retries = 1;
    let mut l = Lanes::new(config);
    l.begin();
    l.t.watch(1);
    let skipped = l.lane_number(1);
    let actions = l.t.skip_current(l.now);
    l.feed(actions);
    assert_eq!(l.cancels.len(), 1);
    l.answer_outcome(0, dead());
    assert!(matches!(l.t.phase(), Phase::Decision(0)));
    let actions = l.t.decide(Decision::Withdraw, l.now);
    l.feed(actions);
    assert_eq!(
        l.cancels.iter().filter(|c| c.0 == 1).count(),
        1,
        "no second cancel for the lane being skipped"
    );
    l.answer_outcome(1, SearchOutcome::Cancelled);
    let status =
        l.t.slots()
            .iter()
            .find(|s| s.pairing.number == skipped)
            .map(|s| s.status.clone());
    assert_eq!(status, Some(mychess_app::tournament::GameStatus::Skipped));
}

#[test]
fn the_question_stays_the_phase_when_a_pause_is_pending() {
    let mut config = parallel(3, 2);
    config.failures.max_failures = 1;
    config.failures.max_retries = 1;
    let mut l = Lanes::new(config);
    l.begin();
    drop(l.t.pause());
    // Lane 0 loses its engine: the question is asked. Lane 1 then ends its
    // move and freezes for the pause, which must not hide the question.
    l.answer_outcome(0, dead());
    assert!(matches!(l.t.phase(), Phase::Decision(0)));
    l.answer(1, GameResult::White);
    assert!(
        matches!(l.t.phase(), Phase::Decision(0)),
        "{:?}",
        l.t.phase()
    );
    let actions = l.t.decide(Decision::Retry, l.now);
    assert!(
        !matches!(
            actions.as_slice(),
            [Action::Notice(TournamentNotice::NotApplicable)]
        ),
        "the question can be answered"
    );
}

#[test]
fn the_lanes_that_run_are_not_the_configuration_that_is_saved() {
    // D2503, D2509: a smaller machine runs fewer games but the user's
    // choice stays in the file.
    let mut t = tournament(parallel(4, 8));
    t.set_lanes(3);
    assert_eq!(t.lane_count(), 3);
    assert_eq!(t.config().concurrency, 8);
    assert_eq!(t.to_saved().config.concurrency, 8);
    let resumed = Tournament::resume(t.to_saved(), None, None).expect("resume");
    assert_eq!(
        resumed.lane_count(),
        8,
        "the machine decides again at resume"
    );
}

#[test]
fn a_late_answer_of_one_lane_never_enters_another_lane() {
    let mut l = Lanes::new(parallel(4, 2));
    l.begin();
    let a = l.job_of(0);
    let b = l.job_of(1);
    assert_ne!(a.epoch, b.epoch, "every request has an epoch of its own");
    let outcome = scripted_result(&a, GameResult::White);
    l.answer_outcome(0, outcome.clone());
    let numbers = |l: &Lanes| -> Vec<(usize, u32)> {
        l.t.lanes().iter().map(|x| (x.lane, x.number)).collect()
    };
    let before = numbers(&l);
    // The same answer comes again (or late): neither lane moves.
    assert!(l.t.searched(a.epoch, outcome, l.now).is_empty());
    assert_eq!(numbers(&l), before);
    // Lane 1 still waits for its own epoch and takes it.
    assert!(l.inflight.iter().any(|j| j.epoch == b.epoch));
    let own = scripted_result(&b, GameResult::White);
    assert!(!l.t.searched(b.epoch, own, l.now).is_empty());
}

#[test]
fn skipping_one_lane_leaves_the_search_of_the_other_alone() {
    let mut l = Lanes::new(parallel(4, 2));
    l.begin();
    let other = l.job_of(0);
    l.t.watch(1);
    let actions = l.t.skip_current(l.now);
    l.feed(actions);
    assert_eq!(l.cancels.len(), 1);
    assert_eq!(l.cancels[0].0, 1, "only the watched lane is cancelled");
    // The search of lane 0 is still the one in flight and is answered.
    assert!(l.inflight.iter().any(|j| j.epoch == other.epoch));
    l.answer(0, GameResult::White);
    assert!(l.t.lanes().iter().any(|x| x.lane == 0));
}

#[test]
fn games_kept_for_a_pause_after_a_retry_return_in_the_order_of_the_schedule() {
    // Regression (D2535): the games of two lanes waiting for a retry went
    // back to the queue lane by lane and the later game came first.
    let mut config = parallel(3, 2);
    config.failures.max_failures = 1;
    config.failures.max_retries = 1;
    let mut l = Lanes::new(config);
    l.begin();
    drop(l.t.pause());
    l.answer_outcome(0, dead());
    assert!(matches!(l.t.phase(), Phase::Decision(0)));
    // The same participant fails in the other lane while the question is
    // open: that game waits for the decision too.
    l.answer_outcome(1, dead());
    let actions = l.t.decide(Decision::Retry, l.now);
    l.feed(actions);
    let queue = l.t.queue_numbers();
    assert!(queue.starts_with(&[1, 2]), "{queue:?}");
}
