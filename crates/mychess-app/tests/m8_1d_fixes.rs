//! M8-1d review fixes (spec D2431-D2438): what lets the *ponder* go, the
//! lock of the assistance over the coach, the depth of the two coach
//! searches, the tags of the coach, the ticks and the engine started only
//! for the coach.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m8_1d_common;

use std::time::{Duration, Instant};

use m8_1d_common::*;
use mychess_app::engines::{EngineId, RunId};
use mychess_app::online::{MarkerInfo, OnlineMessage};
use mychess_app::{
    CoachGrade, ColorChoice, Effect, Message, PlayEnd, PlayMessage, PlayPhase, StartFrom, TimeMode,
};
use mychess_core::{Game, GameResult, Move, Position};
use mychess_engine::{EngineCommand, Score};

const DEPTH: TimeMode = TimeMode::Depth(2);

fn pondering_game() -> (Rig, Vec<Effect>) {
    let mut r = ponder_rig();
    let cfg = ponder_config(&r, ColorChoice::White, DEPTH);
    r.start(cfg);
    let fx = r.player_move("e2", "e4", Instant::now());
    let request = play_search(&fx).expect("asked");
    let run = r.run;
    begin_search(&mut r.state, run, 10, request.tag);
    let fx = event_on(
        &mut r.state,
        run,
        best_event(
            10,
            Some(mv_after(&["e2e4"], "e7e5")),
            Some(mv_after(&["e2e4", "e7e5"], "g1f3")),
        ),
    );
    let tag = ponder_searches(&fx, true)[0].tag;
    begin_search(&mut r.state, run, 11, tag);
    (r, fx)
}

// ---- ponder is let go (D2431) ----

#[test]
fn taking_back_stops_the_ponder_and_drops_its_late_answer() {
    let (mut r, _) = pondering_game();
    assert!(r.view().pondering);
    let fx = r.send(PlayMessage::TakeBack(Instant::now()));
    assert_eq!(count(&fx, &EngineCommand::Stop), 1, "{fx:?}");
    assert!(!r.view().pondering);
    assert!(r.mainline().is_empty());
    assert_eq!(r.phase(), PlayPhase::Player);
    // The stopped search reports: it is not a move of the game.
    let run = r.run;
    event_on(
        &mut r.state,
        run,
        best_event(11, Some(mv_after(&["e2e4", "e7e5", "g1f3"], "b8c6")), None),
    );
    assert!(r.mainline().is_empty());
    assert_eq!(r.phase(), PlayPhase::Player);
}

fn move_in(fen: &str, line: &[&str], uci: &str) -> Move {
    let mut pos = Position::from_fen(fen).unwrap();
    for t in line {
        let m = pos.parse_uci(t).unwrap();
        pos.play(m).unwrap();
    }
    pos.parse_uci(uci).unwrap()
}

#[test]
fn a_player_move_that_ends_the_game_lets_the_ponder_go() {
    // Black mates with ...Rd1# instead of the move the engine expected.
    let fen = "3r2k1/5ppp/p7/8/8/8/5PPP/6K1 b - - 0 1";
    let mut r = ponder_rig();
    r.state
        .session
        .replace_game(Game::from_position(Position::from_fen(fen).unwrap()));
    let mut cfg = ponder_config(&r, ColorChoice::Black, DEPTH);
    cfg.start = StartFrom::CurrentPosition;
    r.start(cfg);
    let fx = r.player_move("a6", "a5", Instant::now());
    let request = play_search(&fx).expect("asked");
    let run = r.run;
    begin_search(&mut r.state, run, 10, request.tag);
    let fx = event_on(
        &mut r.state,
        run,
        best_event(
            10,
            Some(move_in(fen, &["a6a5"], "g1h1")),
            Some(move_in(fen, &["a6a5", "g1h1"], "a5a4")),
        ),
    );
    let ponder = ponder_searches(&fx, true);
    assert_eq!(ponder.len(), 1, "{fx:?}");
    begin_search(&mut r.state, run, 11, ponder[0].tag);
    let fx = r.player_move("d8", "d1", Instant::now());
    let cmds = commands(&fx);
    assert!(cmds.contains(&EngineCommand::Stop), "{cmds:?}");
    assert!(cmds.contains(&EngineCommand::SetPonder(false)), "{cmds:?}");
    assert!(
        !cmds.iter().any(|c| matches!(c, EngineCommand::Search(_))),
        "the game is over: {cmds:?}"
    );
    assert!(matches!(
        r.phase(),
        PlayPhase::Ended(PlayEnd::Checkmate { .. })
    ));
    assert_eq!(r.view().result, GameResult::Black);
    assert!(!r.view().pondering);
}

#[test]
fn switching_engines_after_a_loss_does_not_carry_the_ponder_over() {
    let (mut r, _) = pondering_game();
    let (second, second_run) = add_engine(&mut r, "Second", vec![check("Ponder")]);
    r.event(exited());
    assert_eq!(r.phase(), PlayPhase::Paused { lost: true });
    assert!(!r.view().pondering);
    let fx = r.send(PlayMessage::SwitchEngine(second, Instant::now()));
    let cmds = commands_for(&fx, second_run);
    assert!(cmds.contains(&EngineCommand::SetPonder(true)), "{cmds:?}");
    assert!(!commands(&fx).contains(&EngineCommand::PonderHit), "{fx:?}");
    assert!(
        commands(&fx)
            .iter()
            .all(|c| !matches!(c, EngineCommand::Search(s) if s.ponder)),
        "no ponder on the new engine yet: {fx:?}"
    );
    assert!(!r.view().pondering);
    // The player moves: a normal search on the new engine.
    let fx = r.player_move("g1", "f3", Instant::now());
    let searches = searches_for(&fx, second_run);
    assert_eq!(searches.len(), 1, "{fx:?}");
    assert!(!searches[0].ponder);
}

#[test]
fn the_search_after_a_hit_is_watched_like_any_other() {
    let (mut r, _) = pondering_game();
    let fx = r.player_move("g1", "f3", Instant::now());
    assert_eq!(count(&fx, &EngineCommand::PonderHit), 1);
    // The engine goes silent after the hit: 120 s in `depth` mode.
    r.send(PlayMessage::Tick(Instant::now() + Duration::from_secs(60)));
    assert_eq!(r.phase(), PlayPhase::Opponent);
    r.send(PlayMessage::Tick(Instant::now() + Duration::from_secs(130)));
    assert_eq!(r.phase(), PlayPhase::Paused { lost: true });
}

#[test]
fn ticks_stop_with_the_clocks_but_not_while_a_coach_search_is_in_flight() {
    let (mut r, _) = pondering_game();
    assert!(r.state.play_wants_ticks());
    r.send(PlayMessage::Pause(Instant::now()));
    assert!(!r.state.play_wants_ticks(), "no clock, no coach: no ticks");
    r.send(PlayMessage::Resign(Instant::now()));
    assert!(!r.state.play_wants_ticks());

    let mut g = coach_game();
    let fx = g.r.player_move("e2", "e4", Instant::now());
    let first = searches_for(&fx, g.coach_run).remove(0);
    begin_search(&mut g.r.state, g.coach_run, 50, first.tag);
    g.r.send(PlayMessage::Pause(Instant::now()));
    assert!(g.r.state.play_wants_ticks(), "the coach search is watched");
    // Silent for longer than its time: given up even in the pause.
    let fx =
        g.r.send(PlayMessage::Tick(Instant::now() + Duration::from_secs(40)));
    assert!(
        commands_for(&fx, g.coach_run).contains(&EngineCommand::Stop),
        "{fx:?}"
    );
    assert!(!g.r.state.play_wants_ticks(), "nothing left to watch");
}

// ---- the coach and the lock (D2432) ----

struct Game2 {
    r: Rig,
    coach_run: RunId,
}

fn coach_game() -> Game2 {
    let mut r = Rig::new();
    let (coach, coach_run) = add_engine(&mut r, "Coach1", vec![spin("Hash", 1, 1024)]);
    let mut cfg = r.config(ColorChoice::White, DEPTH);
    cfg.coach = Some(coach_config(coach));
    r.start(cfg);
    Game2 { r, coach_run }
}

fn lock(r: &mut Rig) -> Vec<Effect> {
    let fx = r.send(Message::Online(OnlineMessage::MarkerRead(Some(
        MarkerInfo {
            game_id: "abc12345".to_owned(),
            date: "2026-10-26".to_owned(),
        },
    ))));
    assert!(r.state.assist_locked());
    fx
}

#[test]
fn a_marker_in_the_middle_of_the_game_takes_the_coach_away_and_stops_its_searches() {
    let mut g = coach_game();
    let fx = g.r.player_move("e2", "e4", Instant::now());
    let crun = g.coach_run;
    let first = searches_for(&fx, crun).remove(0);
    let game_search = searches_for(&fx, g.r.run).remove(0);
    begin_search(&mut g.r.state, crun, 50, first.tag);
    assert!(g.r.view().coach.is_some());
    let fx = lock(&mut g.r);
    // Hidden at once; the lock also stops the engine executions (D1330).
    assert!(g.r.view().coach.is_none());
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::StopEngine { run, .. } if *run == crun)),
        "{fx:?}"
    );
    let fx = g.r.send(PlayMessage::Tick(Instant::now()));
    assert!(commands_for(&fx, crun).is_empty(), "{fx:?}");
    // The opponent goes on; the next move of the player makes no coach search.
    let orun = g.r.run;
    begin_search(&mut g.r.state, orun, 100, game_search.tag);
    event_on(
        &mut g.r.state,
        orun,
        best_event(100, Some(mv_after(&["e2e4"], "e7e5")), None),
    );
    assert_eq!(g.r.phase(), PlayPhase::Player);
    let fx = g.r.player_move("g1", "f3", Instant::now());
    assert!(commands_for(&fx, crun).is_empty(), "{fx:?}");
    assert_eq!(searches_for(&fx, orun).len(), 1, "the game itself goes on");
    // A late answer of the stopped search gives no remark.
    event_on(
        &mut g.r.state,
        crun,
        best_event(50, Some(mv_after(&[], "e2e4")), None),
    );
    assert!(g.r.view().coach.is_none());
}

#[test]
fn a_marker_before_the_move_means_no_coach_search_at_all() {
    let mut g = coach_game();
    drop(lock(&mut g.r));
    let fx = g.r.player_move("e2", "e4", Instant::now());
    assert!(commands_for(&fx, g.coach_run).is_empty(), "{fx:?}");
    assert_eq!(searches_for(&fx, g.r.run).len(), 1);
}

// ---- depth of the two searches (D2433) ----

fn judge_depths(d1: u32, d2: u32) -> (Game2, Option<CoachGrade>, bool) {
    let mut g = coach_game();
    let fx = g.r.player_move("a2", "a3", Instant::now());
    let crun = g.coach_run;
    let first = searches_for(&fx, crun).remove(0);
    let fx = coach_answer_at(
        &mut g.r,
        crun,
        &first,
        50,
        mv_after(&[], "e2e4"),
        Score::Cp(300),
        d1,
    );
    let second = searches_for(&fx, crun).remove(0);
    coach_answer_at(
        &mut g.r,
        crun,
        &second,
        51,
        mv_after(&[], "a2a3"),
        Score::Cp(-200),
        d2,
    );
    let view = g.r.view().coach.expect("coach");
    let grade = view.last.map(|v| v.grade);
    let busy = view.busy;
    (g, grade, busy)
}

#[test]
fn searches_that_stopped_at_different_depths_are_not_compared() {
    let (_, grade, busy) = judge_depths(12, 8);
    assert_eq!(grade, None, "horizon noise is not a blunder");
    assert!(!busy, "and the coach is free for the next move");
}

#[test]
fn depths_one_apart_are_compared() {
    let (_, grade, _) = judge_depths(9, 8);
    assert_eq!(grade, Some(CoachGrade::Blunder));
    let (_, grade, _) = judge_depths(8, 9);
    assert_eq!(grade, Some(CoachGrade::Blunder));
}

#[test]
fn an_unreported_depth_is_not_a_reason_to_skip() {
    let mut g = coach_game();
    let fx = g.r.player_move("a2", "a3", Instant::now());
    let crun = g.coach_run;
    let first = searches_for(&fx, crun).remove(0);
    let fx = coach_answer(
        &mut g.r,
        crun,
        &first,
        50,
        mv_after(&[], "e2e4"),
        Score::Cp(300),
    );
    let second = searches_for(&fx, crun).remove(0);
    coach_answer_at(
        &mut g.r,
        crun,
        &second,
        51,
        mv_after(&[], "a2a3"),
        Score::Cp(-200),
        3,
    );
    assert!(g.r.view().coach.unwrap().last.is_some());
}

// ---- tags and runs (D2434, D2436) ----

#[test]
fn the_tags_of_the_coach_never_repeat_across_games() {
    let mut g = coach_game();
    let fx = g.r.player_move("e2", "e4", Instant::now());
    let t1 = searches_for(&fx, g.coach_run).remove(0).tag;
    drop(g.r.send(PlayMessage::Restart(Instant::now())));
    let fx = g.r.player_move("e2", "e4", Instant::now());
    let t2 = searches_for(&fx, g.coach_run).remove(0).tag;
    assert!(is_coach_tag(t1) && is_coach_tag(t2));
    assert!(t2 > t1, "{t1:#x} then {t2:#x}");
}

#[test]
fn an_engine_started_only_for_the_coach_ends_with_the_game() {
    let mut r = Rig::new();
    let coach = add_engine_entry(&mut r, "Coach1");
    let mut cfg = r.config(ColorChoice::White, DEPTH);
    cfg.coach = Some(coach_config(coach));
    let fx = r.start(cfg);
    let crun = fx
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("started for the coach");
    finish_handshake(&mut r, crun, "Coach1", Vec::new());
    let fx = r.send(PlayMessage::Resign(Instant::now()));
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::StopEngine { run, .. } if *run == crun)),
        "{fx:?}"
    );
    // The opponent's run is not touched.
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::StopEngine { run, .. } if *run == r.run))
    );
}

#[test]
fn an_engine_that_was_already_running_is_kept_when_the_game_ends() {
    let mut g = coach_game();
    let fx = g.r.send(PlayMessage::Resign(Instant::now()));
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::StopEngine { .. })),
        "{fx:?}"
    );
    let _: Option<EngineId> = None;
}
