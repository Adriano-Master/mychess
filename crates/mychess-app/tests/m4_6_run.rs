//! Contract tests of the annotation run (spec M4-6, phase A, "Regras e casos de
//! borda — execução", D880-D892): a synthetic UCI engine answers the searches
//! that `update` asks for through `Effect::EngineCommand`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::field_reassign_with_default,
    clippy::manual_assert_eq,
    clippy::many_single_char_names,
    clippy::type_complexity,
    clippy::unnecessary_literal_bound,
    clippy::too_many_lines
)]

mod m4_6_common;

use std::time::Duration;

use m4_6_common::{
    ANNOTATE_TAG, HANGING_QUEEN, Reply, Rig, add, commands, comment_of, game_of, identity,
    last_search, material_engine, material_reply, nags_of, nth_main, own_book_option, ply_of,
    position_of,
};
use mychess_app::engines::{CommandError, EngineDraft, EngineMessage, Hotkey};
use mychess_app::{
    AnnotateMessage, AnnotateNotice, AnnotateOptions, AnnotateOutcome, AnnotateSide,
    AnnotationError, Control, Effect, GameMessage, Message, Nav, ScoreFormat, WithVars,
};
use mychess_core::{Game, Position};
use mychess_engine::{
    BestMove, Bound, EngineCommand, EngineEvent, EngineOption, ExitInfo, ExitReason, OptionKind,
    OptionValue, Protocol, Score, SearchId, SearchInfo, SearchRequest,
};

// `Hotkey` lives in `mychess_app::engines`; re-import under a local alias if
// the crate root does not export it.
#[allow(unused_imports)]
use mychess_app::engines as _engines;

fn report(rig: &Rig) -> mychess_app::AnnotateReport {
    rig.state.annotate_report.expect("a report")
}

fn analyzing(rig: &Rig) -> bool {
    rig.state.engine_runs.get(rig.run).expect("run").analyzing
}

// ---- the whole game ----

#[test]
fn annotates_a_whole_game_one_search_per_position_marks_the_blunder_and_undoes_in_one_step() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    let before = rig.game().clone();
    let undo = rig.state.session.undo_depth();
    let cursor = rig.state.session.cursor();
    let log = rig.run_all(&mut material_engine);
    assert!(!rig.running());
    // The start position and every ply: each position once (D464).
    assert_eq!(log.requests.len(), 21);
    let mut fens: Vec<String> = log.requests.iter().map(|r| position_of(r).fen()).collect();
    fens.sort();
    fens.dedup();
    assert_eq!(fens.len(), 21);
    let r = report(&rig);
    assert_eq!(r.outcome, AnnotateOutcome::Finished);
    assert_eq!((r.plies, r.total), (20, 20));

    let game = rig.game();
    let blunder = nth_main(game, 5);
    let nags = nags_of(game, blunder);
    assert!(nags.contains(&2) || nags.contains(&4), "{nags:?}");
    assert!(nags_of(game, nth_main(game, 4)).contains(&201), "diagram");
    assert!(comment_of(game, blunder).unwrap().contains(" / "));
    assert!(
        game.children(nth_main(game, 4)).unwrap().len() >= 2,
        "the line"
    );
    // Exactly the blunder is flagged: no other `?`/`??` in the main line.
    for (i, node) in game.mainline().enumerate() {
        let n = nags_of(game, node);
        let flagged = n.iter().any(|x| [2, 4, 6].contains(x));
        // The capture 3...Nxe5 also changes the evaluation, but only the
        // queen move is a loss for its mover.
        assert_eq!(flagged, i == 4, "ply {}: {n:?}", i + 1);
    }
    // One undo point; the cursor stays.
    assert_eq!(rig.state.session.cursor(), cursor);
    assert_eq!(rig.state.session.undo_depth(), undo + 1);
    assert!(rig.state.is_dirty());
    rig.send(GameMessage::Undo);
    assert!(rig.game().same_content(&before));
    assert!(matches!(
        rig.notice(),
        Some(AnnotateNotice::Finished { plies: 20, .. })
    ));
}

#[test]
fn quiet_moves_get_only_the_eval_comment_in_the_default_format() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    rig.options(|o| o.var_at_end = false);
    rig.run_all(&mut |_, _| Reply::cp(25, Vec::new()));
    let game = rig.game();
    for ply in 1..=4 {
        assert_eq!(
            comment_of(game, nth_main(game, ply)).as_deref(),
            Some("[%eval +0.25]"),
            "ply {ply}"
        );
    }
}

#[test]
fn existing_comments_are_kept_after_the_score() {
    let mut game = game_of(&HANGING_QUEEN[..2]);
    let first = nth_main(&game, 1);
    game.set_comment(first, "book move").unwrap();
    let mut rig = Rig::new(game);
    rig.options(|o| {
        o.var_at_end = false;
        o.score_format = ScoreFormat::Plain;
    });
    rig.run_all(&mut |_, _| Reply::cp(10, Vec::new()));
    let game = rig.game();
    assert_eq!(
        comment_of(game, nth_main(game, 1)).as_deref(),
        Some("+0.10 book move")
    );
}

#[test]
fn the_three_score_formats_reach_the_game() {
    for (format, expected) in [
        (ScoreFormat::Plain, "+0.25"),
        (ScoreFormat::Bracket, "[% +0.25]"),
        (ScoreFormat::Eval, "[%eval +0.25]"),
    ] {
        let mut rig = Rig::new(game_of(&["e2e4"]));
        rig.options(|o| {
            o.var_at_end = false;
            o.score_format = format;
        });
        rig.run_all(&mut |_, _| Reply::cp(25, Vec::new()));
        let game = rig.game();
        assert_eq!(
            comment_of(game, nth_main(game, 1)).as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn a_mate_score_is_written_with_a_hash_in_the_eval_format() {
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5"]));
    rig.options(|o| o.var_at_end = false);
    rig.run_all(&mut |r: &SearchRequest, _| {
        if ply_of(r) == 2 {
            Reply::mate(-3, Vec::new())
        } else {
            Reply::cp(0, Vec::new())
        }
    });
    let game = rig.game();
    let text = comment_of(game, nth_main(game, 2)).unwrap();
    assert!(text.contains("[%eval #-3]"), "{text}");
}

#[test]
fn the_move_of_the_engine_guess_gets_no_variation() {
    // The engine always expects the move that is played next (rule 2).
    let line = ["e2e4", "e7e5", "g1f3"];
    let mut rig = Rig::new(game_of(&line));
    rig.options(|o| o.var_at_end = false);
    rig.run_all(&mut |r: &SearchRequest, pos: &Position| {
        let next = line.get(ply_of(r)).and_then(|u| pos.parse_uci(u).ok());
        Reply::cp(
            -50 * i32::try_from(ply_of(r)).unwrap(),
            next.into_iter().collect(),
        )
    });
    let game = rig.game();
    for node in game.mainline() {
        assert!(game.children(node).unwrap().len() <= 1, "no variation");
        let n = nags_of(game, node);
        assert!(n.iter().all(|x| *x != 201), "no diagram: {n:?}");
    }
}

// ---- the engine is configured and put back ----

#[test]
fn the_engine_gets_depth_limits_one_line_no_own_book_and_is_put_back() {
    let mut draft = EngineDraft::new("Fake", "/fake");
    draft.multipv = 3;
    let mut rig = Rig::with(
        game_of(&HANGING_QUEEN[..4]),
        identity(vec![own_book_option()], Some(5)),
        draft,
    );
    assert!(analyzing(&rig));
    let fx = rig.start();
    assert!(!analyzing(&rig), "the board analysis is off during the run");
    let first = commands(&fx);
    assert!(first.contains(&EngineCommand::SetMultiPv(1)));
    assert!(first.contains(&EngineCommand::SetOption {
        name: "OwnBook".to_owned(),
        value: OptionValue::Bool(false),
    }));
    let log = rig.drive(fx, 100, &mut material_engine);
    assert!(log.requests.iter().all(|r| r.limits.depth == Some(13)));
    assert!(log.requests.iter().all(|r| r.limits.movetime.is_none()));
    assert!(log.commands.contains(&EngineCommand::SetMultiPv(3)));
    assert!(log.commands.contains(&EngineCommand::SetOption {
        name: "OwnBook".to_owned(),
        value: OptionValue::Bool(true),
    }));
    assert!(analyzing(&rig));
    // The analysis of the board is sent again after the run (not tagged by
    // the annotation).
    assert!(last_search(&log.last).is_some_and(|r| r.tag & ANNOTATE_TAG != ANNOTATE_TAG));
}

#[test]
fn the_engine_is_put_back_when_the_run_is_cancelled() {
    let mut draft = EngineDraft::new("Fake", "/fake");
    draft.multipv = 3;
    let mut rig = Rig::with(
        game_of(&HANGING_QUEEN),
        identity(vec![own_book_option()], Some(5)),
        draft,
    );
    let fx = rig.start();
    let log = rig.drive(fx, 4, &mut material_engine);
    // The search in flight has started (a search that has not is ended at
    // once by the cancel: D894).
    let request = last_search(&log.last).expect("in flight");
    rig.event(EngineEvent::SearchTagged {
        search: SearchId(9000),
        tag: request.tag,
    });
    rig.event(EngineEvent::SearchStarted(SearchId(9000)));
    let fx = rig.send(AnnotateMessage::Cancel);
    assert!(commands(&fx).contains(&EngineCommand::Stop));
    assert!(
        rig.running(),
        "it ends with the best move of the search in flight"
    );
    // Answer the in-flight search with a best move.
    let after = rig.event(EngineEvent::BestMove(BestMove {
        search: SearchId(9000),
        mv: None,
        ponder: None,
    }));
    assert!(!rig.running());
    assert_eq!(report(&rig).outcome, AnnotateOutcome::Cancelled);
    let all = commands(&after);
    assert!(all.contains(&EngineCommand::SetMultiPv(3)), "{all:?}");
    assert!(all.contains(&EngineCommand::SetOption {
        name: "OwnBook".to_owned(),
        value: OptionValue::Bool(true),
    }));
    assert!(analyzing(&rig));
}

#[test]
fn an_engine_without_own_book_gets_no_such_option() {
    let mut rig = Rig::with(
        game_of(&["e2e4"]),
        identity(Vec::new(), Some(5)),
        EngineDraft::new("Fake", "/fake"),
    );
    let fx = rig.start();
    let log = rig.drive(fx, 10, &mut material_engine);
    let any_own_book = log
        .commands
        .iter()
        .any(|c| matches!(c, EngineCommand::SetOption { name, .. } if name == "OwnBook"));
    assert!(!any_own_book);
}

#[test]
fn ponder_is_never_touched_by_the_run() {
    // Scid `okAnnotation` does not change Ponder and the search is
    // `go depth`/`go movetime`, which never ponders: neither is the option
    // switched off nor restored (D478, corrected in round 2).
    let ponder = EngineOption {
        name: "Ponder".to_owned(),
        kind: OptionKind::Check { default: true },
    };
    let mut rig = Rig::with(
        game_of(&["e2e4"]),
        identity(vec![ponder], Some(5)),
        EngineDraft::new("Fake", "/fake"),
    );
    let fx = rig.start();
    let mut log = rig.drive(fx, 10, &mut material_engine);
    assert!(!rig.running());
    log.commands.extend(commands(&log.last));
    assert!(
        !log.commands
            .iter()
            .any(|c| matches!(c, EngineCommand::SetOption { name, .. } if name == "Ponder")),
        "{:?}",
        log.commands
    );
}

#[test]
fn a_time_limit_sends_movetime_and_no_depth_tag() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..2]));
    rig.options(|o| {
        o.control = Control::Time;
        o.movetime = Duration::from_millis(1500);
    });
    let log = rig.run_all(&mut material_engine);
    assert_eq!(log.requests.len(), 3);
    assert!(log.requests.iter().all(|r| {
        r.limits.movetime == Some(Duration::from_millis(1500)) && r.limits.depth.is_none()
    }));
    assert_eq!(rig.game().tags().get_extra("Depth"), None);
    assert_eq!(rig.game().tags().get_extra("Annotator"), Some("Fake 1"));
}

#[test]
fn out_of_range_options_are_normalised_before_the_search() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    rig.options(|o| o.depth = 1);
    let log = rig.run_all(&mut material_engine);
    assert!(log.requests.iter().all(|r| r.limits.depth == Some(10)));
    assert_eq!(rig.game().tags().get_extra("Depth"), Some("10"));
}

#[test]
fn an_xboard_engine_is_limited_to_time_and_has_no_exercises_or_mate_search() {
    let mut draft = EngineDraft::new("Old", "/old");
    draft.protocol = Protocol::XBoard;
    let mut rig = Rig::with(
        game_of(&HANGING_QUEEN[..2]),
        identity(Vec::new(), None),
        draft,
    );
    rig.options(|o| o.mark_exercises = true);
    let caps = rig.state.annotate_caps(Some(rig.run));
    assert!(!caps.depth && !caps.missed_mates && !caps.exercises);
    let log = rig.run_all(&mut material_engine);
    assert!(log.requests.iter().all(|r| r.limits.depth.is_none()));
    assert!(log.requests.iter().all(|r| r.limits.movetime.is_some()));
    assert!(!log.commands.contains(&EngineCommand::SetMultiPv(4)));
    assert_eq!(
        rig.game().tags().get_extra("Depth"),
        None,
        "no depth control"
    );
}

#[test]
fn a_uci_engine_reports_its_capabilities_to_the_dialog() {
    let rig = Rig::new(game_of(&["e2e4"]));
    let caps = rig.state.annotate_caps(Some(rig.run));
    assert!(caps.depth && caps.missed_mates && caps.exercises);
    let rig = Rig::with(
        game_of(&["e2e4"]),
        identity(Vec::new(), None),
        EngineDraft::new("Fake", "/fake"),
    );
    assert!(
        !rig.state.annotate_caps(Some(rig.run)).exercises,
        "needs MultiPV"
    );
}

// ---- cancelling, failures, interference ----

#[test]
fn cancelling_keeps_the_edits_and_one_undo_reverts_them() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    let before = rig.game().clone();
    let undo = rig.state.session.undo_depth();
    let fx = rig.start();
    let log = rig.drive(fx, 6, &mut material_engine);
    assert!(rig.running());
    assert!(rig.state.annotation_progress().unwrap().done_plies >= 4);
    let fx = rig.send(AnnotateMessage::Cancel);
    assert!(commands(&fx).contains(&EngineCommand::Stop));
    let request = last_search(&log.last).expect("in flight");
    rig.event(EngineEvent::SearchTagged {
        search: SearchId(900),
        tag: request.tag,
    });
    rig.event(EngineEvent::SearchStarted(SearchId(900)));
    rig.event(EngineEvent::BestMove(BestMove {
        search: SearchId(900),
        mv: None,
        ponder: None,
    }));
    assert!(!rig.running());
    assert_eq!(report(&rig).outcome, AnnotateOutcome::Cancelled);
    assert!(matches!(
        rig.notice(),
        Some(AnnotateNotice::Cancelled { .. })
    ));
    assert!(rig.state.is_dirty(), "the edits stay");
    assert!(!rig.game().same_content(&before));
    assert_eq!(rig.state.session.undo_depth(), undo + 1);
    rig.send(GameMessage::Undo);
    assert!(rig.game().same_content(&before));
}

#[test]
fn cancelling_while_the_engine_is_still_starting_ends_at_once_without_edits() {
    let mut state_rig = Rig::with(
        game_of(&HANGING_QUEEN[..4]),
        identity(Vec::new(), Some(5)),
        EngineDraft::new("Fake", "/fake"),
    );
    // Stop the running engine and leave only an F2 engine that must start.
    let run = state_rig.run;
    state_rig.send(Message::Engine(EngineMessage::Stopped(run)));
    state_rig
        .state
        .engines
        .set_hotkey(state_rig.engine, Some(Hotkey::F2))
        .expect("hotkey");
    let before = state_rig.game().clone();
    let undo = state_rig.state.session.undo_depth();
    let fx = state_rig.start();
    assert!(
        fx.iter().any(|e| matches!(e, Effect::StartEngine { .. })),
        "F2 is started (D480)"
    );
    assert!(state_rig.running());
    state_rig.send(AnnotateMessage::Cancel);
    assert!(!state_rig.running());
    assert_eq!(report(&state_rig).outcome, AnnotateOutcome::Cancelled);
    assert!(state_rig.game().same_content(&before));
    assert_eq!(state_rig.state.session.undo_depth(), undo);
}

#[test]
fn a_dying_engine_ends_the_run_with_engine_failed_and_keeps_the_edits() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    let before = rig.game().clone();
    let fx = rig.start();
    rig.drive(fx, 6, &mut material_engine);
    rig.event(EngineEvent::Exited(ExitInfo {
        reason: ExitReason::Crashed,
        status: None,
        stderr_tail: Vec::new(),
    }));
    assert!(!rig.running());
    let r = report(&rig);
    assert_eq!(
        r.outcome,
        AnnotateOutcome::Failed(AnnotationError::EngineFailed)
    );
    assert!(r.plies >= 4);
    assert!(matches!(
        rig.notice(),
        Some(AnnotateNotice::EngineFailed { .. })
    ));
    assert!(!rig.game().same_content(&before), "edits stay");
    // The undo group is closed: the next edit has its own undo point.
    let depth = rig.state.session.undo_depth();
    let root = rig.game().root();
    rig.send(GameMessage::SetComment {
        node: root,
        text: "x".to_owned(),
    });
    assert_eq!(rig.state.session.undo_depth(), depth + 1);
}

#[test]
fn other_engine_endings_also_fail_the_run() {
    for kind in 0..2 {
        let mut rig = Rig::new(game_of(&HANGING_QUEEN));
        let fx = rig.start();
        rig.drive(fx, 3, &mut material_engine);
        let run = rig.run;
        if kind == 0 {
            rig.send(Message::Engine(EngineMessage::Stopped(run)));
        } else {
            rig.send(Message::Engine(EngineMessage::CommandDone {
                run,
                result: Err(CommandError::NotRunning),
            }));
        }
        assert!(!rig.running(), "kind {kind}");
        assert_eq!(
            report(&rig).outcome,
            AnnotateOutcome::Failed(AnnotationError::EngineFailed),
            "kind {kind}"
        );
    }
}

#[test]
fn an_engine_that_answers_without_any_score_stops_the_run_with_no_score() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    let mut engine = |_: &SearchRequest, pos: &Position| {
        let mut reply = material_reply(pos);
        reply.silent = true;
        reply
    };
    rig.run_all(&mut engine);
    assert!(!rig.running());
    assert_eq!(
        report(&rig).outcome,
        AnnotateOutcome::Failed(AnnotationError::NoScore)
    );
    assert!(matches!(rig.notice(), Some(AnnotateNotice::NoScore { .. })));
}

#[test]
fn a_lost_search_label_adopts_the_first_search_started() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    let fx = rig.start();
    let request = last_search(&fx).expect("search");
    // No `SearchTagged`: only `SearchStarted` (a full event queue).
    let search = SearchId(777);
    rig.event(EngineEvent::SearchStarted(search));
    rig.event(EngineEvent::Info(SearchInfo {
        search,
        multipv: 1,
        depth: Some(13),
        score: Some((Score::Cp(10), Bound::Exact)),
        pv: Vec::new(),
        ..SearchInfo::default()
    }));
    let fx = rig.event(EngineEvent::BestMove(BestMove {
        search,
        mv: None,
        ponder: None,
    }));
    assert!(rig.running(), "went on to the next position");
    let next = last_search(&fx).expect("the next search");
    assert_ne!(next.tag, request.tag);
    assert_eq!(ply_of(&next), 1);
}

#[test]
fn reports_of_other_searches_do_not_count() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    rig.options(|o| o.var_at_end = false);
    let fx = rig.start();
    let request = last_search(&fx).expect("search");
    let ours = SearchId(5);
    rig.event(EngineEvent::SearchTagged {
        search: ours,
        tag: request.tag,
    });
    rig.event(EngineEvent::SearchStarted(ours));
    // A stale report of another search with a wild score.
    rig.event(EngineEvent::Info(SearchInfo {
        search: SearchId(4),
        multipv: 1,
        depth: Some(40),
        score: Some((Score::Cp(9999), Bound::Exact)),
        ..SearchInfo::default()
    }));
    rig.event(EngineEvent::Info(SearchInfo {
        search: ours,
        multipv: 1,
        depth: Some(13),
        score: Some((Score::Cp(30), Bound::Exact)),
        ..SearchInfo::default()
    }));
    let fx = rig.event(EngineEvent::BestMove(BestMove {
        search: ours,
        mv: None,
        ponder: None,
    }));
    drop(rig.drive(fx, 5, &mut |_, _| Reply::cp(30, Vec::new())));
    let game = rig.game();
    assert_eq!(
        comment_of(game, nth_main(game, 1)).as_deref(),
        Some("[%eval +0.30]")
    );
}

#[test]
fn an_exact_score_wins_over_a_later_bound_and_a_bound_is_kept_until_an_exact_one() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    rig.options(|o| o.var_at_end = false);
    let mut call = 0;
    let mut engine = move |_: &SearchRequest, _: &Position| {
        call += 1;
        // Position 0: a bound only. Position 1: exact then lower bound.
        let mut reply = Reply::cp(if call == 1 { 40 } else { 60 }, Vec::new());
        if call == 1 {
            reply.bound = Bound::Lower;
        }
        reply
    };
    rig.run_all(&mut engine);
    let game = rig.game();
    assert_eq!(
        comment_of(game, nth_main(game, 1)).as_deref(),
        Some("[%eval +0.60]")
    );
    // Now an exact value followed by a lower bound inside one search.
    let mut rig = Rig::new(game_of(&["e2e4"]));
    rig.options(|o| o.var_at_end = false);
    let fx = rig.start();
    let request = last_search(&fx).unwrap();
    let s = SearchId(1);
    rig.event(EngineEvent::SearchTagged {
        search: s,
        tag: request.tag,
    });
    rig.event(EngineEvent::SearchStarted(s));
    for (cp, bound) in [(10, Bound::Exact), (500, Bound::Lower)] {
        rig.event(EngineEvent::Info(SearchInfo {
            search: s,
            multipv: 1,
            depth: Some(13),
            score: Some((Score::Cp(cp), bound)),
            ..SearchInfo::default()
        }));
    }
    let fx = rig.event(EngineEvent::BestMove(BestMove {
        search: s,
        mv: None,
        ponder: None,
    }));
    rig.drive(fx, 5, &mut |_, _| Reply::cp(10, Vec::new()));
    let game = rig.game();
    // Both positions score +0.10 so the quiet move text shows the exact one.
    assert_eq!(
        comment_of(game, nth_main(game, 1)).as_deref(),
        Some("[%eval +0.10]")
    );
}

#[test]
fn the_game_cannot_be_edited_while_the_run_goes() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
    let fx = rig.start();
    let changes = rig.state.session.changes();
    rig.send(GameMessage::Navigate(Nav::Start));
    assert_eq!(rig.notice(), Some(AnnotateNotice::Busy));
    rig.send(GameMessage::Undo);
    assert_eq!(rig.notice(), Some(AnnotateNotice::Busy));
    assert_eq!(rig.state.session.changes(), changes);
    assert!(rig.running());
    rig.drive(fx, 2, &mut material_engine);
    assert!(rig.running());
}

#[test]
fn the_autoplay_tick_is_not_refused_as_busy() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
    rig.start();
    rig.state.notice = None;
    rig.send(GameMessage::AutoplayTick { generation: 0 });
    assert_ne!(rig.notice(), Some(AnnotateNotice::Busy));
}

#[test]
fn a_replaced_game_stops_the_run_with_game_changed() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
    let fx = rig.start();
    rig.drive(fx, 2, &mut material_engine);
    assert!(rig.running());
    rig.state.session.replace_game(Game::new());
    rig.send(GameMessage::AutoplayTick { generation: 0 });
    assert!(!rig.running());
    assert_eq!(report(&rig).outcome, AnnotateOutcome::GameChanged);
    assert!(matches!(
        rig.notice(),
        Some(AnnotateNotice::GameChanged { .. })
    ));
    assert!(analyzing(&rig), "the board analysis is back");
}

#[test]
fn an_edit_that_bypasses_the_message_path_stops_the_run() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
    let fx = rig.start();
    rig.drive(fx, 2, &mut material_engine);
    let root = rig.game().root();
    rig.state.session.update(GameMessage::SetComment {
        node: root,
        text: "someone".to_owned(),
    });
    // Any message: the guard runs at the end of every update. (`ShutdownDone`
    // would now fail the run first, as the engine is gone: D895.)
    rig.send(GameMessage::AutoplayTick { generation: 0 });
    assert!(!rig.running());
    assert_eq!(report(&rig).outcome, AnnotateOutcome::GameChanged);
}

#[test]
fn trial_mode_refuses_to_start() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    rig.send(GameMessage::EnterTrial { null_move: false });
    assert!(rig.state.session.in_trial());
    let fx = rig.start();
    assert_eq!(rig.notice(), Some(AnnotateNotice::TrialMode));
    assert!(!rig.running());
    assert!(commands(&fx).is_empty());
}

#[test]
fn starting_twice_is_refused_and_open_cancels() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    rig.start();
    rig.send(AnnotateMessage::Start);
    assert_eq!(rig.notice(), Some(AnnotateNotice::AlreadyRunning));
    let fx = rig.send(AnnotateMessage::Open);
    assert!(commands(&fx).contains(&EngineCommand::Stop));
}

#[test]
fn starting_without_an_engine_says_so_and_f2_is_started_when_there_is_one() {
    let mut state = mychess_app::AppState::default();
    mychess_app::update(&mut state, AnnotateMessage::Start.into());
    assert!(matches!(
        &state.notice,
        Some(mychess_app::AppNotice::Annotate(AnnotateNotice::NoEngine))
    ));
    assert!(!state.annotation_running());
    let id = state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .unwrap();
    state.engines.set_hotkey(id, Some(Hotkey::F2)).unwrap();
    let fx = mychess_app::update(&mut state, AnnotateMessage::Start.into());
    assert!(fx.iter().any(|e| matches!(e, Effect::StartEngine { .. })));
    assert!(state.annotation_running());
}

#[test]
fn nothing_to_annotate_is_reported_and_the_engine_is_left_alone() {
    let mut game = game_of(&HANGING_QUEEN[..4]);
    let e5 = nth_main(&game, 2);
    let nf3 = add(&mut game, e5, "g1f3");
    let mut rig = Rig::new(game);
    rig.state
        .session
        .update(GameMessage::Navigate(Nav::GoToNode(nf3)));
    let fx = rig.start();
    assert_eq!(rig.notice(), Some(AnnotateNotice::NothingToAnnotate));
    assert!(!rig.running());
    assert!(
        !commands(&fx).iter().any(|c| matches!(
            c,
            EngineCommand::Stop | EngineCommand::SetMultiPv(_) | EngineCommand::SetOption { .. }
        )),
        "the engine is not reconfigured"
    );
    assert!(analyzing(&rig));
}

// ---- where it starts and what it visits ----

#[test]
fn a_game_without_moves_gets_the_score_as_initial_comment() {
    let mut rig = Rig::new(Game::new());
    let log = rig.run_all(&mut |_, _| Reply::cp(20, Vec::new()));
    assert_eq!(log.requests.len(), 1);
    assert_eq!(rig.game().initial_comment(), Some("13:+0.20"));
    assert!(!rig.running());
    assert_eq!(report(&rig).outcome, AnnotateOutcome::Finished);
}

#[test]
fn a_game_without_moves_keeps_its_initial_comment_in_front() {
    let mut game = Game::new();
    game.set_initial_comment("hello");
    let mut rig = Rig::new(game);
    rig.run_all(&mut |_, _| Reply::cp(20, Vec::new()));
    let text = rig.game().initial_comment().unwrap().to_owned();
    assert!(text.starts_with("hello"), "{text}");
    assert!(text.ends_with("13:+0.20"), "{text}");
}

#[test]
fn a_cursor_in_the_middle_starts_there_and_the_move_before_it_is_left_alone() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..8]));
    rig.options(|o| o.var_at_end = false);
    rig.send(GameMessage::Navigate(Nav::Start));
    for _ in 0..3 {
        rig.send(GameMessage::Navigate(Nav::Forward(1)));
    }
    let third = nth_main(rig.game(), 3);
    assert_eq!(rig.state.session.cursor(), third);
    let cursor_fen = rig.game().position_at(third).unwrap().fen();
    let log = rig.run_all(&mut material_engine);
    assert_eq!(position_of(&log.requests[0]).fen(), cursor_fen);
    assert_eq!(log.requests.len(), 6, "the cursor position and five plies");
    let game = rig.game();
    for ply in 1..=3 {
        assert!(comment_of(game, nth_main(game, ply)).is_none(), "ply {ply}");
    }
    assert!(comment_of(game, nth_main(game, 4)).is_some());
}

#[test]
fn the_first_move_after_the_cursor_has_a_real_previous_score() {
    // The Scid compares it with 0 (a false delta, `resetAnalysis`); here the
    // cursor position is analysed first (D465): the same score on both sides
    // of the move means no error even if the score is large.
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    rig.options(|o| o.var_at_end = false);
    rig.send(GameMessage::Navigate(Nav::Start));
    rig.send(GameMessage::Navigate(Nav::Forward(1)));
    rig.run_all(&mut |_, _| Reply::cp(350, Vec::new()));
    let game = rig.game();
    for node in game.mainline() {
        let n = nags_of(game, node);
        assert!(!n.iter().any(|x| [2, 4, 6, 201].contains(x)), "{n:?}");
    }
}

#[test]
fn a_cursor_at_the_end_of_the_main_line_starts_from_the_beginning() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    rig.send(GameMessage::Navigate(Nav::GameEnd));
    let log = rig.run_all(&mut material_engine);
    assert_eq!(log.requests.len(), 5);
    assert_eq!(position_of(&log.requests[0]).fen(), Position::new().fen());
}

#[test]
fn only_one_side_is_annotated_but_every_position_is_searched() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..8]));
    rig.options(|o| {
        o.side = AnnotateSide::White;
        o.var_at_end = false;
    });
    let log = rig.run_all(&mut |_, _| Reply::cp(15, Vec::new()));
    assert_eq!(log.requests.len(), 9);
    let game = rig.game();
    for (i, node) in game.mainline().enumerate() {
        let white_move = i % 2 == 0;
        assert_eq!(
            comment_of(game, node).is_some(),
            white_move,
            "ply {}",
            i + 1
        );
    }
}

#[test]
fn the_opening_limit_stops_after_the_moves_asked_for() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    rig.options(|o| o.opening_only = Some(5));
    let log = rig.run_all(&mut material_engine);
    // Plies 1 to 10 (five full moves) plus the start position.
    assert_eq!(log.requests.len(), 11);
    assert_eq!(report(&rig).plies, 10);
    let game = rig.game();
    assert!(comment_of(game, nth_main(game, 10)).is_some());
    assert!(comment_of(game, nth_main(game, 11)).is_none());
}

#[test]
fn variations_are_annotated_recursively_after_their_parent_when_asked() {
    // 1.e4 e5 2.Qh5 Nc6 3.Qxe5 plus 2.Nf3 (with 2...Nc6 3.Bc4) as a
    // variation of 2.Qh5, and 2.Nc3 as a variation of 2.Nf3's line.
    let mut game = game_of(&HANGING_QUEEN[..6]);
    let e5 = nth_main(&game, 2);
    let nf3 = add(&mut game, e5, "g1f3");
    let nc6 = add(&mut game, nf3, "b8c6");
    let bc4 = add(&mut game, nc6, "f1c4");
    let sub = add(&mut game, nc6, "b1c3");
    let mut rig = Rig::new(game);
    rig.options(|o| {
        o.variations = true;
        o.score_format = ScoreFormat::Plain;
        o.var_at_end = false;
    });
    let log = rig.run_all(&mut |_, _| Reply::cp(10, Vec::new()));
    // Start + 6 main plies + Nf3, Nc6, Bc4, Nc3.
    assert_eq!(log.requests.len(), 11);
    let fens: Vec<String> = log.requests.iter().map(|r| position_of(r).fen()).collect();
    let mut unique = fens.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 11, "each position once");
    // The main line is searched in order before the variation positions
    // that come after their parent.
    let ply_order: Vec<usize> = log.requests.iter().take(7).map(ply_of).collect();
    assert_eq!(
        ply_order,
        vec![0, 1, 2, 3, 4, 5, 6],
        "main line first (D887)"
    );
    let game = rig.game();
    for node in [bc4, sub, nf3, nc6] {
        assert!(comment_of(game, node).is_some(), "variation move annotated");
    }
    // Without the option the variation is left alone.
    let mut other = Rig::new(game.clone());
    other.options(|o| o.variations = false);
    let log = other.run_all(&mut |_, _| Reply::cp(10, Vec::new()));
    assert_eq!(log.requests.len(), 7);
}

#[test]
fn the_variation_root_move_is_compared_with_the_parent_evaluation() {
    // The variation 2.Nf3 starts at the position after 1...e5, which is
    // searched once: its move gets a delta against that evaluation.
    let mut game = game_of(&["e2e4", "e7e5", "d1h5"]);
    let e5 = nth_main(&game, 2);
    let nf3 = add(&mut game, e5, "g1f3");
    let mut rig = Rig::new(game);
    rig.options(|o| {
        o.variations = true;
        o.var_at_end = false;
    });
    // Everything is +0.20 except the position after 2.Nf3 (-2.00): that
    // move is a mistake for White.
    let nf3_fen = {
        let g = rig.game();
        g.position_at(nf3).unwrap().fen()
    };
    let log = rig.run_all(&mut |r: &SearchRequest, pos: &Position| {
        let fen = position_of(r).fen();
        let _ = pos;
        Reply::cp(if fen == nf3_fen { -200 } else { 20 }, Vec::new())
    });
    assert_eq!(log.requests.len(), 5, "start, e4, e5, Qh5 and Nf3");
    let game = rig.game();
    assert!(nags_of(game, nf3).contains(&2), "{:?}", nags_of(game, nf3));
}

// ---- the missed mate, tags, exercises ----

#[test]
fn a_missed_mate_gets_the_mate_comment_on_the_line_and_a_diagram() {
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5", "d1h5", "b8c6", "d2d3"]));
    rig.options(|o| o.var_at_end = false);
    let mut engine = |r: &SearchRequest, pos: &Position| {
        if ply_of(r) == 4 {
            Reply::mate(2, vec![pos.parse_uci("h5f7").unwrap()])
        } else {
            Reply::cp(0, Vec::new())
        }
    };
    rig.run_all(&mut engine);
    let game = rig.game();
    let parent = nth_main(game, 4);
    let kids = game.children(parent).unwrap().to_vec();
    assert_eq!(kids.len(), 2, "the mate line is a variation of 3.d3");
    assert_eq!(game.comment_before(kids[1]).unwrap(), Some("Mate in 2..."),);
    assert!(nags_of(game, parent).contains(&201));
    assert!(nags_of(game, nth_main(game, 5)).contains(&4), "`??`");
}

#[test]
fn the_missed_mate_depth_is_the_depth_option_even_under_a_time_limit() {
    // Rule 4: with `depth 13` a mate in 9 counts (9 < 10), one in 12 does
    // not; the control by time does not change `D`.
    for (mate, comment) in [(9, true), (12, false)] {
        let mut rig = Rig::new(game_of(&["e2e4", "e7e5", "d1h5", "b8c6", "d2d3"]));
        rig.options(|o| {
            o.var_at_end = false;
            o.control = Control::Time;
            o.movetime = Duration::from_millis(200);
        });
        let mut engine = |r: &SearchRequest, pos: &Position| {
            if ply_of(r) == 4 {
                Reply::mate(mate, vec![pos.parse_uci("h5f7").unwrap()])
            } else {
                Reply::cp(0, Vec::new())
            }
        };
        rig.run_all(&mut engine);
        let game = rig.game();
        let kids = game.children(nth_main(game, 4)).unwrap().to_vec();
        assert_eq!(kids.len(), 2);
        assert_eq!(
            game.comment_before(kids[1]).unwrap().is_some(),
            comment,
            "mate in {mate}"
        );
    }
}

#[test]
fn the_annotator_and_depth_tags_are_appended_never_duplicated() {
    let mut game = game_of(&["e2e4"]);
    game.tags_mut().set_extra("Annotator", "Other").unwrap();
    game.tags_mut().set_extra("Depth", "10").unwrap();
    let mut rig = Rig::new(game);
    rig.run_all(&mut material_engine);
    assert_eq!(
        rig.game().tags().get_extra("Annotator"),
        Some("Other , Fake 1")
    );
    assert_eq!(rig.game().tags().get_extra("Depth"), Some("10 , 13"));
    // A second run changes nothing.
    rig.run_all(&mut material_engine);
    assert_eq!(
        rig.game().tags().get_extra("Annotator"),
        Some("Other , Fake 1")
    );
    assert_eq!(rig.game().tags().get_extra("Depth"), Some("10 , 13"));
}

#[test]
fn without_the_tag_option_no_tag_is_written_and_there_is_still_one_undo_point() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
    rig.options(|o| o.add_tag = false);
    let undo = rig.state.session.undo_depth();
    rig.run_all(&mut material_engine);
    assert_eq!(rig.game().tags().get_extra("Annotator"), None);
    assert_eq!(rig.game().tags().get_extra("Depth"), None);
    assert_eq!(rig.state.session.undo_depth(), undo + 1);
}

/// 1.e4 e5 2.Qh5 Nc6 3.Qxe5: the blunder is ply 5. The engine plays by
/// scripted scores; probes answer through `probe`.
fn exercise_engine(
    probe: impl Fn(&SearchRequest, &Position) -> Option<usize> + 'static,
) -> impl FnMut(&SearchRequest, &Position) -> Reply {
    move |r: &SearchRequest, pos: &Position| {
        let n = ply_of(r);
        let is_probe = r.limits.depth != Some(13);
        match n {
            4 => Reply::cp(30, vec![pos.parse_uci("g1f3").unwrap()]),
            5 => {
                let best = pos.parse_uci("c6e5").unwrap();
                let mut reply = Reply::cp(-800, vec![best]);
                reply.second = Some(Score::Cp(0));
                if is_probe && let Some(i) = probe(r, pos) {
                    let other = pos.legal_moves().into_iter().filter(|m| *m != best).nth(i);
                    reply.pv = other.into_iter().collect();
                }
                reply
            }
            _ => Reply::cp(30, Vec::new()),
        }
    }
}

fn blunder_rig(identity_: mychess_engine::EngineIdentity) -> Rig {
    let mut rig = Rig::with(
        game_of(&["e2e4", "e7e5", "d1h5", "b8c6", "h5e5"]),
        identity_,
        EngineDraft::new("Fake", "/fake"),
    );
    rig.options(|o| {
        o.mark_exercises = true;
        o.var_at_end = false;
    });
    rig
}

#[test]
fn a_tactical_shot_is_probed_flagged_and_commented_with_the_difficulty() {
    let mut rig = blunder_rig(identity(vec![own_book_option()], Some(5)));
    let fx = rig.start();
    assert!(commands(&fx).contains(&EngineCommand::SetMultiPv(4)));
    // Wrong at depth 2 and below 50 ms: the last miss is the 10 ms probe
    // (index 3), so the difficulty is 5.
    let mut engine = exercise_engine(|r, _| {
        let ms = r.limits.movetime.map(|d| d.as_millis());
        (r.limits.depth == Some(2) || ms.is_some_and(|m| m < 50)).then_some(0)
    });
    let log = rig.drive(fx, 100, &mut engine);
    assert!(!rig.running());
    // 6 positions + 3 depth probes + 8 time probes.
    assert_eq!(log.requests.len(), 6 + 11);
    let probes: Vec<&SearchRequest> = log
        .requests
        .iter()
        .filter(|r| r.limits.depth != Some(13))
        .collect();
    let depths: Vec<u32> = probes.iter().filter_map(|r| r.limits.depth).collect();
    let times: Vec<u128> = probes
        .iter()
        .filter_map(|r| r.limits.movetime.map(|d| d.as_millis()))
        .collect();
    assert_eq!(depths, vec![1, 2, 3]);
    assert_eq!(times, vec![1, 2, 5, 10, 50, 100, 200, 1000]);
    // Every probe is of the position after the blunder.
    assert!(probes.iter().all(|r| ply_of(r) == 5));
    assert!(
        log.commands.contains(&EngineCommand::SetMultiPv(1)),
        "restored"
    );
    let game = rig.game();
    let text = comment_of(game, nth_main(game, 5)).unwrap();
    // `<score> ****D<n> <prev>-><cur> <comment>` (D884).
    assert_eq!(text.trim_end(), "-8.00 / +0.30 ****D5 0.3->-8.0");
    assert!(rig.state.session.tactics_pending());
    assert_eq!(report(&rig).exercises, 1);
}

#[test]
fn an_obvious_move_is_not_an_exercise() {
    let mut rig = blunder_rig(identity(vec![own_book_option()], Some(5)));
    let fx = rig.start();
    let mut engine = exercise_engine(|_, _| None);
    let log = rig.drive(fx, 100, &mut engine);
    // Found at depths 1, 2 and 3: the probes stop after three.
    assert_eq!(log.requests.len(), 6 + 3);
    assert!(!rig.state.session.tactics_pending());
    let game = rig.game();
    let text = comment_of(game, nth_main(game, 5)).unwrap();
    assert!(!text.contains("****D"), "{text}");
    assert_eq!(report(&rig).exercises, 0);
    // The blunder keeps its other marks.
    assert!(nags_of(game, nth_main(game, 5)).contains(&4));
}

#[test]
fn exercises_need_multipv_in_the_engine() {
    let mut rig = blunder_rig(identity(Vec::new(), None));
    let fx = rig.start();
    assert!(
        !commands(&fx)
            .iter()
            .any(|c| matches!(c, EngineCommand::SetMultiPv(_)))
    );
    let mut engine = exercise_engine(|_, _| Some(0));
    let log = rig.drive(fx, 100, &mut engine);
    assert_eq!(log.requests.len(), 6, "no probes without MultiPV");
    assert!(!rig.state.session.tactics_pending());
}

#[test]
fn exercises_off_means_no_probes_and_no_multipv_change() {
    let mut rig = blunder_rig(identity(Vec::new(), Some(5)));
    rig.options(|o| o.mark_exercises = false);
    let fx = rig.start();
    assert!(!commands(&fx).contains(&EngineCommand::SetMultiPv(4)));
    let mut engine = exercise_engine(|_, _| Some(0));
    let log = rig.drive(fx, 100, &mut engine);
    assert_eq!(log.requests.len(), 6);
    assert!(!rig.state.session.tactics_pending());
}

#[test]
fn a_close_second_line_is_not_an_exercise() {
    let mut rig = blunder_rig(identity(Vec::new(), Some(5)));
    let fx = rig.start();
    let mut engine = |r: &SearchRequest, pos: &Position| {
        let mut reply = exercise_engine(|_, _| Some(0))(r, pos);
        if reply.second.is_some() {
            reply.second = Some(Score::Cp(-700));
        }
        reply
    };
    let log = rig.drive(fx, 100, &mut engine);
    assert_eq!(log.requests.len(), 6);
    assert!(!rig.state.session.tactics_pending());
}

#[test]
fn clear_hash_is_sent_before_each_probe_when_the_engine_has_the_button() {
    let button = EngineOption {
        name: "Clear Hash".to_owned(),
        kind: OptionKind::Button,
    };
    let mut rig = blunder_rig(identity(vec![button], Some(5)));
    let fx = rig.start();
    let mut engine = exercise_engine(|_, _| Some(0));
    let log = rig.drive(fx, 100, &mut engine);
    let clears = log
        .commands
        .iter()
        .filter(|c| matches!(c, EngineCommand::SetOption { name, .. } if name == "Clear Hash"))
        .count();
    assert_eq!(clears, 11);
}

// ---- last move, end of game ----

#[test]
fn the_last_move_gets_the_line_of_the_final_position_as_a_comment() {
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5"]));
    rig.run_all(&mut |r: &SearchRequest, pos: &Position| {
        if ply_of(r) == 2 {
            Reply::cp(30, vec![pos.parse_uci("g1f3").unwrap()])
        } else {
            Reply::cp(30, Vec::new())
        }
    });
    let game = rig.game();
    let text = comment_of(game, nth_main(game, 2)).unwrap();
    assert!(text.starts_with("[%eval +0.30]"), "{text}");
    assert!(text.contains("13:+0.30"), "{text}");
    assert!(text.contains("Nf3"), "{text}");
}

#[test]
fn no_final_line_for_allmoves_no_variations_or_when_the_option_is_off() {
    let last = |f: &dyn Fn(&mut AnnotateOptions)| {
        let mut rig = Rig::new(game_of(&["e2e4", "e7e5"]));
        rig.options(|o| f(o));
        rig.run_all(&mut |r: &SearchRequest, pos: &Position| {
            if ply_of(r) == 2 {
                Reply::cp(30, vec![pos.parse_uci("g1f3").unwrap()])
            } else {
                Reply::cp(30, Vec::new())
            }
        });
        let game = rig.game();
        comment_of(game, nth_main(game, 2)).unwrap_or_default()
    };
    assert!(!last(&|o| o.var_at_end = false).contains("13:+0.30"));
    assert!(!last(&|o| o.with_vars = WithVars::AllMoves).contains("13:+0.30"));
    assert!(!last(&|o| o.with_vars = WithVars::No).contains("13:+0.30"));
    assert!(last(&|o| o.with_vars = WithVars::NotBest).contains("13:+0.30"));
}

#[test]
fn a_game_that_ends_in_mate_asks_no_engine_for_the_last_position() {
    // Fool's mate: 1.f3 e5 2.g4 Qh4#.
    let mut rig = Rig::new(game_of(&["f2f3", "e7e5", "g2g4", "d8h4"]));
    let log = rig.run_all(&mut |r: &SearchRequest, pos: &Position| {
        // Black has the better of it throughout.
        let _ = (r, pos);
        Reply::cp(-100, Vec::new())
    });
    assert_eq!(
        log.requests.len(),
        4,
        "start and three plies; the mate needs none"
    );
    let game = rig.game();
    let text = comment_of(game, nth_main(game, 4)).unwrap();
    // Rule 10: White is mated: -327 as a single score; no final line.
    assert!(text.contains("-327.00"), "{text}");
    assert!(!text.contains("13:"), "{text}");
    assert_eq!(report(&rig).outcome, AnnotateOutcome::Finished);
}

#[test]
fn a_game_that_ends_in_stalemate_scores_zero() {
    // A known short stalemate: 1.e3 a5 2.Qh5 Ra6 3.Qxa5 h5 4.h4 Rah6 5.Qc7 f6
    // 6.Qxd7+ Kf7 7.Qxb7 Qd3 8.Qxb8 Qh7 9.Qxc8 Kg6 10.Qe6.
    let ucis = [
        "e2e3", "a7a5", "d1h5", "a8a6", "h5a5", "h7h5", "h2h4", "a6h6", "a5c7", "f7f6", "c7d7",
        "e8f7", "d7b7", "d8d3", "b7b8", "d3h7", "b8c8", "f7g6", "c8e6",
    ];
    let mut rig = Rig::new(game_of(&ucis));
    rig.options(|o| o.var_at_end = true);
    let log = rig.run_all(&mut |_, _| Reply::cp(10, Vec::new()));
    assert_eq!(
        log.requests.len(),
        ucis.len(),
        "the stalemate is not searched"
    );
    let game = rig.game();
    let text = comment_of(game, nth_main(game, ucis.len())).unwrap();
    assert!(text.contains("+0.00"), "{text}");
}

// ---- progress ----

#[test]
fn progress_grows_one_move_at_a_time_up_to_the_total() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
    let mut fx = rig.start();
    let p0 = rig.state.annotation_progress().expect("progress");
    assert_eq!(p0.total_plies, 6);
    assert_eq!((p0.game_index, p0.game_total), (1, 1));
    let mut last = 0;
    for _ in 0..7 {
        if !rig.running() {
            break;
        }
        let request = last_search(&fx).unwrap();
        let reply = material_reply(&position_of(&request));
        let (after, _) = rig.answer(&fx, &reply).unwrap();
        fx = after;
        if let Some(p) = rig.state.annotation_progress() {
            assert!(p.done_plies >= last && p.done_plies <= p.total_plies);
            last = p.done_plies;
        }
    }
    assert!(!rig.running());
    assert_eq!(rig.state.annotation_progress(), None);
    assert_eq!(report(&rig).plies, 6);
}

// ---- dialog ----

#[test]
fn the_dialog_opens_with_the_last_options_and_start_uses_them() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    rig.send(AnnotateMessage::Open);
    let dialog = rig.state.annotate_dialog.clone().expect("dialog");
    assert_eq!(dialog.options, AnnotateOptions::default());
    assert_eq!(dialog.moves, 4);
    assert!(!dialog.book_available, "book disabled until M5");
    assert_eq!(dialog.estimated_duration(), None);
    let mut options = dialog.options;
    options.with_vars = WithVars::No;
    options.depth = 99;
    rig.send(AnnotateMessage::SetOptions(options));
    rig.send(AnnotateMessage::SetEngine(Some(rig.run)));
    let fx = rig.start();
    assert!(rig.state.annotate_dialog.is_none());
    assert_eq!(rig.state.annotate_options.depth, 99);
    assert_eq!(last_search(&fx).unwrap().limits.depth, Some(99));
}

#[test]
fn the_dialog_counts_the_moves_and_estimates_the_time() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    rig.send(AnnotateMessage::Open);
    let mut options = AnnotateOptions::default();
    options.opening_only = Some(5);
    rig.send(AnnotateMessage::SetOptions(options));
    assert_eq!(rig.state.annotate_dialog.as_ref().unwrap().moves, 10);
    options.opening_only = None;
    options.control = Control::Time;
    options.movetime = Duration::from_secs(5);
    rig.send(AnnotateMessage::SetOptions(options));
    let dialog = rig.state.annotate_dialog.clone().unwrap();
    assert_eq!(dialog.moves, 20);
    assert_eq!(dialog.estimated_duration(), Some(Duration::from_secs(100)));
    rig.send(AnnotateMessage::Close);
    assert!(rig.state.annotate_dialog.is_none());
    assert!(!rig.running());
}

// ---- hostile engine output ----

#[test]
fn an_illegal_or_stale_pv_does_not_panic_and_adds_what_it_can() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
    let stale = Position::new().parse_uci("e2e4").unwrap();
    let log = rig.run_all(&mut |_, _| Reply::cp(0, vec![stale; 5]));
    assert!(!rig.running());
    assert_eq!(report(&rig).outcome, AnnotateOutcome::Finished);
    assert_eq!(log.requests.len(), 7);
}

#[test]
fn absurd_scores_do_not_panic() {
    for score in [
        Score::Cp(i32::MAX),
        Score::Cp(i32::MIN),
        Score::Mate(i32::MAX),
        Score::Mate(i32::MIN),
        Score::Mate(0),
    ] {
        let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
        rig.options(|o| o.mark_exercises = true);
        let mut n = 0;
        rig.run_all(&mut |_, pos: &Position| {
            n += 1;
            let mut reply = Reply::cp(0, pos.legal_moves().into_iter().take(3).collect());
            if n % 2 == 0 {
                reply.score = score;
                reply.second = Some(score);
            }
            reply
        });
        assert!(!rig.running(), "{score:?}");
    }
}

#[test]
fn a_report_without_pv_falls_back_to_the_best_move() {
    // D888: an empty PV reuses the best move of the search.
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5", "d1h5", "b8c6", "h5e5"]));
    rig.options(|o| o.var_at_end = false);
    let best = Position::new();
    let _ = best;
    let log_fx = rig.start();
    let mut fx = log_fx;
    while rig.running() {
        let request = last_search(&fx).expect("search");
        let pos = position_of(&request);
        let reply = material_reply(&pos);
        // Report the score without a PV; the best move comes with the end.
        let search = SearchId(u64::try_from(ply_of(&request)).unwrap() + 1000);
        rig.event(EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        });
        rig.event(EngineEvent::SearchStarted(search));
        rig.event(EngineEvent::Info(SearchInfo {
            search,
            multipv: 1,
            depth: Some(13),
            score: Some((reply.score, Bound::Exact)),
            pv: Vec::new(),
            ..SearchInfo::default()
        }));
        fx = rig.event(EngineEvent::BestMove(BestMove {
            search,
            mv: reply.pv.first().copied(),
            ponder: None,
        }));
    }
    let game = rig.game();
    assert!(
        game.children(nth_main(game, 4)).unwrap().len() >= 2,
        "the line"
    );
}

// ---- a very long game ----

fn shuffle_game(plies: usize) -> Game {
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let ucis: Vec<&str> = (0..plies).map(|i| cycle[i % 4]).collect();
    game_of(&ucis)
}

#[test]
fn a_game_of_more_than_five_hundred_plies_finishes_with_one_undo_point() {
    let plies = 600;
    let mut rig = Rig::new(shuffle_game(plies));
    rig.options(|o| o.var_at_end = false);
    let undo = rig.state.session.undo_depth();
    let started = std::time::Instant::now();
    let log = rig.run_all(&mut |_, pos: &Position| {
        Reply::cp(0, pos.legal_moves().into_iter().take(1).collect())
    });
    let took = started.elapsed();
    assert_eq!(log.requests.len(), plies + 1);
    assert_eq!(report(&rig).plies, 600);
    assert_eq!(rig.state.session.undo_depth(), undo + 1);
    assert!(
        took < Duration::from_secs(60),
        "600 plies took {took:?} (GameSession::edit cost per move)"
    );
    let game = rig.game();
    assert!(comment_of(game, nth_main(game, plies)).is_some());
}

#[test]
#[ignore = "perf finding: 4x the plies takes ~14x the time (GameSession::edit is O(n) per move); run with --ignored"]
fn the_cost_of_a_long_run_grows_roughly_linearly() {
    let time = |plies: usize| {
        let mut rig = Rig::new(shuffle_game(plies));
        rig.options(|o| o.var_at_end = false);
        let t = std::time::Instant::now();
        rig.run_all(&mut |_, pos: &Position| {
            Reply::cp(0, pos.legal_moves().into_iter().take(1).collect())
        });
        t.elapsed()
    };
    let small = time(400).as_secs_f64().max(1e-3);
    let big = time(1600).as_secs_f64();
    assert!(
        big / small < 12.0,
        "4x the plies took {:.1}x the time",
        big / small
    );
}
