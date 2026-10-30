//! Shared helpers of the `m8_3_gui_*` tests (spec
//! `docs/specs/m8-3-lichess-online.md`, "M8-3c"). No test touches the
//! network: the session is put in the state the streams would leave it in,
//! and the effects of the application are never run.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::time::{Duration, Instant};

use mychess_app::online::{AccountView, Link, OnlineGame, OnlinePhase, PlayingState};
use mychess_core::Color;
use mychess_online::models::GameFull;

use super::common::Rig;

pub const GAME_ID: &str = "abcd1234";

/// The `gameFull` of a rapid 10+5 game between `alice` (White) and `bob`.
pub fn full_json(fen: &str, moves: &str, status: &str) -> String {
    format!(
        r#"{{"id":"{GAME_ID}","variant":{{"key":"standard","name":"Standard","short":"Std"}},"speed":"rapid","perf":{{"name":"Rapid"}},"rated":false,"createdAt":1700000000000,"white":{{"id":"alice","name":"alice","title":"NM","rating":1800}},"black":{{"id":"bob","name":"Bob","rating":1500,"provisional":true}},"initialFen":"{fen}","clock":{{"initial":600000,"increment":5000}},"type":"gameFull","state":{{"type":"gameState","moves":"{moves}","wtime":600000,"btime":600000,"winc":5000,"binc":5000,"status":"{status}"}}}}"#
    )
}

pub fn account() -> AccountView {
    AccountView {
        login: "alice".to_owned(),
        id: "alice".to_owned(),
        title: None,
        ratings: vec![("rapid".to_owned(), 1800, false)],
        can_challenge: true,
    }
}

/// Connected and idle, with the event stream up.
pub fn connect(rig: &mut Rig) {
    let online = &mut rig.shell.app.online;
    online.phase = OnlinePhase::Idle;
    online.link = Link::Up;
    online.account = Some(account());
    rig.sync();
}

/// A game on screen, as `me` ("alice" plays White, "bob" Black). The
/// clocks run for the side to move from `at`.
pub fn start_game(rig: &mut Rig, me: &str, fen: &str, moves: &str, at: Instant) {
    let full: GameFull = serde_json::from_str(&full_json(fen, moves, "started")).unwrap();
    let game = OnlineGame::from_full(&full, me).unwrap();
    let turn = game.turn();
    let online = &mut rig.shell.app.online;
    online.phase = OnlinePhase::Playing(PlayingState::Live);
    online.link = Link::Up;
    online.account = Some(account());
    online.game = Some(game);
    online.clock.sync(
        Duration::from_secs(600),
        Duration::from_secs(600),
        turn,
        true,
        at,
    );
    rig.sync();
}

pub fn game_mut(rig: &mut Rig) -> &mut OnlineGame {
    rig.shell.app.online.game.as_mut().unwrap()
}
