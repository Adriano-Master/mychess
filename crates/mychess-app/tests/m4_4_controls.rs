//! M4-4 contract tests of the section controls: lock (D444, D799), exclude
//! (D445, D800), MultiPV capability, low priority restart (D446, D801),
//! display/information, XBoard, failed runs and the protocol log (D447,
//! D802).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string
)]

mod m4_4_common;

use std::time::Duration;

use m4_4_common::*;
use mychess_app::engines::{EngineMessage, RunId};
use mychess_app::{
    AllLinesMode, AnalysisMessage, AnalysisNotice, AppNotice, AppState, Display, Effect,
    GameMessage, Message, Nav, update,
};
use mychess_core::{Game, Position};
use mychess_engine::{
    EngineCommand, EngineEvent, ExitInfo, ExitReason, LogEntry, LogKind, Protocol, SearchRequest,
};

fn search_ucis(request: &SearchRequest) -> Vec<String> {
    let mut at = request.position.start.clone();
    let mut out = Vec::new();
    for mv in &request.position.moves {
        out.push(at.uci(*mv));
        at.play(*mv).unwrap();
    }
    out
}

fn restricted(request: &SearchRequest) -> Vec<String> {
    let at = request.position.start.clone();
    let mut pos = at;
    for mv in &request.position.moves {
        pos.play(*mv).unwrap();
    }
    let mut v: Vec<String> = request.search_moves.iter().map(|m| pos.uci(*m)).collect();
    v.sort();
    v
}

fn run_state(state: &AppState, run: RunId) -> &mychess_app::engines::EngineRun {
    state.engine_runs().iter().find(|r| r.id == run).unwrap()
}

// ---- lock (D444, D799, rule 9, 12) ----

#[test]
fn a_locked_run_does_not_receive_the_new_positions_and_keeps_its_lines() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    play(&mut state, "e2e4");
    started(&mut state, run, 2);
    let pos = state.session.position().clone();
    info(&mut state, run, report(&pos, 2, 1, 10, -20, &["e7e5"]));
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    assert!(run_state(&state, run).locked);
    assert!(view(&state, run).lock.is_some());
    let mv = pos.parse_uci("e7e5").unwrap();
    let fx = game(
        &mut state,
        GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        },
    );
    assert!(searches(&fx).is_empty(), "locked: nothing is sent");
    assert_eq!(lines_of(&state, run), [(1, "1... e5".into())]);
    let fx = nav(&mut state, Nav::Start);
    assert!(searches(&fx).is_empty());
}

#[test]
fn the_lock_records_the_move_number_side_last_move_and_variation_depth() {
    let mut g = Game::new();
    let root = g.root();
    let ids = {
        let mut node = root;
        let mut out = Vec::new();
        for u in ["e2e4", "e7e5", "g1f3"] {
            let mv = g.position_at(node).unwrap().parse_uci(u).unwrap();
            node = g.add_move(node, mv).unwrap();
            out.push(node);
        }
        out
    };
    let mv = g.position_at(ids[0]).unwrap().parse_uci("c7c5").unwrap();
    let c5 = g.add_move(ids[0], mv).unwrap();
    let (mut state, run, _) = analysing(true, true);
    state.session.replace_game(g);
    nav(&mut state, Nav::GoToNode(ids[2]));
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    let lock = view(&state, run).lock.clone().unwrap();
    assert_eq!(lock.move_number, 2);
    assert_eq!(lock.side, mychess_core::Color::Black);
    assert_eq!(lock.last_move.as_deref(), Some("Nf3"));
    assert_eq!(lock.variation_depth, 0);
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    nav(&mut state, Nav::GoToNode(c5));
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    let lock = view(&state, run).lock.clone().unwrap();
    assert_eq!(lock.variation_depth, 1);
    assert_eq!(lock.last_move.as_deref(), Some("c5"));
    assert_eq!(lock.move_number, 2, "after 1...c5 White plays move 2");
    assert_eq!(lock.side, mychess_core::Color::White);
}

#[test]
fn unlocking_forgets_the_lines_and_sends_the_current_position() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    let start = Position::new();
    info(&mut state, run, report(&start, 1, 1, 10, 20, &["e2e4"]));
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    play(&mut state, "d2d4");
    let fx = msg(&mut state, AnalysisMessage::ToggleLock(run));
    assert!(!run_state(&state, run).locked);
    assert!(view(&state, run).lock.is_none());
    assert!(
        view(&state, run).lines.is_empty(),
        "lines of the locked position go"
    );
    let sent = searches(&fx);
    assert_eq!(sent.len(), 1, "the current position is sent: {fx:?}");
    assert_eq!(search_ucis(&sent[0]), ["d2d4"]);
}

#[test]
fn turning_a_locked_analysis_off_and_on_searches_the_locked_position() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    play(&mut state, "e2e4");
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    play(&mut state, "e7e5");
    msg(&mut state, AnalysisMessage::Toggle(run));
    let fx = msg(&mut state, AnalysisMessage::Toggle(run));
    let sent = searches(&fx);
    assert_eq!(sent.len(), 1);
    assert_eq!(search_ucis(&sent[0]), ["e2e4"], "not the board (1.e4 e5)");
}

#[test]
fn a_stopped_analysis_receives_no_search_when_the_game_changes_and_turning_on_sends_the_current() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    let fx = msg(&mut state, AnalysisMessage::Toggle(run));
    assert_eq!(count_stops(&fx), 1);
    assert!(!run_state(&state, run).analyzing);
    let mv = state.session.position().parse_uci("e2e4").unwrap();
    let fx = game(
        &mut state,
        GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        },
    );
    assert!(searches(&fx).is_empty());
    let fx = msg(&mut state, AnalysisMessage::Toggle(run));
    let sent = searches(&fx);
    assert_eq!(sent.len(), 1);
    assert_eq!(search_ucis(&sent[0]), ["e2e4"]);
}

#[test]
fn while_locked_multipv_and_exclusion_are_refused_with_a_notice() {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    for m in [
        AnalysisMessage::SetMultiPv(run, 3),
        AnalysisMessage::SetExclusion(run, "e4".into()),
        AnalysisMessage::OpenExclude(run),
    ] {
        state.notice = None;
        let fx = msg(&mut state, m.clone());
        assert!(commands(&fx).is_empty(), "{m:?}");
        assert_eq!(
            analysis_notice(&state),
            Some(&AnalysisNotice::Locked),
            "{m:?}"
        );
    }
    assert_eq!(view(&state, run).multipv, 1);
    assert!(view(&state, run).exclusion.is_empty());
}

#[test]
fn locking_needs_a_ready_engine() {
    let (mut state, id) = state_with_engine();
    let fx = update(
        &mut state,
        Message::Engine(EngineMessage::Start(
            id,
            mychess_app::engines::StartMode::Analyze,
        )),
    );
    let run = fx
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .unwrap();
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    assert!(!run_state(&state, run).locked, "still starting");
}

// ---- exclude (D445, D800, rule 10) ----

#[test]
fn excluding_moves_resends_the_search_with_the_remaining_legal_moves() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    let fx = msg(
        &mut state,
        AnalysisMessage::SetExclusion(run, "e4, d4".into()),
    );
    let sent = searches(&fx);
    assert_eq!(sent.len(), 1, "the search restarts: {fx:?}");
    let allowed = restricted(&sent[0]);
    assert_eq!(allowed.len(), 18);
    assert!(!allowed.contains(&"e2e4".to_owned()) && !allowed.contains(&"d2d4".to_owned()));
    assert_eq!(view(&state, run).exclusion.len(), 2);
    assert_eq!(view(&state, run).exclusion_san, ["e4", "d4"]);
}

#[test]
fn a_new_position_clears_the_exclusion_and_searches_everything() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    msg(&mut state, AnalysisMessage::SetExclusion(run, "e4".into()));
    let mv = state.session.position().parse_uci("d2d4").unwrap();
    let fx = game(
        &mut state,
        GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        },
    );
    let sent = searches(&fx);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].search_moves.is_empty());
    assert!(view(&state, run).exclusion.is_empty());
}

#[test]
fn an_empty_text_clears_the_exclusion_and_resends() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    msg(&mut state, AnalysisMessage::SetExclusion(run, "e4".into()));
    let fx = msg(&mut state, AnalysisMessage::SetExclusion(run, "  ".into()));
    assert!(view(&state, run).exclusion.is_empty());
    let sent = searches(&fx);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].search_moves.is_empty());
}

#[test]
fn invalid_and_total_exclusions_say_why_and_change_nothing() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    msg(&mut state, AnalysisMessage::SetExclusion(run, "d4".into()));
    let fx = msg(
        &mut state,
        AnalysisMessage::SetExclusion(run, "e4 Ke2".into()),
    );
    assert_eq!(
        analysis_notice(&state),
        Some(&AnalysisNotice::ExcludeInvalid("Ke2".into()))
    );
    assert!(searches(&fx).is_empty());
    assert_eq!(
        view(&state, run).exclusion_san,
        ["d4"],
        "the old exclusion stays"
    );
    let pos = Position::new();
    let all: Vec<String> = pos
        .legal_moves()
        .into_iter()
        .map(|m| pos.san(m).unwrap())
        .collect();
    msg(
        &mut state,
        AnalysisMessage::SetExclusion(run, all.join(" ")),
    );
    assert_eq!(analysis_notice(&state), Some(&AnalysisNotice::ExcludeAll));
    assert_eq!(view(&state, run).exclusion_san, ["d4"]);
}

#[test]
fn excluding_needs_search_moves_support_and_the_analysis_on() {
    let (mut state, run, _) = analysing(true, false);
    msg(&mut state, AnalysisMessage::SetExclusion(run, "e4".into()));
    assert_eq!(
        analysis_notice(&state),
        Some(&AnalysisNotice::ExcludeUnsupported)
    );
    assert!(view(&state, run).exclusion.is_empty());

    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::Toggle(run));
    state.notice = None;
    msg(&mut state, AnalysisMessage::SetExclusion(run, "e4".into()));
    assert!(view(&state, run).exclusion.is_empty(), "analysis is off");
}

#[test]
fn stopping_the_analysis_clears_the_exclusion() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    msg(&mut state, AnalysisMessage::SetExclusion(run, "e4".into()));
    msg(&mut state, AnalysisMessage::Toggle(run));
    assert!(view(&state, run).exclusion.is_empty());
    let fx = msg(&mut state, AnalysisMessage::Toggle(run));
    assert!(searches(&fx)[0].search_moves.is_empty());
}

// ---- low priority (D446, D801) ----

fn stopped_then_started(state: &mut AppState, run: RunId) -> RunId {
    let fx = update(state, Message::Engine(EngineMessage::Stopped(run)));
    fx.iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("the engine starts again")
}

#[test]
fn toggling_low_priority_restarts_with_a_new_run_that_inherits_the_settings() {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    started(&mut state, run, 1);
    msg(&mut state, AnalysisMessage::SetDisplay(run, Display::Wrap));
    msg(&mut state, AnalysisMessage::ToggleInfo(run));
    let shown = view(&state, run).show_info;
    msg(
        &mut state,
        AnalysisMessage::SetLogFilter(run, "needle".into()),
    );
    msg(&mut state, AnalysisMessage::SetExclusion(run, "e4".into()));
    let start = Position::new();
    info(&mut state, run, report(&start, 1, 1, 10, 20, &["d2d4"]));
    let was_low = run_state(&state, run).low_priority;
    let fx = msg(&mut state, AnalysisMessage::ToggleLowPriority(run));
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::StopEngine { run: r, .. } if *r == run))
    );
    assert!(view(&state, run).restarting);
    let new = stopped_then_started(&mut state, run);
    assert_ne!(new, run);
    assert_eq!(run_state(&state, new).low_priority, !was_low);
    assert!(run_state(&state, new).analyzing);
    let v = view(&state, new);
    assert_eq!(v.display, Display::Wrap);
    assert_eq!(v.show_info, shown);
    assert_eq!(v.log_filter, "needle");
    assert_eq!(v.multipv, 3);
    assert_eq!(v.exclusion_san, ["e4"]);
    assert!(
        v.lines.is_empty() && v.history.is_empty(),
        "lines do not carry over"
    );
    assert!(v.log.is_empty() && v.lock.is_none());
    assert!(
        state.analysis_view(run).is_none(),
        "the old section is gone"
    );
}

#[test]
fn the_new_run_searches_the_same_position_with_the_same_exclusion() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    play(&mut state, "e2e4");
    msg(&mut state, AnalysisMessage::SetExclusion(run, "e5".into()));
    msg(&mut state, AnalysisMessage::ToggleLowPriority(run));
    let new = stopped_then_started(&mut state, run);
    let fx = update(
        &mut state,
        Message::Engine(EngineMessage::Started {
            run: new,
            identity: Box::new(identity(true, true)),
            at: 1,
        }),
    );
    let sent = searches(&fx);
    assert_eq!(sent.len(), 1, "{fx:?}");
    assert_eq!(search_ucis(&sent[0]), ["e2e4"]);
    assert_eq!(restricted(&sent[0]).len(), 19);
}

#[test]
fn a_lock_does_not_survive_the_restart() {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::ToggleLock(run));
    msg(&mut state, AnalysisMessage::ToggleLowPriority(run));
    let new = stopped_then_started(&mut state, run);
    assert!(view(&state, new).lock.is_none());
    assert!(!run_state(&state, new).locked);
}

#[test]
fn toggling_twice_goes_back_to_normal_priority() {
    let (mut state, run, _) = analysing(true, true);
    let was = run_state(&state, run).low_priority;
    msg(&mut state, AnalysisMessage::ToggleLowPriority(run));
    let second = stopped_then_started(&mut state, run);
    update(
        &mut state,
        Message::Engine(EngineMessage::Started {
            run: second,
            identity: Box::new(identity(true, true)),
            at: 1,
        }),
    );
    msg(&mut state, AnalysisMessage::ToggleLowPriority(second));
    let third = stopped_then_started(&mut state, second);
    assert_eq!(run_state(&state, third).low_priority, was);
}

#[test]
fn closing_a_section_while_it_restarts_cancels_the_restart() {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::ToggleLowPriority(run));
    msg(&mut state, AnalysisMessage::Close(run));
    let fx = update(&mut state, Message::Engine(EngineMessage::Stopped(run)));
    assert!(!fx.iter().any(|e| matches!(e, Effect::StartEngine { .. })));
    assert!(state.analysis.as_slice().is_empty());
}

// ---- display, information ----

#[test]
fn the_display_cycles_hidden_nowrap_wrap_and_is_per_run() {
    let (mut state, run, _) = analysing(true, true);
    msg(
        &mut state,
        AnalysisMessage::SetDisplay(run, Display::Hidden),
    );
    assert_eq!(view(&state, run).display, Display::Hidden);
    msg(&mut state, AnalysisMessage::CycleDisplay(run));
    assert_eq!(view(&state, run).display, Display::NoWrap);
    msg(&mut state, AnalysisMessage::CycleDisplay(run));
    assert_eq!(view(&state, run).display, Display::Wrap);
    msg(&mut state, AnalysisMessage::CycleDisplay(run));
    assert_eq!(view(&state, run).display, Display::Hidden);
}

#[test]
fn hiding_the_lines_does_not_lose_them() {
    let (mut state, run, _) = with_three_lines();
    msg(
        &mut state,
        AnalysisMessage::SetDisplay(run, Display::Hidden),
    );
    assert_eq!(lines_of(&state, run).len(), 3);
}

#[test]
fn the_wrap_preference_sets_the_initial_display() {
    let mut state = AppState::default();
    state.prefs.analysis.wrap_pv = true;
    let id = state
        .engines
        .add(mychess_app::engines::EngineDraft::new("F", "/f"))
        .unwrap();
    let fx = update(
        &mut state,
        Message::Engine(EngineMessage::Start(
            id,
            mychess_app::engines::StartMode::Analyze,
        )),
    );
    let run = fx
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .unwrap();
    assert_eq!(view(&state, run).display, Display::Wrap);
}

// ---- XBoard ----

fn xboard_run() -> (AppState, RunId) {
    let (mut state, run, _) = analysing(false, false);
    let engine = run_state(&state, run).engine;
    let mut draft = state.engines.get(engine).unwrap().to_draft();
    draft.protocol = Protocol::XBoard;
    state.engines.edit(engine, draft).unwrap();
    (state, run)
}

#[test]
fn request_stats_goes_to_xboard_engines_only() {
    let (mut state, run, _) = analysing(true, true);
    assert!(msg(&mut state, AnalysisMessage::RequestStats(run)).is_empty());
    let (mut state, run) = xboard_run();
    let fx = msg(&mut state, AnalysisMessage::RequestStats(run));
    assert_eq!(commands(&fx), [EngineCommand::RequestStats]);
}

#[test]
fn an_xboard_engine_refuses_multipv_and_exclusion() {
    let (mut state, run) = xboard_run();
    let fx = msg(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    assert!(commands(&fx).is_empty());
    assert_eq!(
        analysis_notice(&state),
        Some(&AnalysisNotice::MultiPvUnsupported)
    );
    msg(&mut state, AnalysisMessage::SetExclusion(run, "e4".into()));
    assert_eq!(
        analysis_notice(&state),
        Some(&AnalysisNotice::ExcludeUnsupported)
    );
}

// ---- failed runs (rule 13) ----

fn crashed_with_a_line() -> (AppState, RunId, mychess_core::NodeId) {
    let (mut state, run, _) = analysing(true, true);
    let mut g = Game::new();
    let root = g.root();
    let mv = Position::new().parse_uci("e2e4").unwrap();
    let e4 = g.add_move(root, mv).unwrap();
    let mv = g.position_at(e4).unwrap().parse_uci("e7e5").unwrap();
    g.add_move(e4, mv).unwrap();
    state.session.replace_game(g);
    nav(&mut state, Nav::GoToNode(e4));
    started(&mut state, run, 2);
    let pos = state.session.position().clone();
    info(&mut state, run, report(&pos, 2, 1, 10, 20, &["c7c5"]));
    event(
        &mut state,
        run,
        EngineEvent::Exited(ExitInfo {
            reason: ExitReason::Crashed,
            status: None,
            stderr_tail: vec!["boom".into()],
        }),
    );
    assert!(matches!(
        run_state(&state, run).state,
        mychess_app::engines::EngineRunState::Failed(_)
    ));
    (state, run, e4)
}

#[rstest::rstest]
#[case::add_move(0)]
#[case::add_score(1)]
#[case::add_line(2)]
#[case::add_line_number(3)]
#[case::add_all(4)]
fn no_edit_button_acts_on_the_lines_of_a_dead_search(#[case] which: u8) {
    let (mut state, run, e4) = crashed_with_a_line();
    let count = state.session.game().node_count();
    let message = match which {
        0 => AnalysisMessage::AddMove(run),
        1 => AnalysisMessage::AddScore(run),
        2 => AnalysisMessage::AddLine(run),
        3 => AnalysisMessage::AddLineNumber(run, 1),
        _ => AnalysisMessage::AddAllLines(run, AllLinesMode::All),
    };
    msg(&mut state, message);
    assert_eq!(
        state.session.game().node_count(),
        count,
        "rule 13: dead search lines must not edit the game"
    );
    assert_eq!(state.session.game().comment(e4).unwrap(), None);
}

// ---- log (D447, D802) ----

fn entry(seq: u64, text: &str) -> LogEntry {
    LogEntry {
        seq,
        kind: LogKind::Received,
        text: text.to_owned(),
    }
}

fn reads(effects: &[Effect]) -> Vec<(RunId, u64)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ReadEngineLog { run, since, .. } => Some((*run, *since)),
            _ => None,
        })
        .collect()
}

#[test]
fn opening_the_log_reads_from_zero_and_ticks_read_only_what_is_new() {
    let (mut state, run, _) = analysing(true, true);
    let fx = msg(&mut state, AnalysisMessage::OpenLog(run));
    assert_eq!(reads(&fx), [(run, 0)]);
    assert!(view(&state, run).log_open);
    msg(
        &mut state,
        AnalysisMessage::LogRead {
            run,
            since: 0,
            entries: vec![entry(0, "uciok"), entry(1, "readyok")],
            next: 2,
        },
    );
    assert_eq!(view(&state, run).log.len(), 2);
    let fx = msg(&mut state, AnalysisMessage::LogTick(run));
    assert_eq!(reads(&fx), [(run, 2)]);
    msg(
        &mut state,
        AnalysisMessage::LogRead {
            run,
            since: 2,
            entries: vec![entry(2, "info")],
            next: 3,
        },
    );
    let texts: Vec<&str> = view(&state, run)
        .log
        .iter()
        .map(|e| e.text.as_str())
        .collect();
    assert_eq!(texts, ["uciok", "readyok", "info"]);
}

#[test]
fn only_one_read_is_in_flight_at_a_time() {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::OpenLog(run));
    assert!(reads(&msg(&mut state, AnalysisMessage::LogTick(run))).is_empty());
    assert!(reads(&msg(&mut state, AnalysisMessage::LogTick(run))).is_empty());
}

#[test]
fn an_old_answer_is_discarded() {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::OpenLog(run));
    msg(
        &mut state,
        AnalysisMessage::LogRead {
            run,
            since: 0,
            entries: vec![entry(0, "a")],
            next: 1,
        },
    );
    // A second answer to the same request arrives late.
    msg(
        &mut state,
        AnalysisMessage::LogRead {
            run,
            since: 0,
            entries: vec![entry(0, "a")],
            next: 1,
        },
    );
    assert_eq!(view(&state, run).log.len(), 1, "no duplicated entries");
}

#[test]
fn an_answer_after_closing_the_log_is_dropped_and_reopening_starts_over() {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::OpenLog(run));
    msg(&mut state, AnalysisMessage::CloseLog(run));
    msg(
        &mut state,
        AnalysisMessage::LogRead {
            run,
            since: 0,
            entries: vec![entry(0, "a")],
            next: 1,
        },
    );
    assert!(view(&state, run).log.is_empty());
    assert!(!view(&state, run).log_open);
    let fx = msg(&mut state, AnalysisMessage::OpenLog(run));
    assert_eq!(reads(&fx), [(run, 0)]);
}

#[test]
fn the_log_keeps_only_the_newest_entries_up_to_log_lines() {
    let (mut state, run, _) = analysing(true, true);
    state.prefs.analysis.log_lines = 5;
    msg(&mut state, AnalysisMessage::OpenLog(run));
    let entries: Vec<LogEntry> = (0..12).map(|i| entry(i, &format!("line {i}"))).collect();
    msg(
        &mut state,
        AnalysisMessage::LogRead {
            run,
            since: 0,
            entries,
            next: 12,
        },
    );
    let seqs: Vec<u64> = view(&state, run).log.iter().map(|e| e.seq).collect();
    assert_eq!(
        seqs,
        [7, 8, 9, 10, 11],
        "the oldest are dropped, not the newest"
    );
}

#[test]
fn a_log_of_size_zero_cannot_be_opened() {
    let (mut state, run, _) = analysing(true, true);
    state.prefs.analysis.log_lines = 0;
    let fx = msg(&mut state, AnalysisMessage::OpenLog(run));
    assert!(reads(&fx).is_empty());
    assert!(!view(&state, run).log_open);
}

#[test]
fn the_log_filter_keeps_only_matching_entries() {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::OpenLog(run));
    msg(
        &mut state,
        AnalysisMessage::LogRead {
            run,
            since: 0,
            entries: vec![
                entry(0, "info depth 3"),
                entry(1, "bestmove e2e4"),
                entry(2, "info depth 4"),
            ],
            next: 3,
        },
    );
    msg(
        &mut state,
        AnalysisMessage::SetLogFilter(run, "bestmove".into()),
    );
    let shown: Vec<u64> = view(&state, run).filtered_log().map(|e| e.seq).collect();
    assert_eq!(shown, [1]);
    msg(
        &mut state,
        AnalysisMessage::SetLogFilter(run, String::new()),
    );
    assert_eq!(view(&state, run).filtered_log().count(), 3);
    msg(&mut state, AnalysisMessage::SetLogFilter(run, "zzz".into()));
    assert_eq!(view(&state, run).filtered_log().count(), 0);
}

#[test]
fn save_log_goes_through_an_effect_never_the_update() {
    let (mut state, run, _) = analysing(true, true);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engine.log");
    let fx = msg(&mut state, AnalysisMessage::SaveLog(run, path.clone()));
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::SaveEngineLog { path: p, .. } if *p == path))
    );
    assert!(!path.exists(), "update must not write files");
}

#[test]
fn a_log_save_answer_sets_the_notice_or_the_file_error() {
    let (mut state, run, _) = analysing(true, true);
    msg(
        &mut state,
        AnalysisMessage::LogSaved {
            run,
            result: Ok(()),
        },
    );
    assert_eq!(analysis_notice(&state), Some(&AnalysisNotice::LogSaved));
    let err = mychess_app::FileError::from_io(
        &std::io::Error::other("x"),
        std::path::PathBuf::from("x.log"),
    );
    msg(
        &mut state,
        AnalysisMessage::LogSaved {
            run,
            result: Err(err),
        },
    );
    assert!(matches!(state.notice, Some(AppNotice::File(_))));
}

#[test]
fn messages_for_a_run_that_does_not_exist_are_harmless() {
    let (mut state, _, _) = analysing(true, true);
    let ghost = RunId(9_999);
    for m in [
        AnalysisMessage::Toggle(ghost),
        AnalysisMessage::AddMove(ghost),
        AnalysisMessage::AddScore(ghost),
        AnalysisMessage::AddLine(ghost),
        AnalysisMessage::AddLineNumber(ghost, 9),
        AnalysisMessage::AddAllLines(ghost, AllLinesMode::All),
        AnalysisMessage::SetMultiPv(ghost, 3),
        AnalysisMessage::ToggleLock(ghost),
        AnalysisMessage::OpenExclude(ghost),
        AnalysisMessage::CloseExclude(ghost),
        AnalysisMessage::SetExclusion(ghost, "e4".into()),
        AnalysisMessage::ToggleLowPriority(ghost),
        AnalysisMessage::ToggleInfo(ghost),
        AnalysisMessage::SetDisplay(ghost, Display::Wrap),
        AnalysisMessage::CycleDisplay(ghost),
        AnalysisMessage::RequestStats(ghost),
        AnalysisMessage::OpenLog(ghost),
        AnalysisMessage::CloseLog(ghost),
        AnalysisMessage::LogTick(ghost),
        AnalysisMessage::SetLogFilter(ghost, "x".into()),
        AnalysisMessage::Close(ghost),
        AnalysisMessage::LogRead {
            run: ghost,
            since: 0,
            entries: vec![],
            next: 0,
        },
    ] {
        drop(msg(&mut state, m));
    }
}

// ---- with the scripted engine ----

#[tokio::test]
async fn a_scripted_engine_log_is_read_incrementally_and_saved_with_unix_line_ends() {
    use mychess_engine::testing::ScriptedEngine;
    let fake = ScriptedEngine::builder().build();
    let mut state = AppState::default();
    state.engine_hub = {
        let fake = fake.clone();
        mychess_app::engines::EngineHub::with_launcher(std::sync::Arc::new(move |config| {
            let fake = fake.clone();
            Box::pin(async move { fake.start(config).await })
        }))
    };
    let id = state
        .engines
        .add(mychess_app::engines::EngineDraft::new(
            "Fake",
            "/fake/engine",
        ))
        .unwrap();
    let fx = update(
        &mut state,
        Message::Engine(EngineMessage::Start(
            id,
            mychess_app::engines::StartMode::Analyze,
        )),
    );
    settle(&mut state, fx).await;
    let run = state.engine_runs()[0].id;
    let fx = msg(&mut state, AnalysisMessage::OpenLog(run));
    settle(&mut state, fx).await;
    let first = view(&state, run).log.len();
    assert!(first > 0, "the handshake is in the log");
    assert!(
        view(&state, run)
            .log
            .iter()
            .any(|e| e.text.contains("uciok"))
    );
    let next = view(&state, run).log.last().unwrap().seq + 1;
    // A tick right after returns only newer entries, never a duplicate.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let fx = msg(&mut state, AnalysisMessage::LogTick(run));
    assert_eq!(reads(&fx), [(run, next)]);
    settle(&mut state, fx).await;
    let seqs: Vec<u64> = view(&state, run).log.iter().map(|e| e.seq).collect();
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "strictly increasing: {seqs:?}"
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engine.log");
    let fx = msg(&mut state, AnalysisMessage::SaveLog(run, path.clone()));
    settle(&mut state, fx).await;
    assert_eq!(analysis_notice(&state), Some(&AnalysisNotice::LogSaved));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("uciok") && !text.contains('\r'));
    let hub = state.engine_hub.clone();
    settle(&mut state, vec![Effect::ShutdownEngines { hub }]).await;
}

async fn settle(state: &mut AppState, effects: Vec<Effect>) {
    let mut queue: std::collections::VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        let answer = mychess_app::perform(effect).await;
        queue.extend(update(state, answer));
    }
}
