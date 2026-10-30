//! Tests of the analysis of the positions of an EPD file: a fake engine
//! answers the searches through synthetic events fed to `update` (spec
//! M7-5, rules 16-22, criteria 13-15).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mychess_core::{Game, Move, Position, SanMode};
use mychess_engine::{BestMove, EngineCommand, EngineEvent, Protocol, SearchId};
use mychess_pgn::epd::{EpdBook, EpdIndex, EpdOperation};

use super::annotate_tests::{Reply, answer, commands, event, last_search, send, setup};
use super::{AppState, update};
use crate::effect::Effect;
use crate::engines::{EngineDraft, EngineMessage, RunId};
use crate::epd_window::{
    AnnotateConfig, AnnotateMode, EpdEffect, EpdFile, EpdFileId, EpdMessage, EpdNotice, LoadReport,
    RunOutcome, RunScope, is_epd_tag,
};
use crate::flow::AppNotice;
use crate::message::Message;

fn mv(position: &Position, san: &str) -> Move {
    position
        .parse_san(san, SanMode::Tolerant)
        .expect("legal")
        .mv
}

fn five() -> Vec<Position> {
    let start = Position::new();
    let mut list = vec![start.clone()];
    for san in ["e4", "d4", "c4", "Nf3"] {
        list.push(start.after(mv(&start, san)).expect("after"));
    }
    list
}

/// A state with a running fake engine (analysing the board) and an EPD
/// file of five records open in the window.
fn fixture(ops: [&str; 5]) -> (AppState, RunId, EpdFileId) {
    let (mut state, run) = setup(Game::new());
    let mut book = EpdBook::new();
    for (position, text) in five().into_iter().zip(ops) {
        let mut operations = Vec::new();
        for part in text.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let (code, rest) = part.split_once(' ').unwrap_or((part, ""));
            operations.push(EpdOperation::new(code, rest));
        }
        book.insert(position, operations);
    }
    let rows: Vec<EpdIndex> = book.iter_indexed().map(|(i, _)| i).collect();
    let id = state.epd.new_id();
    state.epd.files.push(EpdFile::new(
        id,
        PathBuf::from("/x/five.epd"),
        Arc::new(book),
        false,
        Arc::new(rows),
        LoadReport::default(),
    ));
    state.epd.active = Some(id);
    // What the first message after the opening of a file does.
    state.refresh_epd(&mut Vec::new());
    (state, run, id)
}

fn config(mode: AnnotateMode, scope: RunScope) -> AnnotateConfig {
    AnnotateConfig {
        engine: None,
        seconds: 3,
        mode,
        scope,
    }
}

fn start(state: &mut AppState, mode: AnnotateMode, scope: RunScope) -> Vec<Effect> {
    send(state, EpdMessage::Annotate(config(mode, scope)))
}

fn texts(state: &AppState, id: EpdFileId) -> Vec<String> {
    let file = state.epd.file(id).expect("file");
    five()
        .iter()
        .map(|p| file.book.get(p).expect("record").operations_text())
        .collect()
}

fn notice(state: &AppState) -> Option<EpdNotice> {
    match &state.notice {
        Some(AppNotice::Epd(n)) => Some(n.clone()),
        _ => None,
    }
}

/// The engine always plays its first legal move and scores `cp`.
fn reply_for(effects: &[Effect], cp: i32) -> Reply {
    let request = last_search(effects).expect("a search");
    let position = request.position.start.clone();
    let first = position.legal_moves()[0];
    Reply {
        cp,
        pv: vec![first],
        second: None,
        mate: None,
    }
}

/// Answers until the run ends (at most `max` searches).
fn drive(state: &mut AppState, run: RunId, mut effects: Vec<Effect>, max: u64) -> Vec<Position> {
    let mut seen = Vec::new();
    for n in 0..max {
        if !state.epd_running() {
            break;
        }
        let Some(request) = last_search(&effects) else {
            break;
        };
        seen.push(request.position.start.clone());
        let reply = reply_for(&effects, 25);
        let Some((after, _)) = answer(state, run, &effects, 100 + n, &reply) else {
            break;
        };
        effects = after;
    }
    seen
}

/// The searches of the effects that belong to an analysis of positions (the
/// board's own analysis is not one).
fn epd_searches(effects: &[Effect]) -> Vec<mychess_engine::SearchRequest> {
    commands(effects)
        .into_iter()
        .filter_map(|c| match c {
            EngineCommand::Search(request) if is_epd_tag(request.tag) => Some(request),
            _ => None,
        })
        .collect()
}

fn analyzing(state: &AppState, run: RunId) -> bool {
    state.engine_runs.get(run).expect("run").analyzing
}

#[test]
fn annotate_walks_the_five_positions_writes_and_gives_the_engine_back() {
    let (mut state, run, id) = fixture(["bm e4; ce 99; pv d4", "am e5", "id \"x\"", "ce 25", ""]);
    let before = texts(&state, id);
    let effects = start(&mut state, AnnotateMode::Annotate, RunScope::All);
    assert!(state.epd_running());
    assert!(!analyzing(&state, run), "no board analysis meanwhile");
    let request = last_search(&effects).expect("first search");
    assert_eq!(request.limits.movetime, Some(Duration::from_secs(3)));
    assert!(is_epd_tag(request.tag));
    let seen = drive(&mut state, run, effects, 20);
    assert_eq!(seen, five(), "the order of the file, one search each");
    assert!(!state.epd_running());
    assert!(analyzing(&state, run), "the analysis of the board is back");
    assert_eq!(
        notice(&state),
        Some(EpdNotice::RunDone {
            done: 5,
            found: 0,
            counted: 0,
            illegal: 0
        })
    );
    let after = texts(&state, id);
    // The old ce and pv are gone, the new ones are there; the rest stays.
    assert_eq!(after[0].lines().next(), Some("bm e4"));
    assert!(after[0].contains("ce 25") && !after[0].contains("ce 99"));
    assert!(after[0].contains("acd 13") && after[0].contains("dm 0"));
    assert!(!after[0].contains("pv d4"));
    assert!(after[2].starts_with("id \"x\"\n"));
    assert!(after[4].contains("pv "));
    let file = state.epd.file(id).expect("file");
    assert!(file.dirty);
    assert!(file.undo.is_some());
    // Undo restores the five records, byte for byte, and the clean mark.
    send(&mut state, EpdMessage::UndoRun);
    assert_eq!(texts(&state, id), before);
    assert!(!state.epd.file(id).expect("file").dirty);
    assert_eq!(notice(&state), Some(EpdNotice::RunUndone));
    send(&mut state, EpdMessage::UndoRun);
    assert_eq!(notice(&state), Some(EpdNotice::NoRunToUndo));
}

#[test]
fn undo_after_a_dirty_file_keeps_it_dirty() {
    let (mut state, run, id) = fixture(["", "", "", "", ""]);
    send(&mut state, EpdMessage::Edit("id \"mine\"".into()));
    send(&mut state, EpdMessage::Commit);
    assert!(state.epd.file(id).expect("file").dirty);
    let effects = start(&mut state, AnnotateMode::Annotate, RunScope::All);
    drive(&mut state, run, effects, 20);
    send(&mut state, EpdMessage::UndoRun);
    let file = state.epd.file(id).expect("file");
    assert!(file.dirty, "the edit by hand is still unsaved");
    assert_eq!(
        file.book.get(&five()[0]).expect("record").operations_text(),
        "id \"mine\"\n"
    );
}

#[test]
fn count_compares_the_first_move_with_bm_and_am_and_writes_nothing() {
    let first_of = |position: &Position| {
        let legal = position.legal_moves();
        (
            position.san(legal[0]).expect("san"),
            position.san(legal[legal.len() - 1]).expect("san"),
        )
    };
    let p = five();
    let (found_bm, _) = first_of(&p[0]);
    let (_, avoided) = first_of(&p[1]);
    let (state_ops, run, id) = {
        let ops = [
            format!("bm {found_bm}"),
            format!("am {avoided}"),
            "bm Qh5".to_owned(),
            String::new(),
            String::new(),
        ];
        fixture([&ops[0], &ops[1], &ops[2], &ops[3], &ops[4]])
    };
    let mut state = state_ops;
    let before = texts(&state, id);
    let effects = start(&mut state, AnnotateMode::Count, RunScope::All);
    drive(&mut state, run, effects, 20);
    assert_eq!(texts(&state, id), before, "counting writes nothing");
    assert!(!state.epd.file(id).expect("file").dirty);
    // Record 0: the engine plays its first legal move = bm: found. Record
    // 1: it avoids the last legal move: found. Record 2: an illegal bm is
    // no result. The others have neither.
    assert_eq!(
        notice(&state),
        Some(EpdNotice::RunDone {
            done: 5,
            found: 2,
            counted: 2,
            illegal: 1
        })
    );
}

#[test]
fn both_writes_the_engine_opcode_with_the_verdict() {
    let start_position = Position::new();
    let first_san = start_position
        .san(start_position.legal_moves()[0])
        .expect("san");
    let (mut state, run, id) = fixture([&format!("bm {first_san}"), "", "", "", ""]);
    let effects = start(&mut state, AnnotateMode::Both, RunScope::All);
    drive(&mut state, run, effects, 20);
    let first = texts(&state, id).remove(0);
    assert!(first.contains("Fake success"), "{first}");
    let second = texts(&state, id).remove(1);
    assert!(second.contains("Fake no-result"), "{second}");
}

#[test]
fn stopping_keeps_what_was_written_and_reports_where_it_stopped() {
    let (mut state, run, id) = fixture(["", "", "", "", ""]);
    let effects = start(&mut state, AnnotateMode::Annotate, RunScope::All);
    let reply = reply_for(&effects, 10);
    let (second, _) = answer(&mut state, run, &effects, 100, &reply).expect("first answered");
    // The second search starts; the user stops.
    let request = last_search(&second).expect("second search");
    event(
        &mut state,
        run,
        EngineEvent::SearchTagged {
            search: SearchId(101),
            tag: request.tag,
        },
    );
    let stop = send(&mut state, EpdMessage::StopAnnotate);
    assert!(commands(&stop).contains(&EngineCommand::Stop));
    assert!(state.epd_running(), "waits for the best move");
    event(
        &mut state,
        run,
        EngineEvent::BestMove(BestMove {
            search: SearchId(101),
            mv: None,
            ponder: None,
        }),
    );
    assert!(!state.epd_running());
    assert_eq!(
        notice(&state),
        Some(EpdNotice::RunInterrupted { done: 1, total: 5 })
    );
    let after = texts(&state, id);
    assert!(after[0].contains("ce 10"));
    assert_eq!(after[1], "");
    let file = state.epd.file(id).expect("file");
    assert!(matches!(file.last_run, Some((RunOutcome::Interrupted, _))));
    assert!(analyzing(&state, run));
}

#[test]
fn what_a_stopped_search_reports_late_is_not_for_the_analysis_window() {
    let (mut state, run, _id) = fixture(["", "", "", "", ""]);
    let effects = start(&mut state, AnnotateMode::Annotate, RunScope::All);
    let request = last_search(&effects).expect("search");
    event(
        &mut state,
        run,
        EngineEvent::SearchTagged {
            search: SearchId(7),
            tag: request.tag,
        },
    );
    // The engine goes away from under the run: it ends, the search is
    // remembered as stale.
    state.epd_run_end(RunOutcome::Interrupted, true, &mut Vec::new());
    assert!(!state.epd_running());
    assert_eq!(state.epd.stale.len(), 1);
    let handled = state.epd_event(
        run,
        &EngineEvent::BestMove(BestMove {
            search: SearchId(7),
            mv: None,
            ponder: None,
        }),
        &mut Vec::new(),
    );
    assert!(handled, "dropped");
    assert!(state.epd.stale.is_empty(), "forgotten after its best move");
}

#[test]
fn the_watchdog_gives_up_on_a_silent_engine() {
    let (mut state, run, id) = fixture(["", "", "", "", ""]);
    let effects = start(&mut state, AnnotateMode::Annotate, RunScope::All);
    let tick = |effects: &[Effect]| {
        effects.iter().rev().find_map(|e| match e {
            Effect::Epd(EpdEffect::Tick {
                run,
                generation,
                delay,
            }) => Some((*run, *generation, *delay)),
            _ => None,
        })
    };
    let (tick_run, generation, delay) = tick(&effects).expect("a timer");
    assert_eq!(tick_run, run);
    assert!(delay > Duration::from_secs(3), "movetime plus grace");
    // A stale timer does nothing.
    send(
        &mut state,
        EpdMessage::Tick {
            run,
            generation: generation + 1,
        },
    );
    assert!(state.epd_running());
    // The engine said nothing: the run fails and the engine comes back.
    send(&mut state, EpdMessage::Tick { run, generation });
    assert!(!state.epd_running());
    assert_eq!(notice(&state), Some(EpdNotice::EngineFailed));
    assert!(analyzing(&state, run));
    assert!(!state.epd.file(id).expect("file").dirty);
}

#[test]
fn a_dying_engine_ends_the_run_and_keeps_what_was_written() {
    let (mut state, run, id) = fixture(["", "", "", "", ""]);
    let effects = start(&mut state, AnnotateMode::Annotate, RunScope::All);
    let reply = reply_for(&effects, 10);
    answer(&mut state, run, &effects, 100, &reply).expect("answered");
    send(&mut state, EngineMessage::Stopped(run));
    assert!(!state.epd_running());
    assert_eq!(notice(&state), Some(EpdNotice::EngineFailed));
    assert!(texts(&state, id)[0].contains("ce 10"));
    assert!(state.epd.file(id).expect("file").dirty);
}

#[test]
fn an_xboard_engine_is_refused() {
    let (mut state, _run, _id) = fixture(["", "", "", "", ""]);
    let mut draft = EngineDraft::new("Old", "/old");
    draft.protocol = Protocol::XBoard;
    let old = state.engines.add(draft).expect("add");
    let mut cfg = config(AnnotateMode::Annotate, RunScope::All);
    cfg.engine = Some(old);
    let effects = send(&mut state, EpdMessage::Annotate(cfg));
    assert!(epd_searches(&effects).is_empty());
    assert_eq!(notice(&state), Some(EpdNotice::UciOnly));
    assert!(!state.epd_running());
}

#[test]
fn without_an_engine_the_run_says_so() {
    let mut state = AppState::default();
    let id = state.epd.new_id();
    let mut book = EpdBook::new();
    book.insert(Position::new(), Vec::new());
    state.epd.files.push(EpdFile::new(
        id,
        PathBuf::from("/x/a.epd"),
        Arc::new(book),
        false,
        Arc::new(Vec::new()),
        LoadReport::default(),
    ));
    state.epd.active = Some(id);
    send(
        &mut state,
        EpdMessage::Annotate(config(AnnotateMode::Annotate, RunScope::All)),
    );
    assert_eq!(notice(&state), Some(EpdNotice::NoEngine));
}

#[test]
fn a_running_analysis_excludes_the_other_uses_of_the_engine_and_the_edits() {
    let (mut state, run, id) = fixture(["", "", "", "", ""]);
    start(&mut state, AnnotateMode::Annotate, RunScope::All);
    // The annotation of games and the analysis window stay out.
    send(&mut state, crate::annotate::AnnotateMessage::Start);
    assert_eq!(notice(&state), Some(EpdNotice::EngineBusy));
    assert!(!state.annotation_running());
    send(&mut state, crate::analysis::AnalysisMessage::Toggle(run));
    assert!(
        !analyzing(&state, run),
        "the board does not take the engine"
    );
    // A second analysis, an edit and a save wait.
    send(
        &mut state,
        EpdMessage::Annotate(config(AnnotateMode::Count, RunScope::All)),
    );
    assert_eq!(notice(&state), Some(EpdNotice::RunActive));
    send(&mut state, EpdMessage::Edit("bm e4".into()));
    assert_eq!(notice(&state), Some(EpdNotice::RunActive));
    send(&mut state, EpdMessage::Save);
    assert_eq!(notice(&state), Some(EpdNotice::RunActive));
    send(&mut state, EpdMessage::StripOpcode("ce".into()));
    assert_eq!(notice(&state), Some(EpdNotice::RunActive));
    assert!(!state.epd.file(id).expect("file").dirty);
}

#[test]
fn the_analysis_is_help_and_is_dropped_during_an_online_game() {
    let (mut state, _run, _id) = fixture(["", "", "", "", ""]);
    state.online.marker = Some(crate::online::MarkerInfo {
        game_id: "g".into(),
        date: "2026-10-29".into(),
    });
    assert!(state.assist_locked());
    let effects = start(&mut state, AnnotateMode::Annotate, RunScope::All);
    assert!(epd_searches(&effects).is_empty());
    assert!(!state.epd_running());
    assert!(matches!(
        state.notice,
        Some(AppNotice::Online(
            crate::online::OnlineNotice::AssistanceDisabled
        ))
    ));
    // Editing and searching are not help.
    send(&mut state, EpdMessage::Edit("bm e4".into()));
    assert!(state.epd_view().tabs[0].editor_draft);
}

#[test]
fn the_scope_from_the_selected_record_starts_there() {
    let (mut state, run, _id) = fixture(["", "", "", "", ""]);
    let target = state.epd.files[0].rows[2];
    send(&mut state, EpdMessage::Select(target));
    // Selecting does not move the board, which the sync would undo while no
    // run is active; start now.
    let effects = start(&mut state, AnnotateMode::Annotate, RunScope::FromSelected);
    let seen = drive(&mut state, run, effects, 20);
    assert_eq!(seen, five()[2..].to_vec());
}

#[test]
fn the_scope_without_ce_skips_the_records_that_have_it() {
    let (mut state, run, _id) = fixture(["ce 1", "ce 2", "", "ce 4", ""]);
    let effects = start(&mut state, AnnotateMode::Annotate, RunScope::WithoutCe);
    let seen = drive(&mut state, run, effects, 20);
    assert_eq!(seen, vec![five()[2].clone(), five()[4].clone()]);
}

#[test]
fn nothing_to_analyze_leaves_the_engine_alone() {
    let (mut state, run, _id) = fixture(["ce 1", "ce 2", "ce 3", "ce 4", "ce 5"]);
    let effects = start(&mut state, AnnotateMode::Annotate, RunScope::WithoutCe);
    assert!(epd_searches(&effects).is_empty());
    assert_eq!(notice(&state), Some(EpdNotice::NothingToAnalyze));
    assert!(!state.epd_running());
    assert!(analyzing(&state, run));
}

#[test]
fn quit_stops_the_run_and_gives_the_engine_back() {
    let (mut state, run, _id) = fixture(["", "", "", "", ""]);
    let effects = start(&mut state, AnnotateMode::Annotate, RunScope::All);
    let request = last_search(&effects).expect("search");
    event(
        &mut state,
        run,
        EngineEvent::SearchTagged {
            search: SearchId(5),
            tag: request.tag,
        },
    );
    let out = update(&mut state, Message::Quit);
    assert!(!state.epd_running());
    assert!(commands(&out).contains(&EngineCommand::Stop));
    assert!(analyzing(&state, run));
}

#[test]
fn closing_the_file_of_the_run_ends_it() {
    let (mut state, run, id) = fixture(["", "", "", "", ""]);
    start(&mut state, AnnotateMode::Annotate, RunScope::All);
    send(&mut state, EpdMessage::Close(id));
    assert!(!state.epd_running() || state.epd.close_prompt.is_some());
    if state.epd.close_prompt.is_some() {
        send(
            &mut state,
            EpdMessage::AnswerClose(crate::epd_window::EpdCloseChoice::Discard),
        );
    }
    assert!(!state.epd_running());
    assert!(state.epd.files.is_empty());
    assert!(analyzing(&state, run));
}

#[test]
fn paste_writes_the_best_line_of_the_analysis_of_this_position() {
    use crate::analysis::AnalysisView;
    use mychess_engine::{Bound, Score, SearchInfo};
    let (mut state, run, id) = fixture(["ce 1; pv d4; bm e4", "", "", "", ""]);
    // The analysis window shows 20 plies, 1 234 567 nodes, +0.25, e4 e5.
    let prefs = state.prefs.analysis.clone();
    let mut view = AnalysisView::new(run, 1, &prefs);
    let start_position = Position::new();
    let e4 = mv(&start_position, "e4");
    let after = start_position.after(e4).expect("after");
    let e5 = mv(&after, "e5");
    let target = state.analysis_target().expect("target");
    view.begin_search(SearchId(1), Some(target));
    view.apply_info(
        SearchInfo {
            search: SearchId(1),
            multipv: 1,
            depth: Some(20),
            nodes: Some(1_234_567),
            score: Some((Score::Cp(25), Bound::Exact)),
            pv: vec![e4, e5],
            ..SearchInfo::default()
        },
        crate::analysis::FormatOptions::default(),
    );
    state.analysis.retain(|r| r != run);
    state.analysis.push(view);
    send(&mut state, EpdMessage::PasteAnalysis(None));
    let text = texts(&state, id).remove(0);
    assert_eq!(text, "bm e4\nacd 20\nacn 1234567\nce 25\ndm 0\npv e4 e5\n");
    assert!(state.epd.file(id).expect("file").dirty);
}

#[test]
fn paste_inverts_the_sign_with_black_to_move_and_keeps_the_mate_sign() {
    use crate::analysis::AnalysisView;
    use mychess_engine::{Bound, Score, SearchInfo};
    let (mut state, run, id) = fixture(["", "", "", "", ""]);
    let start_position = Position::new();
    let after_e4 = start_position
        .after(mv(&start_position, "e4"))
        .expect("after");
    // Play 1.e4 on the board: the second record is selected.
    send(&mut state, GameMessageAlias::enter("e4"));
    let prefs = state.prefs.analysis.clone();
    let reply_move = mv(&after_e4, "e5");
    for (score, expect) in [
        (Score::Cp(25), "ce -25\ndm 0\n"),
        (Score::Mate(2), "ce -32764\ndm -2\n"),
        (Score::Mate(-3), "ce 32762\ndm 3\n"),
    ] {
        let mut view = AnalysisView::new(run, 1, &prefs);
        let target = state.analysis_target().expect("target");
        view.begin_search(SearchId(1), Some(target));
        view.apply_info(
            SearchInfo {
                search: SearchId(1),
                multipv: 1,
                depth: Some(10),
                nodes: Some(100),
                score: Some((score, Bound::Exact)),
                pv: vec![reply_move],
                ..SearchInfo::default()
            },
            crate::analysis::FormatOptions::default(),
        );
        state.analysis.retain(|r| r != run);
        state.analysis.push(view);
        send(&mut state, EpdMessage::PasteAnalysis(Some(run)));
        let text = state
            .epd
            .file(id)
            .expect("file")
            .book
            .get(&after_e4)
            .expect("record")
            .operations_text();
        assert!(text.contains(expect), "{score:?}: {text}");
    }
}

struct GameMessageAlias;
impl GameMessageAlias {
    fn enter(san: &str) -> crate::session::GameMessage {
        crate::session::GameMessage::EnterMoveText(san.to_owned())
    }
}

// ---- M7-5c review: names, read-only files, stale questions ----

/// The path of the `EpdEffect::Open` among `effects`, with its `create`.
fn open_effect(effects: &[Effect]) -> Option<(PathBuf, bool)> {
    effects.iter().find_map(|effect| match effect {
        Effect::Epd(EpdEffect::Open { path, create, .. }) => Some((path.clone(), *create)),
        _ => None,
    })
}

/// Asks for a dialog with `ask`, answers it with `typed`, returns the effects.
fn pick(state: &mut AppState, ask: EpdMessage, typed: &str) -> Vec<Effect> {
    let effects = send(state, ask);
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec.request),
            _ => None,
        })
        .expect("a dialog was asked for");
    send(
        state,
        Message::FilePicked(request, Some(PathBuf::from(typed))),
    )
}

#[test]
fn opening_an_existing_file_without_extension_keeps_its_name() {
    let (mut state, _run) = setup(Game::new());
    let effects = pick(&mut state, EpdMessage::Open, "dados");
    assert_eq!(open_effect(&effects), Some((PathBuf::from("dados"), false)));
}

#[test]
fn a_new_file_gets_the_extension_and_the_save_as_too() {
    let (mut state, _run) = setup(Game::new());
    let effects = pick(&mut state, EpdMessage::New, "novo");
    assert_eq!(
        open_effect(&effects),
        Some((PathBuf::from("novo.epd"), true))
    );
}

#[test]
fn the_extension_rule_leaves_the_other_names_and_doubles_a_bare_extension() {
    use crate::epd_window::with_epd_extension;
    for (typed, expected) in [
        ("foo", "foo.epd"),
        ("foo.", "foo.epd"),
        ("foo.EPD", "foo.EPD"),
        ("foo.bar", "foo.bar"),
        ("foo.epd.gz", "foo.epd.gz"),
        // The name is only the extension: no extension for the std library.
        (".epd", ".epd.epd"),
    ] {
        assert_eq!(
            with_epd_extension(PathBuf::from(typed)),
            PathBuf::from(expected),
            "{typed}"
        );
    }
}

#[test]
fn a_read_only_file_turns_the_editor_and_the_save_off() {
    let (mut state, _run, id) = fixture(["bm e4", "", "", "", ""]);
    state.epd.file_mut(id).expect("file").read_only = true;
    let view = state.epd_view();
    let tab = view.tabs.iter().find(|t| t.id == id).expect("tab");
    assert!(tab.read_only && !tab.editor_enabled);
    // Typing leaves no draft and the save is refused.
    drop(send(&mut state, EpdMessage::Edit("bm e5".into())));
    assert!(!state.epd_view().tabs[0].editor_draft);
    let effects = send(&mut state, EpdMessage::Save);
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::Epd(EpdEffect::Save { .. })))
    );
}

#[test]
fn a_question_about_a_missing_file_covers_nothing() {
    let (mut state, _run, id) = fixture(["", "", "", "", ""]);
    state.epd.close_prompt = Some(EpdFileId(id.0 + 100));
    assert!(!state.epd_layer_open(), "no file, no question");
    state.epd.close_prompt = Some(id);
    assert!(state.epd_layer_open());
}
