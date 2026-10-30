//! M8-1d contract tests of the coach in the glue (spec D2416-D2430): a
//! second engine, separate from the opponent, judges every move of the
//! player with two searches of the position before it; the remark goes to
//! the view; the opponent never feeds it; taking back is off; a lost or
//! silent coach never stops the game.
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
use mychess_app::{CoachGrade, ColorChoice, Effect, PlayMessage, PlayNotice, PlayPhase, TimeMode};
use mychess_engine::{EngineCommand, OptionValue, Score};

const DEPTH: TimeMode = TimeMode::Depth(2);

struct Game {
    r: Rig,
    coach: EngineId,
    coach_run: RunId,
}

fn coach_options() -> Vec<mychess_engine::EngineOption> {
    vec![spin("Threads", 1, 64), spin("Hash", 1, 1024)]
}

/// A game as `color` with a ready coach engine; `edit` changes the
/// configuration of the coach. The effects of the start are returned too.
fn game_with(
    color: ColorChoice,
    edit: impl FnOnce(&mut mychess_app::CoachConfig),
) -> (Game, Vec<Effect>) {
    let mut r = Rig::new();
    let (coach, coach_run) = add_engine(&mut r, "Coach1", coach_options());
    let mut cfg = r.config(color, DEPTH);
    let mut coach_cfg = coach_config(coach);
    edit(&mut coach_cfg);
    cfg.coach = Some(coach_cfg);
    let fx = r.start(cfg);
    (
        Game {
            r,
            coach,
            coach_run,
        },
        fx,
    )
}

fn game(color: ColorChoice) -> Game {
    game_with(color, |_| {}).0
}

impl Game {
    /// The player (White) plays e2e4; the opponent answers e7e5.
    fn first_round(&mut self) -> Vec<Effect> {
        let fx = self.r.player_move("e2", "e4", Instant::now());
        let request = play_search(&searches_effects(&fx, self.r.run)).expect("asked");
        let run = self.r.run;
        begin_search(&mut self.r.state, run, 100, request.tag);
        event_on(
            &mut self.r.state,
            run,
            best_event(100, Some(mv_after(&["e2e4"], "e7e5")), None),
        );
        fx
    }

    fn remark(&self) -> mychess_app::CoachView {
        self.r.view().coach.expect("the game has a coach")
    }
}

/// The effects reduced to the commands addressed to `run`, so that
/// `play_search` does not take a search of the coach for the game's.
fn searches_effects(fx: &[Effect], run: RunId) -> Vec<Effect> {
    fx.iter()
        .filter(|e| matches!(e, Effect::EngineCommand { run: r, .. } if *r == run))
        .cloned()
        .collect()
}

#[test]
fn the_coach_engine_is_set_up_for_the_game_and_the_game_is_marked() {
    let (g, fx) = game_with(ColorChoice::White, |_| {});
    let cmds = commands_for(&fx, g.coach_run);
    assert!(cmds.contains(&EngineCommand::NewGame), "{cmds:?}");
    let options: Vec<_> = cmds
        .iter()
        .filter_map(|c| match c {
            EngineCommand::SetOption { name, value } => Some((name.as_str(), value.clone())),
            _ => None,
        })
        .collect();
    assert!(
        options.contains(&("Threads", OptionValue::Int(1))),
        "{options:?}"
    );
    assert!(
        options.contains(&("Hash", OptionValue::Int(64))),
        "{options:?}"
    );
    // Nothing of the coach's setup goes to the opponent.
    assert!(
        !commands_for(&fx, g.r.run)
            .iter()
            .any(|c| matches!(c, EngineCommand::SetOption { name, .. } if name == "Threads"))
    );
    assert_eq!(
        g.r.state.session.game().tags().get_extra("Coach"),
        Some("Coach1")
    );
    let view = g.remark();
    assert_eq!(view.engine, "Coach1");
    assert!(!view.busy && !view.lost && view.last.is_none());
    assert!(view.numbers);
}

#[test]
fn the_opponents_own_entry_cannot_be_the_coach() {
    let mut r = Rig::new();
    let mut cfg = r.config(ColorChoice::White, DEPTH);
    cfg.coach = Some(coach_config(r.engine));
    let fx = r.start(cfg);
    assert_eq!(r.notice(), Some(PlayNotice::CoachUnavailable));
    assert!(r.view().coach.is_none());
    assert!(r.state.play_live(), "the game goes on without a coach");
    assert!(r.state.session.game().tags().get_extra("Coach").is_none());
    drop(fx);
}

#[test]
fn a_missing_coach_engine_is_refused_and_the_game_goes_on() {
    let mut r = Rig::new();
    let mut cfg = r.config(ColorChoice::White, DEPTH);
    cfg.coach = Some(coach_config(EngineId(999)));
    r.start(cfg);
    assert_eq!(r.notice(), Some(PlayNotice::CoachUnavailable));
    assert!(r.view().coach.is_none());
}

#[test]
fn the_best_move_is_judged_with_one_search_of_the_position_before_it() {
    let mut g = game(ColorChoice::White);
    let fx = g.r.player_move("e2", "e4", Instant::now());
    let coach_searches = searches_for(&fx, g.coach_run);
    assert_eq!(coach_searches.len(), 1, "{fx:?}");
    let s = &coach_searches[0];
    assert!(is_coach_tag(s.tag));
    assert!(s.position.moves.is_empty(), "the position before 1.e4");
    assert_eq!(s.limits.depth, Some(8));
    assert_eq!(s.limits.movetime, Some(Duration::from_secs(2)));
    assert!(s.search_moves.is_empty());
    assert!(g.remark().busy);
    // The opponent was asked too, on its own engine.
    assert_eq!(searches_for(&fx, g.r.run).len(), 1);

    let e4 = mv_after(&[], "e2e4");
    let (run, s) = (g.coach_run, s.clone());
    let fx = coach_answer(&mut g.r, run, &s, 50, e4, Score::Cp(30));
    assert!(searches_for(&fx, run).is_empty(), "no second search");
    let view = g.remark();
    assert!(!view.busy);
    let v = view.last.expect("a remark");
    assert_eq!(v.grade, CoachGrade::Best);
    assert_eq!((v.loss_cp, v.ply, v.played_san.as_str()), (0, 1, "e4"));
}

#[test]
fn another_move_needs_a_second_search_restricted_to_it() {
    let (mut g, _) = game_with(ColorChoice::White, |c| c.hint = true);
    let fx = g.r.player_move("a2", "a3", Instant::now());
    let run = g.coach_run;
    let first = searches_for(&fx, run).remove(0);
    let e4 = mv_after(&[], "e2e4");
    let fx = coach_answer(&mut g.r, run, &first, 50, e4, Score::Cp(40));
    let second = searches_for(&fx, run);
    assert_eq!(second.len(), 1, "{fx:?}");
    assert_eq!(second[0].search_moves, [mv_after(&[], "a2a3")]);
    assert_eq!(second[0].position.moves.len(), 0, "same position");
    assert_eq!(second[0].limits.depth, Some(8));
    assert!(g.remark().busy);
    coach_answer(
        &mut g.r,
        run,
        &second[0],
        51,
        mv_after(&[], "a2a3"),
        Score::Cp(-45),
    );
    let v = g.remark().last.expect("a remark");
    assert_eq!(v.grade, CoachGrade::Inaccuracy);
    assert_eq!(v.loss_cp, 85);
    assert_eq!(v.best_san.as_deref(), Some("e4"), "the hint was asked for");
    assert!(!v.decisive);
}

#[test]
fn without_the_hint_the_better_move_is_not_in_the_view() {
    let mut g = game(ColorChoice::White);
    let fx = g.r.player_move("a2", "a3", Instant::now());
    let run = g.coach_run;
    let first = searches_for(&fx, run).remove(0);
    let fx = coach_answer(
        &mut g.r,
        run,
        &first,
        50,
        mv_after(&[], "e2e4"),
        Score::Cp(300),
    );
    let second = searches_for(&fx, run).remove(0);
    coach_answer(
        &mut g.r,
        run,
        &second,
        51,
        mv_after(&[], "a2a3"),
        Score::Cp(-100),
    );
    let v = g.remark().last.expect("a remark");
    assert_eq!(v.grade, CoachGrade::Blunder);
    assert_eq!(v.best_san, None);
}

#[test]
fn a_black_player_is_judged_from_black_s_point_of_view() {
    let (mut g, start) = game_with(ColorChoice::Black, |_| {});
    // The engine (White) opens with e4.
    let request = play_search(&searches_effects(&start, g.r.run)).expect("asked");
    let run = g.r.run;
    begin_search(&mut g.r.state, run, 100, request.tag);
    event_on(
        &mut g.r.state,
        run,
        best_event(100, Some(mv_after(&[], "e2e4")), None),
    );
    let fx = g.r.player_move("e7", "e5", Instant::now());
    let crun = g.coach_run;
    let first = searches_for(&fx, crun).remove(0);
    assert_eq!(first.position.moves, [mv_after(&[], "e2e4")]);
    // White's point of view: c5 is -20 (good for Black), e5 is +30.
    let fx = coach_answer(
        &mut g.r,
        crun,
        &first,
        50,
        mv_after(&["e2e4"], "c7c5"),
        Score::Cp(-20),
    );
    let second = searches_for(&fx, crun).remove(0);
    assert_eq!(second.search_moves, [mv_after(&["e2e4"], "e7e5")]);
    coach_answer(
        &mut g.r,
        crun,
        &second,
        51,
        mv_after(&["e2e4"], "e7e5"),
        Score::Cp(30),
    );
    let v = g.remark().last.expect("a remark");
    assert_eq!(v.loss_cp, 50);
    assert_eq!(v.grade, CoachGrade::Inaccuracy);
    assert_eq!(v.ply, 2);
}

#[test]
fn the_opponent_never_feeds_the_coach_and_the_coach_never_moves_the_game() {
    let mut g = game(ColorChoice::White);
    let fx = g.r.player_move("e2", "e4", Instant::now());
    let crun = g.coach_run;
    let coach_search = searches_for(&fx, crun).remove(0);
    let game_search = searches_for(&fx, g.r.run).remove(0);
    let orun = g.r.run;
    // The opponent evaluates the position as a great advantage for White.
    begin_search(&mut g.r.state, orun, 100, game_search.tag);
    event_on(
        &mut g.r.state,
        orun,
        mychess_engine::EngineEvent::Info(mychess_engine::SearchInfo {
            search: mychess_engine::SearchId(100),
            multipv: 1,
            score: Some((Score::Cp(900), mychess_engine::Bound::Exact)),
            ..mychess_engine::SearchInfo::default()
        }),
    );
    // The coach answers with a move: it is a remark, not a move of the game.
    coach_answer(
        &mut g.r,
        crun,
        &coach_search,
        50,
        mv_after(&[], "e2e4"),
        Score::Cp(30),
    );
    assert_eq!(g.r.mainline(), ["e2e4"], "the coach did not move");
    assert_eq!(g.r.phase(), PlayPhase::Opponent);
    assert_eq!(g.remark().last.expect("remark").loss_cp, 0);
    // The opponent answers: the game goes on.
    event_on(
        &mut g.r.state,
        orun,
        best_event(100, Some(mv_after(&["e2e4"], "e7e5")), None),
    );
    assert_eq!(g.r.mainline(), ["e2e4", "e7e5"]);
    // An answer of the opponent's search id on the coach's run is not the
    // coach's search.
    let before = g.remark().last;
    event_on(
        &mut g.r.state,
        crun,
        best_event(100, Some(mv_after(&["e2e4", "e7e5"], "g1f3")), None),
    );
    assert_eq!(g.remark().last, before);
}

#[test]
fn a_late_answer_of_a_replaced_coach_search_is_dropped() {
    let mut g = game(ColorChoice::White);
    let fx = g.r.player_move("a2", "a3", Instant::now());
    let crun = g.coach_run;
    let first = searches_for(&fx, crun).remove(0);
    begin_search(&mut g.r.state, crun, 50, first.tag);
    // Silence: the watchdog gives the search up (2 s + 30 s of grace).
    let fx =
        g.r.send(PlayMessage::Tick(Instant::now() + Duration::from_secs(40)));
    assert_eq!(commands_for(&fx, crun), [EngineCommand::Stop], "{fx:?}");
    assert!(!g.remark().busy);
    // It reports after all.
    event_on(
        &mut g.r.state,
        crun,
        best_event(50, Some(mv_after(&[], "e2e4")), None),
    );
    assert!(
        g.remark().last.is_none(),
        "no verdict from a dropped search"
    );
}

#[test]
fn a_silent_coach_search_is_given_up_and_the_next_move_is_judged() {
    let mut g = game(ColorChoice::White);
    let fx = g.r.player_move("e2", "e4", Instant::now());
    let crun = g.coach_run;
    assert_eq!(searches_for(&fx, crun).len(), 1);
    // The tick without any event of the coach.
    g.r.send(PlayMessage::Tick(Instant::now() + Duration::from_secs(40)));
    assert!(g.remark().last.is_none());
    // The opponent's reply does not matter: it is now the next move.
    let game_search = searches_for(&fx, g.r.run).remove(0);
    let orun = g.r.run;
    begin_search(&mut g.r.state, orun, 100, game_search.tag);
    event_on(
        &mut g.r.state,
        orun,
        best_event(100, Some(mv_after(&["e2e4"], "e7e5")), None),
    );
    // The opponent's watchdog (depth mode) was not tripped by the tick.
    assert_eq!(g.r.phase(), PlayPhase::Player);
    let fx = g.r.player_move("g1", "f3", Instant::now());
    assert_eq!(searches_for(&fx, crun).len(), 1, "judged again");
}

#[test]
fn with_a_coach_taking_back_is_off() {
    let mut g = game(ColorChoice::White);
    g.first_round();
    let view = g.r.view();
    assert!(!view.can_take_back);
    assert!(view.can_offer_draw, "offering a draw stays");
    let fx = g.r.send(PlayMessage::TakeBack(Instant::now()));
    assert_eq!(g.r.notice(), Some(PlayNotice::CoachNoTakeBack));
    assert_eq!(g.r.mainline(), ["e2e4", "e7e5"]);
    assert!(commands(&fx).is_empty());
}

#[test]
fn a_game_without_a_coach_can_still_take_back() {
    let mut r = Rig::new();
    let cfg = r.config(ColorChoice::White, DEPTH);
    r.start(cfg);
    let fx = r.player_move("e2", "e4", Instant::now());
    r.reply(&fx, Some("e7e5"));
    assert!(r.view().can_take_back);
    assert!(r.view().coach.is_none());
}

#[test]
fn a_lost_coach_says_so_and_the_game_goes_on() {
    let mut g = game(ColorChoice::White);
    let fx = g.r.player_move("e2", "e4", Instant::now());
    let game_search = searches_for(&fx, g.r.run).remove(0);
    let crun = g.coach_run;
    event_on(&mut g.r.state, crun, exited());
    assert_eq!(g.r.notice(), Some(PlayNotice::CoachLost));
    let view = g.remark();
    assert!(view.lost && !view.busy);
    assert_eq!(g.r.phase(), PlayPhase::Opponent, "the game is not paused");
    let orun = g.r.run;
    begin_search(&mut g.r.state, orun, 100, game_search.tag);
    event_on(
        &mut g.r.state,
        orun,
        best_event(100, Some(mv_after(&["e2e4"], "e7e5")), None),
    );
    assert_eq!(g.r.phase(), PlayPhase::Player);
    let fx = g.r.player_move("g1", "f3", Instant::now());
    assert!(
        commands_for(&fx, crun).is_empty(),
        "nothing goes to a dead coach"
    );
    assert!(g.remark().lost);
}

#[test]
fn a_coach_that_is_still_starting_judges_the_moves_when_it_is_ready() {
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
        .expect("the coach engine is started");
    let fx = r.player_move("e2", "e4", Instant::now());
    assert!(
        commands_for(&fx, crun).is_empty(),
        "nothing before the handshake"
    );
    let fx = finish_handshake(&mut r, crun, "Coach1", coach_options());
    let cmds = commands_for(&fx, crun);
    assert!(cmds.contains(&EngineCommand::NewGame), "{cmds:?}");
    let search = searches_for(&fx, crun);
    assert_eq!(search.len(), 1, "the waiting move is judged: {cmds:?}");
    assert!(search[0].position.moves.is_empty());
}

#[test]
fn a_coach_that_never_starts_does_not_hold_the_game() {
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
        .unwrap();
    r.send(mychess_app::engines::EngineMessage::StartFailed {
        run: crun,
        failure: mychess_app::engines::EngineFailure::from_error(
            &mychess_engine::EngineError::Closed,
        ),
    });
    // The notice of the failed start (the engine's own) wins the status bar.
    assert!(r.view().coach.is_some_and(|c| c.lost));
    let fx = r.player_move("e2", "e4", Instant::now());
    assert!(play_search(&fx).is_some(), "the opponent is asked");
}

#[test]
fn the_end_of_the_game_stops_the_coach_and_puts_its_engine_back() {
    let mut g = game(ColorChoice::White);
    let fx = g.r.player_move("a2", "a3", Instant::now());
    let crun = g.coach_run;
    let first = searches_for(&fx, crun).remove(0);
    begin_search(&mut g.r.state, crun, 50, first.tag);
    let fx = g.r.send(PlayMessage::Resign(Instant::now()));
    let cmds = commands_for(&fx, crun);
    assert!(cmds.contains(&EngineCommand::Stop), "{cmds:?}");
    let back: Vec<_> = cmds
        .iter()
        .filter_map(|c| match c {
            EngineCommand::SetOption { name, value } => Some((name.as_str(), value.clone())),
            _ => None,
        })
        .collect();
    assert!(back.contains(&("Threads", OptionValue::Int(1))), "{back:?}");
    assert!(
        back.contains(&("Hash", OptionValue::Int(1))),
        "default Hash back: {back:?}"
    );
    // A late answer does nothing, and the view keeps the (empty) coach.
    event_on(
        &mut g.r.state,
        crun,
        best_event(50, Some(mv_after(&[], "e2e4")), None),
    );
    assert!(g.remark().last.is_none());
    assert!(!g.remark().busy);
}

#[test]
fn restarting_creates_the_coach_again() {
    let mut g = game(ColorChoice::White);
    let fx = g.r.send(PlayMessage::Restart(Instant::now()));
    let cmds = commands_for(&fx, g.coach_run);
    assert!(cmds.contains(&EngineCommand::NewGame), "{cmds:?}");
    assert!(g.r.view().coach.is_some());
    let _ = g.coach;
}

#[test]
fn quitting_leaves_no_coach_behind() {
    let mut g = game(ColorChoice::White);
    g.r.send(PlayMessage::Quit(Instant::now()));
    assert!(!g.r.state.play_live());
    assert!(g.r.state.play_view(Instant::now()).is_none());
}
