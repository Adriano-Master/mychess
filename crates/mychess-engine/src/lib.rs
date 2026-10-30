//! Chess engine integration for MyChess.
//!
//! Milestone M4: the UCI protocol (`docs/specs/m4-1-protocolo-uci.md`), the
//! XBoard/WinBoard protocol (`docs/specs/m4-2-protocolo-xboard.md`) and the
//! common engine API that `mychess-app` builds on. Both protocols run the
//! same [`Engine`] API; `EngineConfig::protocol` picks the session.
//!
//! - [`Engine`] is a clonable handle to a session task that owns the engine
//!   process. [`Engine::send`] never blocks; [`EngineEvents`] delivers
//!   [`EngineEvent`]s (identity, searches, rate-limited `info`, best moves,
//!   warnings, exit).
//! - Scores are always from White's point of view ([`Score`]), moves coming
//!   from the engine are validated against the searched position, and
//!   nothing the engine writes can panic the session.
//! - [`probe_uci`] learns an engine's options and ends it again;
//!   [`probe_xboard`] does the `protover 2` handshake the same way, and
//!   [`detect_protocol`] tells UCI from XBoard (an old XBoard engine that
//!   announces no `feature` is not detected: configure
//!   `Protocol::XBoard` explicitly).
//! - [`LogHandle`] is the bounded protocol log.
//! - With the `testing` feature, the `testing` module offers a scripted in-memory
//!   engine and a hand-driven peer, and the `mychess-fake-engine` binary
//!   speaks UCI or XBoard over stdin/stdout.
//!
//! Dependency rule: may depend only on `mychess-core`.

mod coalesce;
mod engine;
mod log;
mod option;
mod process;
mod protocol;
mod request;
mod uci;
mod wire;
mod xboard;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use engine::{
    BestMove, Engine, EngineCommand, EngineConfig, EngineError, EngineEvent, EngineEvents,
    EngineWarning, ExitInfo, ExitReason, MAX_PEER_TEXT, Phase, Timeouts, detect_protocol,
    probe_uci, probe_xboard,
};
pub use log::{DEFAULT_LOG_LINES, LogEntry, LogHandle, LogKind, MAX_LOG_ENTRY_BYTES};
pub use option::{EngineOption, OptionError, OptionKind, OptionValue, is_managed_option};
pub use process::{
    CREATE_NO_WINDOW, CommandPlan, IDLE_PRIORITY_CLASS, NICE_INCREMENT, build_command,
    build_command_with,
};
pub use protocol::{
    Bound, Capabilities, EngineIdentity, MATE_CENTIPAWNS, Protocol, Score, SearchId, SearchInfo,
    Wdl,
};
pub use request::{Clock, SearchLimits, SearchPosition, SearchRequest};
