//! Tests of the evaluation bar, the arrows and the traffic lights in the
//! state: synthetic reports of an engine fed through `update` (spec M4-5).

use mychess_core::{Color, Game, Move, Position, Square};
use mychess_engine::{
    Bound, Capabilities, EngineEvent, EngineIdentity, Score, SearchId, SearchInfo,
};

use super::{AppState, update};
use crate::analysis::AnalysisMessage;
use crate::effect::Effect;
use crate::engines::{EngineDraft, EngineRunState, RunId};
use crate::evalbar::{
    ArrowKind, ArrowMode, ArrowPrefs, EvalBarMessage, EvalBarPrefs, EvalSource, EvalValue,
    SequenceSide, TerminalResult,
};
use crate::message::Message;
use crate::prefs::PrefChange;
use crate::session::{GameMessage, Nav, PlayOptions};
use crate::shortcuts::Action;

fn tick(state: &mut AppState) -> Vec<Effect> {
    update(state, Message::Game(GameMessage::Navigate(Nav::Start)))
}

fn evalbar(state: &mut AppState, message: EvalBarMessage) -> Vec<Effect> {
    update(state, Message::EvalBar(message))
}

fn identity() -> EngineIdentity {
    EngineIdentity {
        name: "Fake".to_owned(),
        author: None,
        options: Vec::new(),
        capabilities: Capabilities {
            multipv: Some(1..=5),
            search_moves: true,
            ..Capabilities::default()
        },
    }
}

/// A state with one engine that is ready and analysing.
fn analysing() -> (AppState, RunId) {
    let mut state = AppState::default();
    let id = state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("add");
    let run = state.engine_runs.begin(id, true);
    let r = state.engine_runs.get_mut(run).expect("run");
    r.state = EngineRunState::Running;
    r.identity = Some(Box::new(identity()));
    tick(&mut state);
    (state, run)
}

fn event(state: &mut AppState, run: RunId, event: EngineEvent) {
    update(
        state,
        Message::Engine(crate::engines::EngineMessage::Event {
            run,
            event: Box::new(event),
        }),
    );
}

fn started(state: &mut AppState, run: RunId, search: u64) {
    event(state, run, EngineEvent::SearchStarted(SearchId(search)));
}

fn mv(position: &Position, uci: &str) -> Move {
    position.parse_uci(uci).expect("uci")
}

fn report(
    position: &Position,
    search: u64,
    multipv: u8,
    depth: u32,
    score: Score,
    pv: &[&str],
) -> EngineEvent {
    let mut pos = position.clone();
    let pv = pv
        .iter()
        .map(|u| {
            let m = pos.parse_uci(u).expect("uci");
            pos.play(m).expect("play");
            m
        })
        .collect();
    EngineEvent::Info(SearchInfo {
        search: SearchId(search),
        multipv,
        depth: Some(depth),
        score: Some((score, Bound::Exact)),
        pv,
        ..SearchInfo::default()
    })
}

fn show_bar(state: &mut AppState) {
    evalbar(state, EvalBarMessage::ToggleBar);
}

fn comment_at_cursor(state: &mut AppState, text: &str) {
    let node = state.session.cursor();
    update(
        state,
        Message::Game(GameMessage::SetComment {
            node,
            text: text.to_owned(),
        }),
    );
}

fn play(state: &mut AppState, uci: &str) {
    let position = state.session.position().clone();
    update(
        state,
        Message::Game(GameMessage::PlayMove {
            mv: mv(&position, uci),
            options: PlayOptions::default(),
        }),
    );
}

// ---- the bar ----

#[test]
fn everything_is_off_by_default() {
    let (mut state, run) = analysing();
    started(&mut state, run, 1);
    event(
        &mut state,
        run,
        report(&Position::new(), 1, 1, 9, Score::Cp(30), &["e2e4"]),
    );
    assert_eq!(state.eval_bar_view().fill, None);
    assert!(state.engine_arrows().is_empty() && state.traffic_lights().is_empty());
    assert_eq!(state.arrows_engine(), None);
}

#[test]
fn toggling_the_bar_changes_and_saves_the_preference() {
    let mut state = AppState::default();
    assert!(!state.prefs.engine_view.eval_bar.show);
    let effects = update(&mut state, Message::Action(Action::ToggleEvalBar));
    assert!(state.prefs.engine_view.eval_bar.show);
    assert!(state.prefs_dirty || !effects.is_empty() || state.flow.prefs_path.is_none());
    evalbar(&mut state, EvalBarMessage::ToggleBar);
    assert!(!state.prefs.engine_view.eval_bar.show);
    evalbar(&mut state, EvalBarMessage::ToggleTrafficLights);
    assert!(state.prefs.engine_view.traffic_lights.show);
    update(&mut state, Message::Action(Action::ToggleArrowNumbers));
    assert!(!state.prefs.engine_view.arrows.number_lines);
}

#[test]
fn without_an_engine_the_bar_follows_the_comment_and_empties() {
    let mut state = AppState::default();
    show_bar(&mut state);
    assert_eq!(state.eval_bar_view().fill, None, "no score yet");
    comment_at_cursor(&mut state, "[%eval 1.5]");
    let bar = state.eval_bar_view();
    assert_eq!(bar.source, EvalSource::Comment);
    assert_eq!(bar.value, Some(EvalValue::Pawns(1.5)));
    assert!((bar.fill.expect("fill") - 0.65).abs() < 1e-6);
    assert_eq!(bar.label.as_deref(), Some("+1.50"));
    play(&mut state, "e2e4");
    assert_eq!(state.eval_bar_view().fill, None, "a move without score");
    comment_at_cursor(&mut state, "+0.35 a good move");
    assert_eq!(state.eval_bar_view().value, Some(EvalValue::Pawns(0.35)));
    comment_at_cursor(&mut state, "[%eval #3]");
    assert!((state.eval_bar_view().fill.expect("fill") - 1.0).abs() < 1e-6);
    assert_eq!(state.eval_bar_view().label.as_deref(), Some("#"));
}

#[test]
fn the_bar_follows_the_flip_scale_and_log_options() {
    let mut state = AppState::default();
    show_bar(&mut state);
    comment_at_cursor(&mut state, "[%eval 1.0]");
    update(&mut state, Message::Action(Action::FlipBoard));
    assert!(state.eval_bar_view().flipped);
    update(
        &mut state,
        Message::Prefs(crate::prefs::PrefsMessage::Set(PrefChange::EvalBar(
            EvalBarPrefs {
                show: true,
                scale: 1,
                ..EvalBarPrefs::default()
            },
        ))),
    );
    assert!((state.eval_bar_view().fill.expect("fill") - 1.0).abs() < 1e-6);
}

#[test]
fn the_engine_feeds_the_bar_before_the_comment() {
    let (mut state, run) = analysing();
    show_bar(&mut state);
    comment_at_cursor(&mut state, "[%eval -3.0]");
    // The engine has said nothing: the comment.
    assert_eq!(state.eval_bar_view().source, EvalSource::Comment);
    started(&mut state, run, 1);
    event(
        &mut state,
        run,
        report(&Position::new(), 1, 1, 9, Score::Cp(50), &["e2e4"]),
    );
    let bar = state.eval_bar_view();
    assert_eq!(bar.source, EvalSource::Engine(run));
    assert_eq!(bar.value, Some(EvalValue::Pawns(0.5)));
    assert!(!bar.stale);
    // A newer report moves the bar.
    event(
        &mut state,
        run,
        report(&Position::new(), 1, 1, 10, Score::Mate(3), &["e2e4"]),
    );
    assert_eq!(state.eval_bar_view().value, Some(EvalValue::Mate(3)));
    assert_eq!(state.eval_bar_view().label.as_deref(), Some("+M3"));
}

#[test]
fn a_new_position_fades_the_value_until_the_engine_reports_again() {
    let (mut state, run) = analysing();
    show_bar(&mut state);
    started(&mut state, run, 1);
    let start = Position::new();
    event(
        &mut state,
        run,
        report(&start, 1, 1, 9, Score::Cp(50), &["e2e4"]),
    );
    play(&mut state, "e2e4");
    assert!(state.eval_bar_view().stale, "lines of the old position");
    assert_eq!(state.eval_bar_view().value, Some(EvalValue::Pawns(0.5)));
    started(&mut state, run, 2);
    let stale = state.eval_bar_view();
    assert!(stale.stale, "lines were cleared, the old value stays faded");
    assert_eq!(stale.source, EvalSource::Engine(run));
    let after = state.session.position().clone();
    event(
        &mut state,
        run,
        report(&after, 2, 1, 5, Score::Cp(-10), &["e7e5"]),
    );
    let bar = state.eval_bar_view();
    assert!(!bar.stale);
    assert_eq!(bar.value, Some(EvalValue::Pawns(-0.1)));
}

#[test]
fn stopping_or_failing_the_engine_goes_back_to_the_comment() {
    let (mut state, run) = analysing();
    show_bar(&mut state);
    comment_at_cursor(&mut state, "[%eval 2.0]");
    started(&mut state, run, 1);
    event(
        &mut state,
        run,
        report(&Position::new(), 1, 1, 9, Score::Cp(-50), &["e2e4"]),
    );
    assert_eq!(state.eval_bar_view().source, EvalSource::Engine(run));
    update(&mut state, Message::Analysis(AnalysisMessage::Toggle(run)));
    assert_eq!(state.eval_bar_view().source, EvalSource::Comment);
    // Back on, then dead: the old score never comes back.
    update(&mut state, Message::Analysis(AnalysisMessage::Toggle(run)));
    started(&mut state, run, 2);
    event(
        &mut state,
        run,
        report(&Position::new(), 2, 1, 9, Score::Cp(-70), &["e2e4"]),
    );
    state.engine_runs.get_mut(run).expect("run").state = EngineRunState::Stopping;
    tick(&mut state);
    let bar = state.eval_bar_view();
    assert_eq!(bar.source, EvalSource::Comment);
    assert_eq!(bar.value, Some(EvalValue::Pawns(2.0)));
}

#[test]
fn a_position_without_moves_shows_the_result() {
    let (mut state, run) = analysing();
    show_bar(&mut state);
    started(&mut state, run, 1);
    let mate = Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
        .expect("fen");
    state.session.replace_game(Game::from_position(mate));
    tick(&mut state);
    let bar = state.eval_bar_view();
    assert_eq!(
        bar.value,
        Some(EvalValue::Terminal(TerminalResult::BlackWins))
    );
    assert_eq!(bar.fill, Some(0.0));
    assert_eq!(bar.label.as_deref(), Some("M0"));
    let stalemate = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").expect("fen");
    state.session.replace_game(Game::from_position(stalemate));
    tick(&mut state);
    started(&mut state, run, 2);
    tick(&mut state);
    assert_eq!(
        state.eval_bar_view().value,
        Some(EvalValue::Terminal(TerminalResult::Draw))
    );
    assert_eq!(state.eval_bar_view().label.as_deref(), Some("="));
}

#[test]
fn a_locked_analysis_left_behind_does_not_feed_the_bar() {
    let (mut state, run) = analysing();
    show_bar(&mut state);
    comment_at_cursor(&mut state, "[%eval 1.0]");
    started(&mut state, run, 1);
    event(
        &mut state,
        run,
        report(&Position::new(), 1, 1, 9, Score::Cp(-300), &["e2e4"]),
    );
    update(
        &mut state,
        Message::Analysis(AnalysisMessage::ToggleLock(run)),
    );
    assert_eq!(state.eval_bar_view().value, Some(EvalValue::Pawns(-3.0)));
    play(&mut state, "e2e4");
    assert_eq!(state.eval_bar_view().source, EvalSource::None);
    assert_eq!(state.eval_bar_view().fill, None);
}

// ---- arrows ----

fn three_lines() -> (AppState, RunId, Position) {
    let (mut state, run) = analysing();
    started(&mut state, run, 1);
    update(
        &mut state,
        Message::Analysis(AnalysisMessage::SetMultiPv(run, 3)),
    );
    let start = Position::new();
    for (n, uci, cp) in [(1, "e2e4", 30), (2, "d2d4", 20), (3, "g1f3", -50)] {
        event(
            &mut state,
            run,
            report(&start, 1, n, 9, Score::Cp(cp), &[uci, "e7e5"]),
        );
    }
    (state, run, start)
}

#[test]
fn marking_an_engine_draws_its_arrows_and_unmarking_removes_them() {
    let (mut state, run, _) = three_lines();
    assert!(state.engine_arrows().is_empty());
    evalbar(&mut state, EvalBarMessage::ToggleArrows(run));
    assert_eq!(state.arrows_engine(), Some(run));
    assert!(state.prefs.engine_view.arrows.enabled);
    let arrows = state.engine_arrows();
    assert_eq!(arrows.len(), 3);
    assert_eq!(arrows[0].kind, ArrowKind::Best);
    assert_eq!(
        arrows.iter().map(|a| a.label).collect::<Vec<_>>(),
        [None, Some(2), Some(3)]
    );
    assert!(arrows.iter().all(|a| a.side == Color::White));
    evalbar(&mut state, EvalBarMessage::ToggleArrows(run));
    assert_eq!(state.arrows_engine(), None);
    assert!(state.engine_arrows().is_empty());
    assert!(!state.prefs.engine_view.arrows.enabled);
}

#[test]
fn enabled_arrows_fall_back_to_the_main_engine() {
    let (mut state, run, _) = three_lines();
    state.prefs.engine_view.arrows.enabled = true;
    tick(&mut state);
    assert_eq!(state.arrows_engine(), Some(run));
    assert_eq!(state.engine_arrows().len(), 3);
}

#[test]
fn arrows_vanish_when_the_cursor_leaves_their_position() {
    let (mut state, run, _) = three_lines();
    evalbar(&mut state, EvalBarMessage::ToggleArrows(run));
    play(&mut state, "e2e4");
    assert!(
        state.engine_arrows().is_empty(),
        "lines of the old position"
    );
    // A locked analysis keeps its arrows only while the cursor is on it.
    update(&mut state, Message::Game(GameMessage::Navigate(Nav::Start)));
    assert_eq!(state.engine_arrows().len(), 3);
    update(
        &mut state,
        Message::Analysis(AnalysisMessage::ToggleLock(run)),
    );
    assert_eq!(state.engine_arrows().len(), 3);
    update(&mut state, Message::Game(GameMessage::Navigate(Nav::End)));
    assert!(state.engine_arrows().is_empty());
}

#[test]
fn the_sequence_mode_draws_the_main_line_with_submodes() {
    let (mut state, run, _) = three_lines();
    evalbar(&mut state, EvalBarMessage::ToggleArrows(run));
    evalbar(&mut state, EvalBarMessage::CycleArrowMode);
    assert_eq!(state.prefs.engine_view.arrows.mode, ArrowMode::Sequence);
    assert_eq!(state.engine_arrows().len(), 2, "the PV has two moves");
    assert!(matches!(
        state.engine_arrows()[1].kind,
        ArrowKind::Sequence(1)
    ));
    evalbar(&mut state, EvalBarMessage::CycleSequenceSide);
    assert_eq!(
        state.prefs.engine_view.arrows.sequence_side,
        SequenceSide::White
    );
    assert_eq!(state.engine_arrows().len(), 1);
}

#[test]
fn arrows_are_not_marked_for_an_engine_that_is_not_running() {
    let (mut state, run, _) = three_lines();
    evalbar(&mut state, EvalBarMessage::ToggleArrows(RunId(999)));
    assert_eq!(state.arrows_engine(), None);
    state.engine_runs.get_mut(run).expect("run").state = EngineRunState::Stopping;
    evalbar(&mut state, EvalBarMessage::ToggleArrows(run));
    assert_eq!(state.arrows_engine(), None);
}

/// The click on the arrow of `line` as drawn now (a1-a1 when none).
fn arrow_click(state: &AppState, run: RunId, line: u8) -> EvalBarMessage {
    let a1: Square = "a1".parse().expect("square");
    let (from, to) = state
        .engine_arrows()
        .iter()
        .find(|a| a.rank == line)
        .map_or((a1, a1), |a| (a.from, a.to));
    EvalBarMessage::PlayArrow {
        run,
        line,
        from,
        to,
    }
}

#[test]
fn a_click_on_an_arrow_plays_the_move_of_that_line() {
    let (mut state, run, _) = three_lines();
    evalbar(&mut state, EvalBarMessage::ToggleArrows(run));
    let click = arrow_click(&state, run, 2);
    evalbar(&mut state, click);
    let played = state.session.game().mv(state.session.cursor()).ok();
    assert_eq!(played, Some(mv(&Position::new(), "d2d4")));
}

#[test]
fn clicking_is_refused_when_the_preference_is_off_or_the_line_is_missing() {
    let (mut state, run, _) = three_lines();
    evalbar(&mut state, EvalBarMessage::ToggleArrows(run));
    let click = arrow_click(&state, run, 9);
    evalbar(&mut state, click);
    assert_eq!(state.session.cursor(), state.session.game().root());
    let click = arrow_click(&state, run, 1);
    state.prefs.engine_view.arrows = ArrowPrefs {
        click_to_play: false,
        enabled: true,
        ..ArrowPrefs::default()
    };
    evalbar(&mut state, click);
    assert_eq!(state.session.cursor(), state.session.game().root());
}

// ---- traffic lights ----

#[test]
fn the_lights_show_one_strip_per_line_in_order() {
    let (mut state, run, _) = three_lines();
    assert!(state.traffic_lights().is_empty());
    evalbar(&mut state, EvalBarMessage::ToggleTrafficLights);
    let lights = state.traffic_lights();
    assert_eq!(lights.iter().map(|l| l.rank).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(lights[0].color.hex(), "#ebf800");
    assert_eq!(lights[2].color.hex(), "#f8e100");
    // The cursor leaves the position: nothing stale is drawn.
    play(&mut state, "e2e4");
    assert!(state.traffic_lights().is_empty());
    let _ = run;
}

#[test]
fn hiding_everything_clears_the_views() {
    let (mut state, run, _) = three_lines();
    show_bar(&mut state);
    evalbar(&mut state, EvalBarMessage::ToggleTrafficLights);
    evalbar(&mut state, EvalBarMessage::ToggleArrows(run));
    assert!(state.eval_bar_view().fill.is_some() && !state.traffic_lights().is_empty());
    show_bar(&mut state);
    evalbar(&mut state, EvalBarMessage::ToggleTrafficLights);
    evalbar(&mut state, EvalBarMessage::ToggleArrows(run));
    assert!(state.eval_bar_view().fill.is_none());
    assert!(state.traffic_lights().is_empty() && state.engine_arrows().is_empty());
}

#[test]
fn hostile_scores_do_not_panic() {
    let (mut state, run) = analysing();
    show_bar(&mut state);
    evalbar(&mut state, EvalBarMessage::ToggleTrafficLights);
    evalbar(&mut state, EvalBarMessage::ToggleArrows(run));
    started(&mut state, run, 1);
    let start = Position::new();
    for score in [
        Score::Cp(i32::MAX),
        Score::Cp(i32::MIN),
        Score::Cp(32_700),
        Score::Mate(i32::MAX),
        Score::Mate(i32::MIN),
        Score::Mate(0),
    ] {
        event(&mut state, run, report(&start, 1, 1, 9, score, &["e2e4"]));
        let fill = state.eval_bar_view().fill.expect("fill");
        assert!((0.0..=1.0).contains(&fill), "{score:?} gave {fill}");
    }
}

#[test]
fn no_arrows_while_an_annotation_runs_and_the_click_is_refused() {
    let (mut state, run, _) = three_lines();
    evalbar(&mut state, EvalBarMessage::ToggleArrows(run));
    assert_eq!(state.engine_arrows().len(), 3);
    let click = arrow_click(&state, run, 1);
    update(
        &mut state,
        Message::from(crate::annotate::AnnotateMessage::Start),
    );
    assert!(state.annotation_running());
    assert_eq!(state.arrows_engine(), None);
    assert!(state.engine_arrows().is_empty());
    evalbar(&mut state, click);
    assert_eq!(state.session.cursor(), state.session.game().root());
}
