//! M8-1d contract tests of *ponder* in the glue (spec D2400-D2411): the
//! option, the search on the expected reply, the hit (`ponderhit`), the
//! miss (a new search, the old answer dropped) and every way the *ponder*
//! is let go. A fake engine answers through synthetic events fed to
//! `update`.
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
use mychess_app::{ColorChoice, PlayMessage, PlayNotice, PlayPhase, TimeMode};
use mychess_engine::{EngineCommand, EngineEvent};

const DEPTH: TimeMode = TimeMode::Depth(2);

/// A game as White where the engine answers 1.e4 with e5 and expects Nf3.
/// Returns the rig and the effects of the engine's `bestmove` (they carry
/// the search of *ponder*).
fn pondering_game() -> (Rig, Vec<mychess_app::Effect>, u64) {
    let mut r = ponder_rig();
    let cfg = ponder_config(&r, ColorChoice::White, DEPTH);
    r.start(cfg);
    let fx = r.player_move("e2", "e4", Instant::now());
    let request = play_search(&fx).expect("the engine is asked");
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
    (r, fx, 10)
}

#[test]
fn starting_with_ponder_turns_the_option_on_and_without_it_nothing_is_sent() {
    let mut r = ponder_rig();
    let cfg = ponder_config(&r, ColorChoice::White, DEPTH);
    let fx = r.start(cfg);
    assert_eq!(count(&fx, &EngineCommand::SetPonder(true)), 1);
    // The command goes before any search.
    let cmds = commands(&fx);
    let at = cmds
        .iter()
        .position(|c| *c == EngineCommand::SetPonder(true))
        .unwrap();
    assert!(
        cmds[..at].contains(&EngineCommand::NewGame),
        "after the new game: {cmds:?}"
    );

    let mut off = ponder_rig();
    let cfg = off.config(ColorChoice::White, DEPTH);
    let fx = off.start(cfg);
    assert_eq!(count(&fx, &EngineCommand::SetPonder(true)), 0);
    assert_eq!(off.notice(), None);
}

#[test]
fn an_engine_without_the_option_plays_on_without_ponder_and_says_so() {
    let mut r = Rig::new();
    let cfg = ponder_config(&r, ColorChoice::White, DEPTH);
    let fx = r.start(cfg);
    assert_eq!(count(&fx, &EngineCommand::SetPonder(true)), 0);
    assert_eq!(r.notice(), Some(PlayNotice::PonderUnsupported));
    let fx = r.player_move("e2", "e4", Instant::now());
    let request = play_search(&fx).unwrap();
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
    assert!(ponder_searches(&fx, true).is_empty(), "no ponder: {fx:?}");
}

#[test]
fn the_expected_reply_starts_a_search_of_ponder_on_the_position_after_it() {
    let (r, fx, _) = pondering_game();
    let searches = ponder_searches(&fx, true);
    assert_eq!(searches.len(), 1, "{fx:?}");
    let s = &searches[0];
    assert!(s.ponder);
    let line = [
        mv_after(&[], "e2e4"),
        mv_after(&["e2e4"], "e7e5"),
        mv_after(&["e2e4", "e7e5"], "g1f3"),
    ];
    assert_eq!(s.position.moves, line);
    assert_eq!(s.limits.depth, Some(2), "the limits of the mode");
    // The game itself is where it was: the player to move after 1.e4 e5.
    assert_eq!(r.mainline(), ["e2e4", "e7e5"]);
    assert_eq!(r.phase(), PlayPhase::Player);
    assert!(r.view().pondering);
}

#[test]
fn the_player_plays_the_expected_reply_and_the_search_goes_on_as_the_real_one() {
    let (mut r, fx, _) = pondering_game();
    let tag = ponder_searches(&fx, true)[0].tag;
    let run = r.run;
    begin_search(&mut r.state, run, 11, tag);
    // Info of the ponder search is not the analysis' business.
    event_on(
        &mut r.state,
        run,
        EngineEvent::Info(mychess_engine::SearchInfo {
            search: mychess_engine::SearchId(11),
            multipv: 1,
            score: Some((mychess_engine::Score::Cp(5), mychess_engine::Bound::Exact)),
            ..mychess_engine::SearchInfo::default()
        }),
    );
    let fx = r.player_move("g1", "f3", Instant::now());
    let cmds = commands(&fx);
    assert_eq!(count(&fx, &EngineCommand::PonderHit), 1, "{cmds:?}");
    assert!(
        !cmds.iter().any(|c| matches!(c, EngineCommand::Search(_))),
        "no new search on a hit: {cmds:?}"
    );
    assert_eq!(r.phase(), PlayPhase::Opponent);
    assert!(!r.view().pondering);
    // The answer of that very search is the engine's move.
    event_on(
        &mut r.state,
        run,
        best_event(11, Some(mv_after(&["e2e4", "e7e5", "g1f3"], "b8c6")), None),
    );
    assert_eq!(r.mainline(), ["e2e4", "e7e5", "g1f3", "b8c6"]);
    assert_eq!(r.phase(), PlayPhase::Player);
}

#[test]
fn another_move_makes_a_normal_search_and_the_old_answer_is_dropped() {
    let (mut r, fx, _) = pondering_game();
    let tag = ponder_searches(&fx, true)[0].tag;
    let run = r.run;
    begin_search(&mut r.state, run, 11, tag);
    let fx = r.player_move("b1", "c3", Instant::now());
    let cmds = commands(&fx);
    assert_eq!(count(&fx, &EngineCommand::PonderHit), 0, "{cmds:?}");
    let normal = ponder_searches(&fx, false);
    assert_eq!(normal.len(), 1, "{cmds:?}");
    assert_eq!(normal[0].position.moves.len(), 3);
    assert_eq!(
        normal[0].position.moves[2],
        mv_after(&["e2e4", "e7e5"], "b1c3")
    );
    // The stopped ponder search reports late: its move must not be played.
    event_on(
        &mut r.state,
        run,
        best_event(11, Some(mv_after(&["e2e4", "e7e5", "g1f3"], "b8c6")), None),
    );
    assert_eq!(r.mainline(), ["e2e4", "e7e5", "b1c3"]);
    assert_eq!(r.phase(), PlayPhase::Opponent);
    // And the real answer works.
    begin_search(&mut r.state, run, 12, normal[0].tag);
    event_on(
        &mut r.state,
        run,
        best_event(12, Some(mv_after(&["e2e4", "e7e5", "b1c3"], "g8f6")), None),
    );
    assert_eq!(r.mainline(), ["e2e4", "e7e5", "b1c3", "g8f6"]);
}

#[test]
fn a_reply_before_the_ponder_search_started_is_a_miss() {
    let (mut r, fx, _) = pondering_game();
    drop(fx); // the engine has not said anything about the ponder search yet
    let fx = r.player_move("g1", "f3", Instant::now());
    assert_eq!(count(&fx, &EngineCommand::PonderHit), 0);
    assert_eq!(ponder_searches(&fx, false).len(), 1, "{fx:?}");
}

#[test]
fn a_bestmove_without_a_ponder_move_pondering_nothing() {
    let mut r = ponder_rig();
    let cfg = ponder_config(&r, ColorChoice::White, DEPTH);
    r.start(cfg);
    let fx = r.player_move("e2", "e4", Instant::now());
    let request = play_search(&fx).unwrap();
    let run = r.run;
    begin_search(&mut r.state, run, 10, request.tag);
    let fx = event_on(
        &mut r.state,
        run,
        best_event(10, Some(mv_after(&["e2e4"], "e7e5")), None),
    );
    assert!(ponder_searches(&fx, true).is_empty());
    assert!(!r.view().pondering);
}

#[test]
fn an_illegal_ponder_move_is_not_pondered() {
    // The session validates it, but the app must not rely on that.
    let mut r = ponder_rig();
    let cfg = ponder_config(&r, ColorChoice::White, DEPTH);
    r.start(cfg);
    let fx = r.player_move("e2", "e4", Instant::now());
    let request = play_search(&fx).unwrap();
    let run = r.run;
    begin_search(&mut r.state, run, 10, request.tag);
    // `e7e5` for White is not legal after 1.e4 e5.
    let illegal = mv_after(&["e2e4"], "e7e5");
    let fx = event_on(
        &mut r.state,
        run,
        best_event(10, Some(mv_after(&["e2e4"], "e7e5")), Some(illegal)),
    );
    assert!(ponder_searches(&fx, true).is_empty());
}

#[test]
fn pausing_stops_the_ponder_search_and_a_late_answer_is_dropped() {
    let (mut r, fx, _) = pondering_game();
    let tag = ponder_searches(&fx, true)[0].tag;
    let run = r.run;
    begin_search(&mut r.state, run, 11, tag);
    let fx = r.send(PlayMessage::Pause(Instant::now()));
    assert_eq!(count(&fx, &EngineCommand::Stop), 1, "{fx:?}");
    assert!(!r.view().pondering);
    event_on(
        &mut r.state,
        run,
        best_event(11, Some(mv_after(&["e2e4", "e7e5", "g1f3"], "b8c6")), None),
    );
    assert_eq!(r.mainline(), ["e2e4", "e7e5"]);
    assert_eq!(r.phase(), PlayPhase::Paused { lost: false });
}

#[test]
fn resigning_quitting_and_restarting_let_the_ponder_go_and_turn_the_option_off() {
    for how in 0..3 {
        let (mut r, fx, _) = pondering_game();
        let tag = ponder_searches(&fx, true)[0].tag;
        let run = r.run;
        begin_search(&mut r.state, run, 11, tag);
        let at = Instant::now();
        let fx = match how {
            0 => r.send(PlayMessage::Resign(at)),
            1 => r.send(PlayMessage::Quit(at)),
            _ => r.send(PlayMessage::Restart(at)),
        };
        let cmds = commands(&fx);
        assert!(cmds.contains(&EngineCommand::Stop), "{how}: {cmds:?}");
        let stop = cmds.iter().position(|c| *c == EngineCommand::Stop).unwrap();
        let off = cmds
            .iter()
            .position(|c| *c == EngineCommand::SetPonder(false))
            .unwrap_or_else(|| panic!("{how}: the option goes back: {cmds:?}"));
        assert!(stop < off, "{how}: stop first: {cmds:?}");
        if how == 2 {
            assert_eq!(count(&fx, &EngineCommand::SetPonder(true)), 1);
        }
    }
}

#[test]
fn losing_the_engine_forgets_the_ponder_without_commands() {
    let (mut r, fx, _) = pondering_game();
    let tag = ponder_searches(&fx, true)[0].tag;
    let run = r.run;
    begin_search(&mut r.state, run, 11, tag);
    let fx = r.event(exited());
    assert!(!r.view().pondering);
    assert!(!commands(&fx).contains(&EngineCommand::PonderHit), "{fx:?}");
    assert_eq!(r.phase(), PlayPhase::Paused { lost: true });
}

#[test]
fn a_ponder_that_outlives_five_minutes_is_stopped_by_the_tick() {
    let (mut r, fx, _) = pondering_game();
    let tag = ponder_searches(&fx, true)[0].tag;
    let run = r.run;
    begin_search(&mut r.state, run, 11, tag);
    // Time is fixed by the test: the game started "now".
    let now = Instant::now();
    let fx = r.send(PlayMessage::Tick(now + Duration::from_secs(60)));
    assert_eq!(
        count(&fx, &EngineCommand::Stop),
        0,
        "still within the limit"
    );
    assert!(r.view().pondering);
    let fx = r.send(PlayMessage::Tick(now + Duration::from_secs(301)));
    assert_eq!(count(&fx, &EngineCommand::Stop), 1, "{fx:?}");
    assert!(!r.view().pondering);
    // The player moves anything: a normal search.
    let fx = r.player_move("g1", "f3", Instant::now());
    assert_eq!(count(&fx, &EngineCommand::PonderHit), 0);
    assert_eq!(ponder_searches(&fx, false).len(), 1);
}

#[test]
fn a_ponder_search_that_ends_on_its_own_is_forgotten() {
    let (mut r, fx, _) = pondering_game();
    let tag = ponder_searches(&fx, true)[0].tag;
    let run = r.run;
    begin_search(&mut r.state, run, 11, tag);
    event_on(&mut r.state, run, best_event(11, None, None));
    assert_eq!(r.mainline(), ["e2e4", "e7e5"], "never a move of the game");
    assert!(!r.view().pondering);
    let fx = r.player_move("g1", "f3", Instant::now());
    assert_eq!(count(&fx, &EngineCommand::PonderHit), 0);
    assert_eq!(ponder_searches(&fx, false).len(), 1);
}

#[test]
fn the_time_of_the_engine_after_a_hit_is_measured_from_the_move_of_the_player() {
    // Timebonus: the engine's clock runs from the player's move, so a
    // ponderhit answer costs little.
    let mut r = ponder_rig();
    let cfg = ponder_config(&r, ColorChoice::White, blitz());
    r.start(cfg);
    let t0 = Instant::now();
    let fx = r.player_move("e2", "e4", t0);
    let request = play_search(&fx).unwrap();
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
    // The ponder request carries the clock like the real one would.
    assert!(ponder_searches(&fx, true)[0].limits.clock.is_some());
    begin_search(&mut r.state, run, 11, tag);
    let fx = r.player_move("g1", "f3", t0 + 20 * SEC);
    assert_eq!(count(&fx, &EngineCommand::PonderHit), 1);
    assert_eq!(r.phase(), PlayPhase::Opponent);
}
