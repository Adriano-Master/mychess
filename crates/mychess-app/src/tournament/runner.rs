//! The state machine of a tournament (spec M8-2, D1260-D1289).
//!
//! [`Tournament`] is pure: it never talks to an engine, reads a clock or
//! touches a file. Every call receives what happened (an [`Instant`], the
//! answer of a task) and returns the [`Action`]s the application must run.
//! Up to `concurrency` games run at once, one per lane (D2500); between the moves the runner waits
//! for the answer of the task that asked the engine (`prepared`,
//! `searched`), so a pause, a skip or an adjudication never blocks.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use mychess_core::{Color, Game, GameResult, Move, PartialDate, Position};
use mychess_pgn::write::{WriteOptions, game_to_string};

use super::adjudicate::TablebaseProbe;
use super::config::{
    ConfigError, OpeningSource, Participant, Scheduling, TimeControl, TournamentConfig,
};
use super::driver::{PonderJob, PrepareFailure, SearchJob, SearchOutcome, SideReady};
use super::game::{GameEnd, GameSetup, PlayedMove, Step, Termination, TournamentGame};
use super::openings::{Opening, OpeningPlan, OpeningSuite};
use super::rating::{RatingRow, los_matrix, rating_table};
use super::record::{PlayerInfo, RecordInput, build_game};
use super::resources::MAX_CONCURRENCY;
use super::schedule::{Bye, Pairing, build_schedule};
use super::standings::{Finished, StandingRow, cross_table, standings, winners};
use super::store::{STORE_VERSION, SavedGame, SavedLine, SavedStatus, SavedTournament, StoreError};
use super::timecheck::{TimeRow, TimeStats, TimedMove, time_report};
use crate::play::PlayBook;
use crate::play::PlayClock;

/// The most entries kept in the log.
const MAX_LOG: usize = 1000;
/// The most discarded games kept in the log with their PGN.
const MAX_DISCARDED: usize = 50;

/// Where the tournament is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Playing (also between two games).
    Running,
    /// A pause was asked; the move in progress finishes first (D1284).
    Pausing,
    /// Paused.
    Paused,
    /// Waiting for the user: a participant failed too often (D1269).
    Decision(usize),
    /// Stopped after a game (D1284).
    StoppedAfterGame,
    /// The schedule is over.
    Finished,
    /// Ended by the user; the games already played stay.
    Aborted,
}

/// What happened to a game of the schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameStatus {
    /// Waiting.
    Pending,
    /// Being played.
    Playing,
    /// Played.
    Played(Outcome),
    /// Skipped by the user (D1286).
    Skipped,
    /// Played and replaced by a redo; it does not count.
    Replaced(Outcome),
    /// A participant left (D1269).
    Withdrawn,
}

/// How a game that was played ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The result.
    pub result: GameResult,
    /// How it ended.
    pub termination: Termination,
    /// Half-moves played.
    pub plies: u32,
    /// The name of the opening.
    pub opening: Option<String>,
}

/// A game of the schedule and its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSlot {
    /// The pairing.
    pub pairing: Pairing,
    /// The state.
    pub status: GameStatus,
}

/// Why a participant failed (D1269).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    /// The process could not be started.
    StartFailed,
    /// The process died.
    Died,
    /// The process did not answer in time.
    Hung,
    /// The game was lost on time and the policy says to replay it.
    TimeLoss,
}

/// What the user decides after too many failures (D1269).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Try again.
    Retry,
    /// The participant leaves; the games left of it are dropped.
    Withdraw,
    /// Another engine takes its place (name, options and rating from the
    /// new participant).
    Replace(Box<Participant>),
}

/// Something that happened, for the log (D1283).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEvent {
    /// The tournament started.
    Started,
    /// A game started.
    GameStarted {
        /// The number of the game.
        number: u32,
        /// The attempt, from 0.
        attempt: u8,
    },
    /// A game ended and was saved.
    GameFinished {
        /// The number of the game.
        number: u32,
        /// The result.
        result: GameResult,
        /// How it ended.
        termination: Termination,
    },
    /// An engine failed.
    EngineFailed {
        /// The participant.
        participant: usize,
        /// What happened.
        kind: FailKind,
    },
    /// A broken game was thrown away; its PGN is kept here only.
    Discarded {
        /// The number of the game.
        number: u32,
        /// The PGN of what was played.
        pgn: String,
    },
    /// A participant left.
    Withdrawn(usize),
    /// An engine took the place of a participant.
    Replaced(usize),
    /// The suite of openings started again.
    SuiteWrapped {
        /// The game that needed it.
        number: u32,
    },
    /// A game was skipped.
    Skipped(u32),
    /// A game will be played again.
    Redo(u32),
    /// The tournament paused.
    Paused,
    /// The tournament went on.
    Resumed,
    /// The tournament stopped after a game.
    Stopped,
    /// The tournament was ended by the user.
    Aborted,
    /// The schedule is over.
    Finished,
}

/// What the application must do after a call.
#[derive(Debug, Clone)]
pub enum Action {
    /// Start the engines of a game and send a new game (`PreparePlan`).
    Prepare {
        /// The lane that plays the game (D2500).
        lane: usize,
        /// `[white, black]`.
        sides: [usize; 2],
        /// Start the engine again even if it is alive.
        restart: [bool; 2],
        /// End the engines of the lane that do not play this game (more than
        /// one lane, D2506).
        prune: bool,
        /// The epoch of the answer.
        epoch: u64,
    },
    /// Ask an engine for a move.
    Search(Box<SearchJob>),
    /// Stop the search of this epoch and the earlier ones, in one lane.
    Cancel {
        /// The lane.
        lane: usize,
        /// The epoch.
        epoch: u64,
    },
    /// Stop the engines of a lane that are pondering.
    StopPonder {
        /// The lane.
        lane: usize,
    },
    /// Tell the engines of a lane the game is over (D2524).
    GameOver {
        /// The lane.
        lane: usize,
        /// `[white, black]`.
        participants: [usize; 2],
        /// The result.
        result: GameResult,
        /// Why the game ended: the comment of the last move, or the
        /// `Termination` tag.
        reason: String,
    },
    /// Append a game to the PGN of the tournament (D1279).
    SaveGame {
        /// The number in the schedule.
        number: u32,
        /// The game.
        game: Box<Game>,
    },
    /// Save the state of the tournament (D1285).
    SaveState(Box<SavedTournament>),
    /// End the engine of a participant.
    Discard(usize),
    /// End every engine.
    Shutdown,
    /// Tell the user.
    Notice(TournamentNotice),
}

/// A remark for the user (translated by the front-end from the variant).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TournamentNotice {
    /// The tournament started.
    Started,
    /// The schedule is over; the winners (D1282).
    Finished {
        /// The participants tied for the first place.
        winners: Vec<usize>,
    },
    /// Paused.
    Paused,
    /// Went on.
    Resumed,
    /// Stopped after the game.
    StoppedAfterGame,
    /// Ended by the user.
    Aborted,
    /// An engine failed.
    EngineFailed {
        /// The participant.
        participant: usize,
        /// What happened.
        kind: FailKind,
    },
    /// A participant failed too often: the tournament waits for a decision.
    TooManyFailures {
        /// The participant.
        participant: usize,
    },
    /// The action does not apply now.
    NotApplicable,
    /// The configuration is refused.
    InvalidConfig(ConfigError),
    /// A game could not be written to the PGN of the tournament.
    GameNotSaved,
    /// The state could not be written.
    StateNotSaved,
    /// The extra destination is read only; the games go to the PGN only.
    DestinationReadOnly,
    /// The tournament cannot start while a game against an engine is on.
    PlayBusy,
    /// A tournament is already on.
    AlreadyRunning,
    /// An annotation or a training mode is on: the tournament cannot start
    /// and an annotation cannot start while the tournament lives (D1311).
    Busy,
    /// Engines of the tournament are not in the list of engines.
    MissingEngines(Vec<String>),
    /// The time control cannot be played by an XBoard participant (D2522).
    UnsupportedTime(String),
    /// The games at once were brought down to what the machine allows
    /// (D2503).
    ConcurrencyReduced {
        /// What was asked.
        from: u32,
        /// What will run.
        to: u32,
    },
    /// A game could not be copied to the extra destination (D2517).
    DestinationFailed,
    /// The game was copied to the file of the destination slot, but the
    /// database could not be opened again (D2519).
    DestinationNotReopened,
    /// The extra destination is not the one asked at the start (the slot
    /// closed or changed, or a resumed tournament: D2519).
    DestinationUnavailable,
    /// The book could not be read; the games go on without it.
    BookUnreadable,
    /// The suite of openings could not be read.
    SuiteUnreadable,
    /// A saved tournament could not be read.
    ResumeFailed,
    /// The tablebase rule is on, but no Syzygy table could be opened: the
    /// games go on without it (D2538).
    TablebaseUnavailable,
    /// The table was exported.
    Exported,
    /// The table could not be exported.
    ExportFailed,
}

impl TournamentNotice {
    /// The Fluent key of the message (to create in the GUI phase).
    pub fn message_key(&self) -> &'static str {
        match self {
            TournamentNotice::Started => "tournament-notice-started",
            TournamentNotice::Finished { .. } => "tournament-notice-finished",
            TournamentNotice::Paused => "tournament-notice-paused",
            TournamentNotice::Resumed => "tournament-notice-resumed",
            TournamentNotice::StoppedAfterGame => "tournament-notice-stopped",
            TournamentNotice::Aborted => "tournament-notice-aborted",
            TournamentNotice::EngineFailed { .. } => "tournament-notice-engine-failed",
            TournamentNotice::TooManyFailures { .. } => "tournament-notice-too-many-failures",
            TournamentNotice::NotApplicable => "tournament-notice-not-applicable",
            TournamentNotice::InvalidConfig(error) => error.message_key(),
            TournamentNotice::GameNotSaved => "tournament-notice-game-not-saved",
            TournamentNotice::StateNotSaved => "tournament-notice-state-not-saved",
            TournamentNotice::DestinationReadOnly => "tournament-notice-destination-read-only",
            TournamentNotice::PlayBusy => "tournament-notice-play-busy",
            TournamentNotice::AlreadyRunning => "tournament-notice-already-running",
            TournamentNotice::Busy => "tournament-notice-busy",
            TournamentNotice::MissingEngines(_) => "tournament-notice-missing-engines",
            TournamentNotice::UnsupportedTime(_) => "tournament-notice-unsupported-time",
            TournamentNotice::ConcurrencyReduced { .. } => "tournament-notice-concurrency-reduced",
            TournamentNotice::DestinationFailed => "tournament-notice-destination-failed",
            TournamentNotice::DestinationNotReopened => {
                "tournament-notice-destination-not-reopened"
            }
            TournamentNotice::DestinationUnavailable => "tournament-notice-destination-unavailable",
            TournamentNotice::BookUnreadable => "tournament-notice-book-unreadable",
            TournamentNotice::SuiteUnreadable => "tournament-notice-suite-unreadable",
            TournamentNotice::ResumeFailed => "tournament-notice-resume-failed",
            TournamentNotice::TablebaseUnavailable => "tournament-notice-tablebase-unavailable",
            TournamentNotice::Exported => "tournament-notice-exported",
            TournamentNotice::ExportFailed => "tournament-notice-export-failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Awaiting {
    Prepare,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    Adjudicate(GameResult),
    Skip,
    Abort,
}

/// The game being played in one lane (D2500).
#[derive(Debug)]
struct Run {
    slot: usize,
    attempt: u8,
    game: Option<TournamentGame>,
    opening: Opening,
    opening_is_book: bool,
    awaiting: Option<(Awaiting, u64)>,
    then: Option<Then>,
    ready: Option<[SideReady; 2]>,
    /// The game waits for a decision about a participant before it starts
    /// again.
    retry_pending: bool,
}

/// The game being played, for the live tab (D1283).
#[derive(Debug, Clone, Copy)]
pub struct LiveView<'a> {
    /// The lane that plays it (D2500).
    pub lane: usize,
    /// The number of the game.
    pub number: u32,
    /// The participant with White.
    pub white: usize,
    /// The participant with Black.
    pub black: usize,
    /// The attempt, from 0.
    pub attempt: u8,
    /// The position the game started from, so a front-end can write the
    /// moves in SAN (`None` until the engines are ready; M8-2b, D1362).
    pub start: Option<&'a Position>,
    /// The position now (`None` until the engines are ready).
    pub position: Option<&'a Position>,
    /// The moves with notes and times.
    pub moves: &'a [PlayedMove],
    /// The clock (`shown` at the instant of the tick).
    pub clock: Option<&'a PlayClock>,
}

/// A game in progress in one lane, for the selector of the live tab (D2508).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneView {
    /// The lane.
    pub lane: usize,
    /// The number of the game.
    pub number: u32,
    /// The participant with White.
    pub white: usize,
    /// The participant with Black.
    pub black: usize,
    /// The lane the live tab shows.
    pub watched: bool,
}

/// A tournament between engines.
#[derive(Debug)]
pub struct Tournament {
    config: TournamentConfig,
    slug: String,
    created: PartialDate,
    byes: Vec<Bye>,
    slots: Vec<GameSlot>,
    queue: VecDeque<usize>,
    plan: OpeningPlan,
    book: Option<PlayBook>,
    tablebase: Option<Arc<dyn TablebaseProbe>>,
    withdrawn: Vec<bool>,
    failures: Vec<u32>,
    /// `needs_restart[lane][participant]` (D2506).
    needs_restart: Vec<Vec<bool>>,
    engine_names: Vec<Option<String>>,
    pair_lines: HashMap<u32, Vec<Move>>,
    phase: Phase,
    /// One entry per lane (D2500).
    runs: Vec<Option<Run>>,
    /// The lane the live tab shows and the controls of "the game in
    /// progress" act on (D2507).
    watch: usize,
    /// Games taken back by an abort, in the order they go to the queue.
    released: Vec<usize>,
    time_stats: Vec<TimeStats>,
    epoch: u64,
    stop_after: bool,
    pause_requested: bool,
    decision: Option<usize>,
    log: Vec<LogEvent>,
    /// The revision of the last state handed out (D1306).
    revision: AtomicU64,
}

fn color_of(pairing: &Pairing, participant: usize) -> Color {
    if pairing.white == participant {
        Color::White
    } else {
        Color::Black
    }
}

/// The first epoch of a new tournament. Epochs carry a generation in the high
/// bits that grows with every tournament of the process, so the late answer of
/// a task of an ended tournament never matches a request of the next one
/// (D2505).
fn first_epoch() -> u64 {
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    ((GENERATION.fetch_add(1, Ordering::Relaxed) % 0x7FFF_FFFF) + 1) << 32
}

/// The lanes of a configuration (`concurrency` is validated to 1..=16).
fn lane_count(config: &TournamentConfig) -> usize {
    usize::try_from(config.concurrency.clamp(1, MAX_CONCURRENCY)).unwrap_or(1)
}

impl Tournament {
    /// A tournament that has not started. `book` and `suite` are what the
    /// configuration asks for, already read (D1271, D1272).
    ///
    /// # Errors
    ///
    /// [`ConfigError`] if the configuration is not valid.
    pub fn new(
        config: TournamentConfig,
        slug: &str,
        today: PartialDate,
        book: Option<PlayBook>,
        suite: Option<OpeningSuite>,
    ) -> Result<Tournament, ConfigError> {
        config.validate()?;
        let n = config.participants.len();
        let schedule = build_schedule(n, config.scheduling, config.cycles, config.tested);
        let slots: Vec<GameSlot> = schedule
            .pairings
            .iter()
            .map(|&pairing| GameSlot {
                pairing,
                status: GameStatus::Pending,
            })
            .collect();
        let queue = (0..slots.len()).collect();
        let plan = OpeningPlan::new(&config, suite);
        let lanes = lane_count(&config);
        Ok(Tournament {
            slug: slug.to_owned(),
            created: today,
            byes: schedule.byes,
            slots,
            queue,
            plan,
            book,
            tablebase: None,
            withdrawn: vec![false; n],
            failures: vec![0; n],
            needs_restart: vec![vec![false; n]; lanes],
            engine_names: vec![None; n],
            pair_lines: HashMap::new(),
            phase: Phase::Running,
            runs: (0..lanes).map(|_| None).collect(),
            watch: 0,
            released: Vec::new(),
            time_stats: vec![TimeStats::default(); n],
            epoch: first_epoch(),
            stop_after: false,
            pause_requested: false,
            decision: None,
            log: Vec::new(),
            revision: AtomicU64::new(0),
            config,
        })
    }

    /// Rebuilds a tournament from its saved state (D1285). The game that
    /// was being played is discarded and will be played again.
    ///
    /// # Errors
    ///
    /// [`StoreError`] if the state does not fit its configuration.
    #[allow(clippy::too_many_lines)] // one constructor that rebuilds every part
    pub fn resume(
        saved: SavedTournament,
        book: Option<PlayBook>,
        suite: Option<OpeningSuite>,
    ) -> Result<Tournament, StoreError> {
        saved.check()?;
        let n = saved.config.participants.len();
        let outcome_of = |g: &SavedGame| Outcome {
            result: GameResult::from_str_lenient(g.result.as_deref().unwrap_or("*")),
            termination: g
                .termination
                .as_deref()
                .and_then(Termination::from_tag)
                .unwrap_or(Termination::Normal),
            plies: g.plies,
            opening: g.opening.clone(),
        };
        let slots: Vec<GameSlot> = saved
            .games
            .iter()
            .map(|g| GameSlot {
                pairing: Pairing {
                    number: g.number,
                    cycle: g.cycle,
                    round: g.round,
                    white: g.white,
                    black: g.black,
                    group: g.group,
                },
                status: match g.status {
                    SavedStatus::Pending => GameStatus::Pending,
                    SavedStatus::Played => GameStatus::Played(outcome_of(g)),
                    SavedStatus::Skipped => GameStatus::Skipped,
                    SavedStatus::Replaced => GameStatus::Replaced(outcome_of(g)),
                    SavedStatus::Withdrawn => GameStatus::Withdrawn,
                },
            })
            .collect();
        // The saved queue, without repeats and without games that are not
        // waiting; a waiting game the file forgot goes to the end, so no game
        // is lost on the way (D1312).
        let mut queue: VecDeque<usize> = VecDeque::new();
        let mut queued = vec![false; slots.len()];
        for &i in &saved.queue {
            let waiting = slots
                .get(i)
                .is_some_and(|s| s.status == GameStatus::Pending);
            if waiting
                && let Some(flag) = queued.get_mut(i)
                && !*flag
            {
                *flag = true;
                queue.push_back(i);
            }
        }
        for (i, slot) in slots.iter().enumerate() {
            if slot.status == GameStatus::Pending && !queued.get(i).copied().unwrap_or(false) {
                queue.push_back(i);
            }
        }
        let mut pair_lines = HashMap::new();
        for line in &saved.book_lines {
            let mut position = Position::new();
            let mut moves = Vec::new();
            for text in &line.moves {
                let Ok(mv) = position.parse_uci(text) else {
                    break;
                };
                if position.play(mv).is_err() {
                    break;
                }
                moves.push(mv);
            }
            pair_lines.insert(line.group, moves);
        }
        let created = saved.created.parse().unwrap_or_default();
        let plan = OpeningPlan::new(&saved.config, suite);
        let finished = saved.finished;
        let lanes = lane_count(&saved.config);
        let mut time_stats = saved.time_stats;
        time_stats.resize(n, TimeStats::default());
        Ok(Tournament {
            slug: saved.slug,
            created,
            byes: Vec::new(),
            slots,
            queue,
            plan,
            book,
            tablebase: None,
            withdrawn: saved.withdrawn,
            failures: saved.failures,
            needs_restart: vec![vec![true; n]; lanes],
            engine_names: vec![None; n],
            pair_lines,
            phase: if finished {
                Phase::Finished
            } else {
                Phase::Paused
            },
            runs: (0..lanes).map(|_| None).collect(),
            watch: 0,
            released: Vec::new(),
            time_stats,
            epoch: first_epoch(),
            stop_after: false,
            pause_requested: !finished,
            decision: None,
            log: Vec::new(),
            revision: AtomicU64::new(saved.revision),
            config: saved.config,
        })
    }

    /// Gives the tournament a tablebase (D1277).
    pub fn set_tablebase(&mut self, probe: Arc<dyn TablebaseProbe>) {
        self.tablebase = Some(probe);
    }

    // ---- reading ----

    /// The configuration.
    pub fn config(&self) -> &TournamentConfig {
        &self.config
    }

    /// The base of the file names.
    pub fn slug(&self) -> &str {
        &self.slug
    }

    /// Where the tournament is.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The games of the schedule, in order, with their state.
    pub fn slots(&self) -> &[GameSlot] {
        &self.slots
    }

    /// The rests of the carousel.
    pub fn byes(&self) -> &[Bye] {
        &self.byes
    }

    /// The numbers of the games waiting, in the order they will be played.
    pub fn queue_numbers(&self) -> Vec<u32> {
        self.queue
            .iter()
            .filter_map(|&i| self.slots.get(i))
            .map(|s| s.pairing.number)
            .collect()
    }

    /// Games played and games that still count (D1283: the progress bar).
    pub fn progress(&self) -> (usize, usize) {
        let counts = |s: &&GameSlot| {
            matches!(
                s.status,
                GameStatus::Pending | GameStatus::Playing | GameStatus::Played(_)
            )
        };
        let total = self.slots.iter().filter(counts).count();
        let done = self
            .slots
            .iter()
            .filter(|s| matches!(s.status, GameStatus::Played(_)))
            .count();
        (done, total)
    }

    /// The failures of each participant.
    pub fn failures(&self) -> &[u32] {
        &self.failures
    }

    /// The participants that left.
    pub fn withdrawn(&self) -> &[bool] {
        &self.withdrawn
    }

    /// The log.
    pub fn log(&self) -> &[LogEvent] {
        &self.log
    }

    /// The games that count for the table, in the order of the schedule
    /// (never the order they ended in, D2504).
    pub fn finished_games(&self) -> Vec<Finished> {
        self.slots
            .iter()
            .filter_map(|s| match &s.status {
                GameStatus::Played(o) => Some(Finished {
                    number: s.pairing.number,
                    white: s.pairing.white,
                    black: s.pairing.black,
                    result: o.result,
                }),
                _ => None,
            })
            .collect()
    }

    /// The table of standings (D1282).
    pub fn standings(&self) -> Vec<StandingRow> {
        standings(
            self.config.participants.len(),
            &self.withdrawn,
            &self.finished_games(),
        )
    }

    /// The winners so far (D1282).
    pub fn winners(&self) -> Vec<usize> {
        winners(&self.standings())
    }

    /// The crosstable.
    pub fn cross_table(&self) -> Vec<Vec<Vec<(u32, super::standings::GameScore)>>> {
        cross_table(self.config.participants.len(), &self.finished_games())
    }

    /// The table of Elo and LOS (D2513).
    pub fn ratings(&self) -> Vec<RatingRow> {
        let elos: Vec<Option<u16>> = self.config.participants.iter().map(|p| p.elo).collect();
        rating_table(&elos, &self.withdrawn, &self.finished_games())
    }

    /// The likelihood of superiority of every participant over every other
    /// (D2512).
    pub fn los_matrix(&self) -> Vec<Vec<Option<f64>>> {
        los_matrix(self.config.participants.len(), &self.finished_games())
    }

    /// The report of time (D2514-D2516).
    pub fn time_report(&self) -> Vec<TimeRow> {
        time_report(&self.time_stats)
    }

    /// Sets the lanes that really run, before the tournament begins: the
    /// application brings the number asked in the configuration down to what
    /// the machine takes without touching the configuration, so a saved
    /// tournament keeps what the user asked for (D2503, D2509).
    pub fn set_lanes(&mut self, lanes: usize) {
        if self.any_active() {
            return;
        }
        let lanes = lanes.clamp(1, MAX_CONCURRENCY as usize);
        let row = self.needs_restart.first().cloned().unwrap_or_default();
        self.runs = (0..lanes).map(|_| None).collect();
        self.needs_restart = vec![row; lanes];
        self.watch = 0;
    }

    /// The number of lanes (D2500).
    pub fn lane_count(&self) -> usize {
        self.runs.len()
    }

    fn run_ref(&self, lane: usize) -> Option<&Run> {
        self.runs.get(lane)?.as_ref()
    }

    fn run_mut(&mut self, lane: usize) -> Option<&mut Run> {
        self.runs.get_mut(lane)?.as_mut()
    }

    fn active_lanes(&self) -> Vec<usize> {
        self.runs
            .iter()
            .enumerate()
            .filter_map(|(lane, run)| run.as_ref().map(|_| lane))
            .collect()
    }

    fn any_active(&self) -> bool {
        self.runs.iter().any(Option::is_some)
    }

    fn any_awaiting(&self) -> bool {
        self.runs.iter().flatten().any(|run| run.awaiting.is_some())
    }

    fn free_lane(&self) -> Option<usize> {
        self.runs.iter().position(Option::is_none)
    }

    /// The lane whose wait is for `epoch` and `kind` (D2505).
    fn lane_awaiting(&self, kind: Awaiting, epoch: u64) -> Option<usize> {
        self.runs.iter().position(|run| {
            run.as_ref()
                .is_some_and(|r| r.awaiting == Some((kind, epoch)))
        })
    }

    /// The lane the live tab shows: the watched one if it has a game, else
    /// the first lane with one.
    fn watched_lane(&self) -> Option<usize> {
        if self.run_ref(self.watch).is_some() {
            Some(self.watch)
        } else {
            self.runs.iter().position(Option::is_some)
        }
    }

    /// Chooses the lane the live tab shows (D2508); a lane that does not
    /// exist is ignored.
    pub fn watch(&mut self, lane: usize) {
        if lane < self.runs.len() {
            self.watch = lane;
        }
    }

    /// The games in progress, one per busy lane (D2508).
    pub fn lanes(&self) -> Vec<LaneView> {
        let watched = self.watched_lane();
        self.runs
            .iter()
            .enumerate()
            .filter_map(|(lane, run)| {
                let slot = self.slots.get(run.as_ref()?.slot)?;
                Some(LaneView {
                    lane,
                    number: slot.pairing.number,
                    white: slot.pairing.white,
                    black: slot.pairing.black,
                    watched: watched == Some(lane),
                })
            })
            .collect()
    }

    /// The game being played in the watched lane.
    pub fn live(&self) -> Option<LiveView<'_>> {
        let lane = self.watched_lane()?;
        let run = self.run_ref(lane)?;
        let slot = self.slots.get(run.slot)?;
        Some(LiveView {
            lane,
            number: slot.pairing.number,
            white: slot.pairing.white,
            black: slot.pairing.black,
            attempt: run.attempt,
            start: run.game.as_ref().map(TournamentGame::start),
            position: run.game.as_ref().map(TournamentGame::position),
            moves: run.game.as_ref().map_or(&[], TournamentGame::played),
            clock: run.game.as_ref().map(TournamentGame::clock),
        })
    }

    /// The epoch of the last request; a cancel of it stops the search.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The name an engine announced, by participant.
    pub fn engine_name(&self, participant: usize) -> Option<&str> {
        self.engine_names.get(participant)?.as_deref()
    }

    /// The state to save (D1285). The games being played are saved as
    /// pending.
    pub fn to_saved(&self) -> SavedTournament {
        let games = self
            .slots
            .iter()
            .map(|slot| {
                let (status, outcome) = match &slot.status {
                    GameStatus::Pending | GameStatus::Playing => (SavedStatus::Pending, None),
                    GameStatus::Played(o) => (SavedStatus::Played, Some(o)),
                    GameStatus::Skipped => (SavedStatus::Skipped, None),
                    GameStatus::Replaced(o) => (SavedStatus::Replaced, Some(o)),
                    GameStatus::Withdrawn => (SavedStatus::Withdrawn, None),
                };
                SavedGame {
                    number: slot.pairing.number,
                    cycle: slot.pairing.cycle,
                    round: slot.pairing.round,
                    white: slot.pairing.white,
                    black: slot.pairing.black,
                    group: slot.pairing.group,
                    status,
                    result: outcome.map(|o| o.result.as_str().to_owned()),
                    termination: outcome.map(|o| o.termination.tag().to_owned()),
                    plies: outcome.map_or(0, |o| o.plies),
                    opening: outcome.and_then(|o| o.opening.clone()),
                }
            })
            .collect();
        // The games being played come back first, in the order of the
        // schedule (D2509).
        let mut playing: Vec<usize> = self.runs.iter().flatten().map(|r| r.slot).collect();
        playing.sort_unstable();
        let mut queue: Vec<usize> = playing
            .into_iter()
            .chain(self.queue.iter().copied())
            .collect();
        queue.dedup();
        let mut lines: Vec<SavedLine> = self
            .pair_lines
            .iter()
            .map(|(&group, moves)| {
                let mut position = Position::new();
                let mut texts = Vec::new();
                for &mv in moves {
                    texts.push(position.uci(mv));
                    if position.play(mv).is_err() {
                        break;
                    }
                }
                SavedLine {
                    group,
                    moves: texts,
                }
            })
            .collect();
        lines.sort_by_key(|l| l.group);
        SavedTournament {
            version: STORE_VERSION,
            slug: self.slug.clone(),
            pgn_file: format!("{}.pgn", self.slug),
            created: self.created.to_string(),
            config: self.config.clone(),
            games,
            queue,
            failures: self.failures.clone(),
            withdrawn: self.withdrawn.clone(),
            book_lines: lines,
            time_stats: self.time_stats.clone(),
            finished: self.phase == Phase::Finished,
            // Capped: the file is TOML and keeps signed 64-bit integers.
            revision: self
                .revision
                .fetch_add(1, Ordering::SeqCst)
                .saturating_add(1)
                .min(u64::MAX >> 1),
        }
    }

    /// How many games with the number of the slot `ix` come before it: zero
    /// for a game of the schedule, `k` for the `k`-th redo (D1312).
    fn redo_count(&self, ix: usize) -> u32 {
        let Some(number) = self.slots.get(ix).map(|s| s.pairing.number) else {
            return 0;
        };
        let before = self
            .slots
            .iter()
            .take(ix)
            .filter(|s| s.pairing.number == number)
            .count();
        u32::try_from(before).unwrap_or(u32::MAX)
    }

    fn save_action(&self) -> Action {
        Action::SaveState(Box::new(self.to_saved()))
    }

    fn note(&mut self, event: LogEvent) {
        if self.log.len() >= MAX_LOG {
            self.log.remove(0);
        }
        self.log.push(event);
    }

    fn participant_info(&self, participant: usize, ready: Option<&SideReady>) -> PlayerInfo {
        let p = self.config.participants.get(participant);
        PlayerInfo {
            name: p.map_or_else(String::new, |p| p.name.clone()),
            elo: p.and_then(|p| p.elo),
            engine_name: ready
                .and_then(|r| r.engine_name.clone())
                .or_else(|| self.engine_names.get(participant).cloned().flatten()),
        }
    }

    // ---- starting ----

    /// Starts the tournament: the first games begin (D1285: the state is
    /// saved at once).
    pub fn begin(&mut self, now: Instant) -> Vec<Action> {
        self.phase = Phase::Running;
        self.note(LogEvent::Started);
        let mut actions = vec![
            Action::Notice(TournamentNotice::Started),
            self.save_action(),
        ];
        actions.extend(self.start_next(now));
        actions
    }

    /// Whether the games of a pair must not run together (D2504): the second
    /// game of a pair repeats the book line of the first.
    fn serializes_groups(&self) -> bool {
        self.config.opening == OpeningSource::Book && self.config.pair_openings
    }

    /// The next game that can start now, taken out of the queue. Games of a
    /// participant that left are marked as such on the way.
    fn next_eligible(&mut self) -> Option<usize> {
        let serialize = self.serializes_groups();
        let in_flight: Vec<u32> = self
            .runs
            .iter()
            .flatten()
            .filter_map(|r| self.slots.get(r.slot))
            .map(|s| s.pairing.group)
            .collect();
        let mut pos = 0;
        while pos < self.queue.len() {
            let Some(&ix) = self.queue.get(pos) else {
                break;
            };
            let Some(slot) = self.slots.get_mut(ix) else {
                self.queue.remove(pos);
                continue;
            };
            if slot.status != GameStatus::Pending {
                self.queue.remove(pos);
                continue;
            }
            let (w, b) = (slot.pairing.white, slot.pairing.black);
            if self.withdrawn.get(w).copied().unwrap_or(false)
                || self.withdrawn.get(b).copied().unwrap_or(false)
            {
                slot.status = GameStatus::Withdrawn;
                self.queue.remove(pos);
                continue;
            }
            if serialize && in_flight.contains(&slot.pairing.group) {
                pos += 1;
                continue;
            }
            self.queue.remove(pos);
            return Some(ix);
        }
        None
    }

    /// Goes on after a game or a decision: fills the free lanes with the
    /// next games, in lane order and queue order (D2504).
    fn start_next(&mut self, now: Instant) -> Vec<Action> {
        if let Some(participant) = self.decision {
            self.phase = Phase::Decision(participant);
            return Vec::new();
        }
        if self.stop_after {
            if self.any_active() {
                // The games in progress end first (D2507).
                return Vec::new();
            }
            self.stop_after = false;
            self.phase = Phase::StoppedAfterGame;
            self.note(LogEvent::Stopped);
            return vec![
                Action::Notice(TournamentNotice::StoppedAfterGame),
                self.save_action(),
                Action::Shutdown,
            ];
        }
        if self.pause_requested {
            return self.settle_pause();
        }
        let mut actions = Vec::new();
        while let Some(lane) = self.free_lane() {
            let Some(ix) = self.next_eligible() else {
                break;
            };
            self.phase = Phase::Running;
            actions.extend(self.begin_game(lane, ix, 0, now));
        }
        if actions.is_empty() && !self.any_active() {
            return self.finish_tournament();
        }
        actions
    }

    /// The pause is complete when no lane waits for an engine (D2507).
    fn settle_pause(&mut self) -> Vec<Action> {
        // A question to the user stays the phase until it is answered.
        if matches!(self.phase, Phase::Decision(_)) {
            return Vec::new();
        }
        if self.any_awaiting() {
            if self.phase == Phase::Running {
                self.phase = Phase::Pausing;
            }
            return Vec::new();
        }
        if self.phase == Phase::Paused {
            return Vec::new();
        }
        self.phase = Phase::Paused;
        self.note(LogEvent::Paused);
        vec![Action::Notice(TournamentNotice::Paused)]
    }

    fn finish_tournament(&mut self) -> Vec<Action> {
        self.phase = Phase::Finished;
        self.note(LogEvent::Finished);
        vec![
            Action::Notice(TournamentNotice::Finished {
                winners: self.winners(),
            }),
            self.save_action(),
            Action::Shutdown,
        ]
    }

    fn begin_game(&mut self, lane: usize, ix: usize, attempt: u8, _now: Instant) -> Vec<Action> {
        let Some(slot) = self.slots.get_mut(ix) else {
            return Vec::new();
        };
        slot.status = GameStatus::Playing;
        let pairing = slot.pairing;
        let chosen = self.plan.opening_for(&pairing);
        // Once per game: neither a retry nor a redo of the same game reports
        // it again (D1316).
        let reported = self
            .log
            .iter()
            .any(|e| matches!(e, LogEvent::SuiteWrapped { number } if *number == pairing.number));
        if chosen.wrapped && !reported {
            self.note(LogEvent::SuiteWrapped {
                number: pairing.number,
            });
        }
        let line = (self.config.opening == OpeningSource::Book && self.config.pair_openings)
            .then(|| self.pair_lines.get(&pairing.group).cloned())
            .flatten();
        let (opening, opening_is_book) = match line {
            Some(moves) => (
                Opening {
                    moves,
                    ..chosen.opening
                },
                true,
            ),
            None => (chosen.opening, false),
        };
        self.note(LogEvent::GameStarted {
            number: pairing.number,
            attempt,
        });
        self.epoch += 1;
        let restart = [pairing.white, pairing.black].map(|p| {
            self.config.restart_each_game
                || self
                    .needs_restart
                    .get(lane)
                    .and_then(|row| row.get(p))
                    .copied()
                    .unwrap_or(false)
        });
        for p in [pairing.white, pairing.black] {
            if let Some(flag) = self
                .needs_restart
                .get_mut(lane)
                .and_then(|row| row.get_mut(p))
            {
                *flag = false;
            }
        }
        let epoch = self.epoch;
        if let Some(place) = self.runs.get_mut(lane) {
            *place = Some(Run {
                slot: ix,
                attempt,
                game: None,
                opening,
                opening_is_book,
                awaiting: Some((Awaiting::Prepare, epoch)),
                then: None,
                ready: None,
                retry_pending: false,
            });
        }
        vec![Action::Prepare {
            lane,
            sides: [pairing.white, pairing.black],
            restart,
            prune: self.runs.len() > 1,
            epoch,
        }]
    }

    // ---- answers of the tasks ----

    /// The engines of the game are ready (or one could not be started).
    pub fn prepared(
        &mut self,
        epoch: u64,
        result: Result<[SideReady; 2], PrepareFailure>,
        now: Instant,
    ) -> Vec<Action> {
        let Some(lane) = self.lane_awaiting(Awaiting::Prepare, epoch) else {
            return Vec::new();
        };
        let Some(run) = self.run_mut(lane) else {
            return Vec::new();
        };
        run.awaiting = None;
        if let Some(then) = run.then.take() {
            return self.apply_then(lane, then, now);
        }
        match result {
            Err(failure) => {
                self.engine_failed(lane, failure.participant, FailKind::StartFailed, now)
            }
            Ok(ready) => {
                for side in &ready {
                    if let Some(name) = self.engine_names.get_mut(side.participant) {
                        name.clone_from(&side.engine_name);
                    }
                }
                self.build_game(lane, ready, now);
                self.step(lane, now)
            }
        }
    }

    fn build_game(&mut self, lane: usize, ready: [SideReady; 2], now: Instant) {
        let Some(run) = self.run_ref(lane) else {
            return;
        };
        let Some(slot) = self.slots.get(run.slot) else {
            return;
        };
        let setup = GameSetup {
            opening: run.opening.clone(),
            opening_is_book: run.opening_is_book,
            time: self.config.time,
            leeway_percent: self.config.leeway_percent,
            margin: Duration::from_millis(u64::from(self.config.margin_ms)),
            depth_limit: Duration::from_secs(u64::from(self.config.depth_limit_secs)),
            claim_draws: self.config.claim_draws,
            adjudication: self.config.adjudication,
            book: if run.opening_is_book || !self.plan.uses_book() {
                None
            } else {
                self.book.clone()
            },
            book_ply_limit: self.config.book_ply_limit,
            ponder: [
                self.config.ponder && ready[0].can_ponder,
                self.config.ponder && ready[1].can_ponder,
            ],
            seed: self.config.seed,
            number: slot.pairing.number,
            tablebase: self.tablebase.clone(),
        };
        let game = TournamentGame::new(setup, now);
        if let Some(run) = self.run_mut(lane) {
            run.game = Some(game);
            run.ready = Some(ready);
        }
    }

    /// Asks for the next move of a lane, or ends its game.
    fn step(&mut self, lane: usize, now: Instant) -> Vec<Action> {
        // A game the last move already decided is finished and saved, not
        // frozen in a pause (D1314).
        let decided = self
            .run_ref(lane)
            .and_then(|r| r.game.as_ref())
            .and_then(|g| g.end().cloned());
        if let Some(end) = decided {
            return self.finish_game(lane, &end, now);
        }
        if self.pause_requested {
            self.forget_ponder(lane);
            let mut actions = vec![Action::StopPonder { lane }];
            actions.extend(self.settle_pause());
            return actions;
        }
        if !matches!(self.phase, Phase::Decision(_)) {
            self.phase = Phase::Running;
        }
        let Some(run) = self.run_ref(lane) else {
            return Vec::new();
        };
        let Some(slot) = self.slots.get(run.slot) else {
            return Vec::new();
        };
        let pairing = slot.pairing;
        let Some(game) = self.run_mut(lane).and_then(|r| r.game.as_mut()) else {
            return Vec::new();
        };
        match game.advance(now) {
            Step::Over(end) => self.finish_game(lane, &end, now),
            Step::Think(turn) => {
                let who = |side: Color| match side {
                    Color::White => pairing.white,
                    Color::Black => pairing.black,
                };
                let participant = who(turn.think.side);
                let ponder = turn
                    .ponder
                    .as_ref()
                    .map(|plan| PonderJob::new(who(plan.side), plan));
                self.epoch += 1;
                let epoch = self.epoch;
                if let Some(run) = self.run_mut(lane) {
                    run.awaiting = Some((Awaiting::Search, epoch));
                }
                vec![Action::Search(Box::new(SearchJob {
                    lane,
                    participant,
                    think: turn.think,
                    deadline: turn.deadline,
                    ponder,
                    epoch,
                }))]
            }
        }
    }

    /// An engine answered (or failed to) a search.
    pub fn searched(&mut self, epoch: u64, outcome: SearchOutcome, now: Instant) -> Vec<Action> {
        let Some(lane) = self.lane_awaiting(Awaiting::Search, epoch) else {
            return Vec::new();
        };
        let Some(Some(run)) = self.runs.get_mut(lane) else {
            return Vec::new();
        };
        run.awaiting = None;
        if let Some(then) = run.then.take() {
            return self.apply_then(lane, then, now);
        }
        let Some(slot) = self.slots.get(run.slot) else {
            return Vec::new();
        };
        let pairing = slot.pairing;
        let Some(game) = run.game.as_mut() else {
            return Vec::new();
        };
        let side = game.side_to_move();
        let mover = match side {
            Color::White => pairing.white,
            Color::Black => pairing.black,
        };
        match outcome {
            SearchOutcome::Moved(reply) => {
                game.reply(&reply, now);
                self.step(lane, now)
            }
            SearchOutcome::NoAnswer { killed } => {
                if killed
                    && let Some(flag) = self
                        .needs_restart
                        .get_mut(lane)
                        .and_then(|row| row.get_mut(mover))
                {
                    *flag = true;
                }
                if matches!(self.config.time, TimeControl::Clock { .. }) {
                    // No answer before the time plus the margin: lost on
                    // time (D1269 a).
                    self.time_lost(lane, mover, side, now)
                } else {
                    self.engine_failed(lane, mover, FailKind::Hung, now)
                }
            }
            SearchOutcome::Dead(_) => self.engine_failed(lane, mover, FailKind::Died, now),
            SearchOutcome::Cancelled => self.step(lane, now),
        }
    }

    fn time_lost(
        &mut self,
        lane: usize,
        participant: usize,
        side: Color,
        now: Instant,
    ) -> Vec<Action> {
        if self.config.failures.on_time_loss == super::config::OnTimeLoss::Replay {
            let retriable = self
                .run_ref(lane)
                .is_some_and(|r| r.attempt < self.config.failures.max_retries);
            if retriable {
                return self.engine_failed(lane, participant, FailKind::TimeLoss, now);
            }
        }
        if let Some(game) = self.run_mut(lane).and_then(|r| r.game.as_mut()) {
            game.flag_fell(side);
        }
        self.step(lane, now)
    }

    // ---- failures ----

    fn engine_failed(
        &mut self,
        lane: usize,
        participant: usize,
        kind: FailKind,
        now: Instant,
    ) -> Vec<Action> {
        if let Some(count) = self.failures.get_mut(participant) {
            *count += 1;
        }
        if let Some(flag) = self
            .needs_restart
            .get_mut(lane)
            .and_then(|row| row.get_mut(participant))
        {
            *flag = true;
        }
        self.note(LogEvent::EngineFailed { participant, kind });
        let mut actions = vec![Action::Notice(TournamentNotice::EngineFailed {
            participant,
            kind,
        })];
        let too_many = self.failures.get(participant).copied().unwrap_or(0)
            >= self.config.failures.max_failures;
        if too_many && self.decision.is_none() {
            self.decision = Some(participant);
            actions.push(Action::Notice(TournamentNotice::TooManyFailures {
                participant,
            }));
        }
        let Some(run) = self.run_ref(lane) else {
            return actions;
        };
        let (attempt, ix) = (run.attempt, run.slot);
        self.keep_discarded(lane);
        if attempt < self.config.failures.max_retries {
            // The game is played again from the start; it is not saved
            // (D1269).
            if self.decision.is_some() {
                if let Some(run) = self.run_mut(lane) {
                    run.game = None;
                    run.awaiting = None;
                    run.retry_pending = true;
                }
                self.phase = Phase::Decision(self.decision.unwrap_or(participant));
            } else {
                actions.extend(self.begin_game(lane, ix, attempt + 1, now));
            }
            return actions;
        }
        // No tries left: the engine loses the game.
        let pairing = self.slots.get(ix).map(|s| s.pairing);
        let Some(pairing) = pairing else {
            return actions;
        };
        let loser = color_of(&pairing, participant);
        if self.run_ref(lane).is_some_and(|r| r.game.is_none()) {
            // It never started (the engine could not even be launched):
            // an empty game with the forfeit.
            self.build_forfeit_game(lane, now);
        }
        if let Some(game) = self.run_mut(lane).and_then(|r| r.game.as_mut()) {
            if kind == FailKind::TimeLoss {
                game.flag_fell(loser);
            } else {
                game.forfeit(loser, Termination::Abandoned);
            }
        }
        let end = self
            .run_ref(lane)
            .and_then(|r| r.game.as_ref())
            .and_then(|g| g.end().cloned());
        match end {
            Some(end) => actions.extend(self.finish_game(lane, &end, now)),
            None => actions.extend(self.start_next(now)),
        }
        actions
    }

    /// A game with no move, to hold the forfeit of an engine that never
    /// started.
    fn build_forfeit_game(&mut self, lane: usize, now: Instant) {
        let ready = [
            SideReady {
                participant: 0,
                engine_name: None,
                can_ponder: false,
            },
            SideReady {
                participant: 0,
                engine_name: None,
                can_ponder: false,
            },
        ];
        self.build_game(lane, ready, now);
    }

    /// Keeps the PGN of a broken game in the log only (D1269).
    fn keep_discarded(&mut self, lane: usize) {
        let discarded = self
            .log
            .iter()
            .filter(|e| matches!(e, LogEvent::Discarded { .. }))
            .count();
        if discarded >= MAX_DISCARDED {
            return;
        }
        let Some(run) = self.run_ref(lane) else {
            return;
        };
        let (Some(game), Some(slot)) = (run.game.as_ref(), self.slots.get(run.slot)) else {
            return;
        };
        if game.played().is_empty() {
            return;
        }
        let end = GameEnd {
            result: GameResult::Unknown,
            termination: Termination::Unterminated,
            comment: None,
        };
        let pairing = slot.pairing;
        let input = RecordInput {
            config: &self.config,
            pairing: &pairing,
            white: &self.participant_info(pairing.white, run.ready.as_ref().map(|r| &r[0])),
            black: &self.participant_info(pairing.black, run.ready.as_ref().map(|r| &r[1])),
            opening: &run.opening,
            game,
            end: &end,
            date: self.created,
            redo: self.redo_count(run.slot),
        };
        if let Ok(built) = build_game(&input)
            && let Ok(pgn) = game_to_string(&built, &WriteOptions::default())
        {
            let number = pairing.number;
            self.note(LogEvent::Discarded { number, pgn });
        }
    }

    /// Settles the question about a participant that failed too often
    /// (D1269).
    pub fn decide(&mut self, decision: Decision, now: Instant) -> Vec<Action> {
        if !matches!(self.phase, Phase::Decision(_)) {
            return vec![Action::Notice(TournamentNotice::NotApplicable)];
        }
        let Some(participant) = self.decision.take() else {
            return vec![Action::Notice(TournamentNotice::NotApplicable)];
        };
        let mut actions = Vec::new();
        if let Some(count) = self.failures.get_mut(participant) {
            *count = 0;
        }
        match decision {
            Decision::Retry => {}
            Decision::Replace(new) => {
                if let Some(slot) = self.config.participants.get_mut(participant) {
                    *slot = *new;
                }
                if let Some(name) = self.engine_names.get_mut(participant) {
                    *name = None;
                }
                self.note(LogEvent::Replaced(participant));
                // The old engine may be out on a search in another lane: it
                // is ended when it comes back, the new one starts in every
                // lane, and the games the old one was playing are played
                // again from the start (D2507).
                for row in &mut self.needs_restart {
                    if let Some(flag) = row.get_mut(participant) {
                        *flag = true;
                    }
                }
                actions.extend(self.give_back_games_of(participant));
                actions.push(Action::Discard(participant));
            }
            Decision::Withdraw => {
                if let Some(flag) = self.withdrawn.get_mut(participant) {
                    *flag = true;
                }
                self.note(LogEvent::Withdrawn(participant));
                for slot in &mut self.slots {
                    if slot.status == GameStatus::Pending && slot.pairing.involves(participant) {
                        slot.status = GameStatus::Withdrawn;
                    }
                }
                // The games in progress of the participant end here, in
                // every lane (D2507), except those the user is already
                // deciding (adjudicate, skip, end).
                for lane in self.active_lanes() {
                    let involved = self
                        .run_ref(lane)
                        .filter(|r| r.then.is_none())
                        .and_then(|r| self.slots.get(r.slot))
                        .is_some_and(|s| s.pairing.involves(participant));
                    if !involved {
                        continue;
                    }
                    let Some(run) = self.runs.get_mut(lane).and_then(Option::take) else {
                        continue;
                    };
                    if let Some((_, epoch)) = run.awaiting {
                        actions.push(Action::Cancel { lane, epoch });
                    }
                    if let Some(slot) = self.slots.get_mut(run.slot) {
                        slot.status = GameStatus::Withdrawn;
                    }
                }
                actions.push(Action::Discard(participant));
            }
        }
        self.phase = Phase::Running;
        let retries: Vec<(usize, usize, u8)> = self
            .runs
            .iter()
            .enumerate()
            .filter_map(|(lane, run)| {
                let run = run.as_ref().filter(|r| r.retry_pending)?;
                Some((lane, run.slot, run.attempt))
            })
            .collect();
        let mut held = Vec::new();
        for (lane, ix, attempt) in retries {
            if let Some(place) = self.runs.get_mut(lane) {
                *place = None;
            }
            if self.pause_requested {
                // Keep the game for when the tournament goes on.
                if let Some(slot) = self.slots.get_mut(ix) {
                    slot.status = GameStatus::Pending;
                }
                held.push(ix);
            } else {
                actions.extend(self.begin_game(lane, ix, attempt + 1, now));
            }
        }
        // Back to the front in the order of the schedule, not of the lanes
        // (D2535).
        held.sort_unstable();
        for ix in held.into_iter().rev() {
            self.queue.push_front(ix);
        }
        actions.extend(self.start_next(now));
        actions.push(self.save_action());
        actions
    }

    /// Takes back the games in progress with `participant` that are not the
    /// one waiting for the decision: they go to the front of the queue, in
    /// the order of the schedule, and the searches in flight are cancelled.
    fn give_back_games_of(&mut self, participant: usize) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut back = Vec::new();
        for lane in self.active_lanes() {
            let involved = self
                .run_ref(lane)
                .filter(|r| r.then.is_none() && !r.retry_pending)
                .and_then(|r| self.slots.get(r.slot))
                .is_some_and(|s| s.pairing.involves(participant));
            if !involved {
                continue;
            }
            let Some(run) = self.runs.get_mut(lane).and_then(Option::take) else {
                continue;
            };
            if let Some((_, epoch)) = run.awaiting {
                actions.push(Action::Cancel { lane, epoch });
            }
            if let Some(slot) = self.slots.get_mut(run.slot) {
                slot.status = GameStatus::Pending;
            }
            back.push(run.slot);
        }
        back.sort_unstable();
        for ix in back.into_iter().rev() {
            self.queue.push_front(ix);
        }
        actions
    }

    // ---- end of a game ----

    fn finish_game(&mut self, lane: usize, end: &GameEnd, now: Instant) -> Vec<Action> {
        let Some(run) = self.runs.get_mut(lane).and_then(Option::take) else {
            return Vec::new();
        };
        let Some(game) = run.game.as_ref() else {
            return Vec::new();
        };
        let pairing = self.slots.get(run.slot).map(|s| s.pairing);
        let Some(pairing) = pairing else {
            return Vec::new();
        };
        let ready = run.ready.as_ref();
        let input = RecordInput {
            config: &self.config,
            pairing: &pairing,
            white: &self.participant_info(pairing.white, ready.map(|r| &r[0])),
            black: &self.participant_info(pairing.black, ready.map(|r| &r[1])),
            opening: &run.opening,
            game,
            end,
            date: self.created,
            redo: self.redo_count(run.slot),
        };
        let built = build_game(&input);
        let outcome = Outcome {
            result: end.result,
            termination: end.termination,
            plies: u32::try_from(game.moves().len()).unwrap_or(u32::MAX),
            opening: run.opening.name.clone(),
        };
        if self.config.opening == OpeningSource::Book && self.config.pair_openings {
            let line = game.book_line();
            if !line.is_empty() && !run.opening_is_book {
                self.pair_lines.entry(pairing.group).or_insert(line);
            }
        }
        self.record_times(&pairing, game, end);
        if let Some(slot) = self.slots.get_mut(run.slot) {
            slot.status = GameStatus::Played(outcome);
        }
        self.note(LogEvent::GameFinished {
            number: pairing.number,
            result: end.result,
            termination: end.termination,
        });
        let mut actions = Vec::new();
        match built {
            Ok(game) => actions.push(Action::SaveGame {
                number: pairing.number,
                game: Box::new(game),
            }),
            Err(_) => actions.push(Action::Notice(TournamentNotice::GameNotSaved)),
        }
        actions.push(Action::GameOver {
            lane,
            participants: [pairing.white, pairing.black],
            result: end.result,
            reason: end
                .comment
                .clone()
                .unwrap_or_else(|| end.termination.tag().to_owned()),
        });
        actions.push(self.save_action());
        actions.extend(self.start_next(now));
        actions
    }

    /// Feeds the report of time with a game that is saved (D2514).
    fn record_times(&mut self, pairing: &Pairing, game: &TournamentGame, end: &GameEnd) {
        for side in [Color::White, Color::Black] {
            let participant = match side {
                Color::White => pairing.white,
                Color::Black => pairing.black,
            };
            let moves = game
                .played()
                .iter()
                .filter(|m| m.side == side && !m.free)
                .filter_map(|m| {
                    Some(TimedMove {
                        spent: m.spent?,
                        left: m.left,
                    })
                });
            let lost_on_time = end.termination == Termination::TimeForfeit
                && match end.result {
                    GameResult::White => side == Color::Black,
                    GameResult::Black => side == Color::White,
                    _ => false,
                };
            let time = self.config.time;
            if let Some(stats) = self.time_stats.get_mut(participant) {
                stats.record_game(time, moves, lost_on_time);
            }
        }
    }

    // ---- controls of the user ----

    fn apply_then(&mut self, lane: usize, then: Then, now: Instant) -> Vec<Action> {
        match then {
            Then::Adjudicate(result) => {
                let end = {
                    let Some(game) = self.run_mut(lane).and_then(|r| r.game.as_mut()) else {
                        return Vec::new();
                    };
                    game.adjudicate(result);
                    game.end().cloned()
                };
                match end {
                    Some(end) => self.finish_game(lane, &end, now),
                    None => Vec::new(),
                }
            }
            Then::Skip => {
                if let Some(run) = self.runs.get_mut(lane).and_then(Option::take)
                    && let Some(slot) = self.slots.get_mut(run.slot)
                {
                    slot.status = GameStatus::Skipped;
                    let number = slot.pairing.number;
                    self.note(LogEvent::Skipped(number));
                }
                let mut actions = vec![self.save_action()];
                actions.extend(self.start_next(now));
                actions
            }
            Then::Abort => {
                self.release_for_abort(lane);
                if self.any_active() {
                    Vec::new()
                } else {
                    self.finish_abort()
                }
            }
        }
    }

    /// Takes the game of a lane back: it will wait again (D2507).
    fn release_for_abort(&mut self, lane: usize) {
        if let Some(run) = self.runs.get_mut(lane).and_then(Option::take)
            && let Some(slot) = self.slots.get_mut(run.slot)
        {
            slot.status = GameStatus::Pending;
            self.released.push(run.slot);
        }
    }

    fn finish_abort(&mut self) -> Vec<Action> {
        // Back to the front of the queue, in the order of the schedule.
        let mut released = std::mem::take(&mut self.released);
        released.sort_unstable();
        released.dedup();
        for ix in released.into_iter().rev() {
            self.queue.push_front(ix);
        }
        self.phase = Phase::Aborted;
        self.note(LogEvent::Aborted);
        vec![
            Action::Notice(TournamentNotice::Aborted),
            self.save_action(),
            Action::Shutdown,
        ]
    }

    /// Runs `then` now, or when the task in flight answers (cancelling the
    /// search first).
    fn after_inflight(&mut self, lane: usize, then: Then, now: Instant) -> Vec<Action> {
        let Some(run) = self.run_mut(lane) else {
            return Vec::new();
        };
        match run.awaiting {
            Some((_, epoch)) => {
                run.then = Some(then);
                // The wait for the engines to start is cancelled too
                // (D1308): the user does not wait for a slow launch.
                vec![Action::Cancel { lane, epoch }]
            }
            None => self.apply_then(lane, then, now),
        }
    }

    /// The user decides the game in progress (D1275), the one of the
    /// watched lane (D2507).
    pub fn adjudicate(&mut self, result: GameResult, now: Instant) -> Vec<Action> {
        let lane = self.watched_lane();
        let has_game = lane
            .and_then(|l| self.run_ref(l))
            .is_some_and(|r| r.game.is_some() && r.then.is_none());
        let Some(lane) = lane.filter(|_| has_game && result != GameResult::Unknown) else {
            return vec![Action::Notice(TournamentNotice::NotApplicable)];
        };
        self.after_inflight(lane, Then::Adjudicate(result), now)
    }

    /// Drops the game in progress without a result (D1286), the one of the
    /// watched lane (D2507).
    pub fn skip_current(&mut self, now: Instant) -> Vec<Action> {
        let lane = self.watched_lane();
        let Some(lane) = lane.filter(|l| self.run_ref(*l).is_some_and(|r| r.then.is_none())) else {
            return vec![Action::Notice(TournamentNotice::NotApplicable)];
        };
        self.after_inflight(lane, Then::Skip, now)
    }

    /// Ends the tournament (D1284): the games in progress are dropped
    /// without result, the state is saved and the engines end.
    pub fn abort(&mut self, now: Instant) -> Vec<Action> {
        if matches!(self.phase, Phase::Finished | Phase::Aborted) {
            return vec![Action::Notice(TournamentNotice::NotApplicable)];
        }
        let _ = now;
        // The question about a participant is moot once the tournament ends.
        self.decision = None;
        let mut actions = Vec::new();
        for lane in self.active_lanes() {
            let Some(run) = self.run_mut(lane) else {
                continue;
            };
            match run.awaiting {
                Some((_, epoch)) => {
                    // Already waiting for the engine because of another
                    // request: the abort replaces what was asked and the
                    // cancellation was already sent.
                    let fresh = run.then.is_none();
                    run.then = Some(Then::Abort);
                    if fresh {
                        actions.push(Action::Cancel { lane, epoch });
                    }
                }
                None => self.release_for_abort(lane),
            }
        }
        if !self.any_active() {
            actions.extend(self.finish_abort());
        }
        actions
    }

    /// Ends the tournament without waiting for the pool (D2538): the pool is
    /// already closed (the program asked to quit and the quit was called
    /// off), so no search will answer. Every game in progress goes back to
    /// the queue, the answers still to come are forgotten (their lanes are
    /// gone) and the state is saved as an aborted tournament, which a resume
    /// takes up again.
    pub fn abort_now(&mut self) -> Vec<Action> {
        if matches!(self.phase, Phase::Finished | Phase::Aborted) {
            return Vec::new();
        }
        self.decision = None;
        for lane in self.active_lanes() {
            self.release_for_abort(lane);
        }
        self.finish_abort()
    }

    /// Pauses after the move in progress (D1284); with several lanes, when
    /// every lane has finished its move (D2507).
    pub fn pause(&mut self) -> Vec<Action> {
        if !matches!(self.phase, Phase::Running) {
            return vec![Action::Notice(TournamentNotice::NotApplicable)];
        }
        self.pause_requested = true;
        let mut actions = Vec::new();
        // The lanes that do not wait for an engine freeze at once.
        let idle: Vec<usize> = self
            .active_lanes()
            .into_iter()
            .filter(|&l| self.run_ref(l).is_some_and(|r| r.awaiting.is_none()))
            .collect();
        if self.runs.iter().all(Option::is_none) {
            actions.push(Action::StopPonder { lane: 0 });
        }
        for lane in idle {
            self.forget_ponder(lane);
            actions.push(Action::StopPonder { lane });
        }
        actions.extend(self.settle_pause());
        actions
    }

    /// The engines of a lane stopped pondering: the move they expected is
    /// forgotten, so the next search is a normal one (D1314).
    fn forget_ponder(&mut self, lane: usize) {
        if let Some(game) = self.run_mut(lane).and_then(|r| r.game.as_mut()) {
            game.forget_ponder();
        }
    }

    /// Goes on after a pause (D1284): the search of the move is sent again
    /// without pondering, in every lane that was frozen.
    pub fn resume_play(&mut self, now: Instant) -> Vec<Action> {
        match self.phase {
            Phase::Pausing | Phase::Paused | Phase::Finished => {
                let noted = self.phase != Phase::Pausing;
                self.pause_requested = false;
                self.phase = Phase::Running;
                let mut actions = Vec::new();
                if noted {
                    self.note(LogEvent::Resumed);
                    actions.push(Action::Notice(TournamentNotice::Resumed));
                }
                let idle: Vec<usize> = self
                    .active_lanes()
                    .into_iter()
                    .filter(|&l| {
                        self.run_ref(l)
                            .is_some_and(|r| r.awaiting.is_none() && r.game.is_some())
                    })
                    .collect();
                for lane in idle {
                    actions.extend(self.step(lane, now));
                }
                actions.extend(self.start_next(now));
                actions
            }
            Phase::StoppedAfterGame => {
                self.phase = Phase::Running;
                self.note(LogEvent::Resumed);
                let mut actions = vec![Action::Notice(TournamentNotice::Resumed)];
                actions.extend(self.start_next(now));
                actions
            }
            _ => vec![Action::Notice(TournamentNotice::NotApplicable)],
        }
    }

    /// Stops when the games in progress end (D1284, D2507).
    pub fn stop_after_game(&mut self, now: Instant) -> Vec<Action> {
        if matches!(self.phase, Phase::Finished | Phase::Aborted) {
            return vec![Action::Notice(TournamentNotice::NotApplicable)];
        }
        self.stop_after = true;
        if !self.any_active() && !matches!(self.phase, Phase::Decision(_)) {
            self.pause_requested = false;
            return self.start_next(now);
        }
        Vec::new()
    }

    fn slot_of(&self, number: u32, wanted: impl Fn(&GameStatus) -> bool) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.pairing.number == number && wanted(&s.status))
    }

    /// Skips a game that waits (D1286).
    pub fn skip_pending(&mut self, number: u32) -> Vec<Action> {
        let Some(ix) = self.slot_of(number, |s| *s == GameStatus::Pending) else {
            return vec![Action::Notice(TournamentNotice::NotApplicable)];
        };
        self.queue.retain(|&i| i != ix);
        if let Some(slot) = self.slots.get_mut(ix) {
            slot.status = GameStatus::Skipped;
        }
        self.note(LogEvent::Skipped(number));
        vec![self.save_action()]
    }

    /// Moves a game that waits to the front of the queue (D1286).
    pub fn move_to_front(&mut self, number: u32) -> Vec<Action> {
        let Some(ix) = self.slot_of(number, |s| *s == GameStatus::Pending) else {
            return vec![Action::Notice(TournamentNotice::NotApplicable)];
        };
        self.queue.retain(|&i| i != ix);
        self.queue.push_front(ix);
        vec![self.save_action()]
    }

    /// Plays a game again (D1286): the old one stops counting and a new one
    /// with the same participants, colours and opening goes to the front of
    /// the queue.
    pub fn redo(&mut self, number: u32) -> Vec<Action> {
        let Some(ix) = self.slot_of(number, |s| matches!(s, GameStatus::Played(_))) else {
            return vec![Action::Notice(TournamentNotice::NotApplicable)];
        };
        let Some(slot) = self.slots.get_mut(ix) else {
            return Vec::new();
        };
        let GameStatus::Played(outcome) = slot.status.clone() else {
            return Vec::new();
        };
        slot.status = GameStatus::Replaced(outcome);
        let pairing = slot.pairing;
        self.slots.push(GameSlot {
            pairing,
            status: GameStatus::Pending,
        });
        self.queue.push_front(self.slots.len() - 1);
        self.note(LogEvent::Redo(number));
        if self.phase == Phase::Finished {
            // The schedule has a game again: the user presses "go on".
            self.phase = Phase::Paused;
            self.pause_requested = true;
        }
        vec![self.save_action()]
    }

    /// The scheduling format, for the Gauntlet's board orientation.
    pub fn scheduling(&self) -> Scheduling {
        self.config.scheduling
    }
}

#[cfg(test)]
mod tests;
