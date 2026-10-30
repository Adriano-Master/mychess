//! Test support (feature `testing`): a scripted fake engine that runs in
//! memory, a hand-driven peer for protocol tests, and the logic behind the
//! `mychess-fake-engine` binary.
//!
//! [`ScriptedEngine`] plays a deterministic material-greedy engine (see
//! [`fake`]) over an in-memory pipe, so session tests need no process and can
//! use the paused clock of `tokio::time`. [`ManualEngine`] lets a test play
//! the engine by hand, line by line. Both feed the real session code.

use std::sync::{Arc, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

use crate::engine::{Engine, EngineConfig, EngineError, EngineEvents, launch_on};
use crate::log::LogHandle;
use crate::wire::Wire;

pub mod fake;
mod fake_xboard;

pub use fake::{FakeBehavior, FakeExit, FakeProtocol, FakeStrength, XBoardFake, run_fake};

/// Size of the in-memory pipes.
const PIPE_BYTES: usize = 1 << 16;

pub(crate) fn pipe_wire() -> (Wire, ReadHalf<DuplexStream>, WriteHalf<DuplexStream>) {
    let (session_side, engine_side) = tokio::io::duplex(PIPE_BYTES);
    let (session_read, session_write) = tokio::io::split(session_side);
    let (engine_read, engine_write) = tokio::io::split(engine_side);
    (
        Wire::from_io(session_read, session_write),
        engine_read,
        engine_write,
    )
}

/// A fake engine in memory with a configurable behavior.
#[derive(Debug, Clone)]
pub struct ScriptedEngine {
    behavior: FakeBehavior,
    received: fake::Recorder,
}

/// Builds a [`ScriptedEngine`]; the methods mirror the flags of the
/// `mychess-fake-engine` binary.
#[derive(Debug, Clone, Default)]
pub struct ScriptedEngineBuilder {
    behavior: FakeBehavior,
}

impl ScriptedEngineBuilder {
    /// The `id name`.
    #[must_use]
    pub fn name(mut self, name: &str) -> Self {
        name.clone_into(&mut self.behavior.name);
        self
    }
    /// Never answers `uci`.
    #[must_use]
    pub fn silent(mut self) -> Self {
        self.behavior.silent = true;
        self
    }
    /// Delays `uciok`.
    #[must_use]
    pub fn uciok_delay(mut self, delay: Duration) -> Self {
        self.behavior.uciok_delay = delay;
        self
    }
    /// Ignores `stop`.
    #[must_use]
    pub fn hang_on_stop(mut self) -> Self {
        self.behavior.hang_on_stop = true;
        self
    }
    /// Answers `readyok` but not `bestmove` after `stop`.
    #[must_use]
    pub fn no_bestmove_after_stop(mut self) -> Self {
        self.behavior.no_bestmove_after_stop = true;
        self
    }
    /// Ignores `quit`.
    #[must_use]
    pub fn ignore_quit(mut self) -> Self {
        self.behavior.ignore_quit = true;
        self
    }
    /// Dies at the first `go`.
    #[must_use]
    pub fn crash_after_go(mut self) -> Self {
        self.behavior.crash_after_go = true;
        self
    }
    /// Mixes junk lines into the output.
    #[must_use]
    pub fn garbage(mut self) -> Self {
        self.behavior.garbage = true;
        self
    }
    /// Does not announce `MultiPV`.
    #[must_use]
    pub fn no_multipv(mut self) -> Self {
        self.behavior.multipv = false;
        self
    }
    /// Announces (or not) `UCI_Chess960`.
    #[must_use]
    pub fn chess960(mut self, announce: bool) -> Self {
        self.behavior.chess960 = announce;
        self
    }
    /// Ends lines with `\r\n`.
    #[must_use]
    pub fn crlf(mut self) -> Self {
        self.behavior.crlf = true;
        self
    }
    /// Speaks XBoard instead of UCI, with the given dialect.
    #[must_use]
    pub fn xboard(mut self, dialect: XBoardFake) -> Self {
        self.behavior.protocol = FakeProtocol::XBoard;
        self.behavior.xboard = dialect;
        self
    }
    /// Finishes the builder.
    pub fn build(self) -> ScriptedEngine {
        ScriptedEngine {
            behavior: self.behavior,
            received: Arc::default(),
        }
    }
}

impl ScriptedEngine {
    /// A builder with the defaults of a well-behaved engine.
    pub fn builder() -> ScriptedEngineBuilder {
        ScriptedEngineBuilder::default()
    }

    /// Every line the session has sent to this engine so far.
    pub fn received(&self) -> Vec<String> {
        self.received
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Starts a session against this engine without waiting for the
    /// handshake (see [`Engine::launch`]).
    pub fn launch(&self, config: EngineConfig) -> (Engine, EngineEvents) {
        let (wire, engine_read, engine_write) = pipe_wire();
        let behavior = self.behavior.clone();
        let recorder = Arc::clone(&self.received);
        tokio::spawn(async move {
            drop(run_fake(engine_read, engine_write, behavior, Some(recorder)).await);
        });
        let log = LogHandle::new(config.log_lines);
        let launched = launch_on(config, wire, log);
        (launched.engine, launched.events)
    }

    /// Starts a session against this engine and waits for the handshake.
    pub async fn start(&self, config: EngineConfig) -> Result<(Engine, EngineEvents), EngineError> {
        let (wire, engine_read, engine_write) = pipe_wire();
        let behavior = self.behavior.clone();
        let recorder = Arc::clone(&self.received);
        tokio::spawn(async move {
            drop(run_fake(engine_read, engine_write, behavior, Some(recorder)).await);
        });
        let log = LogHandle::new(config.log_lines);
        let launched = launch_on(config, wire, log);
        match launched.startup.await {
            Ok(Ok(())) => Ok((launched.engine, launched.events)),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(EngineError::Closed),
        }
    }
}

/// The engine side of an in-memory pipe, played by the test itself.
#[derive(Debug)]
pub struct ManualEngine {
    lines: tokio::io::Lines<BufReader<ReadHalf<DuplexStream>>>,
    out: Option<WriteHalf<DuplexStream>>,
}

impl ManualEngine {
    /// Starts a session whose engine is this handle.
    pub fn launch(config: EngineConfig) -> (ManualEngine, Engine, EngineEvents) {
        let (wire, engine_read, engine_write) = pipe_wire();
        let log = LogHandle::new(config.log_lines);
        let launched = launch_on(config, wire, log);
        let manual = ManualEngine {
            lines: BufReader::new(engine_read).lines(),
            out: Some(engine_write),
        };
        (manual, launched.engine, launched.events)
    }

    /// The next line the session sent; `None` when it closed its output.
    pub async fn next_line(&mut self) -> Option<String> {
        self.lines.next_line().await.ok().flatten()
    }

    /// Writes one line to the session.
    pub async fn say(&mut self, line: &str) {
        self.say_raw(format!("{line}\n").as_bytes()).await;
    }

    /// Writes raw bytes to the session.
    pub async fn say_raw(&mut self, bytes: &[u8]) {
        if let Some(out) = &mut self.out {
            out.write_all(bytes).await.ok();
            out.flush().await.ok();
        }
    }

    /// Plays the engine's part of the handshake: reads `uci`, announces
    /// `option_lines` (complete `option name ...` lines) and `uciok`, reads
    /// what the session sends up to `isready` and answers `readyok`. Returns
    /// the lines read after `uci`, `isready` included.
    pub async fn handshake(&mut self, name: &str, option_lines: &[&str]) -> Vec<String> {
        drop(self.next_line().await);
        self.say(&format!("id name {name}")).await;
        self.say("id author tests").await;
        for line in option_lines {
            self.say(line).await;
        }
        self.say("uciok").await;
        let mut seen = Vec::new();
        while let Some(line) = self.next_line().await {
            let done = line == "isready";
            seen.push(line);
            if done {
                break;
            }
        }
        self.say("readyok").await;
        seen
    }

    /// Closes the engine's output: the session sees the end of file.
    pub async fn close(&mut self) {
        if let Some(mut out) = self.out.take() {
            out.shutdown().await.ok();
        }
    }
}
