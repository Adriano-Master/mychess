//! Contract tests of the fixes of the review of M4-10 phases b/c/d (spec
//! `m4-10-relatorio-da-partida.md`, D1580-D1586): the interceptor leaks no
//! move while trying, a promotion without a piece judges nothing, the
//! engine's own move is never a fault, the report keeps the user's NAG,
//! reads `[%eval` like the rest of the program, handles a mate at the last
//! position, and a cancelled rerun or a preset change never degrades the
//! report on show.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::type_complexity
)]

mod m4_6_common;

use m4_6_common::{
    HANGING_QUEEN, Reply, Rig, game_of, last_search, material_engine, material_reply, nags_of,
    position_of, pv_of,
};
use mychess_app::report::{MoveClass, ReportMessage, ReportPreset, ReviewStep, SideFilter};
use mychess_app::{AnnotateNotice, AppNotice, AppState, GameMessage, PlayOptions};
use mychess_core::{Game, Nag, Position, Square};
use mychess_engine::SearchRequest;

type Engine = dyn FnMut(&SearchRequest, &Position) -> Reply;

fn finished(game: Game, engine: &mut Engine) -> Rig {
    let mut rig = Rig::new(game);
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, engine);
    assert!(rig.state.report_data().is_some(), "the report");
    rig
}

fn trying(rig: &mut Rig) -> usize {
    rig.send(ReportMessage::StartReview {
        side: SideFilter::Both,
        highlights: false,
    });
    rig.send(ReportMessage::ReviewTry);
    assert_eq!(
        rig.state.report.review.as_ref().map(|r| r.step),
        Some(ReviewStep::Trying)
    );
    rig.state
        .report
        .review
        .as_ref()
        .and_then(mychess_app::report::Review::current)
        .expect("a moment")
        .ply
}

fn attempts(rig: &Rig) -> u32 {
    rig.state.report.review.as_ref().map_or(0, |r| r.attempts)
}

// ---- 1. the interceptor does not leak ----

#[test]
fn trying_consumes_typed_text_commit_and_null_move_without_touching_the_game() {
    let mut rig = finished(game_of(&HANGING_QUEEN), &mut material_engine);
    trying(&mut rig);
    let before = rig.game().clone();
    let (changes, cursor, undo) = (
        rig.state.session.changes(),
        rig.state.session.cursor(),
        rig.state.session.undo_depth(),
    );
    for message in [
        GameMessage::EnterMoveText("a3".to_owned()),
        GameMessage::MoveEntryChar('a'),
        GameMessage::MoveEntryChar('3'),
        GameMessage::MoveEntryCommit,
        GameMessage::PlayNullMove,
    ] {
        rig.send(message);
        assert_eq!(rig.state.session.changes(), changes);
        assert_eq!(rig.state.session.cursor(), cursor);
        assert_eq!(rig.state.session.undo_depth(), undo);
        assert!(rig.game().same_content(&before));
    }
    assert!(attempts(&rig) >= 1, "the typed move was judged");
}

// ---- 2. a promotion without a piece judges nothing ----

#[test]
fn a_promotion_left_without_a_piece_is_not_judged_and_writes_nothing() {
    let game = game_of(&[
        "a2a4", "b7b5", "a4b5", "g8f6", "b5b6", "f6e4", "b6c7", "e4g5", "c7b8q",
    ]);
    let mut rig = finished(game, &mut material_engine);
    // The real promotion position is covered by the unit test of the state
    // (`a_promotion_without_a_piece_asks...`); here the contract is that a
    // dismissed or impossible promotion at the moment on show judges and
    // changes nothing.
    trying(&mut rig);
    let before = rig.game().clone();
    let (changes, cursor) = (rig.state.session.changes(), rig.state.session.cursor());
    let count = attempts(&rig);
    rig.send(GameMessage::PlaySquares {
        from: Square::C7,
        to: Square::B8,
        promotion: None,
        options: PlayOptions::default(),
    });
    rig.send(GameMessage::AnswerPromotion(None));
    assert_eq!(attempts(&rig), count, "nothing was judged");
    assert_eq!(rig.state.session.changes(), changes);
    assert_eq!(rig.state.session.cursor(), cursor);
    assert!(rig.game().same_content(&before));
}

// ---- 3. the engine's own move is never a fault (D1581) ----

#[test]
fn the_engines_own_move_is_best_even_when_the_next_search_drops() {
    let ucis = ["e2e4", "e7e5", "g1f3", "b8c6"];
    let game = game_of(&ucis);
    let mut engine = move |request: &SearchRequest, pos: &Position| -> Reply {
        let ply = request.position.moves.len();
        let pv = ucis.get(ply).map(|u| pv_of(pos, &[u])).unwrap_or_default();
        // The position after 3.Nf3 looks much worse to the next search.
        Reply::cp(if ply == 3 { -400 } else { 20 }, pv)
    };
    let rig = finished(game, &mut engine);
    for verdict in &rig.state.report_data().unwrap().report.moves {
        assert_eq!(verdict.class, MoveClass::Best, "{verdict:?}");
        assert_eq!(verdict.loss, 0.0);
    }
}

// ---- 4. a judgement NAG of the user stands alone (D1582) ----

#[test]
fn a_judgement_nag_of_the_user_gets_no_conflicting_second_one() {
    for user in [1u16, 2, 3, 4, 5, 6] {
        let mut game = game_of(&HANGING_QUEEN);
        let blunder = game.mainline().nth(4).expect("3.Qxe5");
        let nag = Nag::new(user).unwrap();
        game.add_nag(blunder, nag).unwrap();
        let mut rig = finished(game, &mut material_engine);
        rig.send(ReportMessage::WriteToGame);
        assert_eq!(nags_of(rig.game(), blunder), vec![user], "NAG {user}");
    }
}

// ---- 5. `[%eval` is read like the rest of the program ----

fn state_with_comments(comments: &[&str]) -> (AppState, Vec<mychess_core::NodeId>) {
    let mut state = AppState::default();
    let mut game = game_of(&HANGING_QUEEN[..comments.len()]);
    let nodes: Vec<_> = game.mainline().collect();
    for (node, text) in nodes.iter().zip(comments) {
        game.set_comment(*node, text).unwrap();
    }
    state.session.replace_game(game);
    (state, nodes)
}

#[test]
fn the_eval_command_wins_over_other_signs_in_the_comment() {
    let (mut state, _) = state_with_comments(&[
        "+/- think [%eval 0.30] -9 trap",
        "[%eval 0.30]",
        "[%eval 0.20]",
        "[%eval 0.30]",
        "[%eval -6.00]",
    ]);
    drop(mychess_app::update(
        &mut state,
        ReportMessage::FromGame.into(),
    ));
    let data = state.report_data().expect("a report from the game");
    assert_eq!(data.report.moves.len(), 5);
    assert!(
        !data.report.moves[0].class.is_fault(),
        "the -9 outside the command was not read: {:?}",
        data.report.moves[0]
    );
}

#[test]
fn writing_keeps_the_eval_of_a_comment_and_adds_none_where_the_sign_is_a_symbol() {
    let mut game = game_of(&HANGING_QUEEN[..8]);
    let nodes: Vec<_> = game.mainline().collect();
    game.set_comment(nodes[0], "better is +/- here").unwrap();
    let mut rig = finished(game, &mut material_engine);
    let undo = rig.state.session.undo_depth();
    rig.send(ReportMessage::WriteToGame);
    assert_eq!(rig.state.session.undo_depth(), undo + 1);
    let comment = rig.game().comment(nodes[0]).unwrap().unwrap().to_owned();
    assert_eq!(comment.matches("[%eval").count(), 1, "{comment}");
    assert!(comment.contains("+/- here"), "{comment}");
}

#[test]
fn writing_a_second_time_creates_no_empty_undo_point() {
    let mut rig = finished(game_of(&HANGING_QUEEN), &mut material_engine);
    rig.send(ReportMessage::WriteToGame);
    let depth = rig.state.session.undo_depth();
    let changes = rig.state.session.changes();
    rig.send(ReportMessage::WriteToGame);
    assert_eq!(rig.state.session.undo_depth(), depth);
    assert_eq!(rig.state.session.changes(), changes);
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::ReportWritten {
            moves: 0,
            variations: 0
        }))
    ));
}

// ---- 6. a mate at the last position ----

#[test]
fn a_game_that_ends_in_mate_is_reported_and_written_without_a_fault() {
    for ucis in [
        &["f2f3", "e7e5", "g2g4", "d8h4"][..],
        &["e2e4", "f7f6", "d2d4", "g7g5", "d1h5"][..],
    ] {
        let mut rig = finished(game_of(ucis), &mut material_engine);
        let data = rig.state.report_data().unwrap();
        assert_eq!(data.report.moves.len(), ucis.len());
        let last = data.report.moves.last().unwrap();
        assert!(!last.class.is_fault(), "{last:?}");
        rig.send(ReportMessage::WriteToGame);
        let node = rig.game().mainline().last().unwrap();
        let comment = rig
            .game()
            .comment(node)
            .unwrap()
            .unwrap_or_default()
            .to_owned();
        assert!(comment.contains("[%eval"), "{comment}");
        // And back: the report rebuilt from the game keeps every move.
        rig.send(ReportMessage::FromGame);
        assert_eq!(
            rig.state.report_data().unwrap().report.moves.len(),
            ucis.len()
        );
    }
}

#[test]
fn a_mate_eval_in_the_last_comment_is_read_back() {
    let (mut state, _) = state_with_comments(&[
        "[%eval 0.20]",
        "[%eval 0.20]",
        "[%eval 0.30]",
        "[%eval #-1]",
    ]);
    drop(mychess_app::update(
        &mut state,
        ReportMessage::FromGame.into(),
    ));
    let data = state.report_data().expect("a report");
    assert_eq!(data.report.moves.len(), 4);
}

// ---- 7. a cancelled rerun keeps the complete report ----

#[test]
fn a_cancelled_rerun_keeps_the_complete_report_and_says_so() {
    let mut rig = finished(game_of(&HANGING_QUEEN), &mut material_engine);
    let full = rig.state.report_data().unwrap().report.moves.len();
    let fx = rig.send(ReportMessage::Generate);
    let log = rig.drive(fx, 3, &mut material_engine);
    rig.send(ReportMessage::Cancel);
    let request = last_search(&log.last).expect("a search in flight");
    let reply = material_reply(&position_of(&request));
    rig.answer(&log.last, &reply);
    assert!(!rig.running());
    assert_eq!(rig.state.report_data().unwrap().report.moves.len(), full);
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::ReportKept { .. }))
    ));
}

// ---- 8. the preset during a run; the review follows the report ----

#[test]
fn the_preset_cannot_change_during_a_run_but_can_after_it() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    let fx = rig.send(ReportMessage::Generate);
    let log = rig.drive(fx, 2, &mut material_engine);
    let started = rig.state.report.preset;
    let other = ReportPreset::ALL
        .iter()
        .copied()
        .find(|p| *p != started)
        .expect("another preset");
    rig.send(ReportMessage::SetPreset(other));
    assert_eq!(rig.state.report.preset, started);
    drop(log);
    rig.send(ReportMessage::Cancel);
    rig.send(ReportMessage::SetPreset(other));
    // After the run (cancelled or not) the choice is free again.
    if !rig.running() {
        assert_eq!(rig.state.report.preset, other);
    }
}

#[test]
fn the_review_ends_when_the_report_is_cleared_or_regenerated() {
    let mut rig = finished(game_of(&HANGING_QUEEN), &mut material_engine);
    trying(&mut rig);
    rig.send(ReportMessage::Clear);
    assert!(rig.state.report.review.is_none());

    let mut rig = finished(game_of(&HANGING_QUEEN), &mut material_engine);
    trying(&mut rig);
    rig.send(ReportMessage::Generate);
    assert!(rig.state.report.review.is_none());
}
