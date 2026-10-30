//! The part of the UCI session before the engine is ready: collecting the
//! answer to `uci`, and the stand-alone probe that only learns the options.

use std::collections::HashMap;

use tokio::time::Instant;

use crate::engine::{EngineConfig, EngineError, Phase};
use crate::log::{LogHandle, LogKind};
use crate::option::{EngineOption, OptionKind};
use crate::protocol::{Capabilities, EngineIdentity};
use crate::uci::parse::{UciLine, parse_line, parse_option_noted};
use crate::wire::{Farewell, Wire, WireEvent};

/// The most options kept from one engine; the rest are ignored with a note.
pub(crate) const MAX_OPTIONS: usize = 512;

/// What `uci` collects before `uciok`.
#[derive(Debug, Default)]
pub(crate) struct HandshakeData {
    name: Option<String>,
    author: Option<String>,
    options: Vec<EngineOption>,
    /// Option name to index in `options`, for the "last one wins" rule.
    index: HashMap<String, usize>,
    overflowed: bool,
}

impl HandshakeData {
    /// Feeds one line seen before `uciok`. Returns `true` at `uciok`.
    pub(crate) fn feed(&mut self, line: UciLine, log: &LogHandle) -> bool {
        match line {
            UciLine::UciOk => return true,
            UciLine::IdName(name) => self.name = Some(name),
            UciLine::IdAuthor(author) => self.author = Some(author),
            UciLine::Option(Ok(option)) => self.add_option(option, log),
            UciLine::Option(Err(reason)) => {
                log.push(LogKind::Note, &format!("option ignored: {reason}"));
            }
            _ => tracing::debug!("ignored line before uciok"),
        }
        false
    }

    /// Parses and feeds one raw line seen before `uciok`; options repaired by
    /// the parser leave a note in the log. Returns `true` at `uciok`.
    pub(crate) fn feed_text(&mut self, text: &str, log: &LogHandle) -> bool {
        if text.trim_start().starts_with("option ") {
            match parse_option_noted(text) {
                Ok((option, note)) => {
                    if let Some(note) = note {
                        log.push(LogKind::Note, &note);
                    }
                    self.add_option(option, log);
                }
                Err(reason) => log.push(LogKind::Note, &format!("option ignored: {reason}")),
            }
            return false;
        }
        self.feed(parse_line(text), log)
    }

    /// Adds an option; a repeated name replaces the earlier one.
    pub(crate) fn add_option(&mut self, option: EngineOption, log: &LogHandle) {
        // Names are matched without regard to case (`find_option`), so a
        // `hash` after a `Hash` is the same option.
        let key = option.name.to_ascii_lowercase();
        if let Some(&at) = self.index.get(&key) {
            self.options[at] = option;
        } else if self.options.len() >= MAX_OPTIONS {
            if !self.overflowed {
                self.overflowed = true;
                log.push(
                    LogKind::Note,
                    &format!("more than {MAX_OPTIONS} options; the rest are ignored"),
                );
            }
        } else {
            self.index.insert(key, self.options.len());
            self.options.push(option);
        }
    }

    pub(crate) fn into_identity(self, fallback_name: &str) -> EngineIdentity {
        let capabilities = capabilities_of(&self.options);
        EngineIdentity {
            name: self
                .name
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| fallback_name.to_owned()),
            author: self.author.filter(|a| !a.is_empty()),
            options: self.options,
            capabilities,
        }
    }
}

pub(crate) fn find_option<'a>(options: &'a [EngineOption], name: &str) -> Option<&'a EngineOption> {
    options.iter().find(|o| o.name.eq_ignore_ascii_case(name))
}

fn capabilities_of(options: &[EngineOption]) -> Capabilities {
    let multipv = find_option(options, "MultiPV").and_then(|o| match o.kind {
        OptionKind::Spin { min, max, .. } => {
            let lo = u8::try_from(min.max(1)).ok()?;
            let hi = u8::try_from(max.min(i64::from(u8::MAX))).ok()?;
            (lo <= hi).then_some(lo..=hi)
        }
        _ => None,
    });
    let has = |name| find_option(options, name).is_some();
    Capabilities {
        multipv,
        search_moves: true,
        chess960: has("UCI_Chess960"),
        analyse_mode: has("UCI_AnalyseMode"),
        wdl: has("UCI_ShowWDL"),
        ponder: has("Ponder"),
    }
}

/// Learns the options of an engine and ends it (the Scid's `uciConfig`).
pub(crate) async fn probe(
    mut wire: Wire,
    config: &EngineConfig,
    log: &LogHandle,
) -> Result<EngineIdentity, EngineError> {
    log.push(LogKind::Sent, "uci");
    if wire.send_line("uci").is_err() {
        let _ = wire.terminate(Farewell::Silent, config.timeouts.quit).await;
        return Err(EngineError::Closed);
    }
    let deadline = Instant::now() + config.timeouts.uciok;
    let mut data = HandshakeData::default();
    let outcome = loop {
        match tokio::time::timeout_at(deadline, wire.recv()).await {
            Err(_) => break Err(EngineError::Timeout(Phase::Uciok)),
            Ok(None | Some(WireEvent::Eof)) => break Err(EngineError::Closed),
            Ok(Some(WireEvent::Error(message))) => {
                break Err(EngineError::Io(std::io::Error::other(message)));
            }
            Ok(Some(WireEvent::Line { text, .. })) => {
                log.push(LogKind::Received, &text);
                if data.feed_text(&text, log) {
                    break Ok(data.into_identity(&config.name));
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
        Err(EngineError::Closed) => Err(EngineError::ExitedEarly {
            status,
            stderr_tail: wire.stderr_tail(),
        }),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_names_that_differ_only_in_case_are_one_option() {
        let log = LogHandle::new(10);
        let mut data = HandshakeData::default();
        data.feed_text("option name Hash type spin default 16 min 1 max 1024", &log);
        data.feed_text("option name hash type spin default 32 min 1 max 2048", &log);
        data.feed_text("uciok", &log);
        let identity = data.into_identity("x");
        assert_eq!(identity.options.len(), 1);
        assert_eq!(identity.options[0].name, "hash");
    }
}
