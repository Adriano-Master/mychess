//! Shared helpers of the `m8_2_*` contract tests (spec
//! `docs/specs/m8-2-torneios-entre-engines.md`, phase A).
//!
//! `Sim` plays the part of the application for the pure state machine
//! (`Tournament`): it answers every `Action` the way the tasks would. The
//! scripted UCI player (`Scripted`) is a tiny in-memory engine for the tests
//! that need the real `EnginePool`.
//!
//! Only the public API of `mychess-app` is used.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use mychess_app::engines::{EngineEntry, EngineFailure, EngineId, FailureKind, Launcher};
use mychess_app::tournament::{
    Action, Adjudication, EnginePool, LaunchRequest, Participant, Phase, PrepareFailure,
    PreparePlan, Reply, ResignRule, Scheduling, SearchJob, SearchOutcome, SideReady, TimeControl,
    Tournament, TournamentConfig, TournamentNotice,
};
use mychess_core::{Color, Game, GameResult, Move, PartialDate, Position};
use mychess_engine::testing::ManualEngine;
use mychess_engine::{EngineConfig, Score};

/// Participants `P0`, `P1`...
pub fn participants(n: usize) -> Vec<Participant> {
    (0..n)
        .map(|i| Participant::new(i as u64 + 1, &format!("P{i}")))
        .collect()
}

/// A configuration of `n` participants, 1 cycle, depth limit, no adjudication.
pub fn cfg(n: usize) -> TournamentConfig {
    TournamentConfig {
        participants: participants(n),
        cycles: 1,
        time: TimeControl::Depth(5),
        ..TournamentConfig::default()
    }
}

pub fn today() -> PartialDate {
    PartialDate::new(Some(2026), Some(10), Some(24)).expect("date")
}

pub fn tournament(config: TournamentConfig) -> Tournament {
    Tournament::new(config, "t", today(), None, None).expect("a valid tournament")
}

/// The position at the end of the history of a search.
pub fn position_of(job: &SearchJob) -> Position {
    let mut position = job.think.start.clone();
    for &mv in &job.think.moves {
        position.play(mv).expect("legal history");
    }
    position
}

pub fn mv(position: &Position, uci: &str) -> Move {
    position.parse_uci(uci).expect("a legal uci move")
}

/// The first legal move by UCI text.
pub fn first_legal(position: &Position) -> Option<Move> {
    let mut moves: Vec<_> = position
        .legal_moves()
        .into_iter()
        .map(|m| (position.uci(m), m))
        .collect();
    moves.sort_by(|a, b| a.0.cmp(&b.0));
    moves.first().map(|(_, m)| *m)
}

/// A reply with the given move (from the current position of `job`).
pub fn reply_with(job: &SearchJob, uci: &str, spent_ms: u64, white_cp: Option<i32>) -> Reply {
    let position = position_of(job);
    Reply {
        mv: Some(mv(&position, uci)),
        spent: Duration::from_millis(spent_ms),
        score: white_cp.map(Score::Cp),
        depth: Some(10),
        ponder: None,
    }
}

pub const SCHOLAR_WHITE: [&str; 4] = ["e2e4", "f1c4", "d1h5", "h5f7"];
pub const SCHOLAR_BLACK: [&str; 3] = ["a7a6", "b7b6", "c7c6"];
pub const FOOL_WHITE: [&str; 2] = ["f2f3", "g2g4"];
pub const FOOL_BLACK: [&str; 2] = ["e7e5", "d8h4"];
/// Eight plies of knights: the start position comes back for the third time.
pub const SHUFFLE: [&str; 8] = [
    "g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8",
];

/// What the policy knows about the game being played.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub number: u32,
    pub white: usize,
    pub black: usize,
}

pub type Policy = Box<dyn FnMut(&SearchJob, Ctx) -> SearchOutcome>;

/// Plays out `result` with the shortest known script (scholar's mate, fool's
/// mate, or the knight shuffle), with a flat score of 0 for each side.
pub fn scripted_result(job: &SearchJob, result: GameResult) -> SearchOutcome {
    let ply = job.think.moves.len();
    let text = match result {
        GameResult::White => {
            if ply.is_multiple_of(2) {
                SCHOLAR_WHITE[ply / 2]
            } else {
                SCHOLAR_BLACK[ply / 2]
            }
        }
        GameResult::Black => {
            if ply.is_multiple_of(2) {
                FOOL_WHITE[ply / 2]
            } else {
                FOOL_BLACK[ply / 2]
            }
        }
        _ => SHUFFLE[ply],
    };
    SearchOutcome::Moved(reply_with(job, text, 100, Some(0)))
}

/// A policy that decides every game with `decide(ctx)`.
pub fn decide_by(decide: impl Fn(Ctx) -> GameResult + 'static) -> Policy {
    Box::new(move |job, ctx| scripted_result(job, decide(ctx)))
}

/// Answers every action like the application would.
pub struct Sim {
    pub t: Tournament,
    pub now: Instant,
    pub games: Vec<(u32, Game)>,
    pub states: Vec<mychess_app::tournament::SavedTournament>,
    pub notices: Vec<TournamentNotice>,
    pub prepares: Vec<([usize; 2], [bool; 2])>,
    pub searches: Vec<SearchJob>,
    pub cancels: Vec<u64>,
    pub shutdowns: usize,
    pub discarded: Vec<usize>,
    pub stop_ponders: usize,
    /// `(lane, participants, result, reason)` of the games the engines were told about.
    pub game_overs: Vec<(usize, [usize; 2], GameResult, String)>,
    pub policy: Policy,
    pub can_ponder: bool,
    /// Searches are held (the engine is thinking).
    pub hold: bool,
    pub held: Vec<Action>,
    /// `Prepare` is held too (engines starting).
    pub hold_prepare: bool,
    pub held_prepare: Vec<Action>,
}

impl Sim {
    pub fn new(config: TournamentConfig, policy: Policy) -> Sim {
        Sim::with(tournament(config), policy)
    }

    pub fn with(t: Tournament, policy: Policy) -> Sim {
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
            game_overs: Vec::new(),
            policy,
            can_ponder: false,
            hold: false,
            held: Vec::new(),
            hold_prepare: false,
            held_prepare: Vec::new(),
        }
    }

    fn ready(&self, sides: [usize; 2]) -> [SideReady; 2] {
        sides.map(|participant| SideReady {
            participant,
            engine_name: Some(format!("Engine {participant}")),
            can_ponder: self.can_ponder,
        })
    }

    fn ctx(&self) -> Ctx {
        let live = self.t.live().expect("a game is live");
        Ctx {
            number: live.number,
            white: live.white,
            black: live.black,
        }
    }

    /// Processes the actions and everything they cause.
    pub fn run(&mut self, first: Vec<Action>) {
        let mut queue: VecDeque<Action> = first.into();
        while let Some(action) = queue.pop_front() {
            let more = match action {
                Action::Prepare {
                    lane,
                    sides,
                    restart,
                    prune,
                    epoch,
                } => {
                    self.prepares.push((sides, restart));
                    if self.hold_prepare {
                        self.held_prepare.push(Action::Prepare {
                            lane,
                            sides,
                            restart,
                            prune,
                            epoch,
                        });
                        Vec::new()
                    } else {
                        let ready = self.ready(sides);
                        self.t.prepared(epoch, Ok(ready), self.now)
                    }
                }
                Action::Search(job) => {
                    self.searches.push((*job).clone());
                    if self.hold {
                        self.held.push(Action::Search(job));
                        Vec::new()
                    } else {
                        let ctx = self.ctx();
                        let outcome = (self.policy)(&job, ctx);
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
                Action::GameOver {
                    lane,
                    participants,
                    result,
                    reason,
                } => {
                    self.game_overs.push((lane, participants, result, reason));
                    Vec::new()
                }
                Action::SaveGame { number, game } => {
                    self.games.push((number, *game));
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

    pub fn start(&mut self) {
        let actions = self.t.begin(self.now);
        self.run(actions);
    }

    /// Answers the search that was held.
    pub fn answer_held(&mut self, outcome: SearchOutcome) {
        let Some(Action::Search(job)) = self.held.pop() else {
            panic!("no held search");
        };
        let actions = self.t.searched(job.epoch, outcome, self.now);
        self.run(actions);
    }

    /// Answers the preparation that was held.
    pub fn answer_prepare(&mut self, result: Result<(), PrepareFailure>) {
        let Some(Action::Prepare { sides, epoch, .. }) = self.held_prepare.pop() else {
            panic!("no held prepare");
        };
        let ready = result.map(|()| self.ready(sides));
        let actions = self.t.prepared(epoch, ready, self.now);
        self.run(actions);
    }

    /// The results of the games played, in the order they were played.
    pub fn results(&self) -> Vec<GameResult> {
        self.t.finished_games().iter().map(|g| g.result).collect()
    }

    /// PGN texts of the games saved.
    pub fn pgn_texts(&self) -> Vec<String> {
        self.games
            .iter()
            .map(|(_, g)| {
                mychess_pgn::write::game_to_string(g, &mychess_pgn::write::WriteOptions::default())
                    .expect("pgn")
            })
            .collect()
    }
}

pub fn dead() -> SearchOutcome {
    SearchOutcome::Dead(EngineFailure {
        kind: FailureKind::Crashed,
        exit_code: None,
        stderr_tail: Vec::new(),
    })
}

pub fn prepare_failure(participant: usize) -> PrepareFailure {
    PrepareFailure {
        participant,
        failure: EngineFailure {
            kind: FailureKind::Crashed,
            exit_code: None,
            stderr_tail: Vec::new(),
        },
    }
}

// ---------------------------------------------------------------------
// A scripted UCI player for the tests of the real pool
// ---------------------------------------------------------------------

/// How a scripted engine behaves.
#[derive(Debug, Clone)]
pub struct Scripted {
    /// Time to think before the `bestmove`.
    pub delay: Duration,
    /// Time to answer after a `ponderhit`.
    pub hit_delay: Duration,
    /// The pipe closes at the first `go`.
    pub crash_on_go: bool,
    /// `stop` is ignored.
    pub ignore_stop: bool,
    /// `bestmove` carries `ponder <reply>`.
    pub ponder: bool,
    /// Declares the `Ponder` option.
    pub declares_ponder: bool,
    /// Answers `isready` only after this long at the handshake.
    pub slow_start: Duration,
}

impl Default for Scripted {
    fn default() -> Scripted {
        Scripted {
            delay: Duration::ZERO,
            hit_delay: Duration::ZERO,
            crash_on_go: false,
            ignore_stop: false,
            ponder: false,
            declares_ponder: true,
            slow_start: Duration::ZERO,
        }
    }
}

pub type Log = Arc<Mutex<Vec<String>>>;

pub fn lines(log: &Log) -> Vec<String> {
    log.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn position_from(words: &[&str]) -> Position {
    let moves_at = words.iter().position(|w| *w == "moves");
    let head = &words[1..moves_at.unwrap_or(words.len())];
    let mut position = match head.first() {
        Some(&"fen") => Position::from_fen(&head[1..].join(" ")).unwrap_or_default(),
        _ => Position::new(),
    };
    if let Some(at) = moves_at {
        for text in &words[at + 1..] {
            match position.parse_uci(text) {
                Ok(m) if position.play(m).is_ok() => {}
                _ => break,
            }
        }
    }
    position
}

fn bestmove_line(position: &Position, ponder: bool) -> String {
    let Some(mv) = first_legal(position) else {
        return "bestmove (none)".to_owned();
    };
    let mut line = format!("bestmove {}", position.uci(mv));
    if ponder && let Ok(after) = position.after(mv) {
        let mut replies: Vec<String> = after
            .legal_moves()
            .into_iter()
            .map(|m| after.uci(m))
            .collect();
        replies.sort();
        if let Some(reply) = replies.first() {
            line.push_str(" ponder ");
            line.push_str(reply);
        }
    }
    line
}

async fn run_player(mut manual: ManualEngine, script: Scripted, log: Log) {
    let options: &[&str] = if script.declares_ponder {
        &[
            "option name Hash type spin default 16 min 1 max 1024",
            "option name Ponder type check default false",
        ]
    } else {
        &["option name Hash type spin default 16 min 1 max 1024"]
    };
    if !script.slow_start.is_zero() {
        tokio::time::sleep(script.slow_start).await;
    }
    let seen = manual.handshake("Scripted 1.0", options).await;
    log.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .extend(seen);
    let mut position = Position::new();
    let mut pondering: Option<Position> = None;
    let mut pending: Option<(tokio::time::Instant, Position)> = None;
    loop {
        let line = match pending.clone() {
            Some((until, found)) => {
                tokio::select! {
                    () = tokio::time::sleep_until(until) => {
                        pending = None;
                        manual.say("info depth 10 score cp 25").await;
                        manual.say(&bestmove_line(&found, script.ponder)).await;
                        continue;
                    }
                    line = manual.next_line() => line,
                }
            }
            None => manual.next_line().await,
        };
        let Some(line) = line else { return };
        log.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.clone());
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.first().copied() {
            Some("isready") => manual.say("readyok").await,
            Some("position") => position = position_from(&words),
            Some("go") => {
                if script.crash_on_go {
                    manual.close().await;
                    return;
                }
                if words.contains(&"ponder") {
                    pondering = Some(position.clone());
                } else {
                    pending = Some((tokio::time::Instant::now() + script.delay, position.clone()));
                }
            }
            Some("ponderhit") => {
                if let Some(found) = pondering.take() {
                    pending = Some((tokio::time::Instant::now() + script.hit_delay, found));
                }
            }
            Some("stop") if !script.ignore_stop => {
                let target = pondering.take().or(pending.take().map(|(_, p)| p));
                if let Some(found) = target {
                    manual.say(&bestmove_line(&found, false)).await;
                }
            }
            Some("quit") => return,
            _ => {}
        }
    }
}

/// A launcher of scripted engines chosen by the name in the configuration.
pub struct Fixture {
    pub launcher: Launcher,
    pub logs: Arc<Mutex<Vec<(String, Log)>>>,
}

impl Fixture {
    pub fn new(scripts: Vec<(&'static str, Scripted)>) -> Fixture {
        let logs: Arc<Mutex<Vec<(String, Log)>>> = Arc::default();
        let recorded = Arc::clone(&logs);
        let launcher: Launcher = Arc::new(move |config: EngineConfig| {
            let script = scripts
                .iter()
                .find(|(name, _)| *name == config.name)
                .map(|(_, s)| s.clone())
                .unwrap_or_default();
            let log: Log = Arc::default();
            recorded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((config.name.clone(), Arc::clone(&log)));
            Box::pin(async move {
                let (manual, engine, events) = ManualEngine::launch(config);
                tokio::spawn(run_player(manual, script, log));
                Ok((engine, events))
            })
        });
        Fixture { launcher, logs }
    }

    pub fn pool(&self) -> EnginePool {
        EnginePool::new(Arc::clone(&self.launcher))
    }

    pub fn launches(&self, name: &str) -> usize {
        self.logs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(n, _)| n == name)
            .count()
    }

    /// The lines received by the last launch of `name`.
    pub fn lines(&self, name: &str) -> Vec<String> {
        let logs = self.logs.lock().unwrap_or_else(PoisonError::into_inner);
        logs.iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, l)| lines(l))
            .unwrap_or_default()
    }
}

pub fn entry(name: &str) -> EngineEntry {
    EngineEntry {
        id: EngineId(1),
        name: name.to_owned(),
        command: "fake".into(),
        args: Vec::new(),
        working_dir: None,
        protocol: mychess_app::engines::Protocol::Uci,
        elo: None,
        url: None,
        last_used: None,
        options: Vec::new(),
        multipv: 1,
        low_priority: None,
    }
}

pub fn plan(white: (usize, &str), black: (usize, &str)) -> PreparePlan {
    let launch = |(participant, name): (usize, &str)| LaunchRequest {
        participant,
        entry: entry(name),
        options: Vec::new(),
        defaults: mychess_app::engines::RunDefaults::default(),
    };
    PreparePlan {
        lane: 0,
        sides: [white.0, black.0],
        restart: [false, false],
        prune_lane: false,
        opponents: [format!("P{}", white.0), format!("P{}", black.0)],
        launches: [launch(white), launch(black)],
    }
}
