//! M4-2 hardening tests: the cases the first round of contract tests left
//! out (`NewGame` rebuilds, very long games, refused searches, `Stop` with a
//! `pong` pending, `colors=0`, absurd `stat01`, `Illegal move` during a
//! search). The engine is played by hand; the clock is paused.

#[path = "m4_1_common/helpers.rs"]
mod common;
#[path = "m4_2_common/xb.rs"]
mod xb;

use std::time::Duration;

use common::*;
use mychess_core::{Move, Position};
use mychess_engine::{
    EngineCommand, EngineError, EngineEvent, EngineWarning, SearchLimits, SearchPosition,
    SearchRequest,
};
use xb::*;

const NO_SETBOARD: &str = r#"feature myname="Manual" analyze=1 ping=1 done=1"#;
const NO_SETBOARD_NO_COLORS: &str = r#"feature myname="Manual" analyze=1 ping=1 colors=0 done=1"#;

fn start_request() -> SearchRequest {
    infinite(START_FEN)
}

/// A legal game of `plies` plies from the start: the knights shuffle.
fn shuffle(plies: usize) -> SearchRequest {
    let start = Position::new();
    let mut p = start.clone();
    let mut moves: Vec<Move> = Vec::new();
    for text in ["g1f3", "g8f6", "f3g1", "f6g8"].iter().cycle().take(plies) {
        let m = p.parse_uci(text).expect("legal");
        p.play(m).expect("play");
        moves.push(m);
    }
    SearchRequest::infinite(SearchPosition { start, moves })
}

fn warnings(events: Vec<EngineEvent>) -> Vec<EngineWarning> {
    events
        .into_iter()
        .filter_map(|e| match e {
            EngineEvent::Warning(w) => Some(w),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Boards rebuilt from the moves (no `setboard`)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn new_game_then_search_without_setboard_rebuilds_with_new() {
    let mut r = rig(&[NO_SETBOARD]).await;
    let (_, first) = r.start(start_request()).await;
    assert_eq!(strs(&first)[..2], ["new", "force"], "{first:?}");
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    next_best(&mut r.events).await;
    // The same position again would only need `force`...
    let (_, again) = r.start(start_request()).await;
    assert_eq!(again[0], "force", "{again:?}");
    assert!(!again.iter().any(|l| l == "new"), "{again:?}");
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    next_best(&mut r.events).await;
    // ...but after `NewGame` the board is rebuilt from scratch.
    r.engine.send(EngineCommand::NewGame).unwrap();
    let (_, fresh) = r.start(start_request()).await;
    assert_eq!(strs(&fresh)[..2], ["new", "force"], "{fresh:?}");
}

#[tokio::test(start_paused = true)]
async fn a_game_too_long_to_replay_is_refused_and_the_session_lives() {
    let mut r = rig(&[NO_SETBOARD]).await;
    // Over 1100 plies used to overflow the write queue and kill the session.
    let verdict = r.engine.send(EngineCommand::Search(shuffle(1200)));
    assert!(
        matches!(verdict, Err(EngineError::Unsupported(_))),
        "{verdict:?}"
    );
    settle().await;
    quiet(&mut r.events);
    // Within the limit it is replayed whole.
    let (_, lines) = r.start(shuffle(400)).await;
    assert_eq!(lines.iter().filter(|l| l.len() == 4).count(), 400);
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    next_best(&mut r.events).await;
}

#[tokio::test(start_paused = true)]
async fn colors_zero_means_no_white_or_black_command() {
    let mut r = rig(&[NO_SETBOARD_NO_COLORS]).await;
    let (_, lines) = r.start(start_request()).await;
    assert!(
        !lines.iter().any(|l| l == "white" || l == "black"),
        "{lines:?}"
    );
    let mut plain = rig(&[NO_SETBOARD]).await;
    let (_, lines) = plain.start(start_request()).await;
    assert!(lines.iter().any(|l| l == "white"), "{lines:?}");
}

// ---------------------------------------------------------------------------
// Refusals and stops
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn a_refused_search_takes_the_engine_out_of_analyze_mode() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    // A path whose moves are illegal from the start: only the session sees it.
    let other = pos("4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
    let bad = SearchRequest::infinite(SearchPosition {
        start: Position::new(),
        moves: vec![mv(&other, "a1a8")],
    });
    r.engine.send(EngineCommand::Search(bad)).unwrap();
    assert_eq!(r.next_line().await, "exit");
    let seen = {
        settle().await;
        drain(&mut r.events)
    };
    assert!(warnings(seen).iter().any(|w| matches!(
        w,
        EngineWarning::CommandRefused {
            command: "position",
            ..
        }
    )));
    // The next search does not send a second `exit`.
    let (_, lines) = r.start(infinite(KPK)).await;
    assert_ne!(lines[0], "exit", "{lines:?}");
}

#[tokio::test(start_paused = true)]
async fn stop_in_go_mode_with_a_pong_pending_still_ends_with_the_move() {
    let mut r = full().await;
    let request = limits(
        KPK,
        SearchLimits {
            movetime: Some(Duration::from_secs(30)),
            ..SearchLimits::default()
        },
    );
    let (id, lines) = r.start_raw(request).await; // no pong yet
    assert!(lines.iter().any(|l| l.starts_with("ping ")), "{lines:?}");
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "?");
    r.peer.say("move e2e4").await;
    let best = next_best(&mut r.events).await;
    assert_eq!(best.search, id);
    assert!(best.mv.is_some());
}

// ---------------------------------------------------------------------------
// Hostile or odd output
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn stat01_with_an_enormous_ply_is_ignored() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    r.peer.say("stat01: 100 5000 4294967295 1 2 e4").await;
    settle().await;
    quiet(&mut r.events);
    r.peer.say("stat01: 100 5000 12 1 2 e4").await;
    let info = next_info(&mut r.events).await;
    assert_eq!(info.depth, Some(12));
}

#[tokio::test(start_paused = true)]
async fn illegal_move_during_a_search_warns_once_per_search() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    for l in [
        "Illegal move: e2e5",
        "Error (bad command): x",
        "Illegal move",
    ] {
        r.peer.say(l).await;
    }
    settle().await;
    let seen = warnings(drain(&mut r.events));
    assert!(
        matches!(seen.as_slice(), [EngineWarning::Misbehaved(_)]),
        "{seen:?}"
    );
    // Go mode too, and a new search warns again.
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    next_best(&mut r.events).await;
    let go = limits(
        KPK,
        SearchLimits {
            depth: Some(5),
            ..SearchLimits::default()
        },
    );
    r.start(go).await;
    r.peer.say("Illegal move: a1a1").await;
    settle().await;
    let seen = warnings(drain(&mut r.events));
    assert!(
        matches!(seen.as_slice(), [EngineWarning::Misbehaved(_)]),
        "{seen:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn complaints_outside_a_search_are_only_logged() {
    let mut r = full().await;
    r.peer.say("Illegal move: e2e5").await;
    settle().await;
    quiet(&mut r.events);
}
