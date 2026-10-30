//! The XBoard session: one task that owns the connection to an engine and
//! runs the state machine
//! `Handshake → Ready ⇄ Searching → Stopping` (spec m4-2).
//!
//! Two ways to search, chosen by what the engine can do (D418):
//! *Analyze* (`analyze`: `exit`, `force`, position, `analyze`), which has no
//! `bestmove`, so the adapter synthesizes one from the last variation (D423);
//! and *Go* (`new`, `force`, position, `st`, `sd`, `post`, `go`), used by
//! engines without `analyze` and by searches with a depth or time limit,
//! which ends with the engine's `move`.
//!
//! The helpers for the event queue (`emit`, `warn`, `finish`) mirror those of
//! the UCI session on purpose: that module is not shared yet.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use mychess_core::{Move, Position};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Duration, Instant};

use crate::coalesce::Coalescer;
use crate::engine::{
    BestMove, COMMAND_QUEUE, EngineCommand, EngineConfig, EngineError, EngineEvent, EngineWarning,
    ExitInfo, ExitReason, Phase, Shared, check_variant, clean_peer_text, clean_result,
};
use crate::log::{LogHandle, LogKind};
use crate::protocol::{EngineIdentity, SearchId, SearchInfo};
use crate::request::SearchRequest;
use crate::wire::{Farewell, Wire, WireEvent};
use crate::xboard::features::{answer, parse_feature_line};
use crate::xboard::format::{self, clock_commands, color_command, limit_commands, move_command};
use crate::xboard::handshake::{Handshake, Settings};
use crate::xboard::thinking::{
    self, Thinking, parse_engine_move, parse_move_line, parse_pong, parse_stat01,
    parse_thinking_line,
};

use crate::uci::session::SessionParts;

/// Old output of a search that was just replaced can still arrive without
/// `ping`; thinking lines deeper than ply 2 are dropped for this long, or
/// until a line with ply <= 2 shows the new search (D419).
const STALE_WINDOW: Duration = Duration::from_secs(1);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Analyze,
    Go,
}

/// A search being run (or being stopped).
#[derive(Debug)]
struct Active {
    id: SearchId,
    mode: Mode,
    /// The position after all the moves: the root of the search.
    position: Position,
    /// `ping` sent and `pong` not yet seen: everything before it is stale.
    pong: Option<(u64, Instant)>,
    /// Without `ping`: thinking lines are suspect until this instant or a
    /// line of ply <= 2.
    stale_until: Option<Instant>,
    /// The first move of the last variation with moves, for the synthesized
    /// `BestMove`.
    best: Option<Move>,
    /// A complaint (`Illegal move`, `Error ...`) was already reported.
    complained: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopStage {
    /// `?` was sent; waiting for the `move`.
    Waiting,
    /// The `move` is late; `ping` N was sent to see if the engine lives.
    Probing(u64),
}

#[derive(Debug)]
enum State {
    Handshake(Box<Handshake>),
    Ready,
    Searching(Active),
    Stopping {
        old: Active,
        deadline: Instant,
        stage: StopStage,
    },
}

pub(crate) struct Session {
    config: EngineConfig,
    wire: Wire,
    commands: mpsc::Receiver<EngineCommand>,
    events: mpsc::Sender<EngineEvent>,
    log: LogHandle,
    shared: Arc<Shared>,
    startup: Option<oneshot::Sender<Result<(), EngineError>>>,
    _done: watch::Sender<()>,
    quit: watch::Receiver<bool>,
    backlog: VecDeque<EngineCommand>,
    settings: Option<Settings>,
    state: State,
    /// A search waiting for the handshake or for the previous search to end;
    /// a later one replaces it.
    pending: Option<SearchRequest>,
    coalescer: Coalescer,
    next_search: u64,
    next_ping: u64,
    /// The moves the engine's board holds, from the standard start (engines
    /// without `setboard`); `None` when unknown.
    synced: Option<Vec<Move>>,
    /// The engine is in `analyze` mode (`exit` is due before the next
    /// position).
    in_analyze: bool,
    /// A new game began: the next search starts with `new`.
    fresh: bool,
    /// The name of the opponent, sent as `name` after every `new` (spec
    /// M8-2c, D2524).
    opponent: Option<String>,
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
            settings: None,
            state: State::Ready, // replaced at the start of `run`
            pending: None,
            coalescer,
            next_search: 1,
            next_ping: 1,
            synced: None,
            in_analyze: false,
            fresh: false,
            opponent: None,
        }
    }

    pub(crate) async fn run(mut self) {
        let stop = self.main_loop().await;
        self.finish(stop).await;
    }

    async fn main_loop(&mut self) -> Stop {
        for line in ["xboard", "protover 2"] {
            if let Err(stop) = self.send_line(line) {
                return stop;
            }
        }
        self.state = State::Handshake(Box::new(Handshake::new(
            Instant::now(),
            self.config.timeouts,
        )));
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

    fn send_line(&mut self, line: &str) -> Flow {
        self.log.push(LogKind::Sent, line);
        self.wire
            .send_line(line)
            .map_err(|e| Stop::Io(e.describe().to_owned()))
    }

    fn send_all(&mut self, lines: &[String]) -> Flow {
        lines.iter().try_for_each(|line| self.send_line(line))
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

    /// Delivers an event that must not be lost; see the UCI session.
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

    fn warn(&mut self, warning: EngineWarning) -> Flow {
        self.note(&warning.to_string());
        match self.events.try_send(EngineEvent::Warning(warning)) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(self.gone()),
        }
    }

    fn refuse(&mut self, command: &'static str, reason: impl Into<String>) -> Flow {
        self.warn(EngineWarning::CommandRefused {
            command,
            reason: reason.into(),
        })
    }

    fn emit_info(&mut self, info: SearchInfo) -> Flow {
        match self.events.try_send(EngineEvent::Info(info)) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(self.gone()),
        }
    }

    fn flush_infos(&mut self) -> Flow {
        for info in self.coalescer.take_all(Instant::now()) {
            self.emit_info(info)?;
        }
        Ok(())
    }

    // ----- timers ---------------------------------------------------------

    fn state_deadline(&self) -> Option<Instant> {
        match &self.state {
            State::Handshake(hs) => Some(hs.deadline()),
            State::Searching(active) => active.pong.map(|(_, deadline)| deadline),
            State::Stopping { deadline, .. } => Some(*deadline),
            State::Ready => None,
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
            State::Handshake(hs) => match hs.on_deadline() {
                Ok(()) => {
                    let State::Handshake(hs) = std::mem::replace(&mut self.state, State::Ready)
                    else {
                        return Ok(());
                    };
                    self.complete_handshake(*hs).await
                }
                Err(phase) => Err(Stop::Timeout(phase)),
            },
            State::Searching(_) => Err(Stop::Timeout(Phase::Pong)),
            State::Stopping {
                stage: StopStage::Probing(_),
                ..
            } => self.give_up_on_stop().await,
            State::Stopping {
                deadline, stage, ..
            } => {
                let ping = self.settings.as_ref().is_some_and(|s| s.ping);
                if ping {
                    // No `move` after `?`: ask whether the engine is alive.
                    let n = self.next_ping;
                    self.next_ping += 1;
                    *stage = StopStage::Probing(n);
                    *deadline = now + self.config.timeouts.stop_recovery;
                    self.note("no move after `?`; sending ping");
                    self.send_line(&format!("ping {n}"))
                } else {
                    self.give_up_on_stop().await
                }
            }
            State::Ready => Ok(()),
        }
    }

    /// The engine ignored `?`: its search ends with the best move so far and
    /// the session closes (spec m4-1, D619).
    async fn give_up_on_stop(&mut self) -> Flow {
        let State::Stopping { old, .. } = std::mem::replace(&mut self.state, State::Ready) else {
            return Ok(());
        };
        self.warn(EngineWarning::Misbehaved(
            "the engine did not answer `?` with a move; it will be restarted".to_owned(),
        ))?;
        self.finalize(&old).await?;
        Err(Stop::Unresponsive)
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
        if let State::Handshake(hs) = &mut self.state {
            let feed = hs.feed(text, Instant::now());
            self.send_all(&feed.responses)?;
            if feed.done {
                let State::Handshake(hs) = std::mem::replace(&mut self.state, State::Ready) else {
                    return Ok(());
                };
                return self.complete_handshake(*hs).await;
            }
            return Ok(());
        }
        let line = text.trim();
        if let Some(n) = parse_pong(line) {
            return self.on_pong(n).await;
        }
        if line.starts_with("feature") {
            // Late announcements are answered like the first ones (D417).
            for (key, value) in parse_feature_line(line) {
                self.send_line(&answer(&key, &value))?;
            }
            return Ok(());
        }
        if line == "resign" {
            // The engine gives up (spec M8-2c, D2526): the search ends
            // without a move instead of waiting for the watchdog.
            return self.on_move(line).await;
        }
        if let Some(thinking) = parse_thinking_line(line) {
            return self.on_thinking(&thinking);
        }
        if let Some(stat) = parse_stat01(line) {
            return self.on_stat(stat);
        }
        if let Some(mv) = parse_move_line(line) {
            return self.on_move(mv).await;
        }
        if line.starts_with("Illegal move") || line.starts_with("Error") {
            return self.on_complaint(line);
        }
        // Results (`1-0`, `resign`) and chatter.
        tracing::debug!(line, "unhandled engine line");
        Ok(())
    }

    /// The engine refused something we sent (`Illegal move`, `Error ...`)
    /// while a search runs: reported once per search.
    fn on_complaint(&mut self, line: &str) -> Flow {
        tracing::debug!(line, "engine complaint");
        let State::Searching(active) = &mut self.state else {
            return Ok(());
        };
        if active.complained {
            return Ok(());
        }
        active.complained = true;
        let text = crate::uci::parse::truncate_utf8(line, 128);
        self.warn(EngineWarning::Misbehaved(format!(
            "the engine refused our input: `{text}`"
        )))
    }

    async fn complete_handshake(&mut self, hs: Handshake) -> Flow {
        let settings = hs.finish(&self.config.name, &self.log);
        let identity: EngineIdentity = settings.identity();
        self.shared.identity.set(identity.clone()).ok();
        self.shared
            .standard_start_only
            .store(!settings.setboard, Ordering::Relaxed);
        for command in settings.startup {
            self.send_line(command)?;
        }
        for command in ["easy", "post", "force"] {
            self.send_line(command)?;
        }
        self.settings = Some(settings);
        self.state = State::Ready;
        self.emit(EngineEvent::Ready(identity)).await?;
        if let Some(startup) = self.startup.take() {
            startup.send(Ok(())).ok();
        }
        self.settle().await
    }

    fn style(&self) -> thinking::Style {
        self.settings.as_ref().map(|s| s.style).unwrap_or_default()
    }

    async fn on_pong(&mut self, n: u64) -> Flow {
        match &mut self.state {
            State::Searching(active) => {
                if active.pong.is_some_and(|(sent, _)| sent == n) {
                    active.pong = None;
                    active.stale_until = None;
                } else {
                    tracing::debug!(n, "stale pong");
                }
                Ok(())
            }
            State::Stopping {
                stage: StopStage::Probing(sent),
                ..
            } if *sent == n => self.give_up_on_stop().await,
            _ => {
                tracing::debug!(n, "unexpected pong");
                Ok(())
            }
        }
    }

    fn on_thinking(&mut self, thinking: &Thinking) -> Flow {
        let style = self.style();
        let State::Searching(active) = &mut self.state else {
            return Ok(());
        };
        if active.pong.is_some() {
            return Ok(()); // output of the previous position
        }
        if let Some(until) = active.stale_until {
            if thinking.ply > 2 && Instant::now() < until {
                return Ok(());
            }
            active.stale_until = None;
        }
        let info = thinking::to_search_info(thinking, active.id, &active.position, style);
        if let Some(first) = info.pv.first() {
            active.best = Some(*first);
        }
        if let Some(ready) = self.coalescer.push(info, Instant::now()) {
            self.emit_info(ready)?;
        }
        Ok(())
    }

    fn on_stat(&mut self, stat: thinking::Stat01) -> Flow {
        let style = self.style();
        let State::Searching(active) = &self.state else {
            return Ok(());
        };
        if active.pong.is_some() {
            return Ok(());
        }
        let info = thinking::stat_to_info(stat, active.id, style);
        if let Some(ready) = self.coalescer.push(info, Instant::now()) {
            self.emit_info(ready)?;
        }
        Ok(())
    }

    async fn on_move(&mut self, text: &str) -> Flow {
        match std::mem::replace(&mut self.state, State::Ready) {
            State::Searching(mut active) if active.pong.is_none() => match active.mode {
                Mode::Go => self.end_go(&active, text).await,
                Mode::Analyze => {
                    // A move in analyze mode is a variation of one move.
                    if let Some(mv) = parse_engine_move(&active.position, text) {
                        active.best = Some(mv);
                        let info = SearchInfo {
                            search: active.id,
                            multipv: 1,
                            pv: vec![mv],
                            ..SearchInfo::default()
                        };
                        if let Some(ready) = self.coalescer.push(info, Instant::now()) {
                            self.emit_info(ready)?;
                        }
                    }
                    self.state = State::Searching(active);
                    Ok(())
                }
            },
            State::Stopping { old, .. } => self.end_go(&old, text).await,
            other => {
                self.state = other;
                self.note("move without a search; ignored");
                Ok(())
            }
        }
    }

    /// The engine answered a Go search with a move.
    async fn end_go(&mut self, active: &Active, text: &str) -> Flow {
        self.flush_infos()?;
        self.synced = None;
        let mv = parse_engine_move(&active.position, text);
        if mv.is_none() {
            self.warn(EngineWarning::IllegalBestMove(format!(
                "illegal move `{}` in {}",
                crate::uci::parse::truncate_utf8(text, 32),
                active.position.fen()
            )))?;
        }
        self.emit(EngineEvent::BestMove(BestMove {
            search: active.id,
            mv,
            ponder: None,
        }))
        .await?;
        self.settle().await
    }

    /// Ends a search with the best move seen so far (D423).
    async fn finalize(&mut self, active: &Active) -> Flow {
        self.flush_infos()?;
        self.emit(EngineEvent::BestMove(BestMove {
            search: active.id,
            mv: active.best,
            ponder: None,
        }))
        .await
    }

    // ----- commands -------------------------------------------------------

    async fn on_command(&mut self, command: EngineCommand) -> Flow {
        match command {
            EngineCommand::Quit => return Err(Stop::Quit),
            EngineCommand::Stop => match self.state {
                State::Searching(_) => self.begin_stop().await?,
                _ => self.pending = None,
            },
            EngineCommand::PonderHit | EngineCommand::SetPonder(_) => {}
            EngineCommand::RequestStats => {
                if let State::Searching(active) = &self.state
                    && active.mode == Mode::Analyze
                    && active.pong.is_none()
                {
                    self.send_line(".")?;
                }
            }
            EngineCommand::NewGame => {
                self.fresh = true;
                self.synced = None;
            }
            EngineCommand::Opponent(name) => {
                let name = clean_peer_text(&name, true);
                self.opponent = (!name.is_empty()).then_some(name);
            }
            EngineCommand::GameOver { result, comment } => {
                // Only between searches: the engine may be thinking about
                // something else (D2524).
                if matches!(self.state, State::Ready) {
                    let comment = clean_peer_text(&comment, false);
                    let mut line = format!("result {}", clean_result(&result));
                    if !comment.is_empty() {
                        line.push_str(" {");
                        line.push_str(&comment);
                        line.push('}');
                    }
                    self.send_line(&line)?;
                    // Whatever the engine pondered is over.
                    self.fresh = true;
                    self.synced = None;
                }
            }
            EngineCommand::SetMultiPv(n) => {
                if n > 1 {
                    self.refuse("setmultipv", "XBoard engines have a single line")?;
                }
            }
            EngineCommand::SetOption { .. } => {
                self.refuse("setoption", "XBoard options are not supported")?;
            }
            EngineCommand::Search(request) => match self.state {
                State::Ready => self.start_search(request).await?,
                State::Searching(_) => {
                    self.pending = Some(request);
                    self.begin_stop().await?;
                }
                State::Handshake(_) | State::Stopping { .. } => self.pending = Some(request),
            },
        }
        Ok(())
    }

    /// Stops the running search. Analyze: `exit` and an immediate
    /// `BestMove`; Go: `?` and the wait for the engine's `move`.
    async fn begin_stop(&mut self) -> Flow {
        let State::Searching(active) = std::mem::replace(&mut self.state, State::Ready) else {
            return Ok(());
        };
        match active.mode {
            Mode::Analyze => {
                // A search about to be replaced keeps the engine in analyze
                // mode: its `exit` is part of the new position (D419).
                if self.pending.is_none() {
                    self.send_line("exit")?;
                    self.in_analyze = false;
                }
                self.finalize(&active).await?;
                self.settle().await
            }
            Mode::Go => {
                self.flush_infos()?;
                self.state = State::Stopping {
                    old: active,
                    deadline: Instant::now() + self.config.timeouts.stop,
                    stage: StopStage::Waiting,
                };
                self.send_line("?")
            }
        }
    }

    /// Runs the search that is waiting, when the state allows.
    async fn settle(&mut self) -> Flow {
        if !matches!(self.state, State::Ready) {
            return Ok(());
        }
        match self.pending.take() {
            Some(request) => self.start_search(request).await,
            None => Ok(()),
        }
    }

    /// Refuses a search. An engine still analyzing the previous position is
    /// brought back first: nobody is listening to it any more.
    fn refuse_search(&mut self, command: &'static str, reason: impl Into<String>) -> Flow {
        if self.in_analyze {
            self.send_line("exit")?;
            self.in_analyze = false;
        }
        self.refuse(command, reason)
    }

    async fn start_search(&mut self, request: SearchRequest) -> Flow {
        let Some(settings) = self.settings.clone() else {
            return Ok(());
        };
        if let Some(identity) = self.shared.identity.get()
            && let Err(e) = check_variant(&identity.capabilities, &request)
        {
            return self.refuse_search("go", e.to_string());
        }
        if !settings.setboard
            && let Some(reason) = format::unsupported_start(&request.position)
        {
            return self.refuse_search("position", reason);
        }
        let plan = match format::plan(&request.position) {
            Ok(plan) => plan,
            Err(reason) => return self.refuse_search("position", reason),
        };
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
            // Nothing to search (spec m4-2, rule 7). An engine still in
            // analyze mode is brought back first.
            if self.in_analyze {
                self.send_line("exit")?;
                self.in_analyze = false;
            }
            return self
                .emit(EngineEvent::BestMove(BestMove {
                    search: id,
                    mv: None,
                    ponder: None,
                }))
                .await;
        }
        // Limits need `st`/`sd`, which only a Go search honors.
        let limits = &request.limits;
        let mode = if settings.analyze
            && limits.depth.is_none()
            && limits.movetime.is_none()
            && limits.clock.is_none()
        {
            Mode::Analyze
        } else {
            Mode::Go
        };
        let mut lines: Vec<String> = Vec::new();
        if self.in_analyze {
            lines.push("exit".to_owned());
        }
        match mode {
            Mode::Analyze => self.analyze_setup(&settings, &plan, &mut lines),
            Mode::Go => self.go_setup(&settings, &plan, &request, &mut lines),
        }
        self.fresh = false;
        self.in_analyze = mode == Mode::Analyze;
        let now = Instant::now();
        let mut active = Active {
            id,
            mode,
            position: plan.current,
            pong: None,
            stale_until: None,
            best: None,
            complained: false,
        };
        if settings.ping {
            let n = self.next_ping;
            self.next_ping += 1;
            // Go: the `ping` goes before `go`, so the engine answers it
            // before it thinks. Analyze: the position is in, `analyze` too.
            let at = if mode == Mode::Go {
                lines.len().saturating_sub(1)
            } else {
                lines.len()
            };
            lines.insert(at, format!("ping {n}"));
            active.pong = Some((n, now + self.config.timeouts.pong));
        } else {
            active.stale_until = Some(now + STALE_WINDOW);
        }
        self.send_all(&lines)?;
        self.state = State::Searching(active);
        Ok(())
    }

    /// `[exit] [new] force <position> analyze` (D419).
    fn analyze_setup(&mut self, settings: &Settings, plan: &format::Plan, lines: &mut Vec<String>) {
        if settings.setboard {
            if self.fresh {
                lines.push("new".to_owned());
            }
            lines.push("force".to_owned());
            lines.push(format!("setboard {}", plan.current.fen()));
            self.synced = None;
        } else {
            let old = if self.fresh { None } else { self.synced.take() };
            let target = &plan.moves;
            match old {
                Some(old) if old == *target => lines.push("force".to_owned()),
                Some(old) if target.len() == old.len() + 1 && target[..old.len()] == old[..] => {
                    lines.push("force".to_owned());
                    if let Some(text) = plan.texts.last() {
                        lines.push(move_command(text, settings.usermove));
                    }
                }
                Some(old) if old.len() == target.len() + 1 && old[..target.len()] == target[..] => {
                    lines.push("force".to_owned());
                    lines.push("undo".to_owned());
                }
                _ => Self::rebuild(settings, plan, lines),
            }
            self.synced = Some(plan.moves.clone());
        }
        lines.push("analyze".to_owned());
    }

    /// `new force <moves> <color>`: the board from scratch.
    fn rebuild(settings: &Settings, plan: &format::Plan, lines: &mut Vec<String>) {
        lines.push("new".to_owned());
        lines.push("force".to_owned());
        lines.extend(
            plan.texts
                .iter()
                .map(|text| move_command(text, settings.usermove)),
        );
        if !settings.no_colors {
            lines.push(color_command(&plan.current).to_owned());
        }
    }

    /// `new force <position> st sd post [ping] go` (D418).
    fn go_setup(
        &mut self,
        settings: &Settings,
        plan: &format::Plan,
        request: &SearchRequest,
        lines: &mut Vec<String>,
    ) {
        if settings.setboard {
            lines.push("new".to_owned());
            if let Some(name) = &self.opponent {
                lines.push(format!("name {name}"));
            }
            lines.push("force".to_owned());
            lines.push(format!("setboard {}", plan.current.fen()));
        } else {
            let from = lines.len();
            Self::rebuild(settings, plan, lines);
            if let Some(name) = &self.opponent {
                // After the `new` that `rebuild` starts with.
                lines.insert(from + 1, format!("name {name}"));
            }
        }
        match &request.limits.clock {
            Some(clock) => {
                lines.extend(clock_commands(clock, plan.current.turn()));
                if let Some(depth) = request.limits.depth {
                    lines.push(format!("sd {}", depth.max(1)));
                }
            }
            None => lines.extend(limit_commands(&request.limits)),
        }
        lines.push("post".to_owned());
        lines.push("go".to_owned());
        // The engine moves on its own board afterwards.
        self.synced = None;
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
        if farewell == Farewell::SayQuit {
            // Leave analyze mode (or stop thinking) before `quit` (D423).
            let leave = match &self.state {
                State::Searching(active) | State::Stopping { old: active, .. } => {
                    Some(if active.mode == Mode::Analyze {
                        "exit"
                    } else {
                        "?"
                    })
                }
                _ if self.in_analyze => Some("exit"),
                _ => None,
            };
            if let Some(command) = leave {
                self.send_line(command).ok();
            }
        }
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
