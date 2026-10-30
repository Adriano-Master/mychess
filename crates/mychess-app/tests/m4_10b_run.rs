//! M4-10b contract tests of the execution of the game report (spec
//! `m4-10-relatorio-da-partida.md`, D1422, D1560-D1565, D1441, D1564): a
//! synthetic UCI engine answers the searches; the run only collects, keeps
//! a partial report when it stops, restores the engine and excludes the
//! other uses of the game and of the engines.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::type_complexity
)]

mod m4_6_common;

use std::time::Instant;

use m4_6_common::{
    HANGING_QUEEN, Reply, Rig, commands, game_of, identity, last_search, material_engine,
    material_reply, own_book_option, position_of,
};
use mychess_app::engines::EngineDraft;
use mychess_app::online::{MarkerInfo, OnlineNotice};
use mychess_app::report::{MoveClass, ReportMessage, ReportPreset, marker_kind};
use mychess_app::tournament::{Participant, TimeControl, TournamentConfig, TournamentMessage};
use mychess_app::{
    AnnotateMessage, AnnotateNotice, AppNotice, ColorChoice, Effect, GameMessage, Message, Nav,
    PlayConfig, PlayMessage, PlayOptions, TimeMode,
};
use mychess_core::{Game, Position};
use mychess_engine::{
    BestMove, EngineCommand, EngineEvent, ExitInfo, ExitReason, OptionValue, Score, SearchId,
    SearchRequest,
};

fn generate(rig: &mut Rig) -> Vec<Effect> {
    rig.send(ReportMessage::Generate)
}

/// A rig that ran the whole report with `engine`.
fn finished(
    game: Game,
    engine: &mut dyn FnMut(&SearchRequest, &Position) -> Reply,
) -> (Rig, m4_6_common::Log) {
    let mut rig = Rig::new(game);
    let fx = generate(&mut rig);
    let log = rig.drive(fx, 5000, engine);
    (rig, log)
}

fn analyzing(rig: &Rig) -> bool {
    rig.state.engine_runs.get(rig.run).expect("run").analyzing
}

fn stopped(rig: &Rig) -> Option<(u32, u32)> {
    match &rig.state.notice {
        Some(AppNotice::Annotate(AnnotateNotice::ReportStopped { plies, total })) => {
            Some((*plies, *total))
        }
        _ => None,
    }
}

// ---- the run ----

#[test]
fn each_preset_searches_every_position_once_at_its_depth() {
    for (preset, depth) in [
        (ReportPreset::Fast, 12),
        (ReportPreset::Standard, 16),
        (ReportPreset::Deep, 22),
    ] {
        let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
        rig.send(ReportMessage::SetPreset(preset));
        let fx = generate(&mut rig);
        let log = rig.drive(fx, 100, &mut material_engine);
        assert!(!rig.running(), "{preset:?}");
        assert_eq!(log.requests.len(), 7, "{preset:?}: one search per position");
        assert!(
            log.requests.iter().all(|r| r.limits.depth == Some(depth)),
            "{preset:?}"
        );
        assert!(log.requests.iter().all(|r| r.limits.movetime.is_none()));
        assert_eq!(rig.state.report.preset, preset);
        assert_eq!(rig.state.report_data().unwrap().preset, preset);
    }
}

#[test]
fn the_default_preset_is_standard_at_depth_16() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..2]));
    let fx = generate(&mut rig);
    let log = rig.drive(fx, 10, &mut material_engine);
    assert!(log.requests.iter().all(|r| r.limits.depth == Some(16)));
}

#[test]
fn the_run_asks_for_two_lines_without_the_own_book_and_puts_the_engine_back() {
    let mut draft = EngineDraft::new("Fake", "/fake");
    draft.multipv = 3;
    let mut rig = Rig::with(
        game_of(&HANGING_QUEEN[..4]),
        identity(vec![own_book_option()], Some(5)),
        draft,
    );
    assert!(analyzing(&rig));
    let fx = generate(&mut rig);
    assert!(!analyzing(&rig), "the board analysis is off during the run");
    let first = commands(&fx);
    assert!(first.contains(&EngineCommand::SetMultiPv(2)), "{first:?}");
    assert!(first.contains(&EngineCommand::SetOption {
        name: "OwnBook".to_owned(),
        value: OptionValue::Bool(false),
    }));
    let log = rig.drive(fx, 100, &mut material_engine);
    assert!(log.commands.contains(&EngineCommand::SetMultiPv(3)));
    assert!(log.commands.contains(&EngineCommand::SetOption {
        name: "OwnBook".to_owned(),
        value: OptionValue::Bool(true),
    }));
    assert!(analyzing(&rig), "the analysis of the board is back");
}

#[test]
fn an_engine_limited_to_one_line_is_asked_for_no_second_line_and_has_no_great_moves() {
    let mut rig = Rig::with(
        game_of(&HANGING_QUEEN[..8]),
        identity(Vec::new(), Some(1)),
        EngineDraft::new("Fake", "/fake"),
    );
    let fx = generate(&mut rig);
    let log = rig.drive(fx, 100, &mut material_engine);
    assert!(
        !log.commands.contains(&EngineCommand::SetMultiPv(2)),
        "limited to the maximum of the engine"
    );
    let data = rig.state.report_data().expect("report");
    assert!(
        data.report
            .moves
            .iter()
            .all(|m| m.class != MoveClass::Great)
    );
}

#[test]
fn an_engine_without_multipv_still_gives_a_report() {
    let mut rig = Rig::with(
        game_of(&HANGING_QUEEN[..8]),
        identity(Vec::new(), None),
        EngineDraft::new("Fake", "/fake"),
    );
    let fx = generate(&mut rig);
    let log = rig.drive(fx, 100, &mut material_engine);
    assert!(
        !log.commands
            .iter()
            .any(|c| matches!(c, EngineCommand::SetMultiPv(n) if *n > 1))
    );
    assert!(rig.state.report_data().unwrap().complete);
}

/// An engine that reports a second line 5 pawns behind the best one.
fn engine_with_second_line(request: &SearchRequest, pos: &Position) -> Reply {
    let _ = request;
    let mut reply = material_reply(pos);
    if let Score::Cp(cp) = reply.score {
        let behind = if pos.turn() == mychess_core::Color::White {
            cp - 500
        } else {
            cp + 500
        };
        reply.second = Some(Score::Cp(behind));
    }
    reply
}

#[test]
fn a_second_line_far_behind_makes_the_best_move_great_and_draws_its_marker() {
    let (rig, _) = finished(game_of(&HANGING_QUEEN[..8]), &mut engine_with_second_line);
    let data = rig.state.report_data().expect("report");
    let great: Vec<usize> = data
        .report
        .moves
        .iter()
        .filter(|m| m.class == MoveClass::Great)
        .map(|m| m.ply)
        .collect();
    assert!(!great.is_empty(), "{:?}", data.report.moves);
    let markers = &data.graph.line.as_ref().expect("a curve").markers;
    for ply in great {
        assert!(
            markers
                .iter()
                .any(|m| m.ply == ply && Some(m.kind) == marker_kind(MoveClass::Great)),
            "ply {ply}"
        );
    }
}

#[test]
fn the_run_only_collects_and_never_edits_the_game() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    let before = rig.game().clone();
    let (changes, depth, cursor) = (
        rig.state.session.changes(),
        rig.state.session.undo_depth(),
        rig.state.session.cursor(),
    );
    let fx = generate(&mut rig);
    rig.drive(fx, 100, &mut material_engine);
    assert!(rig.game().same_content(&before), "comments, NAGs and tags");
    assert_eq!(rig.state.session.changes(), changes);
    assert_eq!(rig.state.session.undo_depth(), depth);
    assert_eq!(rig.state.session.cursor(), cursor);
    assert!(!rig.state.is_dirty());
    // The report does not take the place of the annotation's last report.
    assert!(rig.state.annotate_report.is_none());
}

#[test]
fn a_finished_run_says_nothing_about_stopping() {
    let (rig, _) = finished(game_of(&HANGING_QUEEN[..4]), &mut material_engine);
    assert_eq!(stopped(&rig), None);
    assert!(rig.state.report_data().unwrap().complete);
}

#[test]
fn a_game_without_moves_gets_no_run_and_leaves_the_engine_alone() {
    let mut rig = Rig::new(Game::new());
    let fx = generate(&mut rig);
    assert!(!rig.running());
    // Only the analysis of the board may be sent again, never a search of
    // the run or a reconfiguration.
    assert!(!commands(&fx).iter().any(|c| matches!(
        c,
        EngineCommand::Stop | EngineCommand::SetMultiPv(_) | EngineCommand::SetOption { .. }
    )));
    assert!(
        last_search(&fx)
            .is_none_or(|r| r.tag & m4_6_common::ANNOTATE_TAG != m4_6_common::ANNOTATE_TAG)
    );
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::NothingToAnnotate))
    ));
}

// ---- stopping: cancel, failure, watchdog ----

#[test]
fn cancelling_restores_the_engine_and_keeps_the_positions_analysed() {
    let mut draft = EngineDraft::new("Fake", "/fake");
    draft.multipv = 3;
    let mut rig = Rig::with(
        game_of(&HANGING_QUEEN),
        identity(vec![own_book_option()], Some(5)),
        draft,
    );
    let fx = generate(&mut rig);
    let log = rig.drive(fx, 6, &mut material_engine);
    assert!(rig.state.report_running());
    let request = last_search(&log.last).expect("in flight");
    rig.event(EngineEvent::SearchTagged {
        search: SearchId(9000),
        tag: request.tag,
    });
    rig.event(EngineEvent::SearchStarted(SearchId(9000)));
    let fx = rig.send(ReportMessage::Cancel);
    assert!(commands(&fx).contains(&EngineCommand::Stop));
    let after = rig.event(EngineEvent::BestMove(BestMove {
        search: SearchId(9000),
        mv: None,
        ponder: None,
    }));
    assert!(!rig.running());
    let all = commands(&after);
    assert!(all.contains(&EngineCommand::SetMultiPv(3)), "{all:?}");
    assert!(all.contains(&EngineCommand::SetOption {
        name: "OwnBook".to_owned(),
        value: OptionValue::Bool(true),
    }));
    assert!(analyzing(&rig));
    let data = rig.state.report_data().expect("partial report");
    assert!(!data.complete);
    assert_eq!(data.plies, 20);
    assert_eq!(data.report.moves.len(), 5, "six positions, five moves");
    let (done, total) = stopped(&rig).expect("the notice");
    assert_eq!((done, total), (5, 20));
}

#[test]
fn cancelling_without_a_run_does_nothing() {
    let (mut rig, _) = finished(game_of(&HANGING_QUEEN[..4]), &mut material_engine);
    rig.state.notice = None;
    let fx = rig.send(ReportMessage::Cancel);
    assert!(fx.is_empty());
    assert!(rig.state.notice.is_none());
    assert!(rig.state.report_data().is_some(), "the report stays");
}

#[test]
fn a_dying_engine_keeps_what_it_analysed_and_says_the_report_stopped() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    let fx = generate(&mut rig);
    rig.drive(fx, 6, &mut material_engine);
    rig.event(EngineEvent::Exited(ExitInfo {
        reason: ExitReason::Crashed,
        status: None,
        stderr_tail: Vec::new(),
    }));
    assert!(!rig.running());
    let data = rig.state.report_data().expect("partial report");
    assert!(!data.complete);
    assert_eq!(data.report.moves.len(), 5);
    assert_eq!(stopped(&rig), Some((5, 20)));
}

#[test]
fn an_engine_that_never_scores_leaves_no_report_and_says_so() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    let fx = generate(&mut rig);
    let mut silent = |_: &SearchRequest, pos: &Position| {
        let mut reply = material_reply(pos);
        reply.silent = true;
        reply
    };
    rig.drive(fx, 10, &mut silent);
    assert!(!rig.running());
    assert!(rig.state.report_data().is_none());
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::ReportStopped { .. }))
    ));
}

#[test]
fn the_watchdog_gives_up_on_a_silent_engine_and_keeps_the_partial_report() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    let fx = generate(&mut rig);
    let log = rig.drive(fx, 4, &mut material_engine);
    assert!(rig.state.report_running());
    let tick = log
        .last
        .iter()
        .rev()
        .find_map(|e| match e {
            Effect::ScheduleAnnotateTick {
                run, generation, ..
            } => Some((*run, *generation)),
            _ => None,
        })
        .expect("the search armed the watchdog");
    // A timer of another generation is a stale one: ignored.
    rig.send(AnnotateMessage::Tick {
        run: tick.0,
        generation: tick.1.wrapping_add(1),
    });
    assert!(rig.state.report_running());
    rig.send(AnnotateMessage::Tick {
        run: tick.0,
        generation: tick.1,
    });
    assert!(!rig.running(), "the silent engine is given up");
    let data = rig.state.report_data().expect("partial report");
    assert!(!data.complete);
    assert_eq!(data.report.moves.len(), 3, "four positions, three moves");
    assert_eq!(stopped(&rig), Some((3, 20)));
    assert!(analyzing(&rig), "and the engine is put back");
}

#[test]
fn a_game_replaced_during_the_run_gives_no_report_and_says_the_game_changed() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
    let fx = generate(&mut rig);
    rig.drive(fx, 2, &mut material_engine);
    rig.state.session.replace_game(game_of(&["d2d4"]));
    rig.send(GameMessage::AutoplayTick { generation: 0 });
    assert!(!rig.running());
    assert!(rig.state.report.data.is_none());
    assert!(rig.state.report_data().is_none());
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::GameChanged { .. }))
    ));
    assert!(analyzing(&rig));
}

#[test]
fn an_edit_that_bypasses_the_messages_during_the_run_also_discards_it() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
    let fx = generate(&mut rig);
    rig.drive(fx, 2, &mut material_engine);
    let root = rig.game().root();
    rig.state.session.update(GameMessage::SetComment {
        node: root,
        text: "someone".to_owned(),
    });
    rig.send(GameMessage::AutoplayTick { generation: 0 });
    assert!(!rig.running());
    assert!(rig.state.report.data.is_none());
}

// ---- the game is locked while the run goes ----

#[test]
fn the_game_cannot_be_edited_or_navigated_while_the_report_runs() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..6]));
    let fx = generate(&mut rig);
    let changes = rig.state.session.changes();
    let cursor = rig.state.session.cursor();
    let best = position_of(&last_search(&fx).unwrap()).legal_moves()[0];
    for message in [
        GameMessage::Navigate(Nav::Start),
        GameMessage::Undo,
        GameMessage::PlayMove {
            mv: best,
            options: PlayOptions::default(),
        },
    ] {
        rig.state.notice = None;
        rig.send(message);
        assert_eq!(rig.notice(), Some(AnnotateNotice::Busy));
    }
    assert_eq!(rig.state.session.changes(), changes);
    assert_eq!(rig.state.session.cursor(), cursor);
    assert!(rig.state.report_running());
}

#[test]
fn during_the_run_writing_clearing_reviewing_and_recalculating_do_nothing() {
    let (mut rig, _) = finished(game_of(&HANGING_QUEEN[..8]), &mut material_engine);
    assert!(rig.state.report_data().is_some());
    let fx = generate(&mut rig);
    assert!(rig.state.report_running());
    let changes = rig.state.session.changes();
    rig.send(ReportMessage::WriteToGame);
    assert_eq!(rig.state.session.changes(), changes, "no write");
    rig.send(ReportMessage::Clear);
    assert!(rig.state.report.data.is_some(), "Clear waits for the end");
    rig.send(ReportMessage::StartReview {
        side: mychess_app::report::SideFilter::Both,
        highlights: false,
    });
    assert!(rig.state.report.review.is_none());
    rig.state.notice = None;
    rig.send(ReportMessage::FromGame);
    assert_eq!(rig.notice(), Some(AnnotateNotice::Busy));
    rig.state.notice = None;
    rig.send(ReportMessage::Generate);
    assert_eq!(rig.notice(), Some(AnnotateNotice::AlreadyRunning));
    rig.state.notice = None;
    rig.send(AnnotateMessage::Start);
    assert_eq!(rig.notice(), Some(AnnotateNotice::AlreadyRunning));
    assert_eq!(rig.state.session.changes(), changes);
    drop(fx);
}

// ---- exclusivity ----

#[test]
fn a_game_against_an_engine_refuses_the_report() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    let mut config = PlayConfig::new(rig.engine);
    config.color = ColorChoice::White;
    config.mode = TimeMode::Depth(1);
    rig.send(PlayMessage::Start(Box::new(config)));
    assert!(rig.state.play_live(), "the game against the engine is on");
    let fx = generate(&mut rig);
    assert!(!rig.state.report_running());
    assert!(!rig.state.annotation_running());
    assert!(last_search(&fx).is_none());
    assert!(matches!(rig.state.notice, Some(AppNotice::Play(_))));
}

#[test]
fn the_report_refuses_a_game_against_an_engine_and_an_annotation_while_it_runs() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    generate(&mut rig);
    assert!(rig.state.report_running());
    let mut config = PlayConfig::new(rig.engine);
    config.color = ColorChoice::White;
    config.mode = TimeMode::Depth(1);
    rig.send(PlayMessage::Start(Box::new(config)));
    assert!(!rig.state.play_live(), "no game against the engine");
    assert!(rig.state.report_running(), "the report goes on");
    rig.send(AnnotateMessage::Start);
    assert!(rig.state.report_running(), "and is still the report");
    assert!(rig.state.annotate_report.is_none());
}

#[test]
fn an_annotation_in_progress_refuses_the_report() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    rig.start();
    assert!(rig.running() && !rig.state.report_running());
    rig.state.notice = None;
    generate(&mut rig);
    assert_eq!(rig.notice(), Some(AnnotateNotice::AlreadyRunning));
    assert!(!rig.state.report_running(), "still the annotation");
}

#[test]
fn a_tournament_refuses_the_report() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    let a = rig
        .state
        .engines
        .add(EngineDraft::new("A", "/fake/a"))
        .unwrap();
    let b = rig
        .state
        .engines
        .add(EngineDraft::new("B", "/fake/b"))
        .unwrap();
    let config = TournamentConfig {
        participants: vec![Participant::new(a.0, "A"), Participant::new(b.0, "B")],
        cycles: 2,
        time: TimeControl::Depth(1),
        ..TournamentConfig::default()
    };
    rig.send(Message::Tournament(TournamentMessage::Start {
        config: Box::new(config),
        at: Instant::now(),
    }));
    assert!(rig.state.tournament_live(), "the tournament is on");
    generate(&mut rig);
    assert!(!rig.state.report_running());
    assert!(!rig.state.annotation_running());
    assert!(matches!(rig.state.notice, Some(AppNotice::Tournament(_))));
}

#[test]
fn a_game_in_trial_mode_refuses_the_report() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    rig.send(GameMessage::EnterTrial { null_move: false });
    rig.state.notice = None;
    generate(&mut rig);
    assert!(!rig.state.report_running());
    assert_eq!(rig.notice(), Some(AnnotateNotice::TrialMode));
}

// ---- online (D1441, D1562) ----

fn lock(rig: &mut Rig) {
    rig.state.online.marker = Some(MarkerInfo {
        game_id: "abcd1234".to_owned(),
        date: "2026-10-24".to_owned(),
    });
    assert!(rig.state.assist_locked());
}

#[test]
fn generating_is_dropped_with_a_remark_during_an_online_game() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    lock(&mut rig);
    let fx = generate(&mut rig);
    assert!(fx.is_empty(), "{fx:?}");
    assert!(!rig.running());
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::AssistanceDisabled))
    );
    assert!(rig.state.engine_runs().iter().all(|r| r.analyzing));
}

#[test]
fn the_report_of_the_game_is_available_again_when_the_lock_is_freed() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    lock(&mut rig);
    generate(&mut rig);
    assert!(!rig.running());
    rig.state.online.marker = None;
    let fx = generate(&mut rig);
    rig.drive(fx, 20, &mut material_engine);
    assert!(rig.state.report_data().is_some());
}

#[test]
fn recalculating_reviewing_and_writing_are_dropped_during_an_online_game() {
    let (mut rig, _) = finished(game_of(&HANGING_QUEEN), &mut material_engine);
    assert!(rig.state.report_data().is_some());
    lock(&mut rig);
    let before = rig.game().clone();
    let undo = rig.state.session.undo_depth();
    for message in [
        ReportMessage::WriteToGame,
        ReportMessage::FromGame,
        ReportMessage::StartReview {
            side: mychess_app::report::SideFilter::Both,
            highlights: false,
        },
        ReportMessage::ReviewTry,
        ReportMessage::ReviewShowBest,
    ] {
        rig.state.notice = None;
        let fx = rig.send(message);
        assert!(fx.is_empty(), "{message:?}");
        assert_eq!(
            rig.state.notice,
            Some(AppNotice::Online(OnlineNotice::AssistanceDisabled)),
            "{message:?}"
        );
    }
    assert!(rig.game().same_content(&before));
    assert_eq!(rig.state.session.undo_depth(), undo);
    assert!(rig.state.report.review.is_none());
}

#[test]
fn the_messages_that_only_look_stay_allowed_during_an_online_game() {
    let (mut rig, _) = finished(game_of(&HANGING_QUEEN[..8]), &mut material_engine);
    lock(&mut rig);
    rig.state.notice = None;
    rig.send(ReportMessage::Filter(Some(MoveClass::Blunder)));
    rig.send(ReportMessage::SetPreset(ReportPreset::Deep));
    rig.send(ReportMessage::GoTo(2));
    rig.send(ReportMessage::Cancel);
    rig.send(ReportMessage::ReviewExit);
    assert!(rig.state.notice.is_none());
    assert_eq!(rig.state.report.filter, Some(MoveClass::Blunder));
    assert_eq!(rig.state.report.preset, ReportPreset::Deep);
    rig.send(ReportMessage::Clear);
    assert!(rig.state.report.data.is_none(), "Clear is a plain forget");
}

// ---- validity of the report (D1564) ----

#[test]
fn the_report_is_valid_only_while_the_main_line_is_the_same() {
    let (mut rig, _) = finished(game_of(&HANGING_QUEEN[..6]), &mut material_engine);
    assert!(rig.state.report_data().is_some());

    // Comments and variations do not invalidate it.
    let mut game = rig.game().clone();
    let first = game.mainline().next().unwrap();
    game.set_comment(first, "a note").unwrap();
    let mv = game
        .position_at(game.root())
        .unwrap()
        .parse_uci("d2d4")
        .unwrap();
    game.add_move(game.root(), mv).unwrap();
    rig.state.session.replace_game(game);
    assert!(rig.state.report_data().is_some(), "variation + comment");

    // A longer main line does.
    let mut longer = game_of(&HANGING_QUEEN[..6]);
    let last = longer.mainline().last().unwrap();
    let next = longer
        .position_at(last)
        .unwrap()
        .parse_uci(HANGING_QUEEN[6])
        .unwrap();
    longer.add_move(last, next).unwrap();
    rig.state.session.replace_game(longer);
    assert!(rig.state.report_data().is_none(), "one move more");

    // A shorter one does.
    rig.state.session.replace_game(game_of(&HANGING_QUEEN[..5]));
    assert!(rig.state.report_data().is_none(), "one move less");

    // A different move does.
    let mut other: Vec<&str> = HANGING_QUEEN[..6].to_vec();
    other[5] = "d8e7";
    rig.state.session.replace_game(game_of(&other));
    assert!(rig.state.report_data().is_none(), "another move");

    // Back to the same moves: the report applies again.
    rig.state.session.replace_game(game_of(&HANGING_QUEEN[..6]));
    assert!(rig.state.report_data().is_some());
}

#[test]
fn generating_and_clearing_change_the_stamp_of_the_curve_cache() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN[..4]));
    let s0 = rig.state.report.stamp;
    let fx = generate(&mut rig);
    rig.drive(fx, 20, &mut material_engine);
    let s1 = rig.state.report.stamp;
    assert_ne!(s0, s1);
    rig.send(ReportMessage::Clear);
    assert_ne!(rig.state.report.stamp, s1);
    assert!(rig.state.report.data.is_none());
}

#[test]
fn the_table_of_the_report_adds_up_to_the_moves_of_each_side() {
    let (rig, _) = finished(game_of(&HANGING_QUEEN), &mut material_engine);
    let data = rig.state.report_data().unwrap();
    assert_eq!(data.report.white.total(), 10);
    assert_eq!(data.report.black.total(), 10);
    for acc in [data.report.white.accuracy, data.report.black.accuracy] {
        let acc = acc.expect("moves outside the book");
        assert!((0.0..=100.0).contains(&acc), "{acc}");
    }
    assert_eq!(data.sans.len(), 20);
    assert_eq!(data.sans[4], "Qxe5+");
    assert_eq!(data.report.moves[4].ply, 5);
}

#[test]
fn the_same_game_and_the_same_analysis_give_the_same_report() {
    let (a, _) = finished(game_of(&HANGING_QUEEN), &mut material_engine);
    let (b, _) = finished(game_of(&HANGING_QUEEN), &mut material_engine);
    assert_eq!(
        a.state.report_data().unwrap().report,
        b.state.report_data().unwrap().report
    );
}

#[test]
fn the_curve_of_the_report_has_one_point_per_position_and_no_marker_on_quiet_moves() {
    let (rig, _) = finished(game_of(&HANGING_QUEEN), &mut material_engine);
    let data = rig.state.report_data().unwrap();
    assert_eq!(data.graph.plies, 20);
    let line = data.graph.line.as_ref().expect("a curve");
    for marker in &line.markers {
        let verdict = &data.report.moves[marker.ply - 1];
        assert_eq!(marker_kind(verdict.class), Some(marker.kind));
    }
    let quiet = data
        .report
        .moves
        .iter()
        .filter(|m| marker_kind(m.class).is_none())
        .count();
    assert_eq!(line.markers.len() + quiet, 20);
}
