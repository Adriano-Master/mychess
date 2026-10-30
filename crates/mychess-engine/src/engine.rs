//! The public engine API: configuration, commands, events and the [`Engine`]
//! handle.
//!
//! An [`Engine`] is a cheap, clonable handle to a *session*: one task that
//! owns the engine process and speaks the protocol. Commands go in through
//! [`Engine::send`], which never blocks; everything the engine says comes out
//! as [`EngineEvent`]s from the single [`EngineEvents`] receiver.
//!
//! ```no_run
//! use mychess_core::Position;
//! use mychess_engine::{
//!     Engine, EngineCommand, EngineConfig, EngineEvent, SearchPosition, SearchRequest,
//! };
//!
//! # async fn demo() -> Result<(), mychess_engine::EngineError> {
//! let (engine, mut events) = Engine::start(EngineConfig::new("/usr/games/stockfish")).await?;
//! let request = SearchRequest::infinite(SearchPosition::new(Position::new()));
//! engine.send(EngineCommand::Search(request))?;
//! while let Some(event) = events.recv().await {
//!     if let EngineEvent::Info(info) = event {
//!         println!("{:?} {:?}", info.score, info.pv);
//!         break;
//!     }
//! }
//! engine.shutdown().await;
//! # Ok(())
//! # }
//! ```

use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use mychess_core::Move;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};

use crate::log::{DEFAULT_LOG_LINES, LogHandle};
use crate::option::{OptionError, OptionValue, is_managed_option};
use crate::protocol::{Capabilities, EngineIdentity, Protocol, SearchId, SearchInfo};
use crate::request::SearchRequest;
use crate::uci::session::{Session, SessionParts};
use crate::wire::Wire;
use crate::{process, uci, xboard};

/// Capacity of the command channel: a full channel answers
/// [`EngineError::Busy`].
pub(crate) const COMMAND_QUEUE: usize = 16;
/// Capacity of the event channel (spec m4-1, D412).
pub(crate) const EVENT_QUEUE: usize = 1024;

/// The deadlines of the session (spec m4-1, D404). New deadlines may be
/// added, so build one from [`Timeouts::default`] and assign the fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Timeouts {
    /// Waiting for `uciok` after `uci`.
    pub uciok: Duration,
    /// Waiting for `readyok` after `isready` (loading hash or tablebases).
    pub readyok: Duration,
    /// Waiting for `bestmove` after `stop`.
    pub stop: Duration,
    /// After `stop` timed out: waiting for the `readyok` that proves the
    /// engine is alive, before giving up on it.
    pub stop_recovery: Duration,
    /// Waiting for the process to exit after `quit`, before killing it.
    pub quit: Duration,
    /// XBoard: waiting for `feature` lines after `protover 2`, and between
    /// two of them (spec m4-2, D416). An engine that stays silent this long
    /// is taken for a `protover 1` engine.
    pub feature: Duration,
    /// XBoard: the ceiling for the whole announcement when the engine said
    /// `done=0` (it then needs more time to start up).
    pub feature_done: Duration,
    /// XBoard: waiting for `pong` after `ping`.
    pub pong: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            uciok: Duration::from_secs(10),
            readyok: Duration::from_secs(30),
            stop: Duration::from_millis(1500),
            stop_recovery: Duration::from_millis(1500),
            quit: Duration::from_millis(500),
            feature: Duration::from_secs(2),
            feature_done: Duration::from_secs(15),
            pong: Duration::from_secs(2),
        }
    }
}

/// How to start and run an engine. New settings may be added, so build one
/// with [`EngineConfig::new`] and the `with_*` methods or assign the fields.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EngineConfig {
    /// A display name, used until the engine announces its own.
    pub name: String,
    /// The executable. A `.bat` or `.cmd` script is not supported on Windows:
    /// point to the `.exe`.
    pub command: PathBuf,
    /// Command line arguments, one per element (no shell is involved).
    pub args: Vec<String>,
    /// The working directory; the engine's own directory when `None`.
    pub working_dir: Option<PathBuf>,
    /// The protocol.
    pub protocol: Protocol,
    /// Saved option values, sent after the handshake (options the interface
    /// manages itself are skipped).
    pub options: Vec<(String, OptionValue)>,
    /// Run the engine at low CPU priority. Changing it needs a restart.
    pub low_priority: bool,
    /// The deadlines.
    pub timeouts: Timeouts,
    /// Size of the protocol log ring; `0` turns the log off.
    pub log_lines: usize,
    /// Minimum time between two [`EngineEvent::Info`] of the same line.
    pub info_interval: Duration,
}

impl EngineConfig {
    /// A configuration with the defaults: UCI, normal priority, no saved
    /// options, a 5000-line log and a 50 ms `info` interval.
    pub fn new(command: impl Into<PathBuf>) -> EngineConfig {
        let command = command.into();
        let name = command.file_stem().map_or_else(
            || command.display().to_string(),
            |s| s.to_string_lossy().into_owned(),
        );
        EngineConfig {
            name,
            command,
            args: Vec::new(),
            working_dir: None,
            protocol: Protocol::Uci,
            options: Vec::new(),
            low_priority: false,
            timeouts: Timeouts::default(),
            log_lines: DEFAULT_LOG_LINES,
            info_interval: Duration::from_millis(50),
        }
    }
}

impl EngineConfig {
    /// Sets the display name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the command line arguments.
    #[must_use]
    pub fn with_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the working directory.
    #[must_use]
    pub fn with_working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Sets the saved option values.
    #[must_use]
    pub fn with_options(mut self, options: Vec<(String, OptionValue)>) -> Self {
        self.options = options;
        self
    }

    /// Sets low CPU priority.
    #[must_use]
    pub fn with_low_priority(mut self, low: bool) -> Self {
        self.low_priority = low;
        self
    }

    /// Sets the deadlines.
    #[must_use]
    pub fn with_timeouts(mut self, timeouts: Timeouts) -> Self {
        self.timeouts = timeouts;
        self
    }
}

/// The step of the session an operation timed out in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Waiting for `uciok`.
    Uciok,
    /// Waiting for `readyok`.
    Readyok,
    /// Waiting for `bestmove` after `stop`.
    Bestmove,
    /// XBoard: waiting for `done=1` after `done=0`.
    Features,
    /// XBoard: waiting for `pong`.
    Pong,
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Phase::Uciok => "uciok",
            Phase::Readyok => "readyok",
            Phase::Bestmove => "bestmove",
            Phase::Features => "feature",
            Phase::Pong => "pong",
        })
    }
}

/// Why an engine operation failed.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The program could not be started.
    #[error("unable to start the program `{}`: {source}", path.display())]
    Spawn {
        /// The executable.
        path: PathBuf,
        /// The operating system error.
        source: io::Error,
    },
    /// Communication with the engine failed.
    #[error("engine I/O error: {0}")]
    Io(#[from] io::Error),
    /// The engine did not answer in time.
    #[error("the engine did not answer `{0}` in time")]
    Timeout(Phase),
    /// The engine ended before the handshake finished.
    #[error("the engine terminated before it was ready (status: {status:?})")]
    ExitedEarly {
        /// The exit status, if known.
        status: Option<ExitStatus>,
        /// The last lines the engine wrote to its standard error.
        stderr_tail: Vec<String>,
    },
    /// The engine cannot do what was asked.
    #[error("not supported by this engine: {0}")]
    Unsupported(&'static str),
    /// A Chess960 game, but the engine has no `UCI_Chess960` option.
    #[error("the engine does not support Chess960")]
    UnsupportedVariant,
    /// An option value was refused.
    #[error("invalid option: {0}")]
    InvalidOption(#[from] OptionError),
    /// The command queue is full; try again later.
    #[error("the engine command queue is full")]
    Busy,
    /// The session has ended.
    #[error("the engine session is closed")]
    Closed,
    /// The program speaks neither UCI nor XBoard (see [`detect_protocol`]).
    #[error("the engine speaks neither UCI nor XBoard")]
    Unrecognised,
}

/// A command for the engine session.
///
/// Which commands are valid in which state is described in spec m4-1, rule 1:
/// during start-up they wait their turn; during a search `Search`,
/// `SetOption`, `SetMultiPv` and `NewGame` first stop the running search.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[allow(clippy::large_enum_variant)] // commands are rare; boxing would burden every caller
pub enum EngineCommand {
    /// A new game started (another game was loaded, not navigation).
    ///
    /// UCI: queued like an option; it stops a running search and sends
    /// `ucinewgame` (then waits for `readyok`). XBoard: nothing is sent and a
    /// running search is left alone; the engine is only marked fresh, so the
    /// next search starts with `new` (and, without `setboard`, rebuilds the
    /// board from the moves).
    NewGame,
    /// Sets an option. Restarts the search that was running.
    SetOption {
        /// The option name as announced by the engine.
        name: String,
        /// The new value, validated against the announced type.
        value: OptionValue,
    },
    /// Sets the number of lines (`MultiPV`), limited to the engine's range.
    /// Restarts the search that was running.
    SetMultiPv(u8),
    /// Searches a position. Replaces the search in progress.
    ///
    /// A search that was pending when the session ends with
    /// [`ExitReason::StopTimeout`] dies with it: the application restarts the
    /// engine and sends the search again.
    Search(SearchRequest),
    /// Stops the search; its `bestmove` is still delivered.
    Stop,
    /// The opponent played the expected move (`ponderhit`).
    PonderHit,
    /// Turns the `Ponder` option on or off (UCI only: `Ponder` is managed by
    /// the interface, so `SetOption` refuses it). Queued like an option: it
    /// stops a running search, waits for `readyok`, and is refused without
    /// the option and on XBoard (M8-1d, D2402).
    SetPonder(bool),
    /// Asks an XBoard engine for its statistics (`.`, answered by `stat01:`
    /// as an [`EngineEvent::Info`] without a line). Ignored by UCI engines.
    RequestStats,
    /// Tells the engine who it plays against (spec M8-2c, D2524). XBoard:
    /// `name <opponent>` after every `new`. UCI: ignored. The text is cleaned
    /// before it is written (no control characters, at most
    /// [`MAX_PEER_TEXT`] characters).
    Opponent(String),
    /// The game ended (spec M8-2c, D2524). XBoard: `result <result>
    /// {<comment>}`. UCI: ignored. `result` is `1-0`, `0-1`, `1/2-1/2` or
    /// `*`; anything else is sent as `*`. The comment is cleaned like the
    /// opponent's name and loses its braces.
    GameOver {
        /// The result of the game.
        result: String,
        /// Why the game ended.
        comment: String,
    },
    /// Ends the session: `quit`, a short wait and then a kill.
    Quit,
}

/// The longest text of an opponent name or a result comment sent to an
/// engine, in characters.
pub const MAX_PEER_TEXT: usize = 64;

/// Makes `text` safe to put inside a line sent to an engine: control
/// characters (and so line breaks) become spaces, `braces` are dropped when
/// asked, runs of spaces collapse and the length is limited.
pub(crate) fn clean_peer_text(text: &str, braces: bool) -> String {
    let mut out = String::new();
    let mut last_space = true;
    for c in text.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if !braces && (c == '{' || c == '}') {
            continue;
        }
        if c == ' ' {
            if last_space {
                continue;
            }
            last_space = true;
        } else {
            last_space = false;
        }
        out.push(c);
        if out.chars().count() >= MAX_PEER_TEXT {
            break;
        }
    }
    out.trim().to_owned()
}

/// The result text an XBoard `result` command takes.
pub(crate) fn clean_result(result: &str) -> &'static str {
    match result.trim() {
        "1-0" => "1-0",
        "0-1" => "0-1",
        "1/2-1/2" => "1/2-1/2",
        _ => "*",
    }
}

/// The result of a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BestMove {
    /// The search that ended.
    pub search: SearchId,
    /// The chosen move; `None` when the position has no moves, or when the
    /// engine named a move that is illegal in the searched position.
    pub mv: Option<Move>,
    /// The move the engine expects in reply (to ponder on).
    pub ponder: Option<Move>,
}

/// Something worth telling the user that does not end the session.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum EngineWarning {
    /// A command was refused (invalid option, unsupported feature, queue
    /// full, illegal move in the requested position).
    #[error("`{command}` was refused: {reason}")]
    CommandRefused {
        /// Which command.
        command: &'static str,
        /// Why.
        reason: String,
    },
    /// The engine answered `bestmove` with an illegal move.
    #[error("{0}")]
    IllegalBestMove(String),
    /// A saved option could not be applied.
    #[error("saved option `{name}` was not applied: {reason}")]
    OptionSkipped {
        /// The option name.
        name: String,
        /// Why.
        reason: String,
    },
    /// The engine broke the protocol. Either the session recovered, or this
    /// precedes the end of the session (an engine that never answered `stop`
    /// gets this, then the closing `BestMove` and `Exited`).
    #[error("{0}")]
    Misbehaved(String),
}

/// Why a session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExitReason {
    /// `Quit` or [`Engine::shutdown`].
    Quit,
    /// The engine closed its output or exited by itself.
    Crashed,
    /// A deadline passed: during the handshake, while waiting for a barrier
    /// `readyok` (UCI) or for `pong` (XBoard).
    Timeout(Phase),
    /// The engine did not answer `stop`/`?` with a move and then either did
    /// not answer `isready`/`ping` or answered it and still ignored the stop.
    StopTimeout,
    /// A pipe failed.
    Io(String),
    /// All handles were dropped.
    Dropped,
}

/// Final report of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitInfo {
    /// Why the session ended.
    pub reason: ExitReason,
    /// The exit status of the process, when known.
    pub status: Option<ExitStatus>,
    /// The last (up to 10) lines of the engine's standard error.
    pub stderr_tail: Vec<String>,
}

/// What an engine session reports.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EngineEvent {
    /// The handshake is done; commands now run immediately.
    Ready(EngineIdentity),
    /// The request with a non-zero [`SearchRequest::tag`](crate::SearchRequest::tag)
    /// is about to start as `search`. Delivered right before
    /// [`EngineEvent::SearchStarted`] of the same search, so the front-end
    /// knows which of its requests started (a replaced one never does).
    SearchTagged {
        /// The search that starts.
        search: SearchId,
        /// The label of the request.
        tag: u64,
    },
    /// A search began (its `go` was sent).
    SearchStarted(SearchId),
    /// An analysis report, rate limited (spec m4-1, D412).
    Info(SearchInfo),
    /// A search ended. Delivered for every search that started, including
    /// stopped and replaced ones, unless the session itself ends first
    /// ([`EngineEvent::Exited`] then closes the stream).
    BestMove(BestMove),
    /// A non-fatal problem.
    Warning(EngineWarning),
    /// The session ended; nothing comes after this.
    Exited(ExitInfo),
}

/// The receiving end of the events of an engine. There is exactly one.
#[derive(Debug)]
pub struct EngineEvents {
    rx: mpsc::Receiver<EngineEvent>,
}

impl EngineEvents {
    /// Waits for the next event; `None` after the session ended and every
    /// event was read.
    pub async fn recv(&mut self) -> Option<EngineEvent> {
        self.rx.recv().await
    }

    /// The next event if one is waiting.
    pub fn try_recv(&mut self) -> Option<EngineEvent> {
        self.rx.try_recv().ok()
    }
}

/// State shared between the handle and the session.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) identity: OnceLock<EngineIdentity>,
    pid: AtomicU32,
    /// Raised by `Quit` and `shutdown`; seen by the session even when the
    /// command queue is full.
    pub(crate) quit: watch::Sender<bool>,
    /// The protocol the session speaks.
    pub(crate) protocol: Protocol,
    /// XBoard engine that cannot take a position by FEN: only searches from
    /// the standard start position are accepted (spec m4-2, D418).
    pub(crate) standard_start_only: AtomicBool,
}

impl Default for Shared {
    fn default() -> Self {
        Shared::new(Protocol::Uci)
    }
}

impl Shared {
    pub(crate) fn new(protocol: Protocol) -> Shared {
        Shared {
            identity: OnceLock::new(),
            pid: AtomicU32::new(0),
            quit: watch::channel(false).0,
            protocol,
            standard_start_only: AtomicBool::new(false),
        }
    }

    pub(crate) fn set_pid(&self, pid: Option<u32>) {
        self.pid.store(pid.unwrap_or(0), Ordering::Relaxed);
    }
}

/// A handle to an engine session. Clones share the same session.
#[derive(Debug, Clone)]
pub struct Engine {
    commands: mpsc::Sender<EngineCommand>,
    shared: Arc<Shared>,
    log: LogHandle,
    done: watch::Receiver<()>,
}

/// The pieces of a freshly launched session.
pub(crate) struct Launched {
    pub(crate) engine: Engine,
    pub(crate) events: EngineEvents,
    pub(crate) startup: oneshot::Receiver<Result<(), EngineError>>,
}

/// Starts the session task on `wire`.
pub(crate) fn launch_on(config: EngineConfig, wire: Wire, log: LogHandle) -> Launched {
    let (command_tx, command_rx) = mpsc::channel(COMMAND_QUEUE);
    let (event_tx, event_rx) = mpsc::channel(EVENT_QUEUE);
    let (startup_tx, startup_rx) = oneshot::channel();
    let (done_tx, done_rx) = watch::channel(());
    let shared = Arc::new(Shared::new(config.protocol));
    shared.set_pid(wire.pid());
    let parts = SessionParts {
        commands: command_rx,
        events: event_tx,
        startup: Some(startup_tx),
        done: done_tx,
    };
    match config.protocol {
        Protocol::Uci => {
            let session = Session::new(config, wire, log.clone(), Arc::clone(&shared), parts);
            tokio::spawn(session.run());
        }
        Protocol::XBoard => {
            let session = xboard::session::Session::new(
                config,
                wire,
                log.clone(),
                Arc::clone(&shared),
                parts,
            );
            tokio::spawn(session.run());
        }
    }
    Launched {
        engine: Engine {
            commands: command_tx,
            shared,
            log,
            done: done_rx,
        },
        events: EngineEvents { rx: event_rx },
        startup: startup_rx,
    }
}

impl Engine {
    /// Starts the engine and waits for the handshake: the process is
    /// launched, `uci` is answered, the saved options are sent and the engine
    /// is ready. The first event is [`EngineEvent::Ready`].
    ///
    /// Fails when the program cannot be started, exits early, or misses a
    /// deadline; no process is left behind.
    ///
    /// # Panics
    ///
    /// Must be called inside a Tokio runtime (it spawns tasks); outside one
    /// the Tokio spawn functions panic.
    pub async fn start(config: EngineConfig) -> Result<(Engine, EngineEvents), EngineError> {
        let launched = Engine::launch_detached(config)?;
        match launched.startup.await {
            Ok(Ok(())) => Ok((launched.engine, launched.events)),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(EngineError::Closed),
        }
    }

    /// Starts the engine without waiting for the handshake. Commands sent now
    /// wait for the engine to be ready; a failed handshake arrives as
    /// [`EngineEvent::Exited`].
    ///
    /// # Panics
    ///
    /// Must be called inside a Tokio runtime, like [`Engine::start`].
    pub fn launch(config: EngineConfig) -> Result<(Engine, EngineEvents), EngineError> {
        let launched = Engine::launch_detached(config)?;
        Ok((launched.engine, launched.events))
    }

    fn launch_detached(config: EngineConfig) -> Result<Launched, EngineError> {
        let log = LogHandle::new(config.log_lines);
        let wire = process::spawn(&config, &log)?;
        Ok(launch_on(config, wire, log))
    }

    /// Sends a command to the session. Never blocks.
    ///
    /// Once the engine is ready the command is checked right here: an option
    /// value outside the announced range, `MultiPV` on an engine without it
    /// and a Chess960 search on an engine without `UCI_Chess960` fail with an
    /// error instead of reaching the engine.
    pub fn send(&self, command: EngineCommand) -> Result<(), EngineError> {
        if command == EngineCommand::Quit {
            // `Quit` always gets through, even when the queue is full.
            return if self.commands.is_closed() {
                Err(EngineError::Closed)
            } else {
                self.shared.quit.send_replace(true);
                Ok(())
            };
        }
        if command == EngineCommand::RequestStats && self.shared.protocol == Protocol::Uci {
            return Ok(());
        }
        if let EngineCommand::Search(request) = &command
            && self.shared.standard_start_only.load(Ordering::Relaxed)
            && let Some(reason) = xboard::format::unsupported_start(&request.position)
        {
            return Err(EngineError::Unsupported(reason));
        }
        let command = match self.shared.identity.get() {
            Some(identity) => check_command(identity, self.shared.protocol, command)?,
            None => command,
        };
        self.commands.try_send(command).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => EngineError::Busy,
            mpsc::error::TrySendError::Closed(_) => EngineError::Closed,
        })
    }

    /// The protocol log of this engine.
    pub fn log(&self) -> LogHandle {
        self.log.clone()
    }

    /// Who the engine says it is; `None` until the handshake is done.
    pub fn identity(&self) -> Option<&EngineIdentity> {
        self.shared.identity.get()
    }

    /// The operating system process id of a real engine process.
    pub fn process_id(&self) -> Option<u32> {
        match self.shared.pid.load(Ordering::Relaxed) {
            0 => None,
            pid => Some(pid),
        }
    }

    /// Ends the session politely (`quit`, a short wait, then a kill) and
    /// waits until the process is gone.
    pub async fn shutdown(self) {
        let mut done = self.done.clone();
        self.shared.quit.send_replace(true);
        // Keep the handle (and so the command channel) open until the session
        // is gone: closing it early races with the quit flag.
        tokio::time::timeout(Duration::from_secs(5), async {
            while done.changed().await.is_ok() {}
        })
        .await
        .ok();
        drop(self);
    }
}

/// Validates a command against what the engine announced.
pub(crate) fn check_command(
    identity: &EngineIdentity,
    protocol: Protocol,
    command: EngineCommand,
) -> Result<EngineCommand, EngineError> {
    match command {
        EngineCommand::SetOption { name, value } => {
            let (option, value) = resolve_option(identity, &name, &value)?;
            Ok(EngineCommand::SetOption {
                name: option.name.clone(),
                value,
            })
        }
        EngineCommand::SetMultiPv(n) => match &identity.capabilities.multipv {
            Some(range) => Ok(EngineCommand::SetMultiPv(
                n.clamp(*range.start(), *range.end()),
            )),
            // An XBoard engine has a single line: asking for one is a no-op, more is refused.
            None if n <= 1 && protocol == Protocol::XBoard => Ok(EngineCommand::SetMultiPv(1)),
            None => Err(EngineError::Unsupported("MultiPV")),
        },
        EngineCommand::Search(request) => {
            check_variant(&identity.capabilities, &request)?;
            if protocol == Protocol::XBoard {
                // XBoard: neither `searchmoves` nor pondering exist.
                if !request.search_moves.is_empty() {
                    return Err(EngineError::Unsupported("searchmoves"));
                }
                if request.ponder {
                    return Err(EngineError::Unsupported("ponder"));
                }
            }
            Ok(EngineCommand::Search(request))
        }
        EngineCommand::SetPonder(on) => {
            if protocol == Protocol::XBoard || !identity.capabilities.ponder {
                return Err(EngineError::Unsupported("Ponder"));
            }
            Ok(EngineCommand::SetPonder(on))
        }
        other => Ok(other),
    }
}

pub(crate) fn check_variant(
    capabilities: &Capabilities,
    request: &SearchRequest,
) -> Result<(), EngineError> {
    if request.position.start.variant() == mychess_core::Variant::Chess960 && !capabilities.chess960
    {
        return Err(EngineError::UnsupportedVariant);
    }
    Ok(())
}

/// Finds the option and normalizes the value; refuses options managed by the
/// interface.
pub(crate) fn resolve_option<'a>(
    identity: &'a EngineIdentity,
    name: &str,
    value: &OptionValue,
) -> Result<(&'a crate::option::EngineOption, OptionValue), OptionError> {
    if is_managed_option(name) {
        return Err(OptionError::Managed(name.to_owned()));
    }
    let option = identity
        .options
        .iter()
        .find(|o| o.name == name)
        .or_else(|| {
            identity
                .options
                .iter()
                .find(|o| o.name.eq_ignore_ascii_case(name))
        })
        .ok_or_else(|| OptionError::Unknown(name.to_owned()))?;
    let value = option.validate(value)?;
    Ok((option, value))
}

/// Starts the engine, learns its options and ends it again (the Scid's
/// `uciConfig`, `uci.tcl:512`). The options are not sent and the log is
/// discarded.
///
/// Meant for the options dialog and for "test engine" when registering one.
///
/// # Panics
///
/// Must be called inside a Tokio runtime (the pipes are served by tasks).
pub async fn probe_uci(config: EngineConfig) -> Result<EngineIdentity, EngineError> {
    let log = LogHandle::new(config.log_lines);
    let wire = process::spawn(&config, &log)?;
    uci::handshake::probe(wire, &config, &log).await
}

/// Starts an XBoard engine, runs the `protover 2` handshake and ends it again
/// (spec m4-2). The identity has no options; `capabilities` are those of the
/// XBoard adapter. A `protover 1` engine (no `feature` at all) is accepted
/// too: use [`detect_protocol`] to tell the protocols apart.
///
/// # Panics
///
/// Must be called inside a Tokio runtime (the pipes are served by tasks).
pub async fn probe_xboard(config: EngineConfig) -> Result<EngineIdentity, EngineError> {
    let log = LogHandle::new(config.log_lines);
    let wire = process::spawn(&config, &log)?;
    xboard::handshake::probe(wire, &config, &log)
        .await
        .map(|(identity, _)| identity)
}

/// How long [`detect_protocol`] waits for each protocol to show itself.
const DETECT_WAIT: Duration = Duration::from_secs(3);

/// Finds out which protocol an engine speaks: UCI (`uciok` within 3 s), then
/// XBoard (`feature` within 3 s). The Scid asks the user instead
/// (`analysis.tcl:634-646`).
///
/// Fails with [`EngineError::Unrecognised`] when neither answers, with
/// [`EngineError::Spawn`] when the program cannot be started and with
/// [`EngineError::Io`] when a pipe fails while probing for XBoard.
///
/// An old XBoard engine that ignores `protover 2` (no `feature` line at all)
/// cannot be told apart from a silent program, so it is reported as
/// `Unrecognised`: such an engine must be configured with
/// [`Protocol::XBoard`] explicitly.
///
/// # Panics
///
/// Must be called inside a Tokio runtime.
pub async fn detect_protocol(config: EngineConfig) -> Result<Protocol, EngineError> {
    let mut uci_config = config.clone();
    uci_config.timeouts.uciok = DETECT_WAIT;
    match probe_uci(uci_config).await {
        Ok(_) => return Ok(Protocol::Uci),
        Err(e @ EngineError::Spawn { .. }) => return Err(e),
        Err(_) => {}
    }
    let mut xboard_config = config;
    xboard_config.timeouts.feature = DETECT_WAIT;
    let log = LogHandle::new(xboard_config.log_lines);
    let wire = process::spawn(&xboard_config, &log)?;
    match xboard::handshake::probe(wire, &xboard_config, &log).await {
        Ok((_, true)) => Ok(Protocol::XBoard),
        Err(e @ EngineError::Io(_)) => Err(e),
        Ok((_, false)) | Err(_) => Err(EngineError::Unrecognised),
    }
}
