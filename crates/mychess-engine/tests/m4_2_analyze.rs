//! M4-2 contract tests: Analyze and Go modes, the exact command sequences,
//! `ping`/`pong`, `Stop` in every state, refusals, `RequestStats`, shutdown
//! (spec `docs/specs/m4-2-protocolo-xboard.md`, D418, D419, D422, D423,
//! rules 7, 8, 10, 11). The engine is played by hand; the clock is paused.

#[path = "m4_1_common/helpers.rs"]
mod common;
#[path = "m4_2_common/xb.rs"]
mod xb;

use std::time::Duration;

use common::*;
use mychess_core::{Move, Position};
use mychess_engine::{
    EngineCommand, EngineError, EngineEvent, EngineWarning, ExitReason, Phase, SearchLimits,
    SearchPosition, SearchRequest,
};
use tokio::time::Instant;
use xb::*;

fn fen_of(fen: &str) -> String {
    pos(fen).fen()
}

fn setboard(fen: &str) -> String {
    format!("setboard {}", fen_of(fen))
}

fn from_start(moves: &[&str]) -> SearchRequest {
    let start = Position::new();
    let mut p = start.clone();
    let mut list: Vec<Move> = Vec::new();
    for text in moves {
        let m = p.parse_uci(text).expect("legal");
        p.play(m).expect("play");
        list.push(m);
    }
    SearchRequest::infinite(SearchPosition { start, moves: list })
}

fn movetime(fen: &str, ms: u64) -> SearchRequest {
    limits(
        fen,
        SearchLimits {
            movetime: Some(Duration::from_millis(ms)),
            ..SearchLimits::default()
        },
    )
}

// ---------------------------------------------------------------------------
// Analyze mode
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn analyze_with_setboard_sends_force_setboard_analyze_then_ping() {
    let mut r = full().await;
    let (id, lines) = r.start_raw(infinite(KPK)).await;
    assert_eq!(strs(&lines), ["force", &setboard(KPK), "analyze", "ping 1"]);
    r.pong(&lines).await;
    let info = r.think(" 5. +34 20 300 e2e4 e8e7").await;
    assert_eq!(
        info.search, id,
        "info belongs to the search that was started"
    );
    assert_eq!(info.multipv, 1);
}

#[tokio::test(start_paused = true)]
async fn analyze_without_ping_has_no_ping_line() {
    let mut r = no_ping().await;
    let (_, lines) = r.start_raw(infinite(KPK)).await;
    assert_eq!(strs(&lines), ["force", &setboard(KPK), "analyze"]);
}

#[tokio::test(start_paused = true)]
async fn setboard_carries_the_side_to_move_of_the_fen() {
    let mut r = full().await;
    let (_, lines) = r.start_raw(infinite(BLACK_WINS_QUEEN)).await;
    assert_eq!(lines[1], setboard(BLACK_WINS_QUEEN));
    assert!(lines[1].contains(" b "), "{}", lines[1]);
}

#[tokio::test(start_paused = true)]
async fn replacing_an_analysis_sends_exit_first_and_ends_the_old_search_with_a_best_move() {
    let mut r = full().await;
    let (first, lines) = r.start(infinite(KPK)).await;
    assert_eq!(lines.len(), 4);
    r.think(" 5. +34 20 300 e2e4 e8e7").await;
    r.engine
        .send(EngineCommand::Search(infinite(BLACK_WINS_QUEEN)))
        .unwrap();
    let lines = r.read_search_lines().await;
    assert_eq!(
        strs(&lines),
        [
            "exit",
            "force",
            &setboard(BLACK_WINS_QUEEN),
            "analyze",
            "ping 2"
        ]
    );
    let best = next_best(&mut r.events).await;
    assert_eq!(best.search, first);
    assert_eq!(best.mv, Some(mv(&pos(KPK), "e2e4")));
    let second = r.started().await;
    assert_ne!(second, first);
}

#[tokio::test(start_paused = true)]
async fn old_search_events_come_before_the_new_started() {
    let mut r = full().await;
    let (first, _) = r.start(infinite(KPK)).await;
    r.engine
        .send(EngineCommand::Search(infinite(BLACK_WINS_QUEEN)))
        .unwrap();
    r.read_search_lines().await;
    let order = until_events(&mut r, 2).await;
    assert!(
        matches!(order[0], EngineEvent::BestMove(b) if b.search == first),
        "{order:?}"
    );
    assert!(
        matches!(order[1], EngineEvent::SearchStarted(_)),
        "{order:?}"
    );
}

async fn until_events(r: &mut Rig, n: usize) -> Vec<EngineEvent> {
    let mut v = Vec::new();
    while v.len() < n {
        v.push(next(&mut r.events).await);
    }
    v
}

#[tokio::test(start_paused = true)]
async fn stop_in_analyze_sends_exit_and_synthesizes_the_best_move_from_the_last_pv() {
    let mut r = full().await;
    let (id, _) = r.start(infinite(KPK)).await;
    r.think(" 3. +10 5 100 e1d1").await;
    r.think(" 5. +34 20 300 e2e4 e8e7").await;
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    let best = next_best(&mut r.events).await;
    assert_eq!(best.search, id);
    assert_eq!(
        best.mv,
        Some(mv(&pos(KPK), "e2e4")),
        "first move of the LAST pv"
    );
    assert_eq!(best.ponder, None);
}

#[tokio::test(start_paused = true)]
async fn stop_without_any_pv_yields_best_move_none() {
    let mut r = full().await;
    let (id, _) = r.start(infinite(KPK)).await;
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    let best = next_best(&mut r.events).await;
    assert_eq!((best.search, best.mv), (id, None));
}

#[tokio::test(start_paused = true)]
async fn stop_while_idle_writes_nothing_and_emits_nothing() {
    let mut r = full().await;
    r.engine.send(EngineCommand::Stop).unwrap();
    settle().await;
    quiet(&mut r.events);
    // Nothing was written: the next search begins with `force`.
    let (_, lines) = r.start_raw(infinite(KPK)).await;
    assert_eq!(lines[0], "force");
}

#[tokio::test(start_paused = true)]
async fn a_second_stop_after_the_first_is_a_no_op() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    r.engine.send(EngineCommand::Stop).unwrap();
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    next_best(&mut r.events).await;
    settle().await;
    quiet(&mut r.events);
    let (_, lines) = r.start_raw(infinite(KPK)).await;
    assert_eq!(lines[0], "force", "no second `exit` was written");
}

#[tokio::test(start_paused = true)]
async fn stop_during_the_ping_synchronization_ends_the_search_and_late_output_is_dropped() {
    let mut r = full().await;
    let (id, lines) = r.start_raw(infinite(KPK)).await; // no pong yet
    assert_eq!(lines.last().map(String::as_str), Some("ping 1"));
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    let best = next_best(&mut r.events).await;
    assert_eq!((best.search, best.mv), (id, None));
    // The old engine output arrives late: nobody must see it.
    r.peer.say("pong 1").await;
    r.peer.say(" 9. +99 10 100 e2e4").await;
    settle().await;
    quiet(&mut r.events);
    // And the session is healthy: a new search runs normally.
    let (id2, lines) = r.start(infinite(KPK)).await;
    assert_ne!(id2, id);
    assert_eq!(lines[0], "force");
    r.think(" 2. +1 1 1 e2e4").await;
}

#[tokio::test(start_paused = true)]
async fn thinking_lines_before_pong_are_dropped_and_after_pong_delivered() {
    let mut r = full().await;
    let (_, lines) = r.start_raw(infinite(KPK)).await;
    r.peer.say(" 20. +500 10 100 e2e4").await; // output of the previous position
    r.pong(&lines).await;
    let info = r.think(" 2. +1 1 1 e1d1").await;
    assert_eq!(info.depth, Some(2), "the stale ply 20 line did not leak");
}

#[tokio::test(start_paused = true)]
async fn a_pong_with_another_number_does_not_release_the_search() {
    let mut r = full().await;
    let (_, _) = r.start_raw(infinite(KPK)).await;
    r.peer.say("pong 99").await;
    r.peer.say(" 7. +1 1 1 e2e4").await;
    settle().await;
    assert!(
        drain(&mut r.events)
            .iter()
            .all(|e| !matches!(e, EngineEvent::Info(_)))
    );
    r.peer.say("pong 1").await;
    r.think(" 8. +1 1 1 e2e4").await;
}

#[tokio::test(start_paused = true)]
async fn without_ping_deep_output_is_dropped_until_a_shallow_line_shows_the_new_search() {
    let mut r = no_ping().await;
    r.start_raw(infinite(KPK)).await;
    r.peer.say(" 12. +50 10 100 e2e4").await; // old search, within the window
    r.peer.say(" 1. +3 1 1 e2e4").await; // new search
    let info = next_info(&mut r.events).await;
    assert_eq!(info.depth, Some(1));
    let info = r.think(" 12. +60 10 100 e2e4").await;
    assert_eq!(
        info.depth,
        Some(12),
        "after the shallow line everything passes"
    );
}

#[tokio::test(start_paused = true)]
async fn without_ping_the_stale_window_ends_after_about_one_second() {
    let mut r = no_ping().await;
    r.start_raw(infinite(KPK)).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let info = r.think(" 12. +60 10 100 e2e4").await;
    assert_eq!(info.depth, Some(12));
}

#[tokio::test(start_paused = true)]
async fn a_missing_pong_ends_the_session_with_a_pong_timeout_after_two_seconds() {
    let mut r = full().await;
    let t0 = Instant::now();
    r.start_raw(infinite(KPK)).await;
    let exit = next_exit(&mut r.events).await;
    assert_eq!(exit.reason, ExitReason::Timeout(Phase::Pong));
    let waited = t0.elapsed();
    assert!(
        waited >= Duration::from_secs(2) && waited < Duration::from_secs(3),
        "{waited:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn new_game_makes_the_next_search_start_with_new_and_writes_nothing_by_itself() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    next_best(&mut r.events).await;
    r.engine.send(EngineCommand::NewGame).unwrap();
    settle().await;
    r.engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    let lines = r.read_search_lines().await;
    assert_eq!(
        strs(&lines),
        ["new", "force", &setboard(KPK), "analyze", "ping 2"]
    );
}

#[tokio::test(start_paused = true)]
async fn new_game_then_a_search_is_followed_by_normal_lines_without_a_second_new() {
    let mut r = full().await;
    r.engine.send(EngineCommand::NewGame).unwrap();
    let (_, lines) = r.start(infinite(KPK)).await;
    assert_eq!(lines[0], "new");
    r.engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    let lines = r.read_search_lines().await;
    assert_eq!(lines[0], "exit");
    assert!(!lines.iter().any(|l| l == "new"), "{lines:?}");
}

// ---------------------------------------------------------------------------
// Terminal positions and non-standard starts
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn a_terminal_position_answers_best_move_none_without_talking_to_the_engine() {
    for fen in [WHITE_MATED, BLACK_STALEMATED] {
        let mut r = full().await;
        r.engine.send(EngineCommand::Search(infinite(fen))).unwrap();
        let id = r.started().await;
        let best = next_best(&mut r.events).await;
        assert_eq!((best.search, best.mv), (id, None), "{fen}");
        let (_, lines) = r.start_raw(infinite(KPK)).await;
        assert_eq!(lines[0], "force", "nothing was written for {fen}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_terminal_position_after_an_analysis_first_leaves_analyze_mode() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    r.engine
        .send(EngineCommand::Search(infinite(WHITE_MATED)))
        .unwrap();
    assert_eq!(r.next_line().await, "exit");
    let best = next_best(&mut r.events).await; // old search
    assert_eq!(best.mv, None);
    let best = next_best(&mut r.events).await; // terminal
    assert_eq!(best.mv, None);
}

#[tokio::test(start_paused = true)]
async fn nonstandard_start_without_setboard_is_refused_synchronously() {
    let r = rig(&["feature analyze=1 done=1"]).await;
    let err = r
        .engine
        .send(EngineCommand::Search(infinite(KPK)))
        .expect_err("refused");
    assert!(
        matches!(err, EngineError::Unsupported("non-standard start position")),
        "{err:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn nonstandard_start_with_setboard_is_accepted() {
    let r = rig(&["feature analyze=1 setboard=1 done=1"]).await;
    r.engine
        .send(EngineCommand::Search(infinite(KPK)))
        .expect("accepted");
}

#[tokio::test(start_paused = true)]
async fn standard_start_without_setboard_is_accepted() {
    let r = rig(&["feature analyze=1 done=1"]).await;
    r.engine
        .send(EngineCommand::Search(from_start(&["e2e4"])))
        .expect("accepted");
}

// ---------------------------------------------------------------------------
// Refusals (D422)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn multipv_above_one_searchmoves_and_ponder_are_unsupported() {
    let r = full().await;
    assert!(matches!(
        r.engine.send(EngineCommand::SetMultiPv(2)),
        Err(EngineError::Unsupported(_))
    ));
    assert!(matches!(
        r.engine.send(EngineCommand::SetMultiPv(255)),
        Err(EngineError::Unsupported(_))
    ));
    r.engine
        .send(EngineCommand::SetMultiPv(1))
        .expect("1 line is fine");
    let mut with_moves = infinite(START_FEN);
    with_moves.search_moves = vec![mv(&Position::new(), "e2e4")];
    assert!(matches!(
        r.engine.send(EngineCommand::Search(with_moves)),
        Err(EngineError::Unsupported(_))
    ));
    let mut pondering = infinite(START_FEN);
    pondering.ponder = true;
    assert!(matches!(
        r.engine.send(EngineCommand::Search(pondering)),
        Err(EngineError::Unsupported(_))
    ));
}

#[tokio::test(start_paused = true)]
async fn refused_commands_write_nothing_to_the_engine() {
    let mut r = full().await;
    drop(r.engine.send(EngineCommand::SetMultiPv(3)));
    settle().await;
    let (_, lines) = r.start_raw(infinite(KPK)).await;
    assert_eq!(lines[0], "force");
}

// ---------------------------------------------------------------------------
// Go mode (D418): limits, GoFallback
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn a_depth_limit_selects_go_mode_with_sd_and_default_st() {
    let mut r = full().await;
    let (_, lines) = r.start_raw(depth(KPK, 12)).await;
    assert_eq!(
        strs(&lines),
        [
            "new",
            "force",
            &setboard(KPK),
            "st 120000",
            "sd 12",
            "post",
            "ping 1",
            "go"
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn movetime_becomes_st_in_whole_seconds_rounded_up_with_a_minimum_of_one() {
    for (ms, st) in [
        (1_u64, 1_u64),
        (500, 1),
        (999, 1),
        (1000, 1),
        (1001, 2),
        (2500, 3),
        (60_000, 60),
    ] {
        let mut r = full().await;
        let (_, lines) = r.start_raw(movetime(KPK, ms)).await;
        assert!(lines.contains(&format!("st {st}")), "{ms} ms -> {lines:?}");
        assert!(lines.contains(&"sd 50".to_owned()), "{lines:?}");
    }
}

#[tokio::test(start_paused = true)]
async fn depth_zero_is_sent_as_at_least_one() {
    let mut r = full().await;
    let (_, lines) = r.start_raw(depth(KPK, 0)).await;
    assert!(lines.contains(&"sd 1".to_owned()), "{lines:?}");
}

#[tokio::test(start_paused = true)]
async fn an_engine_without_analyze_uses_go_mode_for_an_infinite_search() {
    let mut r = rig(&["feature setboard=1 ping=1 done=1"]).await;
    let (_, lines) = r.start_raw(infinite(KPK)).await;
    assert_eq!(
        strs(&lines),
        [
            "new",
            "force",
            &setboard(KPK),
            "st 120000",
            "sd 50",
            "post",
            "ping 1",
            "go"
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn go_search_ends_with_the_engines_move() {
    let mut r = full().await;
    let (id, _) = r.start(depth(START_FEN, 5)).await;
    r.think(" 1. +10 1 1 e2e4").await;
    r.peer.say("move e2e4").await;
    let best = next_best(&mut r.events).await;
    assert_eq!(
        (best.search, best.mv),
        (id, Some(mv(&Position::new(), "e2e4")))
    );
    // And the session is idle again.
    r.engine.send(EngineCommand::Stop).unwrap();
    settle().await;
    quiet(&mut r.events);
}

#[tokio::test(start_paused = true)]
async fn go_move_notations_are_all_understood() {
    let start = Position::new();
    let black = pos("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1");
    let cases: Vec<(&str, &str, &str)> = vec![
        (START_FEN, "move e2e4", "e2e4"),
        (START_FEN, "My move is: e2e4", "e2e4"),
        (START_FEN, "My move is e2e4", "e2e4"),
        (START_FEN, "move e4", "e2e4"),
        (START_FEN, "move Nf3", "g1f3"),
        (START_FEN, "  move   d2d4  ", "d2d4"),
        (
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
            "1. ... e7e5",
            "e7e5",
        ),
        (
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
            "1. ... e5",
            "e7e5",
        ),
    ];
    for (fen, say, want) in cases {
        let mut r = full().await;
        let (id, _) = r.start(depth(fen, 5)).await;
        r.peer.say(say).await;
        let best = next_best(&mut r.events).await;
        let position = if fen == START_FEN { &start } else { &black };
        assert_eq!(
            (best.search, best.mv),
            (id, Some(mv(position, want))),
            "{say}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn an_illegal_go_move_gives_best_move_none_and_a_warning() {
    let mut r = full().await;
    let (id, _) = r.start(depth(START_FEN, 5)).await;
    r.peer.say("move e2e5").await;
    let events = until_best(&mut r.events).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Warning(EngineWarning::IllegalBestMove(_)))),
        "{events:?}"
    );
    assert!(
        matches!(events.last(), Some(EngineEvent::BestMove(b)) if b.search == id && b.mv.is_none())
    );
}

#[tokio::test(start_paused = true)]
async fn in_analyze_mode_a_move_line_is_a_one_move_variation_not_a_best_move() {
    let mut r = full().await;
    let (id, _) = r.start(infinite(START_FEN)).await;
    let info = r.think("move e2e4").await;
    assert_eq!(info.search, id);
    assert_eq!(info.pv, vec![mv(&Position::new(), "e2e4")]);
    settle().await;
    assert!(
        drain(&mut r.events)
            .iter()
            .all(|e| !matches!(e, EngineEvent::BestMove(_)))
    );
}

#[tokio::test(start_paused = true)]
async fn result_and_error_lines_do_not_disturb_the_search() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    for l in [
        "1-0 {White mates}",
        "0-1",
        "1/2-1/2 {Draw by repetition}",
        "resign",
        "Illegal move: e2e5",
        "Error (unknown command): foo",
        "tellusererror hello",
    ] {
        r.peer.say(l).await;
    }
    settle().await;
    // Spec m4-2 (revised): `Illegal move` and `Error ...` are reported, once
    // per search, as a warning; nothing else comes out and the search goes on.
    let seen = drain(&mut r.events);
    assert!(
        matches!(
            seen.as_slice(),
            [EngineEvent::Warning(EngineWarning::Misbehaved(_))]
        ),
        "{seen:?}"
    );
    r.think(" 4. +10 1 1 e2e4").await;
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    assert!(next_best(&mut r.events).await.mv.is_some());
}

#[tokio::test(start_paused = true)]
async fn a_move_line_with_no_search_running_is_ignored() {
    let mut r = full().await;
    r.peer.say("move e2e4").await;
    r.peer.say("My move is: e2e4").await;
    settle().await;
    quiet(&mut r.events);
}

#[tokio::test(start_paused = true)]
async fn output_after_the_search_ended_never_reaches_the_app() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    next_best(&mut r.events).await;
    r.peer.say(" 30. +10 1 1 e2e4").await;
    r.peer.say("stat01: 1 2 3 4 5 e2e4").await;
    settle().await;
    quiet(&mut r.events);
}

// ---------------------------------------------------------------------------
// Stop in Go mode (D423, D619)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn stop_in_go_mode_sends_question_mark_and_waits_for_the_move() {
    let mut r = full().await;
    let (id, _) = r.start(depth(START_FEN, 20)).await;
    r.think(" 2. +10 1 1 e2e4").await;
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "?");
    settle().await;
    quiet(&mut r.events);
    r.peer.say("move d2d4").await;
    let best = next_best(&mut r.events).await;
    assert_eq!(
        (best.search, best.mv),
        (id, Some(mv(&Position::new(), "d2d4")))
    );
}

#[tokio::test(start_paused = true)]
async fn stop_in_go_mode_during_the_ping_synchronization_still_sends_the_question_mark() {
    let mut r = full().await;
    let (id, lines) = r.start_raw(depth(START_FEN, 20)).await; // no pong
    assert!(lines.contains(&"ping 1".to_owned()));
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "?");
    r.peer.say("move e2e4").await;
    let best = next_best(&mut r.events).await;
    assert_eq!(best.search, id);
}

#[tokio::test(start_paused = true)]
async fn go_engine_ignoring_the_question_mark_without_ping_ends_the_session() {
    let mut r = rig(&["feature setboard=1 done=1"]).await;
    let (id, _) = r.start(depth(START_FEN, 20)).await;
    r.peer.say(" 1. +10 1 1 e2e4").await;
    next_info(&mut r.events).await;
    let t0 = Instant::now();
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "?");
    let mut got_best = None;
    let exit = loop {
        match next(&mut r.events).await {
            EngineEvent::BestMove(b) => got_best = Some(b),
            EngineEvent::Exited(x) => break x,
            _ => {}
        }
    };
    assert!(
        t0.elapsed() >= Duration::from_millis(1500),
        "{:?}",
        t0.elapsed()
    );
    let best = got_best.expect("every search ends in a BestMove (spec m4-1 contract)");
    assert_eq!(best.search, id);
    assert_eq!(
        best.mv,
        Some(mv(&Position::new(), "e2e4")),
        "best known move"
    );
    assert_eq!(exit.reason, ExitReason::StopTimeout);
}

#[tokio::test(start_paused = true)]
async fn go_engine_ignoring_the_question_mark_with_ping_is_probed_with_a_ping() {
    let mut r = full().await;
    r.start(depth(START_FEN, 20)).await;
    let t0 = Instant::now();
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "?");
    let probe = r.next_line().await;
    assert!(probe.starts_with("ping "), "{probe}");
    assert!(t0.elapsed() >= Duration::from_millis(1500));
    // The engine is alive (pong) but did not move: misbehaving.
    r.peer.say(&format!("pong {}", &probe[5..])).await;
    let mut seen_warning = false;
    let mut seen_best = false;
    let exit = loop {
        match next(&mut r.events).await {
            EngineEvent::Warning(EngineWarning::Misbehaved(_)) => seen_warning = true,
            EngineEvent::BestMove(_) => seen_best = true,
            EngineEvent::Exited(x) => break x,
            _ => {}
        }
    };
    assert!(seen_warning && seen_best);
    assert_eq!(exit.reason, ExitReason::StopTimeout);
}

#[tokio::test(start_paused = true)]
async fn go_engine_dead_after_the_question_mark_gives_a_stop_timeout_after_two_waits() {
    let mut r = full().await;
    r.start(depth(START_FEN, 20)).await;
    let t0 = Instant::now();
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "?");
    assert!(r.next_line().await.starts_with("ping "));
    let exit = next_exit(&mut r.events).await;
    assert_eq!(exit.reason, ExitReason::StopTimeout);
    let waited = t0.elapsed();
    assert!(
        waited >= Duration::from_millis(3000) && waited < Duration::from_secs(5),
        "{waited:?}"
    );
}

// ---------------------------------------------------------------------------
// Positions without `setboard` (rules 10-11, D419)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn without_setboard_the_analyze_updates_send_only_the_difference() {
    for usermove in [false, true] {
        let features = if usermove {
            "feature analyze=1 usermove=1 done=1"
        } else {
            "feature analyze=1 done=1"
        };
        let um = |m: &str| {
            if usermove {
                format!("usermove {m}")
            } else {
                m.to_owned()
            }
        };
        let mut r = rig(&[features]).await;

        r.engine
            .send(EngineCommand::Search(from_start(&[])))
            .unwrap();
        let l = r.read_search_lines().await;
        assert_eq!(strs(&l), ["new", "force", "white", "analyze"]);

        r.engine
            .send(EngineCommand::Search(from_start(&["e2e4"])))
            .unwrap();
        let l = r.read_search_lines().await;
        assert_eq!(
            strs(&l),
            ["exit", "force", &um("e2e4"), "analyze"],
            "one new move"
        );

        r.engine
            .send(EngineCommand::Search(from_start(&["e2e4"])))
            .unwrap();
        let l = r.read_search_lines().await;
        assert_eq!(strs(&l), ["exit", "force", "analyze"], "same position");

        r.engine
            .send(EngineCommand::Search(from_start(&[])))
            .unwrap();
        let l = r.read_search_lines().await;
        assert_eq!(
            strs(&l),
            ["exit", "force", "undo", "analyze"],
            "one move less"
        );

        r.engine
            .send(EngineCommand::Search(from_start(&["e2e4", "e7e5", "g1f3"])))
            .unwrap();
        let l = r.read_search_lines().await;
        assert_eq!(
            strs(&l),
            [
                "exit",
                "new",
                "force",
                &um("e2e4"),
                &um("e7e5"),
                &um("g1f3"),
                "black",
                "analyze"
            ],
            "more than one move apart: from scratch"
        );

        r.engine
            .send(EngineCommand::Search(from_start(&[
                "e2e4", "e7e5", "g1f3", "b8c6",
            ])))
            .unwrap();
        let l = r.read_search_lines().await;
        assert_eq!(strs(&l), ["exit", "force", &um("b8c6"), "analyze"]);
    }
}

#[tokio::test(start_paused = true)]
async fn a_divergent_line_without_setboard_is_rebuilt_not_patched() {
    let mut r = rig(&["feature analyze=1 done=1"]).await;
    r.engine
        .send(EngineCommand::Search(from_start(&["e2e4", "e7e5"])))
        .unwrap();
    r.read_search_lines().await;
    r.engine
        .send(EngineCommand::Search(from_start(&["d2d4", "d7d5"])))
        .unwrap();
    let l = r.read_search_lines().await;
    assert_eq!(
        strs(&l),
        ["exit", "new", "force", "d2d4", "d7d5", "white", "analyze"]
    );
}

#[tokio::test(start_paused = true)]
async fn castling_is_sent_as_the_kings_two_square_move_in_lower_case() {
    let mut r = rig(&["feature analyze=1 done=1"]).await;
    let moves = ["e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "f8c5", "e1g1"];
    r.engine
        .send(EngineCommand::Search(from_start(&moves)))
        .unwrap();
    let l = r.read_search_lines().await;
    assert!(l.contains(&"e1g1".to_owned()), "{l:?}");
    assert!(
        l.iter()
            .all(|x| x == &x.to_lowercase() || x.starts_with("setboard")),
        "{l:?}"
    );
    assert_eq!(l[l.len() - 2], "black");
}

#[tokio::test(start_paused = true)]
async fn go_mode_without_setboard_sends_the_moves_color_and_limits() {
    let mut r = rig(&["feature done=1"]).await;
    let mut req = from_start(&["e2e4"]);
    req.limits.depth = Some(8);
    r.engine.send(EngineCommand::Search(req)).unwrap();
    let l = r.read_search_lines().await;
    assert_eq!(
        strs(&l),
        [
            "new",
            "force",
            "e2e4",
            "black",
            "st 120000",
            "sd 8",
            "post",
            "go"
        ]
    );
}

// ---------------------------------------------------------------------------
// RequestStats (rule 3)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn request_stats_sends_a_dot_and_stat01_becomes_an_info_without_pv() {
    let mut r = full().await;
    let (id, _) = r.start(infinite(KPK)).await;
    r.think(" 5. +34 20 300 e2e4 e8e7").await;
    r.engine.send(EngineCommand::RequestStats).unwrap();
    assert_eq!(r.next_line().await, ".");
    r.peer.say("stat01: 120 4500 7 20 35 e2e4").await;
    let info = next_info(&mut r.events).await;
    assert_eq!(info.search, id);
    assert_eq!(info.depth, Some(7));
    assert_eq!(info.time_ms, Some(1200), "centiseconds");
    assert_eq!(info.nodes, Some(4500));
    assert_eq!(info.pv.len(), 0);
    assert_eq!(info.score, None);
}

#[tokio::test(start_paused = true)]
async fn request_stats_while_idle_writes_nothing() {
    let mut r = full().await;
    r.engine.send(EngineCommand::RequestStats).unwrap();
    settle().await;
    let (_, lines) = r.start_raw(infinite(KPK)).await;
    assert_eq!(lines[0], "force");
}

#[tokio::test(start_paused = true)]
async fn malformed_stat01_lines_are_ignored() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    for l in [
        "stat01:",
        "stat01: 1 2",
        "stat01: a b c",
        "stat01: -1 2 3",
        "stat01: 1 99999999999999999999999 3",
    ] {
        r.peer.say(l).await;
    }
    settle().await;
    quiet(&mut r.events);
}

// ---------------------------------------------------------------------------
// Ending the session
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn quit_while_idle_sends_quit() {
    let mut r = full().await;
    r.engine.send(EngineCommand::Quit).unwrap();
    assert_eq!(r.next_line().await, "quit");
    assert_eq!(next_exit(&mut r.events).await.reason, ExitReason::Quit);
}

#[tokio::test(start_paused = true)]
async fn quit_during_analysis_sends_exit_then_quit() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    r.engine.send(EngineCommand::Quit).unwrap();
    assert_eq!(r.next_line().await, "exit");
    assert_eq!(r.next_line().await, "quit");
    assert_eq!(next_exit(&mut r.events).await.reason, ExitReason::Quit);
}

#[tokio::test(start_paused = true)]
async fn quit_during_a_go_search_sends_question_mark_then_quit() {
    let mut r = full().await;
    r.start(depth(START_FEN, 30)).await;
    r.engine.send(EngineCommand::Quit).unwrap();
    assert_eq!(r.next_line().await, "?");
    assert_eq!(r.next_line().await, "quit");
    assert_eq!(next_exit(&mut r.events).await.reason, ExitReason::Quit);
}

#[tokio::test(start_paused = true)]
async fn quit_after_stop_in_analyze_does_not_repeat_exit() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "exit");
    r.engine.send(EngineCommand::Quit).unwrap();
    assert_eq!(r.next_line().await, "quit");
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_dies_in_the_middle_of_a_search_ends_the_session_as_crashed() {
    let mut r = full().await;
    r.start(infinite(KPK)).await;
    r.think(" 5. +34 20 300 e2e4").await;
    r.peer.close().await;
    let exit = next_exit(&mut r.events).await;
    assert_eq!(exit.reason, ExitReason::Crashed);
    assert!(
        r.events.recv().await.is_none(),
        "the stream ends after Exited"
    );
    assert!(matches!(
        r.engine.send(EngineCommand::Search(infinite(KPK))),
        Err(EngineError::Closed)
    ));
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_dies_while_stopping_a_go_search_ends_the_session() {
    let mut r = full().await;
    r.start(depth(START_FEN, 20)).await;
    r.engine.send(EngineCommand::Stop).unwrap();
    assert_eq!(r.next_line().await, "?");
    r.peer.close().await;
    assert_eq!(next_exit(&mut r.events).await.reason, ExitReason::Crashed);
}
