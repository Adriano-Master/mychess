//! Engine executions and the bridge to the engine processes (spec M4-3,
//! D433-D435).
//!
//! - [`RunId`], [`EngineRun`], [`EngineRunState`], [`EngineRuns`]: what is
//!   running, as plain data owned by `AppState`.
//! - [`EngineFailure`]: why an engine did not start or ended, as data the
//!   front-end translates.
//! - [`EngineHub`]: the live [`Engine`] handles and the single channel of
//!   `(RunId, EngineEvent)` that the front-end turns into a subscription.
//!   Compared by identity, like [`SharedEco`](crate::SharedEco).

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mychess_core::{Move, Position};
use mychess_engine::{
    Engine, EngineCommand, EngineConfig, EngineError, EngineEvent, EngineEvents, EngineIdentity,
    ExitInfo, ExitReason, LogHandle, Phase,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

use super::model::EngineId;

/// The identity of one execution of an engine (D433). A new id for every
/// start, never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RunId(pub u64);

/// How long quitting waits for the engines to end (D435).
pub const SHUTDOWN_WAIT: Duration = Duration::from_millis(1500);

/// From this many executions the interface warns about the load (D433).
pub const MANY_RUNS: usize = 4;

/// What went wrong, without text: the front-end translates from the variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FailureKind {
    /// The program could not be started (missing, not executable...).
    CannotStart,
    /// The engine ended before it was ready.
    ExitedEarly,
    /// The engine ended or closed its output by itself.
    Crashed,
    /// The engine did not answer in time, in this step.
    Timeout(Phase),
    /// The engine ignored a `stop` and was ended.
    StopTimeout,
    /// The program speaks neither UCI nor XBoard.
    NotAnEngine,
    /// A pipe failed.
    Io,
    /// The session was already closed.
    Closed,
    /// Anything else.
    Other,
}

/// Why an engine did not start or ended (spec M4-3, rule 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineFailure {
    /// The kind of failure.
    pub kind: FailureKind,
    /// The exit code of the process, when it ended and has one.
    pub exit_code: Option<i32>,
    /// The last lines the engine wrote to its standard error.
    pub stderr_tail: Vec<String>,
}

impl EngineFailure {
    fn plain(kind: FailureKind) -> EngineFailure {
        EngineFailure {
            kind,
            exit_code: None,
            stderr_tail: Vec::new(),
        }
    }

    /// The failure that an error of the engine crate describes.
    pub fn from_error(error: &EngineError) -> EngineFailure {
        match error {
            EngineError::Spawn { .. } => EngineFailure::plain(FailureKind::CannotStart),
            EngineError::Io(_) => EngineFailure::plain(FailureKind::Io),
            EngineError::Timeout(phase) => EngineFailure::plain(FailureKind::Timeout(*phase)),
            EngineError::ExitedEarly {
                status,
                stderr_tail,
            } => EngineFailure {
                kind: FailureKind::ExitedEarly,
                exit_code: status.and_then(|s| s.code()),
                stderr_tail: stderr_tail.clone(),
            },
            EngineError::Closed => EngineFailure::plain(FailureKind::Closed),
            EngineError::Unrecognised | EngineError::Unsupported(_) => {
                EngineFailure::plain(FailureKind::NotAnEngine)
            }
            _ => EngineFailure::plain(FailureKind::Other),
        }
    }

    /// The failure of a session that ended; `None` when it ended because it
    /// was asked to.
    pub fn from_exit(info: &ExitInfo) -> Option<EngineFailure> {
        let kind = match &info.reason {
            ExitReason::Quit => return None,
            ExitReason::Crashed => FailureKind::Crashed,
            ExitReason::Timeout(phase) => FailureKind::Timeout(*phase),
            ExitReason::StopTimeout => FailureKind::StopTimeout,
            ExitReason::Io(_) => FailureKind::Io,
            ExitReason::Dropped => FailureKind::Closed,
            _ => FailureKind::Other,
        };
        Some(EngineFailure {
            kind,
            exit_code: info.status.and_then(|s| s.code()),
            stderr_tail: info.stderr_tail.clone(),
        })
    }

    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self.kind {
            FailureKind::CannotStart => "engines-failure-cannot-start",
            FailureKind::ExitedEarly => "engines-failure-exited-early",
            FailureKind::Crashed => "engines-failure-crashed",
            FailureKind::Timeout(_) => "engines-failure-timeout",
            FailureKind::StopTimeout => "engines-failure-stop-timeout",
            FailureKind::NotAnEngine => "engines-failure-not-an-engine",
            FailureKind::Io => "engines-failure-io",
            FailureKind::Closed => "engines-failure-closed",
            FailureKind::Other => "engines-failure-other",
        }
    }
}

/// Where an execution is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EngineRunState {
    /// The process is starting and the handshake is going on.
    Starting,
    /// Ready: it receives commands.
    Running,
    /// It did not start or it died; the run stays so the user sees why
    /// (and can restart it).
    Failed(EngineFailure),
    /// It is being ended.
    Stopping,
}

/// The position an engine is asked to search (D436): where the game
/// started and the moves from there to the cursor, variations included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisTarget {
    /// The first position of the game.
    pub start: Position,
    /// The moves from `start` to the cursor.
    pub moves: Vec<Move>,
}

/// One execution of an engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRun {
    /// The execution.
    pub id: RunId,
    /// The engine of the list it runs.
    pub engine: EngineId,
    /// Where it is.
    pub state: EngineRunState,
    /// What the engine said about itself (from `Running` on).
    pub identity: Option<Box<EngineIdentity>>,
    /// Whether the analysis is on: the engine searches the position shown.
    pub analyzing: bool,
    /// The analysis is locked on a position (set by the analysis window,
    /// slice 4): no search is sent when the position changes.
    pub locked: bool,
    /// The engine was started at low CPU priority (D446).
    pub low_priority: bool,
    /// The last position sent to the engine, and the game it belonged to.
    pub(crate) sent: Option<(Arc<AnalysisTarget>, u64)>,
    /// The game generation of the last position ever sent, kept when `sent`
    /// is cleared: an engine that saw a game needs `NewGame` before another.
    pub(crate) seen_game: Option<u64>,
}

impl EngineRun {
    /// Whether the engine is ready and analysing a position that follows the
    /// board.
    pub fn is_active(&self) -> bool {
        self.state == EngineRunState::Running && self.analyzing && !self.locked
    }
}

/// The executions, in the order they started.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineRuns {
    runs: Vec<EngineRun>,
    last: u64,
}

impl EngineRuns {
    /// The executions.
    pub fn as_slice(&self) -> &[EngineRun] {
        &self.runs
    }

    /// Number of executions (failed ones included).
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// Whether there is none.
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// The execution `id`.
    pub fn get(&self, id: RunId) -> Option<&EngineRun> {
        self.runs.iter().find(|r| r.id == id)
    }

    /// The execution of the engine `engine` (at most one, D433).
    pub fn of_engine(&self, engine: EngineId) -> Option<&EngineRun> {
        self.runs.iter().find(|r| r.engine == engine)
    }

    /// Whether any execution may still have a process.
    pub fn any_alive(&self) -> bool {
        self.runs
            .iter()
            .any(|r| !matches!(r.state, EngineRunState::Failed(_)))
    }

    pub(crate) fn get_mut(&mut self, id: RunId) -> Option<&mut EngineRun> {
        self.runs.iter_mut().find(|r| r.id == id)
    }

    /// Locks or unlocks the analysis of an execution on its position (the
    /// analysis window, spec M4-4): while locked, a change of position is
    /// not sent to the engine. Returns whether the execution exists.
    ///
    /// Unlocking forgets the position last sent, so the next update sends
    /// the position now shown. Prefer
    /// [`AnalysisMessage::ToggleLock`](crate::AnalysisMessage::ToggleLock),
    /// which also keeps the window's view of the lock.
    pub fn set_locked(&mut self, id: RunId, locked: bool) -> bool {
        let Some(run) = self.get_mut(id) else {
            return false;
        };
        if run.locked && !locked {
            run.sent = None;
        }
        run.locked = locked;
        true
    }

    /// Adds an execution in the `Starting` state.
    pub(crate) fn begin(&mut self, engine: EngineId, analyzing: bool) -> RunId {
        self.last = self.last.wrapping_add(1);
        let id = RunId(self.last);
        self.runs.push(EngineRun {
            id,
            engine,
            state: EngineRunState::Starting,
            identity: None,
            analyzing,
            locked: false,
            low_priority: false,
            sent: None,
            seen_game: None,
        });
        id
    }

    pub(crate) fn remove(&mut self, id: RunId) -> Option<EngineRun> {
        let at = self.runs.iter().position(|r| r.id == id)?;
        Some(self.runs.remove(at))
    }

    pub(crate) fn clear(&mut self) {
        self.runs.clear();
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut EngineRun> {
        self.runs.iter_mut()
    }
}

/// What a launcher returns.
pub type LaunchResult = Result<(Engine, EngineEvents), EngineError>;

/// The future of a launcher.
pub type LaunchFuture = Pin<Box<dyn Future<Output = LaunchResult> + Send>>;

/// How the hub starts an engine. The default starts the process; a test (or a
/// front-end with an embedded engine) substitutes its own.
pub type Launcher = Arc<dyn Fn(EngineConfig) -> LaunchFuture + Send + Sync>;

/// Room of the event channel, in events (D732).
const EVENT_QUEUE: usize = 1024;
/// Free slots kept for the events that must arrive: `Info` and warnings are
/// dropped when fewer than `EVENT_QUEUE / 2` are free, the other events
/// when fewer than this are, so that `Exited` always finds room.
const RESERVED_SLOTS: usize = 16;

/// The events of every engine, in one stream (D435).
#[derive(Debug)]
pub struct EngineEventStream {
    rx: mpsc::Receiver<(RunId, EngineEvent)>,
}

impl EngineEventStream {
    /// Waits for the next event; `None` when the hub is gone.
    pub async fn next(&mut self) -> Option<(RunId, EngineEvent)> {
        self.rx.recv().await
    }

    /// The next event if one is waiting.
    pub fn try_next(&mut self) -> Option<(RunId, EngineEvent)> {
        self.rx.try_recv().ok()
    }
}

/// A launch in progress: how to cancel it and how to wait until its future
/// (and the process it may hold) is gone.
struct Launching {
    abort: AbortHandle,
    gone: oneshot::Receiver<()>,
}

struct HubInner {
    engines: Mutex<HashMap<RunId, Engine>>,
    launching: Mutex<HashMap<RunId, Launching>>,
    tx: mpsc::Sender<(RunId, EngineEvent)>,
    rx: Mutex<Option<mpsc::Receiver<(RunId, EngineEvent)>>>,
    /// Set when the front-end took the receiver; until then only the events
    /// the `update` needs are kept.
    consumed: AtomicBool,
    launcher: Launcher,
}

/// The live engines and their event channel. Clones share the same hub;
/// equality and hashing are by identity, so it can travel in an
/// [`Effect`](crate::Effect) and a `Message`.
///
/// The channel is bounded (1024 events). Until [`EngineHub::events`] is
/// called nothing but `BestMove` and `Exited` is kept; afterwards `Info` and
/// warnings are dropped while the queue is more than half full (the
/// consumer is slow: the next report replaces them), and the other events
/// while fewer than 16 slots are free. `Exited` is never dropped: it waits
/// for room. Dropped reports are not an error: an `Info` is a snapshot.
#[derive(Clone)]
pub struct EngineHub {
    inner: Arc<HubInner>,
}

impl fmt::Debug for EngineHub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let alive = self.inner.engines.lock().map_or(0, |engines| engines.len());
        f.debug_struct("EngineHub").field("alive", &alive).finish()
    }
}

impl PartialEq for EngineHub {
    fn eq(&self, other: &EngineHub) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for EngineHub {}

impl Hash for EngineHub {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.inner).hash(state);
    }
}

impl Default for EngineHub {
    fn default() -> EngineHub {
        EngineHub::new()
    }
}

fn process_launcher() -> Launcher {
    Arc::new(|config| Box::pin(async move { Engine::start(config).await }))
}

impl EngineHub {
    /// A hub that starts real processes.
    pub fn new() -> EngineHub {
        EngineHub::with_launcher(process_launcher())
    }

    /// A hub that starts engines with `launcher`.
    pub fn with_launcher(launcher: Launcher) -> EngineHub {
        let (tx, rx) = mpsc::channel(EVENT_QUEUE);
        EngineHub {
            inner: Arc::new(HubInner {
                engines: Mutex::default(),
                launching: Mutex::default(),
                tx,
                rx: Mutex::new(Some(rx)),
                consumed: AtomicBool::new(false),
                launcher,
            }),
        }
    }

    /// Takes the receiver of the events. There is one: the first call gets
    /// it (the front-end's subscription), later calls get `None`.
    pub fn events(&self) -> Option<EngineEventStream> {
        let rx = self
            .inner
            .rx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()?;
        self.inner.consumed.store(true, Ordering::Release);
        Some(EngineEventStream { rx })
    }

    /// The protocol log of an execution, if its engine is alive.
    pub fn log(&self, run: RunId) -> Option<LogHandle> {
        self.get(run).map(|engine| engine.log())
    }

    /// How many engines are alive.
    pub fn alive(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<RunId, Engine>> {
        self.inner
            .engines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn get(&self, run: RunId) -> Option<Engine> {
        self.lock().get(&run).cloned()
    }

    pub(crate) fn remove(&self, run: RunId) -> Option<Engine> {
        self.lock().remove(&run)
    }

    pub(crate) fn launcher(&self) -> Launcher {
        Arc::clone(&self.inner.launcher)
    }

    /// Events waiting in the channel.
    #[cfg(test)]
    pub(crate) fn pending_events(&self) -> usize {
        self.inner.tx.max_capacity() - self.inner.tx.capacity()
    }

    /// Keeps `engine` under `run` and forwards its events to the channel
    /// until the session ends; then the engine is dropped from the hub.
    pub(crate) fn adopt(&self, run: RunId, engine: Engine, mut events: EngineEvents) {
        self.lock().insert(run, engine);
        let hub = self.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let exited = matches!(event, EngineEvent::Exited(_));
                hub.forward(run, event).await;
                if exited {
                    break;
                }
            }
            drop(hub.remove(run));
        });
    }

    /// Puts one event in the channel, or drops it as the type documents.
    /// Never waits, except for `Exited`.
    async fn forward(&self, run: RunId, event: EngineEvent) {
        let tx = &self.inner.tx;
        if matches!(event, EngineEvent::Exited(_)) {
            // The receiver may be gone (no front-end): then it is no loss.
            tx.send((run, event)).await.ok();
            return;
        }
        let consumed = self.inner.consumed.load(Ordering::Acquire);
        let keep = match event {
            EngineEvent::BestMove(_) => true,
            _ if !consumed => false,
            EngineEvent::Info(_) | EngineEvent::Warning(_) => tx.capacity() > EVENT_QUEUE / 2,
            _ => true,
        };
        if keep && tx.capacity() > RESERVED_SLOTS {
            tx.try_send((run, event)).ok();
        }
    }

    /// Starts the engine `config` describes through the launcher, as a task
    /// the hub knows: [`stop`](Self::stop) and
    /// [`shutdown_all`](Self::shutdown_all) cancel it (dropping the process)
    /// and wait until it is gone. `None` when it was cancelled.
    pub(crate) async fn launch(&self, run: RunId, config: EngineConfig) -> Option<LaunchResult> {
        let (result_tx, result_rx) = oneshot::channel();
        let (gone_tx, gone) = oneshot::channel::<()>();
        let launcher = self.launcher();
        let task = tokio::spawn(async move {
            let _gone = gone_tx;
            result_tx.send(launcher(config).await).ok();
        });
        self.lock_launching().insert(
            run,
            Launching {
                abort: task.abort_handle(),
                gone,
            },
        );
        let result = result_rx.await.ok();
        drop(self.lock_launching().remove(&run));
        result
    }

    fn lock_launching(&self) -> std::sync::MutexGuard<'_, HashMap<RunId, Launching>> {
        self.inner
            .launching
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// How many launches are in progress.
    #[cfg(test)]
    pub(crate) fn launching(&self) -> usize {
        self.lock_launching().len()
    }

    /// Sends a command to the engine of `run`; never waits.
    pub(crate) fn send(&self, run: RunId, command: EngineCommand) -> Result<(), CommandError> {
        let engine = self.get(run).ok_or(CommandError::NotRunning)?;
        engine.send(command).map_err(CommandError::from)
    }

    /// Ends the engine of `run` (cancelling its launch if it is still
    /// starting) and waits for the process to be gone.
    pub(crate) async fn stop(&self, run: RunId) {
        let launch = self.lock_launching().remove(&run);
        if let Some(launch) = launch {
            launch.abort.abort();
            launch.gone.await.ok();
        }
        if let Some(engine) = self.remove(run) {
            engine.shutdown().await;
        }
    }

    /// Ends every engine and cancels every launch, waiting `wait` at most;
    /// the processes that are still alive then are killed when their
    /// handles drop.
    pub(crate) async fn shutdown_all(&self, wait: Duration) {
        let launches: Vec<Launching> = self.lock_launching().drain().map(|(_, l)| l).collect();
        for launch in &launches {
            launch.abort.abort();
        }
        let engines: Vec<Engine> = self.lock().drain().map(|(_, engine)| engine).collect();
        let tasks: Vec<_> = engines
            .into_iter()
            .map(|engine| tokio::spawn(engine.shutdown()))
            .collect();
        tokio::time::timeout(wait, async {
            for launch in launches {
                launch.gone.await.ok();
            }
            for task in tasks {
                task.await.ok();
            }
        })
        .await
        .ok();
    }
}

/// Which command an [`EngineMessage::CommandFailed`](super::EngineMessage)
/// is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandKind {
    /// A search.
    Search,
    /// `Stop`.
    Stop,
    /// A `setoption`.
    SetOption,
    /// A change of the number of lines.
    SetMultiPv,
    /// Any other command.
    Other,
}

impl CommandKind {
    /// The kind of `command`.
    pub fn of(command: &EngineCommand) -> CommandKind {
        match command {
            EngineCommand::Search(_) => CommandKind::Search,
            EngineCommand::Stop => CommandKind::Stop,
            EngineCommand::SetOption { .. } => CommandKind::SetOption,
            EngineCommand::SetMultiPv(_) => CommandKind::SetMultiPv,
            _ => CommandKind::Other,
        }
    }
}

/// Why a command did not reach an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandError {
    /// The execution has no live engine (it ended meanwhile).
    NotRunning,
    /// The engine's queue is full.
    Busy,
    /// The engine cannot do it (an option out of range, `MultiPV`,
    /// Chess960...).
    Refused,
    /// The session is closed.
    Closed,
}

impl From<EngineError> for CommandError {
    fn from(error: EngineError) -> CommandError {
        match error {
            EngineError::Busy => CommandError::Busy,
            EngineError::Closed => CommandError::Closed,
            _ => CommandError::Refused,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_get_increasing_ids_and_one_run_per_engine_is_found() {
        let mut runs = EngineRuns::default();
        let a = runs.begin(EngineId(1), true);
        let b = runs.begin(EngineId(2), false);
        assert!(a < b);
        assert_eq!(runs.of_engine(EngineId(2)).map(|r| r.id), Some(b));
        assert!(runs.remove(a).is_some());
        let c = runs.begin(EngineId(1), true);
        assert!(c > b, "ids are never reused");
        assert_eq!(runs.len(), 2);
    }

    #[test]
    fn only_a_ready_analysing_unlocked_run_is_active() {
        let mut runs = EngineRuns::default();
        let id = runs.begin(EngineId(1), true);
        assert!(!runs.get(id).is_some_and(EngineRun::is_active));
        let run = runs.get_mut(id).expect("run");
        run.state = EngineRunState::Running;
        assert!(run.is_active());
        run.locked = true;
        assert!(!run.is_active());
        run.locked = false;
        run.analyzing = false;
        assert!(!run.is_active());
    }

    #[test]
    fn hub_equality_is_identity_and_events_are_taken_once() {
        let a = EngineHub::new();
        let b = a.clone();
        assert_eq!(a, b);
        assert_ne!(a, EngineHub::new());
        assert!(a.events().is_some());
        assert!(b.events().is_none());
    }

    #[test]
    fn failures_keep_exit_code_and_stderr() {
        let failure = EngineFailure::from_error(&EngineError::ExitedEarly {
            status: None,
            stderr_tail: vec!["boom".to_owned()],
        });
        assert_eq!(failure.kind, FailureKind::ExitedEarly);
        assert_eq!(failure.stderr_tail, ["boom"]);
        let quit = ExitInfo {
            reason: ExitReason::Quit,
            status: None,
            stderr_tail: Vec::new(),
        };
        assert_eq!(EngineFailure::from_exit(&quit), None);
        let crashed = ExitInfo {
            reason: ExitReason::Crashed,
            ..quit
        };
        assert_eq!(
            EngineFailure::from_exit(&crashed).map(|f| f.kind),
            Some(FailureKind::Crashed)
        );
    }

    use mychess_core::Position;
    use mychess_engine::testing::ManualEngine;
    use mychess_engine::{SearchPosition, SearchRequest};
    use std::fmt::Write as _;
    use std::sync::atomic::AtomicUsize;

    /// Adopts a ready manual engine that is searching; returns the peer.
    async fn searching_hub(hub: &EngineHub, run: RunId) -> ManualEngine {
        let mut config = EngineConfig::new("fake");
        config.info_interval = Duration::ZERO;
        let (mut peer, engine, events) = ManualEngine::launch(config);
        peer.handshake("Manual", &[]).await;
        hub.adopt(run, engine, events);
        tokio::time::sleep(Duration::from_millis(10)).await;
        let search = SearchRequest::infinite(SearchPosition::new(Position::default()));
        assert!(hub.send(run, EngineCommand::Search(search)).is_ok());
        for _ in 0..2 {
            assert!(peer.next_line().await.is_some());
        }
        peer
    }

    async fn flood(peer: &mut ManualEngine) {
        let mut blob = String::new();
        for depth in 1..=100_000_u32 {
            writeln!(
                blob,
                "info depth {depth} multipv 1 score cp {depth} pv e2e4"
            )
            .ok();
        }
        peer.say_raw(blob.as_bytes()).await;
        peer.say("bestmove e2e4").await;
        peer.close().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_consumer_a_flood_of_infos_is_dropped_but_exit_arrives() {
        let hub = EngineHub::new();
        let mut peer = searching_hub(&hub, RunId(1)).await;
        flood(&mut peer).await;
        assert!(
            hub.pending_events() <= 2,
            "{} events queued",
            hub.pending_events()
        );
        let mut stream = hub.events().expect("events");
        let mut kinds = Vec::new();
        while let Some((run, event)) = stream.try_next() {
            assert_eq!(run, RunId(1));
            kinds.push(matches!(event, EngineEvent::Exited(_)));
        }
        assert_eq!(kinds.last(), Some(&true), "Exited is the last event");
        assert_eq!(hub.alive(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn with_a_slow_consumer_the_queue_stays_bounded_and_exit_still_arrives() {
        let hub = EngineHub::new();
        let mut stream = hub.events().expect("events");
        let mut peer = searching_hub(&hub, RunId(1)).await;
        flood(&mut peer).await;
        assert!(
            hub.pending_events() <= EVENT_QUEUE,
            "{} events queued",
            hub.pending_events()
        );
        let mut infos = 0;
        let exited = loop {
            match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                Ok(Some((_, EngineEvent::Info(_)))) => infos += 1,
                Ok(Some((_, EngineEvent::Exited(_)))) => break true,
                Ok(Some(_)) => {}
                _ => break false,
            }
        };
        assert!(exited, "Exited never arrived");
        assert!(infos <= EVENT_QUEUE, "{infos} infos");
    }

    /// A launcher whose future never ends and notes when it is dropped.
    fn pending_launcher(dropped: &Arc<AtomicUsize>) -> Launcher {
        struct Guard(Arc<AtomicUsize>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let dropped = Arc::clone(dropped);
        Arc::new(move |_| {
            let guard = Guard(Arc::clone(&dropped));
            Box::pin(async move {
                let _guard = guard;
                std::future::pending::<LaunchResult>().await
            })
        })
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_all_cancels_a_launch_that_is_still_starting() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let hub = EngineHub::with_launcher(pending_launcher(&dropped));
        let task = tokio::spawn({
            let hub = hub.clone();
            async move {
                hub.launch(RunId(1), EngineConfig::new("fake"))
                    .await
                    .is_none()
            }
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(hub.launching(), 1);
        hub.shutdown_all(Duration::from_millis(1500)).await;
        assert_eq!(dropped.load(Ordering::SeqCst), 1, "the launch was dropped");
        assert!(task.await.expect("task"), "the launch answered cancelled");
        assert_eq!(hub.launching(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn stop_cancels_a_launch_that_is_still_starting_and_waits_for_it() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let hub = EngineHub::with_launcher(pending_launcher(&dropped));
        let task = tokio::spawn({
            let hub = hub.clone();
            async move {
                hub.launch(RunId(7), EngineConfig::new("fake"))
                    .await
                    .is_none()
            }
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        hub.stop(RunId(7)).await;
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert!(task.await.expect("task"));
    }
}
