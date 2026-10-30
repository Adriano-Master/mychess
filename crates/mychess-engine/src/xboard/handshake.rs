//! The start of an XBoard session (spec m4-2, D416-D417, D421): `xboard`,
//! `protover 2`, the `feature` lines and the quirk table, plus the
//! stand-alone probe.

use tokio::time::Instant;

use crate::engine::{EngineConfig, EngineError, Phase, Timeouts};
use crate::log::{LogHandle, LogKind};
use crate::protocol::{Capabilities, EngineIdentity};
use crate::wire::{Farewell, Wire, WireEvent};
use crate::xboard::features::{Features, answer, parse_feature_line};
use crate::xboard::quirks::{self, Detected};
use crate::xboard::thinking::Style;

/// The most start-up lines kept for the quirk table.
const MAX_BANNER_LINES: usize = 64;
/// The longest start-up line kept, in bytes.
const MAX_BANNER_BYTES: usize = 512;

/// The outcome of one line.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Feed {
    /// `accepted`/`rejected` replies to send.
    pub(crate) responses: Vec<String>,
    /// `done=1` was seen: the announcement is over.
    pub(crate) done: bool,
}

/// What the session knows about the engine once it has said everything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Settings {
    pub(crate) name: String,
    pub(crate) setboard: bool,
    pub(crate) analyze: bool,
    pub(crate) usermove: bool,
    pub(crate) ping: bool,
    /// The engine asked for no `white`/`black` commands (`colors=0`).
    pub(crate) no_colors: bool,
    pub(crate) style: Style,
    /// Commands the quirk table asks for.
    pub(crate) startup: &'static [&'static str],
    /// The table entry that matched.
    pub(crate) quirk: Option<&'static str>,
    /// At least one `feature` line arrived (the engine knows `protover 2`).
    pub(crate) saw_feature: bool,
}

impl Settings {
    /// The identity the application sees: no options, the capabilities of
    /// the adapter (spec m4-2, D422).
    pub(crate) fn identity(&self) -> EngineIdentity {
        EngineIdentity {
            name: self.name.clone(),
            author: None,
            options: Vec::new(),
            capabilities: Capabilities {
                multipv: None,
                search_moves: false,
                chess960: false,
                analyse_mode: false,
                wdl: false,
                ponder: false,
            },
        }
    }
}

/// Collects the engine's answer to `protover 2`.
#[derive(Debug)]
pub(crate) struct Handshake {
    features: Features,
    texts: Vec<String>,
    saw_feature: bool,
    done_pending: bool,
    started: Instant,
    deadline: Instant,
    timeouts: Timeouts,
}

impl Handshake {
    pub(crate) fn new(now: Instant, timeouts: Timeouts) -> Handshake {
        Handshake {
            features: Features::default(),
            texts: Vec::new(),
            saw_feature: false,
            done_pending: false,
            started: now,
            deadline: now + timeouts.feature,
            timeouts,
        }
    }

    /// When the wait ends: 2 s after the last `feature` line, or the 15 s
    /// ceiling after `done=0`.
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Feeds one line of the engine.
    pub(crate) fn feed(&mut self, line: &str, now: Instant) -> Feed {
        let trimmed = line.trim_start();
        let is_feature = trimmed
            .strip_prefix("feature")
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace));
        if !is_feature {
            if self.texts.len() < MAX_BANNER_LINES {
                self.texts
                    .push(crate::uci::parse::truncate_utf8(trimmed, MAX_BANNER_BYTES));
            }
            return Feed::default();
        }
        self.saw_feature = true;
        let mut feed = Feed::default();
        for (key, value) in parse_feature_line(trimmed) {
            feed.responses.push(answer(&key, &value));
            self.features.apply(&key, &value);
            if key == "myname" && self.texts.len() < MAX_BANNER_LINES {
                self.texts.push(value);
            }
        }
        match self.features.done {
            Some(true) => feed.done = true,
            Some(false) => {
                self.done_pending = true;
                self.deadline = self.started + self.timeouts.feature_done;
            }
            None => self.deadline = now + self.timeouts.feature,
        }
        feed
    }

    /// The deadline passed: an engine that said `done=0` and never `done=1`
    /// is too slow (`Err`); any other is taken as finished (without a
    /// single `feature`: a `protover 1` engine).
    pub(crate) fn on_deadline(&self) -> Result<(), Phase> {
        if self.done_pending {
            Err(Phase::Features)
        } else {
            Ok(())
        }
    }

    /// Settles the engine's abilities: what it announced wins over the
    /// quirk table.
    pub(crate) fn finish(self, fallback_name: &str, log: &LogHandle) -> Settings {
        let detected: Option<Detected> = quirks::detect(self.texts.iter().map(String::as_str));
        let quirk = detected.map(|d| d.quirk);
        if let Some(d) = &detected {
            log.push(
                LogKind::Note,
                &format!(
                    "recognised `{}` from its start-up text{}",
                    d.quirk.name,
                    if d.scores_white_pov {
                        "; scores are from White's point of view"
                    } else {
                        ""
                    }
                ),
            );
        }
        if !self.saw_feature {
            log.push(
                LogKind::Note,
                "no `feature` line: treating the engine as protover 1",
            );
        }
        let f = &self.features;
        Settings {
            name: f
                .myname
                .clone()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| fallback_name.to_owned()),
            setboard: f
                .setboard
                .or(quirk.and_then(|q| q.setboard))
                .unwrap_or(false),
            analyze: f.analyze.or(quirk.and_then(|q| q.analyze)).unwrap_or(false),
            usermove: f.usermove,
            ping: f.ping,
            no_colors: f.no_colors,
            style: Style {
                scores_white_pov: detected.is_some_and(|d| d.scores_white_pov),
                whole_seconds: quirk.is_some_and(|q| q.whole_seconds),
            },
            startup: quirk.map_or(&[], |q| q.startup),
            quirk: quirk.map(|q| q.name),
            saw_feature: self.saw_feature,
        }
    }
}

/// Learns the identity of an XBoard engine and ends it (`probe_xboard`).
/// Also tells whether the engine sent any `feature` line.
pub(crate) async fn probe(
    mut wire: Wire,
    config: &EngineConfig,
    log: &LogHandle,
) -> Result<(EngineIdentity, bool), EngineError> {
    for line in ["xboard", "protover 2"] {
        log.push(LogKind::Sent, line);
        if wire.send_line(line).is_err() {
            wire.terminate(Farewell::Silent, config.timeouts.quit).await;
            return Err(EngineError::Closed);
        }
    }
    let mut hs = Handshake::new(Instant::now(), config.timeouts);
    let outcome = loop {
        match tokio::time::timeout_at(hs.deadline(), wire.recv()).await {
            Err(_) => {
                break match hs.on_deadline() {
                    Ok(()) => Ok(()),
                    Err(phase) => Err(EngineError::Timeout(phase)),
                };
            }
            Ok(None | Some(WireEvent::Eof)) => break Err(EngineError::Closed),
            Ok(Some(WireEvent::Error(message))) => {
                break Err(EngineError::Io(std::io::Error::other(message)));
            }
            Ok(Some(WireEvent::Line { text, .. })) => {
                log.push(LogKind::Received, &text);
                let feed = hs.feed(&text, Instant::now());
                for response in &feed.responses {
                    log.push(LogKind::Sent, response);
                    // A failed write shows up as the engine closing.
                    wire.send_line(response).ok();
                }
                if feed.done {
                    break Ok(());
                }
            }
        }
    };
    let eof = matches!(outcome, Err(EngineError::Closed));
    let farewell = if eof {
        Farewell::Silent
    } else {
        Farewell::SayQuit
    };
    let status = wire.terminate(farewell, config.timeouts.quit).await;
    match outcome {
        Ok(()) => {
            let settings = hs.finish(&config.name, log);
            Ok((settings.identity(), settings.saw_feature))
        }
        Err(EngineError::Closed) => Err(EngineError::ExitedEarly {
            status,
            stderr_tail: wire.stderr_tail(),
        }),
        Err(other) => Err(other),
    }
}
