//! Tests of the analysis window flows: synthetic reports of an engine fed
//! through `update`, and a scripted in-memory engine through `perform`
//! (spec M4-4).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mychess_core::{Game, Move, Position};
use mychess_engine::testing::ScriptedEngine;
use mychess_engine::{
    BestMove, Bound, Capabilities, EngineCommand, EngineEvent, EngineIdentity, LogEntry, LogKind,
    Score, SearchId, SearchInfo, Wdl,
};

use super::{AppState, update};
use crate::analysis::{AllLinesMode, AnalysisMessage, AnalysisNotice, Display, Terminal};
use crate::effect::{Effect, perform};
use crate::engines::{EngineDraft, EngineHub, EngineMessage, EngineRunState, RunId, StartMode};
use crate::flow::AppNotice;
use crate::message::Message;
use crate::session::{GameMessage, Nav, PlayOptions};

fn identity(multipv: bool, search_moves: bool) -> EngineIdentity {
    EngineIdentity {
        name: "Fake".to_owned(),
        author: None,
        options: Vec::new(),
        capabilities: Capabilities {
            multipv: multipv.then_some(1..=5),
            search_moves,
            ..Capabilities::default()
        },
    }
}

/// A state with one engine that is ready and analysing, and its first
/// search sent (`Started`).
fn analysing(multipv: bool, search_moves: bool) -> (AppState, RunId, Vec<Effect>) {
    let mut state = AppState::default();
    let id = state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("add");
    let run = state.engine_runs.begin(id, true);
    let r = state.engine_runs.get_mut(run).expect("run");
    r.state = EngineRunState::Running;
    r.identity = Some(Box::new(identity(multipv, search_moves)));
    let effects = tick(&mut state);
    (state, run, effects)
}

/// Any update: the engines are refreshed at its end.
fn tick(state: &mut AppState) -> Vec<Effect> {
    update(state, Message::Game(GameMessage::Navigate(Nav::Start)))
}

fn event(state: &mut AppState, run: RunId, event: EngineEvent) -> Vec<Effect> {
    update(
        state,
        Message::Engine(EngineMessage::Event {
            run,
            event: Box::new(event),
        }),
    )
}

fn analysis(state: &mut AppState, message: AnalysisMessage) -> Vec<Effect> {
    update(state, Message::Analysis(message))
}

fn commands(effects: &[Effect]) -> Vec<EngineCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand { command, .. } => Some((**command).clone()),
            _ => None,
        })
        .collect()
}

fn searches(effects: &[Effect]) -> Vec<mychess_engine::SearchRequest> {
    commands(effects)
        .into_iter()
        .filter_map(|c| match c {
            EngineCommand::Search(request) => Some(request),
            _ => None,
        })
        .collect()
}

fn mv(position: &Position, uci: &str) -> Move {
    position.parse_uci(uci).expect("uci")
}

/// A report with a line: the moves are played from `position`.
fn report(
    position: &Position,
    search: u64,
    multipv: u8,
    depth: u32,
    cp: i32,
    pv: &[&str],
) -> SearchInfo {
    let mut pos = position.clone();
    let pv = pv
        .iter()
        .map(|u| {
            let m = pos.parse_uci(u).expect("uci");
            pos.play(m).expect("play");
            m
        })
        .collect();
    SearchInfo {
        search: SearchId(search),
        multipv,
        depth: Some(depth),
        score: Some((Score::Cp(cp), Bound::Exact)),
        pv,
        time_ms: Some(1000),
        nodes: Some(1_000_000),
        ..SearchInfo::default()
    }
}

fn started(state: &mut AppState, run: RunId, search: u64) {
    event(state, run, EngineEvent::SearchStarted(SearchId(search)));
}

fn lines_of(state: &AppState, run: RunId) -> Vec<(u8, String)> {
    state
        .analysis_view(run)
        .expect("view")
        .lines
        .iter()
        .map(|l| (l.multipv, l.text.clone()))
        .collect()
}

fn analysis_notice(state: &AppState) -> Option<&AnalysisNotice> {
    match state.notice.as_ref() {
        Some(AppNotice::Analysis(n)) => Some(n),
        _ => None,
    }
}

// ---- reports ----

#[test]
fn the_first_update_creates_the_view_and_sends_the_position() {
    let (state, run, effects) = analysing(true, true);
    assert_eq!(searches(&effects).len(), 1);
    let view = state.analysis_view(run).expect("view");
    assert_eq!(view.multipv, 1);
    assert!(view.lines.is_empty());
    assert_eq!(state.analysis.as_slice().len(), 1);
}

#[test]
fn reports_merge_into_lines_and_a_stale_search_is_dropped() {
    let (mut state, run, _) = analysing(true, true);
    analysis(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    let start = Position::new();
    // Before the engine says a search began nothing is accepted.
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 1, 1, 5, 10, &["e2e4"])),
    );
    assert!(lines_of(&state, run).is_empty());
    started(&mut state, run, 1);
    for (n, uci) in [(3, "g1f3"), (1, "e2e4"), (2, "d2d4")] {
        event(
            &mut state,
            run,
            EngineEvent::Info(report(&start, 1, n, 9, 30 - i32::from(n), &[uci])),
        );
    }
    assert_eq!(
        lines_of(&state, run),
        [(1, "1.e4".into()), (2, "1.d4".into()), (3, "1.Nf3".into())]
    );
    // A report of search 0 (old) and one for line 4 (beyond the count).
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 0, 1, 20, 99, &["c2c4"])),
    );
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 1, 4, 20, 99, &["c2c4"])),
    );
    assert_eq!(lines_of(&state, run).len(), 3);
    assert_eq!(lines_of(&state, run)[0].1, "1.e4");
}

#[test]
fn a_report_neither_rebuilds_the_notation_nor_asks_for_effects() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    let start = Position::new();
    let revision = state.notation().revision;
    let mut effects = 0;
    let n = 5000_u32;
    for depth in 1..=n {
        let info = report(
            &start,
            1,
            1,
            depth,
            20,
            &["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6"],
        );
        effects += event(&mut state, run, EngineEvent::Info(info)).len();
    }
    assert_eq!(effects, 0);
    assert_eq!(state.notation().revision, revision);
    assert_eq!(
        state.analysis_view(run).expect("view").history.len(),
        1,
        "the same PV is one history line"
    );
}

#[test]
fn the_best_move_marks_the_search_as_finished_only_for_the_current_one() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    let best = |search| BestMove {
        search: SearchId(search),
        mv: None,
        ponder: None,
    };
    event(&mut state, run, EngineEvent::BestMove(best(0)));
    assert!(!state.analysis_view(run).expect("view").finished);
    event(&mut state, run, EngineEvent::BestMove(best(1)));
    assert!(state.analysis_view(run).expect("view").finished);
    started(&mut state, run, 2);
    assert!(!state.analysis_view(run).expect("view").finished);
}

#[test]
fn a_new_position_clears_the_lines_when_its_search_starts() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    let start = Position::new();
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 1, 1, 5, 10, &["e2e4"])),
    );
    let e4 = mv(&start, "e2e4");
    let effects = update(
        &mut state,
        Message::Game(GameMessage::PlayMove {
            mv: e4,
            options: PlayOptions::default(),
        }),
    );
    assert_eq!(searches(&effects).len(), 1, "the new position is sent");
    assert_eq!(lines_of(&state, run).len(), 1, "until its search starts");
    started(&mut state, run, 2);
    assert!(lines_of(&state, run).is_empty());
}

// ---- the position travels with the search (D780) ----

/// The engine's two events for a request: its label, then the start.
fn started_as(
    state: &mut AppState,
    run: RunId,
    search: u64,
    request: &mychess_engine::SearchRequest,
) {
    event(
        state,
        run,
        EngineEvent::SearchTagged {
            search: SearchId(search),
            tag: request.tag,
        },
    );
    started(state, run, search);
}

fn play(state: &mut AppState, uci: &str) -> Vec<Effect> {
    let position = state.session.position().clone();
    update(
        state,
        Message::Game(GameMessage::PlayMove {
            mv: mv(&position, uci),
            options: PlayOptions::default(),
        }),
    )
}

#[test]
fn a_started_event_in_transit_binds_to_the_position_it_was_sent_for() {
    let (mut state, run, effects) = analysing(true, true);
    let request_a = searches(&effects).remove(0);
    assert_ne!(request_a.tag, 0, "every search is labelled");
    let start = Position::new();
    // The cursor moves on before `SearchStarted(A)` arrives.
    let effects = play(&mut state, "e2e4");
    let request_b = searches(&effects).remove(0);
    assert_ne!(request_a.tag, request_b.tag);
    started_as(&mut state, run, 1, &request_a);
    // A report of search A is about the start position, not about 1.e4.
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 1, 1, 9, 30, &["d2d4"])),
    );
    let at_cursor = state.analysis_target().expect("target");
    let view = state.analysis_view(run).expect("view");
    assert!(
        view.lines.is_empty() || !view.is_about(&at_cursor),
        "no line may be tied to the position at the cursor"
    );
    // Whatever the lines are, nothing edits the game from them.
    let count = state.session.game().node_count();
    for message in [
        AnalysisMessage::AddMove(run),
        AnalysisMessage::AddScore(run),
        AnalysisMessage::AddLine(run),
        AnalysisMessage::AddAllLines(run, AllLinesMode::All),
    ] {
        analysis(&mut state, message);
    }
    assert_eq!(state.session.game().node_count(), count);
    // Then B starts and its lines are shown.
    started_as(&mut state, run, 2, &request_b);
    let after_e4 = state.session.position().clone();
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&after_e4, 2, 1, 9, -20, &["e7e5"])),
    );
    assert_eq!(lines_of(&state, run), [(1, "1... e5".into())]);
}

#[test]
fn a_superseded_search_never_taints_the_one_that_starts() {
    let (mut state, run, effects) = analysing(true, true);
    let _request_a = searches(&effects).remove(0);
    play(&mut state, "e2e4");
    let effects = play(&mut state, "e7e5");
    let request_c = searches(&effects).remove(0);
    // Only C starts (A and B were replaced before they began).
    started_as(&mut state, run, 1, &request_c);
    let pos = state.session.position().clone();
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&pos, 1, 1, 9, 10, &["g1f3"])),
    );
    assert_eq!(lines_of(&state, run), [(1, "2.Nf3".into())]);
}

#[test]
fn a_start_with_an_unknown_position_clears_the_lines_and_drops_reports() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    let start = Position::new();
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 1, 1, 5, 10, &["e2e4"])),
    );
    event(
        &mut state,
        run,
        EngineEvent::SearchTagged {
            search: SearchId(2),
            tag: 9999,
        },
    );
    started(&mut state, run, 2);
    assert!(lines_of(&state, run).is_empty());
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 2, 1, 9, 30, &["d2d4"])),
    );
    assert!(
        lines_of(&state, run).is_empty(),
        "nothing to check it against"
    );
}

#[test]
fn a_terminal_position_stops_a_search_that_has_not_started_yet() {
    let (mut state, run, _) = analysing(true, true);
    // Fool's mate: 1.f3 e5 2.g4 Qh4#.
    for uci in ["f2f3", "e7e5", "g2g4"] {
        play(&mut state, uci);
    }
    let effects = play(&mut state, "d8h4");
    // No `SearchStarted` has arrived for any of the searches sent.
    assert!(
        commands(&effects).contains(&EngineCommand::Stop),
        "{effects:?}"
    );
    assert_eq!(
        state.analysis_view(run).expect("view").terminal,
        Some(Terminal::Checkmate)
    );
}

#[test]
fn a_terminal_position_after_a_finished_search_sends_no_stop() {
    let (mut state, run, _) = analysing(true, true);
    let mut last = None;
    for uci in ["f2f3", "e7e5", "g2g4"] {
        last = searches(&play(&mut state, uci)).pop();
    }
    let request = last.expect("search of the last position");
    started_as(&mut state, run, 1, &request);
    event(
        &mut state,
        run,
        EngineEvent::BestMove(BestMove {
            search: SearchId(1),
            mv: None,
            ponder: None,
        }),
    );
    let effects = play(&mut state, "d8h4");
    assert!(!commands(&effects).contains(&EngineCommand::Stop));
}

#[test]
fn changing_the_exclusion_forgets_the_lines_that_held_the_old_one() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    let start = Position::new();
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 1, 1, 9, 30, &["e2e4"])),
    );
    analysis(&mut state, AnalysisMessage::SetExclusion(run, "e4".into()));
    assert!(lines_of(&state, run).is_empty());
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 1, 1, 9, 30, &["d2d4"])),
    );
    analysis(
        &mut state,
        AnalysisMessage::SetExclusion(run, String::new()),
    );
    assert!(lines_of(&state, run).is_empty(), "clearing it also resets");
    let count = state.session.game().node_count();
    analysis(&mut state, AnalysisMessage::AddMove(run));
    assert_eq!(state.session.game().node_count(), count);
}

#[test]
fn the_exclusion_dialog_does_not_open_on_a_dead_run() {
    let (mut state, run, _) = analysing(true, true);
    state.engine_runs.get_mut(run).expect("run").state = EngineRunState::Stopping;
    analysis(&mut state, AnalysisMessage::OpenExclude(run));
    assert!(!state.analysis_view(run).expect("view").exclude_open);
}

#[test]
fn settings_changed_during_a_restart_reach_the_new_view() {
    let (mut state, run, _) = analysing(true, true);
    analysis(&mut state, AnalysisMessage::ToggleLowPriority(run));
    analysis(
        &mut state,
        AnalysisMessage::SetDisplay(run, Display::Hidden),
    );
    analysis(&mut state, AnalysisMessage::SetLogFilter(run, "x".into()));
    let (_, restart) = state
        .analysis
        .restarts
        .iter()
        .find(|(r, _)| *r == run)
        .expect("restart");
    assert_eq!(restart.view.display, Display::Hidden);
    assert_eq!(restart.view.log_filter, "x");
}

// ---- start, stop, lock ----

#[test]
fn turning_the_analysis_off_stops_and_on_sends_the_position_now_shown() {
    let (mut state, run, _) = analysing(true, true);
    let effects = analysis(&mut state, AnalysisMessage::Toggle(run));
    assert_eq!(commands(&effects), [EngineCommand::Stop]);
    assert!(!state.engine_runs.get(run).expect("run").analyzing);
    // The game moves while it is off: nothing is sent.
    let start = Position::new();
    let e4 = mv(&start, "e2e4");
    let effects = update(
        &mut state,
        Message::Game(GameMessage::PlayMove {
            mv: e4,
            options: PlayOptions::default(),
        }),
    );
    assert!(searches(&effects).is_empty());
    let effects = analysis(&mut state, AnalysisMessage::Toggle(run));
    let sent = searches(&effects);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].position.moves,
        [e4],
        "the position is the current one"
    );
}

#[test]
fn a_locked_analysis_ignores_the_board_until_it_is_unlocked() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    let start = Position::new();
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 1, 1, 5, 10, &["e2e4"])),
    );
    analysis(&mut state, AnalysisMessage::ToggleLock(run));
    let lock = state
        .analysis_view(run)
        .expect("view")
        .lock
        .clone()
        .expect("locked");
    assert_eq!(lock.move_number, 1);
    assert_eq!(lock.variation_depth, 0);
    assert!(state.engine_runs.get(run).expect("run").locked);
    let e4 = mv(&start, "e2e4");
    let effects = update(
        &mut state,
        Message::Game(GameMessage::PlayMove {
            mv: e4,
            options: PlayOptions::default(),
        }),
    );
    assert!(searches(&effects).is_empty(), "not sent while locked");
    assert_eq!(lines_of(&state, run).len(), 1, "the lines stay");
    // The buttons that change things refuse.
    analysis(&mut state, AnalysisMessage::AddMove(run));
    assert_eq!(analysis_notice(&state), Some(&AnalysisNotice::Locked));
    analysis(&mut state, AnalysisMessage::SetMultiPv(run, 2));
    assert_eq!(state.analysis_view(run).expect("view").multipv, 1);
    // Unlocking sends the position now shown and forgets the lines.
    let effects = analysis(&mut state, AnalysisMessage::ToggleLock(run));
    assert_eq!(searches(&effects).len(), 1);
    assert!(state.analysis_view(run).expect("view").lock.is_none());
    assert!(lines_of(&state, run).is_empty());
}

#[test]
fn turning_a_locked_analysis_off_and_on_searches_the_locked_position() {
    let (mut state, run, _) = analysing(true, true);
    analysis(&mut state, AnalysisMessage::ToggleLock(run));
    let start = Position::new();
    let e4 = mv(&start, "e2e4");
    update(
        &mut state,
        Message::Game(GameMessage::PlayMove {
            mv: e4,
            options: PlayOptions::default(),
        }),
    );
    analysis(&mut state, AnalysisMessage::Toggle(run));
    let effects = analysis(&mut state, AnalysisMessage::Toggle(run));
    let sent = searches(&effects);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].position.moves.is_empty(),
        "the locked start position"
    );
}

#[test]
fn the_lock_remembers_the_variation_depth_and_the_last_move() {
    let (mut state, run, _) = analysing(true, true);
    let start = Position::new();
    let play = |state: &mut AppState, from: &Position, uci: &str, force: bool| {
        update(
            state,
            Message::Game(GameMessage::PlayMove {
                mv: mv(from, uci),
                options: PlayOptions {
                    force_variation: force,
                },
            }),
        );
    };
    play(&mut state, &start, "e2e4", false);
    // A variation 1.d4 of the first move.
    update(&mut state, Message::Game(GameMessage::Navigate(Nav::Start)));
    play(&mut state, &start, "d2d4", true);
    analysis(&mut state, AnalysisMessage::ToggleLock(run));
    let lock = state
        .analysis_view(run)
        .expect("view")
        .lock
        .clone()
        .expect("lock");
    assert_eq!(lock.variation_depth, 1);
    assert_eq!(lock.last_move.as_deref(), Some("d4"));
    assert_eq!(lock.move_number, 1);
    assert_eq!(lock.side, mychess_core::Color::Black);
}

// ---- lines, exclusion ----

#[test]
fn the_number_of_lines_is_clamped_sent_cut_and_saved_to_the_list() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    let start = Position::new();
    let effects = analysis(&mut state, AnalysisMessage::SetMultiPv(run, 9));
    assert_eq!(commands(&effects), [EngineCommand::SetMultiPv(5)]);
    for (n, uci) in [(1, "e2e4"), (2, "d2d4"), (3, "g1f3")] {
        event(
            &mut state,
            run,
            EngineEvent::Info(report(&start, 1, n, 5, 0, &[uci])),
        );
    }
    analysis(&mut state, AnalysisMessage::SetMultiPv(run, 2));
    assert_eq!(lines_of(&state, run).len(), 2);
    let engine = state.engine_runs.get(run).expect("run").engine;
    assert_eq!(state.engines.get(engine).expect("entry").multipv, 2);
    // The same number again does nothing.
    let effects = analysis(&mut state, AnalysisMessage::SetMultiPv(run, 2));
    assert!(commands(&effects).is_empty());
}

#[test]
fn an_engine_without_multipv_refuses_the_count() {
    let (mut state, run, _) = analysing(false, true);
    let effects = analysis(&mut state, AnalysisMessage::SetMultiPv(run, 2));
    assert!(commands(&effects).is_empty());
    assert_eq!(
        analysis_notice(&state),
        Some(&AnalysisNotice::MultiPvUnsupported)
    );
}

#[test]
fn excluding_moves_sends_the_rest_as_searchmoves_and_a_new_position_clears_it() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    analysis(&mut state, AnalysisMessage::OpenExclude(run));
    assert!(state.analysis_view(run).expect("view").exclude_open);
    let effects = analysis(
        &mut state,
        AnalysisMessage::SetExclusion(run, "e4, d4".into()),
    );
    let sent = searches(&effects);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].search_moves.len(), 18);
    let view = state.analysis_view(run).expect("view");
    assert_eq!(view.exclusion_san, ["e4", "d4"]);
    assert!(!view.exclude_open);
    // The position changes: the exclusion goes and the search is plain.
    let start = Position::new();
    let effects = update(
        &mut state,
        Message::Game(GameMessage::PlayMove {
            mv: mv(&start, "g1f3"),
            options: PlayOptions::default(),
        }),
    );
    let sent = searches(&effects);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].search_moves.is_empty());
    assert!(state.analysis_view(run).expect("view").exclusion.is_empty());
}

#[test]
fn an_empty_exclusion_clears_it_and_resends() {
    let (mut state, run, _) = analysing(true, true);
    analysis(&mut state, AnalysisMessage::SetExclusion(run, "e4".into()));
    let effects = analysis(&mut state, AnalysisMessage::SetExclusion(run, "  ".into()));
    let sent = searches(&effects);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].search_moves.is_empty());
    assert!(state.analysis_view(run).expect("view").exclusion.is_empty());
}

#[test]
fn bad_exclusions_say_why_and_change_nothing() {
    let (mut state, run, _) = analysing(true, true);
    let effects = analysis(
        &mut state,
        AnalysisMessage::SetExclusion(run, "e4 Nf6".into()),
    );
    assert!(effects.is_empty());
    assert_eq!(
        analysis_notice(&state),
        Some(&AnalysisNotice::ExcludeInvalid("Nf6".into()))
    );
    let all = "a3 a4 b3 b4 c3 c4 d3 d4 e3 e4 f3 f4 g3 g4 h3 h4 Na3 Nc3 Nf3 Nh3";
    analysis(&mut state, AnalysisMessage::SetExclusion(run, all.into()));
    assert_eq!(analysis_notice(&state), Some(&AnalysisNotice::ExcludeAll));
    assert!(state.analysis_view(run).expect("view").exclusion.is_empty());

    let (mut xboard, run, _) = analysing(false, false);
    analysis(&mut xboard, AnalysisMessage::SetExclusion(run, "e4".into()));
    assert_eq!(
        analysis_notice(&xboard),
        Some(&AnalysisNotice::ExcludeUnsupported)
    );
}

#[test]
fn stopping_the_analysis_clears_the_exclusion() {
    let (mut state, run, _) = analysing(true, true);
    analysis(&mut state, AnalysisMessage::SetExclusion(run, "e4".into()));
    analysis(&mut state, AnalysisMessage::Toggle(run));
    assert!(state.analysis_view(run).expect("view").exclusion.is_empty());
}

#[test]
fn a_position_without_moves_is_not_searched_and_has_a_fixed_score() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    let mate = Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
        .expect("fen");
    state.session.replace_game(Game::from_position(mate));
    let effects = tick(&mut state);
    assert!(searches(&effects).is_empty(), "nothing to search");
    assert!(
        commands(&effects).contains(&EngineCommand::Stop),
        "the old search ends"
    );
    let view = state.analysis_view(run).expect("view");
    assert_eq!(view.terminal, Some(Terminal::Checkmate));
    assert_eq!(view.main_score(), Some((Score::Cp(-32_700), Bound::Exact)));
}

// ---- adding to the game ----

/// A state at the start position with three lines shown.
fn with_three_lines() -> (AppState, RunId, Position) {
    let (mut state, run, _) = analysing(true, true);
    analysis(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    started(&mut state, run, 1);
    let start = Position::new();
    for (n, depth, cp, pv) in [
        (1, 20, 35, vec!["e2e4", "e7e5", "g1f3"]),
        (2, 19, 30, vec!["d2d4", "d7d5"]),
        (3, 18, 25, vec!["g1f3"]),
    ] {
        let mut info = report(&start, 1, n, depth, cp, &pv);
        if n == 1 {
            info.wdl = Some(Wdl {
                win: 450,
                draw: 300,
                loss: 250,
            });
        }
        event(&mut state, run, EngineEvent::Info(info));
    }
    (state, run, start)
}

#[test]
fn add_move_plays_the_first_move_of_the_first_line() {
    let (mut state, run, _) = with_three_lines();
    analysis(&mut state, AnalysisMessage::AddMove(run));
    assert_eq!(state.session.game().mainline().count(), 1);
    assert_ne!(state.session.cursor(), state.session.game().root());
}

#[test]
fn add_move_equal_to_the_next_move_only_advances_and_a_different_one_asks() {
    let (mut state, run, start) = with_three_lines();
    // The game has 1.e4 and the cursor is back at the start.
    state.session.update(GameMessage::PlayMove {
        mv: mv(&start, "e2e4"),
        options: PlayOptions::default(),
    });
    state.session.update(GameMessage::Navigate(Nav::Start));
    tick(&mut state);
    started(&mut state, run, 2);
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 2, 1, 10, 0, &["e2e4"])),
    );
    let depth = state.session.undo_depth();
    analysis(&mut state, AnalysisMessage::AddMove(run));
    assert_eq!(state.session.undo_depth(), depth, "no edit");
    assert_ne!(
        state.session.cursor(),
        state.session.game().root(),
        "advanced"
    );
    // Another move at a position that has a continuation: the question.
    state.session.update(GameMessage::Navigate(Nav::Start));
    tick(&mut state);
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&start, 2, 1, 11, 0, &["d2d4"])),
    );
    analysis(&mut state, AnalysisMessage::AddMove(run));
    assert!(state.session.prompt().is_some());
}

#[test]
fn add_move_with_an_old_line_or_a_stopped_engine_does_nothing() {
    let (mut state, run, _) = with_three_lines();
    let e4 = mv(&Position::new(), "e2e4");
    update(
        &mut state,
        Message::Game(GameMessage::PlayMove {
            mv: e4,
            options: PlayOptions::default(),
        }),
    );
    // The lines are about the start position, the board shows another one.
    analysis(&mut state, AnalysisMessage::AddMove(run));
    assert_eq!(state.session.game().mainline().count(), 1);
    assert_eq!(state.notice, None);
    let (mut stopped, run, _) = with_three_lines();
    analysis(&mut stopped, AnalysisMessage::Toggle(run));
    analysis(&mut stopped, AnalysisMessage::AddMove(run));
    assert_eq!(stopped.session.game().mainline().count(), 0);
}

#[test]
fn add_score_puts_depth_and_score_in_front_of_the_comment() {
    let (mut state, run, _) = with_three_lines();
    analysis(&mut state, AnalysisMessage::AddScore(run));
    assert_eq!(
        state.session.game().initial_comment(),
        Some("[Fake] 20:+0.35")
    );
    analysis(&mut state, AnalysisMessage::AddScore(run));
    assert_eq!(
        state.session.game().initial_comment(),
        Some("[Fake] 20:+0.35 [Fake] 20:+0.35")
    );
    assert!(state.session.can_undo());
}

#[test]
fn add_line_on_an_empty_game_makes_the_main_line_with_wdl() {
    let (mut state, run, _) = with_three_lines();
    analysis(&mut state, AnalysisMessage::AddLine(run));
    let game = state.session.game();
    assert_eq!(game.mainline().count(), 3);
    let first = game.mainline().next().expect("first");
    assert_eq!(
        game.comment_before(first).expect("node"),
        Some("[Fake] 20:+0.35 W: 45% D: 30% B: 25%")
    );
    assert_eq!(state.session.cursor(), game.root(), "the cursor stays");
}

#[test]
fn add_all_lines_in_each_mode() {
    let (mut state, run, _) = with_three_lines();
    state.session.update(GameMessage::PlayMove {
        mv: mv(&Position::new(), "e2e4"),
        options: PlayOptions::default(),
    });
    // Back to the start with a continuation, so the lines can branch.
    state.session.update(GameMessage::Navigate(Nav::Start));
    tick(&mut state);
    started(&mut state, run, 2);
    let start = Position::new();
    for (n, pv) in [
        (1, vec!["e2e4", "e7e5"]),
        (2, vec!["d2d4", "d7d5"]),
        (3, vec!["c2c4"]),
    ] {
        event(
            &mut state,
            run,
            EngineEvent::Info(report(&start, 2, n, 12, 10, &pv)),
        );
    }
    analysis(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::All),
    );
    let root = state.session.game().root();
    assert_eq!(
        state.session.game().children(root).expect("children").len(),
        3
    );
    state.session.update(GameMessage::Undo);
    analysis(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::SecondOnly),
    );
    let children = state
        .session
        .game()
        .children(root)
        .expect("children")
        .to_vec();
    assert_eq!(children.len(), 2);
    assert_eq!(
        state
            .session
            .game()
            .comment_before(children[1])
            .expect("node"),
        Some("[Fake] 12:+0.10")
    );
    state.session.update(GameMessage::Undo);
    analysis(
        &mut state,
        AnalysisMessage::AddAllLines(run, AllLinesMode::FirstMovesOnly),
    );
    let children = state
        .session
        .game()
        .children(root)
        .expect("children")
        .to_vec();
    assert_eq!(children.len(), 3);
    for child in &children[1..] {
        assert!(
            state
                .session
                .game()
                .children(*child)
                .expect("children")
                .is_empty()
        );
    }
    // One undo for the whole action.
    state.session.update(GameMessage::Undo);
    assert_eq!(
        state.session.game().children(root).expect("children").len(),
        1
    );
}

#[test]
fn add_line_at_the_end_of_the_main_line_is_refused_with_a_notice() {
    let (mut state, run, _) = with_three_lines();
    let e4 = mv(&Position::new(), "e2e4");
    update(
        &mut state,
        Message::Game(GameMessage::PlayMove {
            mv: e4,
            options: PlayOptions::default(),
        }),
    );
    started(&mut state, run, 2);
    let after = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
        .expect("fen");
    event(
        &mut state,
        run,
        EngineEvent::Info(report(&after, 2, 1, 10, 0, &["e7e5"])),
    );
    analysis(&mut state, AnalysisMessage::AddLine(run));
    assert_eq!(analysis_notice(&state), Some(&AnalysisNotice::AtGameEnd));
    assert_eq!(state.session.game().mainline().count(), 1);
}

#[test]
fn a_failed_run_does_not_edit_the_game() {
    let (mut state, run, _) = with_three_lines();
    state.engine_runs.get_mut(run).expect("run").state =
        EngineRunState::Failed(crate::engines::EngineFailure {
            kind: crate::engines::FailureKind::Crashed,
            exit_code: None,
            stderr_tail: Vec::new(),
        });
    analysis(&mut state, AnalysisMessage::AddMove(run));
    assert_eq!(state.session.game().mainline().count(), 0);
}

// ---- display, info, priority ----

#[test]
fn display_and_information_toggles_are_per_run() {
    let (mut state, run, _) = analysing(true, true);
    assert_eq!(
        state.analysis_view(run).expect("view").display,
        Display::NoWrap
    );
    analysis(&mut state, AnalysisMessage::CycleDisplay(run));
    assert_eq!(
        state.analysis_view(run).expect("view").display,
        Display::Wrap
    );
    analysis(
        &mut state,
        AnalysisMessage::SetDisplay(run, Display::Hidden),
    );
    assert_eq!(
        state.analysis_view(run).expect("view").display,
        Display::Hidden
    );
    analysis(&mut state, AnalysisMessage::ToggleInfo(run));
    assert!(state.analysis_view(run).expect("view").show_info);
}

#[test]
fn the_low_priority_switch_restarts_the_engine_keeping_the_section() {
    let (mut state, run, _) = analysing(true, true);
    analysis(
        &mut state,
        AnalysisMessage::SetDisplay(run, Display::Hidden),
    );
    analysis(&mut state, AnalysisMessage::ToggleInfo(run));
    analysis(&mut state, AnalysisMessage::SetExclusion(run, "e4".into()));
    let effects = analysis(&mut state, AnalysisMessage::ToggleLowPriority(run));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::StopEngine { .. }))
    );
    assert!(state.analysis_view(run).expect("view").restarting);
    let effects = update(&mut state, Message::Engine(EngineMessage::Stopped(run)));
    let start = effects
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, defaults, .. } => Some((*run, defaults.low_priority)),
            _ => None,
        })
        .expect("started again");
    assert!(start.1, "now at low priority");
    assert_ne!(start.0, run);
    assert!(state.analysis_view(run).is_none());
    let view = state.analysis_view(start.0).expect("the new view");
    assert_eq!(view.display, Display::Hidden);
    assert!(view.show_info);
    assert_eq!(view.exclusion_san, ["e4"]);
    assert!(!view.restarting);
    assert!(state.engine_runs.get(start.0).expect("run").low_priority);
    assert!(state.engine_runs.get(start.0).expect("run").analyzing);
}

#[test]
fn closing_a_section_ends_the_run_and_cancels_a_restart() {
    let (mut state, run, _) = analysing(true, true);
    analysis(&mut state, AnalysisMessage::ToggleLowPriority(run));
    analysis(&mut state, AnalysisMessage::Close(run));
    let effects = update(&mut state, Message::Engine(EngineMessage::Stopped(run)));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::StartEngine { .. }))
    );
    assert!(state.analysis.as_slice().is_empty());
}

#[test]
fn request_stats_goes_only_to_xboard_engines() {
    let (mut state, run, _) = analysing(true, true);
    assert!(analysis(&mut state, AnalysisMessage::RequestStats(run)).is_empty());
    let engine = state.engine_runs.get(run).expect("run").engine;
    let mut draft = state.engines.get(engine).expect("entry").to_draft();
    draft.protocol = mychess_engine::Protocol::XBoard;
    state.engines.edit(engine, draft).expect("edit");
    let effects = analysis(&mut state, AnalysisMessage::RequestStats(run));
    assert_eq!(commands(&effects), [EngineCommand::RequestStats]);
}

// ---- the log ----

fn entry(seq: u64, text: &str) -> LogEntry {
    LogEntry {
        seq,
        kind: LogKind::Received,
        text: text.to_owned(),
    }
}

#[test]
fn the_log_is_read_incrementally_one_read_at_a_time_and_bounded() {
    let (mut state, run, _) = analysing(true, true);
    state.prefs.analysis.log_lines = 3;
    // Closed: a tick reads nothing.
    assert!(analysis(&mut state, AnalysisMessage::LogTick(run)).is_empty());
    let effects = analysis(&mut state, AnalysisMessage::OpenLog(run));
    assert!(matches!(
        effects.as_slice(),
        [Effect::ReadEngineLog { since: 0, .. }]
    ));
    // A read is in flight: ticks wait.
    assert!(analysis(&mut state, AnalysisMessage::LogTick(run)).is_empty());
    analysis(
        &mut state,
        AnalysisMessage::LogRead {
            run,
            since: 0,
            entries: vec![entry(0, "uci"), entry(1, "uciok")],
            next: 2,
        },
    );
    let effects = analysis(&mut state, AnalysisMessage::LogTick(run));
    assert!(matches!(
        effects.as_slice(),
        [Effect::ReadEngineLog { since: 2, .. }]
    ));
    analysis(
        &mut state,
        AnalysisMessage::LogRead {
            run,
            since: 2,
            entries: vec![entry(2, "a"), entry(3, "b")],
            next: 4,
        },
    );
    let log = &state.analysis_view(run).expect("view").log;
    assert_eq!(
        log.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [1, 2, 3],
        "oldest dropped"
    );
    // A late answer to an old read is ignored.
    analysis(
        &mut state,
        AnalysisMessage::LogRead {
            run,
            since: 0,
            entries: vec![entry(0, "late")],
            next: 1,
        },
    );
    assert_eq!(state.analysis_view(run).expect("view").log.len(), 3);
    analysis(&mut state, AnalysisMessage::SetLogFilter(run, "B".into()));
    assert_eq!(
        state
            .analysis_view(run)
            .expect("view")
            .filtered_log()
            .count(),
        1
    );
    analysis(&mut state, AnalysisMessage::CloseLog(run));
    let view = state.analysis_view(run).expect("view");
    assert!(view.log.is_empty() && !view.log_open);
}

#[test]
fn a_log_that_is_turned_off_does_not_open() {
    let (mut state, run, _) = analysing(true, true);
    state.prefs.analysis.log_lines = 0;
    assert!(analysis(&mut state, AnalysisMessage::OpenLog(run)).is_empty());
    assert!(!state.analysis_view(run).expect("view").log_open);
}

// ---- with the scripted in-memory engine ----

fn scripted_hub(fake: &ScriptedEngine) -> EngineHub {
    let fake = fake.clone();
    EngineHub::with_launcher(Arc::new(move |config| {
        let fake = fake.clone();
        Box::pin(async move { fake.start(config).await })
    }))
}

async fn settle(state: &mut AppState, effects: Vec<Effect>) -> Vec<Effect> {
    let mut seen = Vec::new();
    let mut queue: VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        seen.push(effect.clone());
        let answer = perform(effect).await;
        queue.extend(update(state, answer));
    }
    seen
}

#[tokio::test]
async fn a_scripted_engine_fills_the_lines_and_its_log_is_read_and_saved() {
    let fake = ScriptedEngine::builder().build();
    let mut state = AppState {
        engine_hub: scripted_hub(&fake),
        ..AppState::default()
    };
    let id = state
        .engines
        .add(EngineDraft::new("Fake", "/fake/engine"))
        .expect("add");
    let mut events = state.engine_hub.events().expect("receiver");
    let effects = update(
        &mut state,
        Message::Engine(EngineMessage::Start(id, StartMode::Analyze)),
    );
    settle(&mut state, effects).await;
    let run = state.engine_runs()[0].id;
    let effects = analysis(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    settle(&mut state, effects).await;
    // Pump the events until three lines are shown.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (from, ev) = tokio::time::timeout(Duration::from_secs(5), events.next())
            .await
            .expect("an event")
            .expect("open");
        let effects = event(&mut state, from, ev);
        settle(&mut state, effects).await;
        let view = state.analysis_view(run).expect("view");
        if view.lines.len() == 3 && view.lines.iter().all(|l| l.depth >= 2) {
            break;
        }
        assert!(Instant::now() < deadline, "no lines: {:?}", view.lines);
    }
    let view = state.analysis_view(run).expect("view");
    assert_eq!(
        view.lines.iter().map(|l| l.multipv).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(view.lines[0].text.starts_with("1."));
    assert!(view.stats.nodes > 0);

    // The log: read through `perform`, then saved with `\n` line ends.
    let effects = analysis(&mut state, AnalysisMessage::OpenLog(run));
    settle(&mut state, effects).await;
    let log = &state.analysis_view(run).expect("view").log;
    assert!(log.iter().any(|e| e.text.contains("uciok")), "{log:?}");
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("engine.log");
    let effects = analysis(&mut state, AnalysisMessage::SaveLog(run, path.clone()));
    settle(&mut state, effects).await;
    assert_eq!(analysis_notice(&state), Some(&AnalysisNotice::LogSaved));
    let text = std::fs::read_to_string(&path).expect("written");
    assert!(text.contains("< uciok\n"), "{text}");
    assert!(!text.contains('\r'));
    // A run that is gone cannot save: the notice is the file error.
    let effects = analysis(
        &mut state,
        AnalysisMessage::SaveLog(RunId(99), dir.path().join("none.log")),
    );
    settle(&mut state, effects).await;
    assert!(matches!(state.notice, Some(AppNotice::File(_))));
    state.engine_hub.shutdown_all(Duration::from_secs(2)).await;
}
