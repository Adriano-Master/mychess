//! Helpers shared by the `m4_2_*` contract tests: an XBoard engine played by
//! hand over an in-memory pipe (`ManualEngine`).
//!
//! Included with `#[path = "m4_2_common/xb.rs"] mod xb;` next to
//! `#[path = "m4_1_common/helpers.rs"] mod common;` (it uses `crate::common`).
#![allow(dead_code)]

use std::time::Duration;

use crate::common::*;
use mychess_engine::testing::ManualEngine;
use mychess_engine::{
    Engine, EngineCommand, EngineConfig, EngineEvent, EngineEvents, Protocol, SearchId, SearchInfo,
    SearchRequest,
};

pub(crate) const FULL: &str = r#"feature myname="Manual" setboard=1 analyze=1 ping=1 done=1"#;
pub(crate) const NO_PING: &str = r#"feature myname="Manual" setboard=1 analyze=1 done=1"#;
pub(crate) const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

pub(crate) fn xconfig() -> EngineConfig {
    let mut c = EngineConfig::new("fake");
    c.protocol = Protocol::XBoard;
    c.info_interval = Duration::ZERO;
    c
}

pub(crate) struct Rig {
    pub peer: ManualEngine,
    pub engine: Engine,
    pub events: EngineEvents,
    /// Lines the session wrote after `protover 2`, up to and including
    /// `force`.
    pub init: Vec<String>,
    pub ping: bool,
}

/// Plays the start of the handshake: `banner` lines, then each `feature`
/// line; reads everything up to `force` and waits for `Ready`.
pub(crate) async fn rig_with(config: EngineConfig, banner: &[&str], features: &[&str]) -> Rig {
    let (mut peer, engine, mut events) = ManualEngine::launch(config);
    expect_lines(&mut peer, &["xboard", "protover 2"]).await;
    for b in banner {
        peer.say(b).await;
    }
    for f in features {
        peer.say(f).await;
    }
    let mut init = Vec::new();
    loop {
        let l = line(&mut peer).await;
        let done = l == "force";
        init.push(l);
        if done {
            break;
        }
    }
    match next(&mut events).await {
        EngineEvent::Ready(_) => {}
        other => panic!("expected Ready, got {other:?}"),
    }
    let ping = features.iter().any(|f| f.contains("ping=1"));
    Rig {
        peer,
        engine,
        events,
        init,
        ping,
    }
}

pub(crate) async fn rig(features: &[&str]) -> Rig {
    rig_with(xconfig(), &[], features).await
}

/// Full-featured engine (setboard, analyze, ping).
pub(crate) async fn full() -> Rig {
    rig(&[FULL]).await
}

pub(crate) async fn no_ping() -> Rig {
    rig(&[NO_PING]).await
}

impl Rig {
    /// Sends a search and reads what the engine receives up to the line that
    /// starts the search (`analyze` or `go`, then the `ping` after `analyze`).
    pub(crate) async fn start_raw(&mut self, request: SearchRequest) -> (SearchId, Vec<String>) {
        self.engine
            .send(EngineCommand::Search(request))
            .expect("send");
        let lines = self.read_search_lines().await;
        let id = self.started().await;
        (id, lines)
    }

    pub(crate) async fn read_search_lines(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            let l = line(&mut self.peer).await;
            let last = l == "analyze" || l == "go";
            lines.push(l);
            if last {
                break;
            }
        }
        if self.ping && lines.last().is_some_and(|l| l == "analyze") {
            lines.push(line(&mut self.peer).await);
        }
        lines
    }

    pub(crate) async fn started(&mut self) -> SearchId {
        until(&mut self.events, |e| match e {
            EngineEvent::SearchStarted(id) => Some(id),
            _ => None,
        })
        .await
    }

    /// Answers the `ping N` found in `lines`, if any.
    pub(crate) async fn pong(&mut self, lines: &[String]) {
        if let Some(n) = lines.iter().find_map(|l| l.strip_prefix("ping ")) {
            self.peer.say(&format!("pong {n}")).await;
        }
    }

    /// Starts a search and, when the engine uses `ping`, answers it.
    pub(crate) async fn start(&mut self, request: SearchRequest) -> (SearchId, Vec<String>) {
        let (id, lines) = self.start_raw(request).await;
        self.pong(&lines).await;
        (id, lines)
    }

    pub(crate) async fn start_fen(&mut self, fen: &str) -> SearchId {
        self.start(infinite(fen)).await.0
    }

    /// Writes a thinking line and returns the `Info` it produced.
    pub(crate) async fn think(&mut self, text: &str) -> SearchInfo {
        self.peer.say(text).await;
        next_info(&mut self.events).await
    }

    /// The next line the session wrote.
    pub(crate) async fn next_line(&mut self) -> String {
        line(&mut self.peer).await
    }
}

pub(crate) fn strs(lines: &[String]) -> Vec<&str> {
    lines.iter().map(String::as_str).collect()
}

/// Lets the session run, then asserts it emitted no event.
pub(crate) fn quiet(events: &mut EngineEvents) {
    let seen = drain(events);
    assert_eq!(seen.len(), 0, "unexpected events: {seen:?}");
}
