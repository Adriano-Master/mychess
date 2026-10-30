//! The tasks that talk to the engines of a tournament (spec M8-2, D1267,
//! D1268, D1270, D1278, D1288).
//!
//! The engines of a tournament are **private**: they do not go through the
//! [`EngineHub`](crate::engines::EngineHub) and never meet the analysis of
//! the user. An [`EnginePool`] owns them. A search (`run_search`) takes the
//! engine out of the pool, measures the time of the move on a monotonic
//! clock **inside the task** (the latency of the interface never enters the
//! clock of an engine, D1267), enforces the deadline of the watchdog (`stop`,
//! one second, then kill, D1268) and puts the engine back.
//!
//! Nothing here blocks a thread: every wait is a `tokio` timeout over the
//! event channel of the engine, polled every 50 ms so a cancellation is seen
//! quickly.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mychess_core::{GameResult, Move};
use mychess_engine::{
    BestMove, Engine, EngineCommand, EngineEvent, EngineEvents, EngineIdentity,
    Score as EngineScore, SearchId,
};
use tokio::time::{Instant, timeout};

use super::game::{PonderPlan, Reply};
use crate::engines::{EngineEntry, EngineFailure, FailureKind, Launcher, RunDefaults};
use crate::play::Think;

/// How often a wait looks at the cancellation flag.
const POLL: Duration = Duration::from_millis(50);
/// How long the engine has to answer a `stop` (D1268).
pub const STOP_WAIT: Duration = Duration::from_secs(1);
/// How long starting an engine may take.
const LAUNCH_WAIT: Duration = Duration::from_secs(60);

/// How to start the engine of one participant.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchRequest {
    /// The participant.
    pub participant: usize,
    /// The engine of the list.
    pub entry: EngineEntry,
    /// The options that override the saved ones: those of the participant
    /// and the uniform ones (D1288).
    pub options: Vec<(String, String)>,
    /// The settings that do not belong to one engine.
    pub defaults: RunDefaults,
}

/// What to do before a game: the engines that play it.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparePlan {
    /// The lane that plays the game (D2500): its engines are its own.
    pub lane: usize,
    /// `[white, black]`.
    pub sides: [usize; 2],
    /// Start the engine again even if it is alive (D1288).
    pub restart: [bool; 2],
    /// End the engines of the lane that do not play this game, so the
    /// memory of the hash tables stays bounded (D2506).
    pub prune_lane: bool,
    /// The display names `[white, black]`, told to XBoard engines as the
    /// name of their opponent (D2524).
    pub opponents: [String; 2],
    /// How to start each of the two.
    pub launches: [LaunchRequest; 2],
}

/// An engine ready for a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideReady {
    /// The participant.
    pub participant: usize,
    /// The `id name` the engine announced.
    pub engine_name: Option<String>,
    /// The engine declares the `Ponder` option.
    pub can_ponder: bool,
}

/// An engine that could not be made ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareFailure {
    /// The participant.
    pub participant: usize,
    /// What went wrong.
    pub failure: EngineFailure,
}

/// An engine that ponders while the other one thinks (D1270).
#[derive(Debug, Clone, PartialEq)]
pub struct PonderJob {
    /// The participant that ponders.
    pub participant: usize,
    /// The search: the position after the expected move.
    pub think: Think,
    /// The expected move.
    pub expected: Move,
}

impl PonderJob {
    /// The job of a ponder plan of the game.
    pub fn new(participant: usize, plan: &PonderPlan) -> PonderJob {
        PonderJob {
            participant,
            think: plan.think.clone(),
            expected: plan.expected,
        }
    }
}

/// A request to one engine: "play in this position".
#[derive(Debug, Clone, PartialEq)]
pub struct SearchJob {
    /// The lane of the game (D2500).
    pub lane: usize,
    /// The participant that moves.
    pub participant: usize,
    /// The search.
    pub think: Think,
    /// The longest to wait for the `bestmove` (D1268).
    pub deadline: Duration,
    /// The opponent ponders while this engine thinks.
    pub ponder: Option<PonderJob>,
    /// The epoch of the runner; a cancellation of this epoch or a later one
    /// stops the search.
    pub epoch: u64,
}

/// A command to the engines of the tournament that is not a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolCommand {
    /// Stop the searches of this epoch and the earlier ones, in every lane.
    Cancel(u64),
    /// Stop the searches of this epoch and the earlier ones in one lane
    /// (D2505).
    CancelLane {
        /// The lane.
        lane: usize,
        /// The epoch.
        epoch: u64,
    },
    /// Stop the engines that are pondering.
    StopPonder,
    /// Stop the engines of one lane that are pondering.
    StopPonderLane(usize),
    /// End the engines of a participant, in every lane.
    Discard(usize),
    /// Tell the engines of a lane the game is over (D2524). Engines that are
    /// gone are skipped.
    GameOver {
        /// The lane.
        lane: usize,
        /// `[white, black]`.
        participants: [usize; 2],
        /// The result.
        result: GameResult,
        /// Why the game ended.
        reason: String,
    },
    /// End every engine.
    Shutdown,
}

/// How a search ended.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchOutcome {
    /// The engine answered.
    Moved(Reply),
    /// No answer before the deadline; `killed` tells whether the process had
    /// to be ended.
    NoAnswer {
        /// The process was killed.
        killed: bool,
    },
    /// The engine died or was never started.
    Dead(EngineFailure),
    /// The runner cancelled the search.
    Cancelled,
}

#[derive(Debug)]
struct Pondering {
    expected: Move,
    tag: u64,
}

struct Slot {
    /// The generation of the participant when the engine started: an engine
    /// of an older generation (the participant was replaced or left while it
    /// was out on a search) is ended when it comes back (D2506).
    generation: u64,
    engine: Engine,
    events: EngineEvents,
    identity: Option<EngineIdentity>,
    pondering: Option<Pondering>,
}

impl fmt::Debug for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Slot")
            .field("pondering", &self.pondering)
            .finish_non_exhaustive()
    }
}

struct PoolInner {
    /// The engines by `(lane, participant)` (D2506).
    slots: Mutex<HashMap<(usize, usize), Slot>>,
    launcher: Launcher,
    /// The highest epoch cancelled in every lane.
    cancelled: AtomicU64,
    /// The highest epoch cancelled in each lane.
    cancelled_lanes: Mutex<HashMap<usize, u64>>,
    /// The current generation of each participant that was discarded.
    generations: Mutex<HashMap<usize, u64>>,
    tag_seq: AtomicU64,
    /// Set by `shutdown`, cleared by `prepare`: while it is set an engine
    /// that comes back from a search is ended instead of kept, so a search
    /// that finishes after the shutdown does not leak its process (D1307).
    closed: AtomicBool,
}

/// The engines of a tournament. Clones share the same engines; equality is
/// by identity so the pool can travel in an [`Effect`](crate::Effect).
#[derive(Clone)]
pub struct EnginePool {
    inner: Arc<PoolInner>,
}

impl fmt::Debug for EnginePool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnginePool").finish_non_exhaustive()
    }
}

impl PartialEq for EnginePool {
    fn eq(&self, other: &EnginePool) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for EnginePool {}

/// The tags of the searches of the tournament (the other flows use other
/// high bits: annotation `0x4E4E`, modes `0x5452`, play `0x504C`).
const TAG_BASE: u64 = 0x544F_0000_0000_0000;

impl EnginePool {
    /// A pool that starts engines with `launcher` (the same type as the
    /// hub's, so a test can substitute a fake).
    pub fn new(launcher: Launcher) -> EnginePool {
        EnginePool {
            inner: Arc::new(PoolInner {
                slots: Mutex::new(HashMap::new()),
                launcher,
                cancelled: AtomicU64::new(0),
                cancelled_lanes: Mutex::new(HashMap::new()),
                generations: Mutex::new(HashMap::new()),
                tag_seq: AtomicU64::new(0),
                closed: AtomicBool::new(false),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(usize, usize), Slot>> {
        self.inner
            .slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn take(&self, lane: usize, participant: usize) -> Option<Slot> {
        self.lock().remove(&(lane, participant))
    }

    fn put(&self, lane: usize, participant: usize, slot: Slot) {
        let mut slots = self.lock();
        // Checked under the lock `shutdown` drains with, so an engine is
        // either drained or ended here, never left behind.
        if self.inner.closed.load(Ordering::SeqCst)
            || slot.generation != self.generation(participant)
        {
            drop(slots);
            Self::discard(slot);
            return;
        }
        let replaced = slots.insert((lane, participant), slot);
        drop(slots);
        // Two engines for one (lane, participant) must not happen; if they
        // do, the older one is ended instead of being dropped with its
        // process still running (D2532).
        if let Some(old) = replaced {
            tracing::warn!(lane, participant, "an engine of the pool was replaced");
            Self::discard(old);
        }
    }

    fn generation(&self, participant: usize) -> u64 {
        self.inner
            .generations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&participant)
            .copied()
            .unwrap_or(0)
    }

    fn bump_generation(&self, participant: usize) {
        let mut generations = self
            .inner
            .generations
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let entry = generations.entry(participant).or_insert(0);
        *entry += 1;
    }

    fn next_tag(&self) -> u64 {
        TAG_BASE | (self.inner.tag_seq.fetch_add(1, Ordering::Relaxed) & 0x0000_FFFF_FFFF_FFFF)
    }

    /// Cancels the searches of `epoch` and earlier ones, in every lane. They
    /// stop within about 50 ms and answer [`SearchOutcome::Cancelled`].
    pub fn cancel(&self, epoch: u64) {
        self.inner.cancelled.fetch_max(epoch, Ordering::Relaxed);
    }

    /// As [`EnginePool::cancel`], for one lane only (D2505).
    pub fn cancel_lane(&self, lane: usize, epoch: u64) {
        let mut lanes = self
            .inner
            .cancelled_lanes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let entry = lanes.entry(lane).or_insert(0);
        *entry = (*entry).max(epoch);
    }

    fn is_cancelled(&self, lane: usize, epoch: u64) -> bool {
        if epoch <= self.inner.cancelled.load(Ordering::Relaxed) {
            return true;
        }
        self.inner
            .cancelled_lanes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&lane)
            .is_some_and(|&cancelled| epoch <= cancelled)
    }

    /// How many engines are alive.
    pub fn alive(&self) -> usize {
        self.lock().len()
    }

    /// Stops the engines that are pondering (a pause, D1284): the next
    /// search of each one starts normally.
    pub fn stop_pondering(&self) {
        self.stop_pondering_where(|_| true);
    }

    /// As [`EnginePool::stop_pondering`], for one lane.
    pub fn stop_pondering_lane(&self, lane: usize) {
        self.stop_pondering_where(|l| l == lane);
    }

    fn stop_pondering_where(&self, wanted: impl Fn(usize) -> bool) {
        for ((lane, _), slot) in self.lock().iter_mut() {
            if wanted(*lane) && slot.pondering.take().is_some() {
                // A refused `Stop` means the engine is gone; the next
                // search finds out.
                slot.engine.send(EngineCommand::Stop).ok();
            }
        }
    }

    /// Tells the engines of a game that it ended (D2524). An engine that is
    /// out on a search or gone is skipped.
    pub fn game_over(
        &self,
        lane: usize,
        participants: [usize; 2],
        result: GameResult,
        reason: &str,
    ) {
        let mut slots = self.lock();
        for participant in participants {
            if let Some(slot) = slots.get_mut(&(lane, participant)) {
                slot.engine
                    .send(EngineCommand::GameOver {
                        result: result.as_str().to_owned(),
                        comment: reason.to_owned(),
                    })
                    .ok();
            }
        }
    }

    /// Carries out a [`PoolCommand`].
    pub async fn command(&self, command: PoolCommand) {
        match command {
            PoolCommand::Cancel(epoch) => self.cancel(epoch),
            PoolCommand::CancelLane { lane, epoch } => self.cancel_lane(lane, epoch),
            PoolCommand::StopPonder => self.stop_pondering(),
            PoolCommand::StopPonderLane(lane) => self.stop_pondering_lane(lane),
            PoolCommand::Discard(participant) => {
                // First the generation: an engine that is out on a search
                // is ended when it comes back, an idle one right now.
                self.bump_generation(participant);
                let gone: Vec<Slot> = {
                    let mut slots = self.lock();
                    let keys: Vec<(usize, usize)> = slots
                        .keys()
                        .filter(|(_, p)| *p == participant)
                        .copied()
                        .collect();
                    keys.into_iter()
                        .filter_map(|key| slots.remove(&key))
                        .collect()
                };
                for slot in gone {
                    Self::discard(slot);
                }
            }
            PoolCommand::GameOver {
                lane,
                participants,
                result,
                reason,
            } => self.game_over(lane, participants, result, &reason),
            PoolCommand::Shutdown => self.shutdown().await,
        }
    }

    /// Ends every engine politely (`quit`, a wait, a kill) and empties the
    /// pool. An engine that is out on a search when this runs is ended when
    /// it comes back; the next [`EnginePool::prepare`] opens the pool again
    /// (a tournament that stopped after a game can go on).
    pub async fn shutdown(&self) {
        let slots: Vec<Slot> = {
            let mut guard = self.lock();
            self.inner.closed.store(true, Ordering::SeqCst);
            guard.drain().map(|(_, slot)| slot).collect()
        };
        let ends = slots.into_iter().map(|slot| {
            tokio::spawn(async move {
                drop(slot.events);
                slot.engine.shutdown().await;
            })
        });
        for end in ends.collect::<Vec<_>>() {
            end.await.ok();
        }
    }

    /// Ends the engine of `participant` without waiting for it.
    fn discard(slot: Slot) {
        slot.engine.send(EngineCommand::Quit).ok();
        tokio::spawn(async move {
            drop(slot.events);
            slot.engine.shutdown().await;
        });
    }

    // ---- before a game ----

    /// Makes the two engines ready: starts those that are missing, dead or
    /// to be restarted, and tells all of them a new game starts (`ucinewgame`
    /// then `isready`, D1288).
    ///
    /// `epoch` is the epoch of the request: a [`PoolCommand::Cancel`] of it
    /// (or a later one) stops the wait within about 50 ms, even in the middle
    /// of an engine start, and the answer is a [`PrepareFailure`] of kind
    /// `Closed` that the runner ignores because it asked for the
    /// cancellation (D1308).
    ///
    /// # Errors
    ///
    /// [`PrepareFailure`] for the first engine that could not be started.
    pub async fn prepare(
        &self,
        plan: &PreparePlan,
        epoch: u64,
    ) -> Result<[SideReady; 2], PrepareFailure> {
        self.inner.closed.store(false, Ordering::SeqCst);
        if plan.prune_lane {
            self.prune_lane(plan);
        }
        let first = self.prepare_side(plan, 0, epoch).await?;
        let second = self.prepare_side(plan, 1, epoch).await?;
        Ok([first, second])
    }

    /// Ends the engines of the lane that do not play the next game (D2506).
    fn prune_lane(&self, plan: &PreparePlan) {
        let gone: Vec<Slot> = {
            let mut slots = self.lock();
            let keys: Vec<(usize, usize)> = slots
                .keys()
                .filter(|(lane, p)| *lane == plan.lane && !plan.sides.contains(p))
                .copied()
                .collect();
            keys.into_iter()
                .filter_map(|key| slots.remove(&key))
                .collect()
        };
        for slot in gone {
            Self::discard(slot);
        }
    }

    /// Waits for `future` in slices of [`POLL`], looking at the cancellation
    /// of `epoch` between them; `None` when cancelled.
    async fn unless_cancelled<T>(
        &self,
        lane: usize,
        epoch: u64,
        future: impl Future<Output = T>,
    ) -> Option<T> {
        tokio::pin!(future);
        loop {
            if self.is_cancelled(lane, epoch) {
                return None;
            }
            if let Ok(done) = timeout(POLL, &mut future).await {
                return Some(done);
            }
        }
    }

    async fn prepare_side(
        &self,
        plan: &PreparePlan,
        side: usize,
        epoch: u64,
    ) -> Result<SideReady, PrepareFailure> {
        let launch = &plan.launches[side];
        let participant = launch.participant;
        let lane = plan.lane;
        if self.is_cancelled(lane, epoch) {
            return Err(cancelled_failure(participant));
        }
        let restart = plan.restart.get(side).copied().unwrap_or(false);
        let mut slot = self.take(lane, participant);
        if let Some(old) = slot.take_if(|s| restart || !is_alive(s)) {
            Self::discard(old);
        }
        let mut slot = match slot {
            Some(slot) => slot,
            None => self.launch(launch, lane, epoch).await?,
        };
        slot.pondering = None;
        if slot.engine.send(EngineCommand::NewGame).is_err() {
            Self::discard(slot);
            return Err(PrepareFailure {
                participant,
                failure: EngineFailure {
                    kind: FailureKind::Closed,
                    exit_code: None,
                    stderr_tail: Vec::new(),
                },
            });
        }
        // Who the opponent is (XBoard `name`, D2524); UCI ignores it.
        if let Some(name) = plan.opponents.get(1 - side.min(1)) {
            slot.engine.send(EngineCommand::Opponent(name.clone())).ok();
        }
        let ready = SideReady {
            participant,
            engine_name: slot.identity.as_ref().map(|i| i.name.clone()),
            can_ponder: slot
                .identity
                .as_ref()
                .is_some_and(|i| i.capabilities.ponder),
        };
        self.put(lane, participant, slot);
        Ok(ready)
    }

    async fn launch(
        &self,
        request: &LaunchRequest,
        lane: usize,
        epoch: u64,
    ) -> Result<Slot, PrepareFailure> {
        // Read before the start: a participant replaced meanwhile makes this
        // engine old (D2506).
        let generation = self.generation(request.participant);
        let fail = |failure| PrepareFailure {
            participant: request.participant,
            failure,
        };
        let mut config = request.entry.to_config(&request.defaults);
        for (name, text) in &request.options {
            let value = crate::engines::option_value(text);
            match config
                .options
                .iter_mut()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
            {
                Some((_, old)) => *old = value,
                None => config.options.push((name.clone(), value)),
            }
        }
        let launching = timeout(LAUNCH_WAIT, (self.inner.launcher)(config));
        let Some(launched) = self.unless_cancelled(lane, epoch, launching).await else {
            return Err(cancelled_failure(request.participant));
        };
        let (engine, mut events) = match launched {
            Err(_) => {
                return Err(fail(EngineFailure::from_error(
                    &mychess_engine::EngineError::Timeout(mychess_engine::Phase::Uciok),
                )));
            }
            Ok(Err(error)) => return Err(fail(EngineFailure::from_error(&error))),
            Ok(Ok(pair)) => pair,
        };
        // The identity arrives with the handshake; wait for it if the
        // launcher did not.
        let started = Instant::now();
        while engine.identity().is_none() {
            if self.is_cancelled(lane, epoch) {
                engine.send(EngineCommand::Quit).ok();
                return Err(cancelled_failure(request.participant));
            }
            let left = LAUNCH_WAIT.saturating_sub(started.elapsed());
            let slice = left.min(POLL);
            match timeout(slice, events.recv()).await {
                Err(_) if slice < left => {}
                Ok(Some(EngineEvent::Exited(info))) => {
                    return Err(fail(EngineFailure::from_exit(&info).unwrap_or(
                        EngineFailure {
                            kind: FailureKind::Crashed,
                            exit_code: None,
                            stderr_tail: Vec::new(),
                        },
                    )));
                }
                Ok(Some(EngineEvent::Ready(_))) => break,
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => {
                    engine.send(EngineCommand::Quit).ok();
                    return Err(fail(EngineFailure::from_error(
                        &mychess_engine::EngineError::Timeout(mychess_engine::Phase::Uciok),
                    )));
                }
            }
        }
        let identity = engine.identity().cloned();
        Ok(Slot {
            generation,
            engine,
            events,
            identity,
            pondering: None,
        })
    }

    // ---- a move ----

    /// Asks an engine for a move (see the module docs).
    pub async fn search(&self, job: SearchJob) -> SearchOutcome {
        let lane = job.lane;
        let Some(mut slot) = self.take(lane, job.participant) else {
            return SearchOutcome::Dead(EngineFailure {
                kind: FailureKind::Closed,
                exit_code: None,
                stderr_tail: Vec::new(),
            });
        };
        if let Some(ponder) = &job.ponder {
            self.start_ponder(lane, ponder);
        }
        let tag = self.next_tag();
        let expected_hit = slot
            .pondering
            .as_ref()
            .filter(|p| job.think.moves.last() == Some(&p.expected))
            .map(|p| p.tag);
        let sent_at = Instant::now();
        let mut wait = Waiting {
            tag: expected_hit.unwrap_or(tag),
            id: None,
            started: None,
            hit_at: None,
            score: None,
            depth: None,
        };
        let mut request_sent = false;
        if expected_hit.is_some() {
            if slot.engine.send(EngineCommand::PonderHit).is_ok() {
                wait.hit_at = Some(Instant::now());
                request_sent = true;
            } else {
                wait.tag = tag;
            }
        }
        // A wrong guess, or no guess: a normal search. It replaces the
        // pondering one; the `bestmove` of that one has another id and is
        // ignored.
        if !request_sent
            && slot
                .engine
                .send(EngineCommand::Search(job.think.to_request(tag)))
                .is_err()
        {
            return SearchOutcome::Dead(EngineFailure {
                kind: FailureKind::Closed,
                exit_code: None,
                stderr_tail: Vec::new(),
            });
        }
        slot.pondering = None;
        let replaced = !request_sent;
        let deadline_at = sent_at + job.deadline;
        let outcome = self
            .wait_best(&mut slot, &mut wait, deadline_at, lane, job.epoch)
            .await;
        match outcome {
            Waited::Best(best, at) => {
                let start = wait
                    .hit_at
                    .or(if replaced { None } else { wait.started })
                    .unwrap_or(sent_at);
                let reply = Reply {
                    mv: best.mv,
                    spent: at.saturating_duration_since(start),
                    score: wait.score,
                    depth: wait.depth,
                    ponder: best.ponder,
                };
                self.put(lane, job.participant, slot);
                SearchOutcome::Moved(reply)
            }
            Waited::Expired => {
                slot.engine.send(EngineCommand::Stop).ok();
                let answered = self.after_stop(&mut slot, &mut wait).await;
                if answered {
                    self.put(lane, job.participant, slot);
                } else {
                    Self::discard(slot);
                }
                SearchOutcome::NoAnswer { killed: !answered }
            }
            Waited::Cancelled => {
                slot.engine.send(EngineCommand::Stop).ok();
                if self.after_stop(&mut slot, &mut wait).await {
                    self.put(lane, job.participant, slot);
                } else {
                    Self::discard(slot);
                }
                SearchOutcome::Cancelled
            }
            Waited::Dead(failure) => {
                Self::discard(slot);
                SearchOutcome::Dead(failure)
            }
        }
    }

    /// Starts the search of the engine that ponders. A failure only means
    /// no pondering.
    fn start_ponder(&self, lane: usize, job: &PonderJob) {
        let Some(mut slot) = self.take(lane, job.participant) else {
            return;
        };
        let tag = self.next_tag();
        let mut request = job.think.to_request(tag);
        request.ponder = true;
        if slot.engine.send(EngineCommand::Search(request)).is_ok() {
            slot.pondering = Some(Pondering {
                expected: job.expected,
                tag,
            });
        }
        self.put(lane, job.participant, slot);
    }

    async fn wait_best(
        &self,
        slot: &mut Slot,
        wait: &mut Waiting,
        deadline_at: Instant,
        lane: usize,
        epoch: u64,
    ) -> Waited {
        loop {
            if self.is_cancelled(lane, epoch) {
                return Waited::Cancelled;
            }
            let now = Instant::now();
            if now >= deadline_at {
                return Waited::Expired;
            }
            let slice = POLL.min(deadline_at - now);
            match timeout(slice, slot.events.recv()).await {
                Err(_) => {}
                Ok(None) => return Waited::Dead(closed_failure()),
                Ok(Some(event)) => {
                    if let Some(done) = wait.absorb(event) {
                        return done;
                    }
                }
            }
        }
    }

    /// After a `stop`: whether the engine answered within [`STOP_WAIT`].
    async fn after_stop(&self, slot: &mut Slot, wait: &mut Waiting) -> bool {
        let until = Instant::now() + STOP_WAIT;
        loop {
            let now = Instant::now();
            if now >= until {
                return false;
            }
            match timeout(until - now, slot.events.recv()).await {
                Err(_) | Ok(None) => return false,
                Ok(Some(event)) => {
                    if let Some(Waited::Best(..)) = wait.absorb(event) {
                        return true;
                    }
                }
            }
        }
    }
}

/// The answer of a `prepare` that was cancelled.
fn cancelled_failure(participant: usize) -> PrepareFailure {
    PrepareFailure {
        participant,
        failure: EngineFailure {
            kind: FailureKind::Closed,
            exit_code: None,
            stderr_tail: Vec::new(),
        },
    }
}

fn closed_failure() -> EngineFailure {
    EngineFailure {
        kind: FailureKind::Crashed,
        exit_code: None,
        stderr_tail: Vec::new(),
    }
}

fn is_alive(slot: &mut Slot) -> bool {
    // Anything that arrived while the engine was idle is read: an `Exited`
    // means the process is gone.
    while let Some(event) = slot.events.try_recv() {
        if matches!(event, EngineEvent::Exited(_)) {
            return false;
        }
    }
    slot.engine.send(EngineCommand::Stop).is_ok()
}

enum Waited {
    Best(BestMove, Instant),
    Expired,
    Cancelled,
    Dead(EngineFailure),
}

/// What the wait for a `bestmove` has seen so far.
struct Waiting {
    /// The label of our request (or of the pondering one we adopted).
    tag: u64,
    id: Option<SearchId>,
    /// When the engine reported the start of the search (`go` sent).
    started: Option<Instant>,
    /// When `ponderhit` was sent: the clock of the engine starts then.
    hit_at: Option<Instant>,
    score: Option<EngineScore>,
    depth: Option<u32>,
}

impl Waiting {
    fn absorb(&mut self, event: EngineEvent) -> Option<Waited> {
        match event {
            EngineEvent::SearchTagged { search, tag } if tag == self.tag => {
                self.id = Some(search);
            }
            EngineEvent::SearchStarted(id) if self.id == Some(id) => {
                self.started = Some(Instant::now());
            }
            EngineEvent::Info(info) if self.id == Some(info.search) && info.multipv <= 1 => {
                if let Some((score, _)) = info.score {
                    self.score = Some(score);
                    self.depth = info.depth;
                }
            }
            EngineEvent::BestMove(best) if self.id == Some(best.search) => {
                return Some(Waited::Best(best, Instant::now()));
            }
            EngineEvent::Exited(info) => {
                return Some(Waited::Dead(
                    EngineFailure::from_exit(&info).unwrap_or_else(closed_failure),
                ));
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
pub(crate) mod tests;
