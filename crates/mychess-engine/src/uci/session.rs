//! The UCI session: one task that owns the connection to an engine and runs
//! the state machine
//! `Handshake → Configuring → Ready ⇄ Searching → Stopping`, with `Barrier`
//! for "wait for `readyok` before going on".
//!
//! Rules (spec m4-1):
//! - a command is written only in a state where it is valid; commands that
//!   arrive earlier wait in a coalescing [`Pending`] queue (D407);
//! - `bestmove` is the barrier between searches; `isready`/`readyok` is the
//!   barrier after `setoption`, `ucinewgame` and `UCI_Chess960` changes;
//! - every wait has a deadline (D404);
//! - `info` of a replaced search never reaches the consumer;
//! - nothing the engine writes can panic the session (D411).

use std::collections::VecDeque;
use std::sync::Arc;

use mychess_core::{CastlingMode, Position, Variant};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Duration, Instant};

use crate::coalesce::Coalescer;
use crate::engine::{
    BestMove, COMMAND_QUEUE, EngineCommand, EngineConfig, EngineError, EngineEvent, EngineWarning,
    ExitInfo, ExitReason, Phase, Shared, check_variant, resolve_option,
};
use crate::log::{LogHandle, LogKind};
use crate::option::{OptionValue, is_managed_option};
use crate::protocol::{EngineIdentity, SearchId};
use crate::request::SearchRequest;
use crate::uci::convert::{to_best_move, to_search_info};
use crate::uci::format::{go_command, position_command};
use crate::uci::handshake::{HandshakeData, find_option};
use crate::uci::parse::{RawBestMove, RawInfo, UciLine, parse_line};
use crate::wire::{Farewell, Wire, WireEvent};

/// How the session ended.
#[derive(Debug)]
enum Stop {
    Quit,
    Eof,
    Timeout(Phase),
    Unresponsive,
    Io(String),
    Dropped,
}

type Flow = Result<(), Stop>;

/// A search being run (or being stopped).
#[derive(Debug, Clone)]
struct Active {
    id: SearchId,
    request: SearchRequest,
    /// The position after all the moves: the root of the search.
    position: Position,
    ponder: bool,
}

/// How far the wait for the `bestmove` of a stopped search has gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopStage {
    /// `stop` was sent; waiting for the `bestmove`.
    Waiting,
    /// The `bestmove` is late; `isready` was sent to see if the engine lives.
    Probing,
}

#[derive(Debug)]
enum State {
    Handshake {
        data: HandshakeData,
        deadline: Instant,
    },
    Configuring {
        deadline: Instant,
    },
    Ready,
    Barrier {
        deadline: Instant,
    },
    Searching(Active),
    Stopping {
        old: Active,
        deadline: Instant,
        stage: StopStage,
    },
}

/// Commands waiting for the session to be able to run them.
///
/// Coalescing (spec m4-1, D407): a later `Search` replaces an earlier one;
/// everything else keeps its order and runs before the search.
#[derive(Debug, Default)]
struct Pending {
    actions: VecDeque<EngineCommand>,
    search: Option<SearchRequest>,
}

pub(crate) struct Session {
    config: EngineConfig,
    wire: Wire,
    commands: mpsc::Receiver<EngineCommand>,
    events: mpsc::Sender<EngineEvent>,
    log: LogHandle,
    shared: Arc<Shared>,
    startup: Option<oneshot::Sender<Result<(), EngineError>>>,
    // Dropped when the session ends: tells `Engine::shutdown` it is over.
    _done: watch::Sender<()>,
    /// Set by `Engine::shutdown` and `Quit`; works even with a full queue.
    quit: watch::Receiver<bool>,
    /// Commands read while waiting for room in the event queue.
    backlog: VecDeque<EngineCommand>,
    /// `isready` sent and not yet answered: only the last answer counts.
    isready_out: u32,
    identity: Option<EngineIdentity>,
    state: State,
    pending: Pending,
    coalescer: Coalescer,
    next_search: u64,
    chess960_on: bool,
}

/// The channels and handles a session is built from.
pub(crate) struct SessionParts {
    pub(crate) commands: mpsc::Receiver<EngineCommand>,
    pub(crate) events: mpsc::Sender<EngineEvent>,
    pub(crate) startup: Option<oneshot::Sender<Result<(), EngineError>>>,
    pub(crate) done: watch::Sender<()>,
}

impl Session {
    pub(crate) fn new(
        config: EngineConfig,
        wire: Wire,
        log: LogHandle,
        shared: Arc<Shared>,
        parts: SessionParts,
    ) -> Session {
        let coalescer = Coalescer::new(config.info_interval);
        let quit = shared.quit.subscribe();
        Session {
            config,
            wire,
            commands: parts.commands,
            events: parts.events,
            log,
            shared,
            startup: parts.startup,
            _done: parts.done,
            quit,
            backlog: VecDeque::new(),
            isready_out: 0,
            identity: None,
            state: State::Ready, // replaced at the start of `run`
            pending: Pending::default(),
            coalescer,
            next_search: 1,
            chess960_on: false,
        }
    }

    /// Runs the session to its end and reports it.
    pub(crate) async fn run(mut self) {
        let stop = self.main_loop().await;
        self.finish(stop).await;
    }

    async fn main_loop(&mut self) -> Stop {
        if let Err(stop) = self.send_line("uci") {
            return stop;
        }
        self.state = State::Handshake {
            data: HandshakeData::default(),
            deadline: Instant::now() + self.config.timeouts.uciok,
        };
        loop {
            while let Some(command) = self.backlog.pop_front() {
                if let Err(stop) = self.on_command(command).await {
                    return stop;
                }
            }
            let wake = self.next_wake();
            let flow = tokio::select! {
                biased;
                () = quit_requested(&mut self.quit) => Err(Stop::Quit),
                event = self.wire.recv() => match event {
                    Some(event) => self.on_wire(event).await,
                    None => Err(Stop::Eof),
                },
                command = self.commands.recv() => match command {
                    Some(command) => self.on_command(command).await,
                    None => Err(self.gone()),
                },
                () = sleep_until_opt(wake) => self.on_timer().await,
                () = self.events.closed() => Err(self.gone()),
            };
            if let Err(stop) = flow {
                return stop;
            }
        }
    }

    // ----- output ---------------------------------------------------------

    fn send_isready(&mut self) -> Flow {
        self.isready_out += 1;
        self.send_line("isready")
    }

    fn send_line(&mut self, line: &str) -> Flow {
        self.log.push(LogKind::Sent, line);
        self.wire
            .send_line(line)
            .map_err(|e| Stop::Io(e.describe().to_owned()))
    }

    fn note(&self, text: &str) {
        self.log.push(LogKind::Note, text);
    }

    /// Why the session ends when the handles are gone: `Engine::shutdown`
    /// raises the quit flag and then drops its handle, so a closed channel
    /// seen with the flag up is a polite quit, not an abandonment (the
    /// engine must still get its `quit` and its wait).
    fn gone(&self) -> Stop {
        if *self.quit.borrow() {
            Stop::Quit
        } else {
            Stop::Dropped
        }
    }

    /// Delivers an event that must not be lost. With a full event queue it
    /// waits for room, but keeps listening for `Quit` and for the end of the
    /// handles, so a consumer that stopped reading cannot hold the process.
    async fn emit(&mut self, event: EngineEvent) -> Flow {
        let permit = loop {
            tokio::select! {
                permit = self.events.reserve() => match permit {
                    Ok(permit) => break permit,
                    Err(_) => return Err(self.gone()),
                },
                () = quit_requested(&mut self.quit) => return Err(Stop::Quit),
                command = self.commands.recv(), if self.backlog.len() < COMMAND_QUEUE * 4 => {
                    match command {
                        Some(EngineCommand::Quit) => return Err(Stop::Quit),
                        Some(other) => self.backlog.push_back(other),
                        None => return Err(self.gone()),
                    }
                }
            }
        };
        permit.send(event);
        Ok(())
    }

    /// Warnings are advisory: with a full event queue they are dropped.
    fn warn(&mut self, warning: EngineWarning) -> Flow {
        self.note(&warning.to_string());
        match self.events.try_send(EngineEvent::Warning(warning)) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(self.gone()),
        }
    }

    fn emit_info(&mut self, info: crate::protocol::SearchInfo) -> Flow {
        match self.events.try_send(EngineEvent::Info(info)) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(self.gone()),
        }
    }

    /// Delivers every pending `info` (before something more important).
    fn flush_infos(&mut self) -> Flow {
        for info in self.coalescer.take_all(Instant::now()) {
            self.emit_info(info)?;
        }
        Ok(())
    }

    // ----- timers ---------------------------------------------------------

    fn state_deadline(&self) -> Option<Instant> {
        match &self.state {
            State::Handshake { deadline, .. }
            | State::Configuring { deadline }
            | State::Barrier { deadline }
            | State::Stopping { deadline, .. } => Some(*deadline),
            State::Ready | State::Searching(_) => None,
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        [self.state_deadline(), self.coalescer.next_due()]
            .into_iter()
            .flatten()
            .min()
    }

    async fn on_timer(&mut self) -> Flow {
        let now = Instant::now();
        for info in self.coalescer.take_due(now) {
            self.emit_info(info)?;
        }
        let Some(deadline) = self.state_deadline() else {
            return Ok(());
        };
        if now < deadline {
            return Ok(());
        }
        match &mut self.state {
            State::Handshake { .. } => Err(Stop::Timeout(Phase::Uciok)),
            State::Configuring { .. } | State::Barrier { .. } => Err(Stop::Timeout(Phase::Readyok)),
            State::Stopping {
                stage: StopStage::Probing,
                ..
            } => self.give_up_on_stop().await,
            State::Stopping {
                deadline, stage, ..
            } => {
                // No bestmove after `stop`: ask whether the engine is alive.
                *stage = StopStage::Probing;
                *deadline = now + self.config.timeouts.stop_recovery;
                self.note("no bestmove after stop; sending isready");
                self.send_isready()
            }
            State::Ready | State::Searching(_) => Ok(()),
        }
    }

    // ----- engine output --------------------------------------------------

    async fn on_wire(&mut self, event: WireEvent) -> Flow {
        match event {
            WireEvent::Line { text, truncated } => {
                self.log.push(LogKind::Received, &text);
                if truncated {
                    self.note("line longer than 1 MiB; the rest was discarded");
                }
                if text.trim().is_empty() {
                    return Ok(());
                }
                self.on_line(&text).await
            }
            WireEvent::Eof => Err(Stop::Eof),
            WireEvent::Error(message) => Err(Stop::Io(message)),
        }
    }

    async fn on_line(&mut self, text: &str) -> Flow {
        if let State::Handshake { data, .. } = &mut self.state {
            return if data.feed_text(text, &self.log) {
                self.on_uciok()
            } else {
                Ok(())
            };
        }
        match parse_line(text) {
            UciLine::UciOk => Ok(()),
            UciLine::ReadyOk => self.on_readyok().await,
            UciLine::Info(raw) => self.on_info(raw),
            UciLine::BestMove(raw) => self.on_bestmove(raw).await,
            UciLine::IdName(_) | UciLine::IdAuthor(_) | UciLine::Option(_) => {
                tracing::debug!("ignored handshake line after uciok");
                Ok(())
            }
            UciLine::Unknown => {
                tracing::debug!(line = text, "unknown engine line");
                Ok(())
            }
        }
    }

    fn on_uciok(&mut self) -> Flow {
        let State::Handshake { data, .. } = std::mem::replace(&mut self.state, State::Ready) else {
            // `uciok` at any other time is noise.
            return Ok(());
        };
        let identity = data.into_identity(&self.config.name);
        self.shared.identity.set(identity.clone()).ok();
        self.identity = Some(identity.clone());
        // All saved options, no wait between them (D409), then one isready.
        for (name, value) in self.config.options.clone() {
            if is_managed_option(&name) {
                continue;
            }
            match resolve_option(&identity, &name, &value) {
                Ok((option, value)) => {
                    let line = option.setoption_line(&value);
                    self.send_line(&line)?;
                }
                Err(e) => {
                    self.warn(EngineWarning::OptionSkipped {
                        name,
                        reason: e.to_string(),
                    })?;
                }
            }
        }
        if identity.capabilities.analyse_mode {
            self.send_line("setoption name UCI_AnalyseMode value true")?;
        }
        self.send_line("ucinewgame")?;
        self.send_isready()?;
        self.state = State::Configuring {
            deadline: Instant::now() + self.config.timeouts.readyok,
        };
        Ok(())
    }

    async fn on_readyok(&mut self) -> Flow {
        // Only the answer to the last `isready` counts: an older one (from a
        // recovery whose `bestmove` came after all) must not end a barrier.
        self.isready_out = self.isready_out.saturating_sub(1);
        if self.isready_out > 0 {
            tracing::debug!("stale readyok");
            return Ok(());
        }
        match &mut self.state {
            State::Configuring { .. } => {
                self.state = State::Ready;
                if let Some(identity) = self.identity.clone() {
                    self.emit(EngineEvent::Ready(identity)).await?;
                }
                if let Some(startup) = self.startup.take() {
                    startup.send(Ok(())).ok();
                }
                self.settle().await
            }
            State::Barrier { .. } => {
                self.state = State::Ready;
                self.settle().await
            }
            State::Stopping {
                stage: StopStage::Probing,
                ..
            } => {
                // The engine is alive, or silent, but never answered `stop`.
                self.give_up_on_stop().await
            }
            _ => {
                tracing::debug!("unexpected readyok");
                Ok(())
            }
        }
    }

    /// The engine never answered `stop` with a `bestmove`. A `bestmove` of
    /// the old search could still limp in and be mistaken for the next one's,
    /// so the engine is given up (spec m4-1, D619): the old search ends
    /// without a move (after a warning) and the session closes. This holds
    /// whether or not the `isready` probe was answered, so every search that
    /// started ends in a `BestMove`.
    async fn give_up_on_stop(&mut self) -> Flow {
        let State::Stopping { old, .. } = std::mem::replace(&mut self.state, State::Ready) else {
            return Ok(());
        };
        self.warn(EngineWarning::Misbehaved(
            "the engine did not answer `stop` with `bestmove`; it will be restarted".to_owned(),
        ))?;
        self.flush_infos()?;
        self.emit(EngineEvent::BestMove(BestMove {
            search: old.id,
            mv: None,
            ponder: None,
        }))
        .await?;
        Err(Stop::Unresponsive)
    }

    fn on_info(&mut self, raw: RawInfo) -> Flow {
        let State::Searching(active) = &self.state else {
            // Not searching, or a search being stopped: stale.
            return Ok(());
        };
        let info = to_search_info(raw, active.id, &active.position);
        if let Some(ready) = self.coalescer.push(info, Instant::now()) {
            self.emit_info(ready)?;
        }
        Ok(())
    }

    async fn on_bestmove(&mut self, raw: RawBestMove) -> Flow {
        let active = match std::mem::replace(&mut self.state, State::Ready) {
            State::Searching(active) => active,
            State::Stopping { old, .. } => old,
            other => {
                self.state = other;
                self.note("bestmove without a search; ignored");
                return Ok(());
            }
        };
        self.flush_infos()?;
        if raw.missing {
            self.warn(EngineWarning::Misbehaved(
                "`bestmove` without a move".to_owned(),
            ))?;
        }
        let converted = to_best_move(&raw, &active.position);
        if let Some(message) = converted.warning {
            self.warn(EngineWarning::IllegalBestMove(message))?;
        }
        self.emit(EngineEvent::BestMove(BestMove {
            search: active.id,
            mv: converted.mv,
            ponder: converted.ponder,
        }))
        .await?;
        self.settle().await
    }

    // ----- commands -------------------------------------------------------

    async fn on_command(&mut self, command: EngineCommand) -> Flow {
        match command {
            EngineCommand::Quit => return Err(Stop::Quit),
            EngineCommand::Stop => match self.state {
                State::Searching(_) => self.begin_stopping()?,
                // Already stopping, or nothing to stop: a stop also cancels a
                // search that was waiting its turn.
                _ => self.pending.search = None,
            },
            // XBoard only; `Engine::send` already drops it for UCI.
            // `Opponent` and `GameOver` are XBoard only too (spec M8-2c,
            // D2524): UCI has no such command.
            EngineCommand::RequestStats
            | EngineCommand::Opponent(_)
            | EngineCommand::GameOver { .. } => {}
            EngineCommand::PonderHit => {
                if let State::Searching(active) = &mut self.state
                    && active.ponder
                {
                    active.ponder = false;
                    self.send_line("ponderhit")?;
                }
            }
            EngineCommand::Search(request) => {
                self.pending.search = Some(request);
                self.after_enqueue().await?;
            }
            action @ (EngineCommand::NewGame
            | EngineCommand::SetOption { .. }
            | EngineCommand::SetPonder(_)
            | EngineCommand::SetMultiPv(_)) => {
                if self.pending.actions.len() >= COMMAND_QUEUE {
                    let command = match action {
                        EngineCommand::NewGame => "ucinewgame",
                        EngineCommand::SetMultiPv(_) => "setmultipv",
                        _ => "setoption",
                    };
                    return self.warn(EngineWarning::CommandRefused {
                        command,
                        reason: "too many commands waiting".to_owned(),
                    });
                }
                let restarts = !matches!(action, EngineCommand::NewGame);
                self.pending.actions.push_back(action);
                if restarts
                    && self.pending.search.is_none()
                    && let State::Searching(active) = &self.state
                {
                    // Changing an option restarts the analysis (Scid:
                    // `changePVSize`, `analysis.tcl:2748`).
                    self.pending.search = Some(active.request.clone());
                }
                self.after_enqueue().await?;
            }
        }
        Ok(())
    }

    /// After queueing: stop a running search, or run the queue if idle.
    async fn after_enqueue(&mut self) -> Flow {
        match self.state {
            State::Searching(_) => self.begin_stopping(),
            State::Ready => self.settle().await,
            _ => Ok(()),
        }
    }

    fn begin_stopping(&mut self) -> Flow {
        let State::Searching(active) = std::mem::replace(&mut self.state, State::Ready) else {
            return Ok(());
        };
        // Reports received before the stop are still good; later ones are not.
        let flushed = self.flush_infos();
        self.state = State::Stopping {
            old: active,
            deadline: Instant::now() + self.config.timeouts.stop,
            stage: StopStage::Waiting,
        };
        flushed?;
        self.send_line("stop")
    }

    // ----- running the queue ----------------------------------------------

    /// Runs what is waiting. Called whenever the state becomes `Ready`.
    async fn settle(&mut self) -> Flow {
        loop {
            if !matches!(self.state, State::Ready) {
                return Ok(());
            }
            if !self.pending.actions.is_empty() {
                let mut sent = false;
                while let Some(action) = self.pending.actions.pop_front() {
                    sent |= self.apply_action(action)?;
                }
                if sent {
                    self.send_isready()?;
                    self.state = State::Barrier {
                        deadline: Instant::now() + self.config.timeouts.readyok,
                    };
                    return Ok(());
                }
                continue;
            }
            let Some(request) = self.pending.search.take() else {
                return Ok(());
            };
            self.start_search(request).await?;
        }
    }

    /// Sends one queued action. Returns whether something was written.
    fn apply_action(&mut self, action: EngineCommand) -> Result<bool, Stop> {
        let Some(identity) = self.identity.clone() else {
            return Ok(false);
        };
        let line = match action {
            EngineCommand::NewGame => "ucinewgame".to_owned(),
            EngineCommand::SetOption { name, value } => {
                match resolve_option(&identity, &name, &value) {
                    Ok((option, value)) => option.setoption_line(&value),
                    Err(e) => {
                        self.warn(EngineWarning::CommandRefused {
                            command: "setoption",
                            reason: e.to_string(),
                        })?;
                        return Ok(false);
                    }
                }
            }
            EngineCommand::SetMultiPv(n) => {
                let (Some(range), Some(option)) = (
                    &identity.capabilities.multipv,
                    find_option(&identity.options, "MultiPV"),
                ) else {
                    self.warn(EngineWarning::CommandRefused {
                        command: "setmultipv",
                        reason: "the engine has no MultiPV option".to_owned(),
                    })?;
                    return Ok(false);
                };
                let n = n.clamp(*range.start(), *range.end());
                option.setoption_line(&OptionValue::Int(i64::from(n)))
            }
            EngineCommand::SetPonder(on) => {
                let Some(option) = find_option(&identity.options, "Ponder") else {
                    self.warn(EngineWarning::CommandRefused {
                        command: "setponder",
                        reason: "the engine has no Ponder option".to_owned(),
                    })?;
                    return Ok(false);
                };
                option.setoption_line(&OptionValue::Bool(on))
            }
            _ => return Ok(false),
        };
        self.send_line(&line)?;
        Ok(true)
    }

    async fn start_search(&mut self, request: SearchRequest) -> Flow {
        let Some(identity) = self.identity.clone() else {
            return Ok(());
        };
        if let Err(e) = check_variant(&identity.capabilities, &request) {
            return self.warn(EngineWarning::CommandRefused {
                command: "go",
                reason: e.to_string(),
            });
        }
        let plan = match position_command(&request.position) {
            Ok(plan) => plan,
            Err(reason) => {
                return self.warn(EngineWarning::CommandRefused {
                    command: "position",
                    reason,
                });
            }
        };
        let chess960 = request.position.start.variant() == Variant::Chess960;
        if chess960 != self.chess960_on && identity.capabilities.chess960 {
            // Switch the engine's rule set first, and wait for it.
            self.send_line(&format!("setoption name UCI_Chess960 value {chess960}"))?;
            self.send_isready()?;
            self.chess960_on = chess960;
            self.pending.search = Some(request);
            self.state = State::Barrier {
                deadline: Instant::now() + self.config.timeouts.readyok,
            };
            return Ok(());
        }
        let id = SearchId(self.next_search);
        self.next_search += 1;
        if request.tag != 0 {
            self.emit(EngineEvent::SearchTagged {
                search: id,
                tag: request.tag,
            })
            .await?;
        }
        self.emit(EngineEvent::SearchStarted(id)).await?;
        if plan.current.legal_moves().is_empty() {
            // Nothing to search (rule 7): answer without bothering the engine.
            return self
                .emit(EngineEvent::BestMove(BestMove {
                    search: id,
                    mv: None,
                    ponder: None,
                }))
                .await;
        }
        let mode: CastlingMode = request.position.start.castling_mode();
        let go = go_command(&request, mode);
        self.send_line(&plan.command)?;
        self.send_line(&go)?;
        self.state = State::Searching(Active {
            id,
            ponder: request.ponder,
            request,
            position: plan.current,
        });
        Ok(())
    }

    // ----- the end --------------------------------------------------------

    async fn finish(&mut self, stop: Stop) {
        let quit_wait = self.config.timeouts.quit;
        let (reason, farewell, wait) = match &stop {
            Stop::Quit => (ExitReason::Quit, Farewell::SayQuit, quit_wait),
            Stop::Eof => (ExitReason::Crashed, Farewell::Silent, quit_wait),
            Stop::Timeout(phase) => (ExitReason::Timeout(*phase), Farewell::SayQuit, quit_wait),
            Stop::Unresponsive => (ExitReason::StopTimeout, Farewell::SayQuit, quit_wait),
            Stop::Io(message) => (ExitReason::Io(message.clone()), Farewell::Silent, quit_wait),
            Stop::Dropped => (ExitReason::Dropped, Farewell::SayQuit, Duration::ZERO),
        };
        self.log
            .push(LogKind::Note, &format!("session ended: {reason:?}"));
        let status = self.wire.terminate(farewell, wait).await;
        let info = ExitInfo {
            reason,
            status,
            stderr_tail: self.wire.stderr_tail(),
        };
        if let Some(startup) = self.startup.take() {
            let error = match &stop {
                Stop::Eof => EngineError::ExitedEarly {
                    status: info.status,
                    stderr_tail: info.stderr_tail.clone(),
                },
                Stop::Timeout(phase) => EngineError::Timeout(*phase),
                Stop::Unresponsive => EngineError::Timeout(Phase::Bestmove),
                Stop::Io(message) => EngineError::Io(std::io::Error::other(message.clone())),
                Stop::Quit | Stop::Dropped => EngineError::Closed,
            };
            startup.send(Err(error)).ok();
        }
        if !matches!(stop, Stop::Dropped) {
            // The last reports of a search that was running are not worth a
            // wait; the exit notice always is.
            self.coalescer.clear();
            let event = EngineEvent::Exited(info);
            if let Err(mpsc::error::TrySendError::Full(event)) = self.events.try_send(event) {
                tokio::time::timeout(Duration::from_secs(1), self.events.send(event))
                    .await
                    .ok();
            }
        }
    }
}

/// Completes when `Engine::shutdown` or a `Quit` asked for the end.
async fn quit_requested(quit: &mut watch::Receiver<bool>) {
    loop {
        if *quit.borrow_and_update() {
            return;
        }
        if quit.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

async fn sleep_until_opt(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests;
