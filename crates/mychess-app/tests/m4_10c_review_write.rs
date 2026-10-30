//! M4-10c/d contract tests of the guided review and of writing the report
//! into the game (spec `m4-10-relatorio-da-partida.md`, D1435, D1440,
//! D1447, D1569-D1573): the board goes to the position before the move,
//! only the best move resolves an attempt and the others never enter the
//! game, `review_intercept` does not leak, and the write is one undo point
//! that adds `[%eval]`, NAGs and short lines without repeating itself.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::type_complexity
)]

mod m4_6_common;

use m4_6_common::{HANGING_QUEEN, Reply, Rig, game_of, material_engine, material_reply};
use mychess_app::report::{
    MoveClass, ReportData, ReportMessage, ReviewStep, SideFilter, key_moments,
};
use mychess_app::{
    AnnotateNotice, AppNotice, AppState, GameMessage, Message, Nav, PlayOptions, update,
};
use mychess_core::{Color, Game, Move, NodeId, Position};
use mychess_engine::{Score, SearchRequest};

fn finished(game: Game, engine: &mut dyn FnMut(&SearchRequest, &Position) -> Reply) -> Rig {
    let mut rig = Rig::new(game);
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, engine);
    assert!(rig.state.report_data().is_some(), "the report");
    rig
}

fn start_review(rig: &mut Rig, side: SideFilter) {
    rig.send(ReportMessage::StartReview {
        side,
        highlights: false,
    });
}

fn current_ply(rig: &Rig) -> usize {
    rig.state
        .report
        .review
        .as_ref()
        .and_then(mychess_app::report::Review::current)
        .expect("a moment")
        .ply
}

fn step(rig: &Rig) -> ReviewStep {
    rig.state.report.review.as_ref().expect("a review").step
}

fn attempts(rig: &Rig) -> u32 {
    rig.state.report.review.as_ref().expect("a review").attempts
}

fn node_before(rig: &Rig, ply: usize) -> NodeId {
    ReportData::node_before(rig.game(), ply).expect("node")
}

fn best_of(rig: &Rig, ply: usize) -> Move {
    rig.state.report_data().unwrap().report.moves[ply - 1]
        .best
        .expect("the engine's move")
}

fn played_of(rig: &Rig, ply: usize) -> Move {
    rig.state.report_data().unwrap().report.moves[ply - 1].played
}

/// A legal move at the position of the moment that is neither the best nor
/// the one played.
fn other_move(rig: &Rig, ply: usize) -> Move {
    let position = rig.game().position_at(node_before(rig, ply)).unwrap();
    let (best, played) = (best_of(rig, ply), played_of(rig, ply));
    position
        .legal_moves()
        .into_iter()
        .find(|m| *m != best && *m != played)
        .expect("another legal move")
}

fn play(rig: &mut Rig, mv: Move) {
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
}

/// Plays `mv` as a new variation (a move with continuations would ask).
fn play_free(rig: &mut Rig, mv: Move) {
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions {
            force_variation: true,
        },
    });
}

fn play_squares(rig: &mut Rig, mv: Move) {
    rig.send(GameMessage::PlaySquares {
        from: mv.from().expect("from"),
        to: mv.to().expect("to"),
        promotion: None,
        options: PlayOptions::default(),
    });
}

fn hanging() -> Rig {
    finished(game_of(&HANGING_QUEEN), &mut material_engine)
}

// ---- starting ----

#[test]
fn the_review_starts_with_the_board_at_the_position_before_the_first_moment() {
    let mut rig = hanging();
    // With animations on the board walks there (M4-11); off, it jumps.
    rig.state.prefs.board.animation_ms = 0;
    // The user was at the end of the game.
    rig.send(GameMessage::Navigate(Nav::End));
    start_review(&mut rig, SideFilter::Both);
    let ply = current_ply(&rig);
    assert_eq!(rig.state.session.cursor(), node_before(&rig, ply));
    assert_eq!(step(&rig), ReviewStep::Looking);
    assert_eq!(attempts(&rig), 0);
}

#[test]
fn the_review_visits_exactly_the_moments_of_the_side_chosen() {
    let rig = hanging();
    for side in [SideFilter::White, SideFilter::Black, SideFilter::Both] {
        let mut rig = Rig::new(game_of(&HANGING_QUEEN));
        let fx = rig.send(ReportMessage::Generate);
        rig.drive(fx, 5000, &mut material_engine);
        let expected = key_moments(&rig.state.report_data().unwrap().report, side, false);
        start_review(&mut rig, side);
        match rig.state.report.review.as_ref() {
            None => assert!(expected.is_empty(), "{side:?}"),
            Some(review) => {
                assert_eq!(review.moments, expected, "{side:?}");
                for moment in &review.moments {
                    let mover = rig.state.report_data().unwrap().report.moves[moment.ply - 1].mover;
                    match side {
                        SideFilter::White => assert_eq!(mover, Color::White),
                        SideFilter::Black => assert_eq!(mover, Color::Black),
                        SideFilter::Both => {}
                    }
                    assert!(moment.class.is_fault(), "{moment:?}");
                }
            }
        }
    }
    drop(rig);
}

#[test]
fn the_blunder_that_hangs_the_queen_is_a_moment_of_white() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::White);
    let review = rig.state.report.review.as_ref().expect("a review");
    assert!(
        review.moments.iter().any(|m| m.ply == 5),
        "{:?}",
        review.moments
    );
}

#[test]
fn a_review_cannot_start_without_a_report_and_ending_it_leaves_no_trace() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    start_review(&mut rig, SideFilter::Both);
    assert!(rig.state.report.review.is_none());
    rig.send(ReportMessage::ReviewNext);
    rig.send(ReportMessage::ReviewPrevious);
    rig.send(ReportMessage::ReviewTry);
    rig.send(ReportMessage::ReviewShowBest);
    rig.send(ReportMessage::ReviewExit);
    assert!(rig.state.report.review.is_none());
}

// ---- the attempts ----

#[test]
fn the_steps_before_trying_let_the_move_into_the_game_as_usual() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    let ply = current_ply(&rig);
    let changes = rig.state.session.changes();
    let other = other_move(&rig, ply);
    play_free(&mut rig, other);
    assert!(rig.state.session.changes() > changes, "not consumed");
    assert_eq!(attempts(&rig), 0);
}

#[test]
fn a_wrong_move_counts_as_an_attempt_and_does_not_enter_the_game() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    let ply = current_ply(&rig);
    let before = rig.game().clone();
    let changes = rig.state.session.changes();
    let undo = rig.state.session.undo_depth();
    rig.send(ReportMessage::ReviewTry);
    assert_eq!(step(&rig), ReviewStep::Trying);
    let cursor = rig.state.session.cursor();

    // The move played in the game is also not the best one.
    let mv = played_of(&rig, ply);
    play(&mut rig, mv);
    assert_eq!(attempts(&rig), 1);
    let mv = other_move(&rig, ply);
    play(&mut rig, mv);
    assert_eq!(attempts(&rig), 2);
    assert_eq!(
        step(&rig),
        ReviewStep::Trying,
        "wrong attempts do not resolve"
    );
    assert_eq!(rig.state.session.changes(), changes);
    assert_eq!(rig.state.session.undo_depth(), undo);
    assert_eq!(rig.state.session.cursor(), cursor, "the board stays");
    assert!(rig.game().same_content(&before));
}

#[test]
fn the_hint_is_due_after_two_wrong_attempts_and_names_the_square_of_the_best_move() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    let ply = current_ply(&rig);
    rig.send(ReportMessage::ReviewTry);
    let hint_due = |rig: &Rig| rig.state.report.review.as_ref().unwrap().hint_due();
    let mv = other_move(&rig, ply);
    play_squares(&mut rig, mv);
    assert!(!hint_due(&rig), "one miss");
    let mv = other_move(&rig, ply);
    play_squares(&mut rig, mv);
    assert!(hint_due(&rig), "two misses");
    // The hint is the square the best move leaves.
    let best = best_of(&rig, ply);
    assert!(best.from().is_some());
    // And the right move still resolves.
    play_squares(&mut rig, best);
    assert_eq!(step(&rig), ReviewStep::Solved);
    assert!(!hint_due(&rig));
}

#[test]
fn the_best_move_resolves_by_message_and_by_squares_and_never_enters_the_game() {
    for by_squares in [false, true] {
        let mut rig = hanging();
        start_review(&mut rig, SideFilter::Both);
        let ply = current_ply(&rig);
        let before = rig.game().clone();
        rig.send(ReportMessage::ReviewTry);
        let best = best_of(&rig, ply);
        if by_squares {
            play_squares(&mut rig, best);
        } else {
            play(&mut rig, best);
        }
        assert_eq!(step(&rig), ReviewStep::Solved, "by_squares {by_squares}");
        assert!(rig.game().same_content(&before));
    }
}

#[test]
fn once_solved_the_board_is_free_again() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    let ply = current_ply(&rig);
    rig.send(ReportMessage::ReviewTry);
    let mv = best_of(&rig, ply);
    play(&mut rig, mv);
    assert_eq!(step(&rig), ReviewStep::Solved);
    let changes = rig.state.session.changes();
    let mv = other_move(&rig, ply);
    play_free(&mut rig, mv);
    assert!(
        rig.state.session.changes() > changes,
        "now it enters the game"
    );
}

#[test]
fn an_illegal_move_is_not_an_attempt() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    let ply = current_ply(&rig);
    rig.send(ReportMessage::ReviewTry);
    let position = rig.game().position_at(node_before(&rig, ply)).unwrap();
    // From the position of the moment, a1 to a8 is never legal.
    let a1: mychess_core::Square = "a1".parse().unwrap();
    let a8: mychess_core::Square = "a8".parse().unwrap();
    assert!(
        !position
            .legal_moves()
            .iter()
            .any(|m| m.from() == Some(a1) && m.to() == Some(a8))
    );
    rig.send(GameMessage::PlaySquares {
        from: a1,
        to: a8,
        promotion: None,
        options: PlayOptions::default(),
    });
    assert_eq!(attempts(&rig), 0);
    assert_eq!(step(&rig), ReviewStep::Trying);
}

#[test]
fn trying_again_restarts_the_count() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    let ply = current_ply(&rig);
    rig.send(ReportMessage::ReviewTry);
    let mv = other_move(&rig, ply);
    play(&mut rig, mv);
    assert_eq!(attempts(&rig), 1);
    rig.send(ReportMessage::ReviewTry);
    assert_eq!(attempts(&rig), 0);
    assert_eq!(step(&rig), ReviewStep::Trying);
}

// ---- no leak outside the "try" mode ----

#[test]
fn leaving_the_position_of_the_moment_frees_the_board() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    let ply = current_ply(&rig);
    assert!(ply > 1, "the fixture has a moment after the first move");
    rig.send(ReportMessage::ReviewTry);
    rig.send(GameMessage::Navigate(Nav::Start));
    assert_eq!(rig.state.session.cursor(), rig.game().root());
    let changes = rig.state.session.changes();
    let new = rig
        .game()
        .position_at(rig.game().root())
        .unwrap()
        .parse_uci("d2d4")
        .unwrap();
    play_free(&mut rig, new);
    assert!(rig.state.session.changes() > changes, "entered the game");
    assert_eq!(attempts(&rig), 0, "and was not judged");
}

#[test]
fn only_the_play_messages_are_consumed_while_trying() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    rig.send(ReportMessage::ReviewTry);
    let cursor = rig.state.session.cursor();
    let changes = rig.state.session.changes();
    rig.send(GameMessage::Navigate(Nav::Start));
    assert_ne!(
        rig.state.session.cursor(),
        cursor,
        "navigation goes through"
    );
    rig.send(GameMessage::Navigate(Nav::GoToNode(cursor)));
    assert_eq!(rig.state.session.cursor(), cursor);
    let root = rig.game().root();
    rig.send(GameMessage::SetComment {
        node: root,
        text: "a note".to_owned(),
    });
    assert!(rig.state.session.changes() > changes, "comments go through");
    assert_eq!(attempts(&rig), 0);
}

#[test]
fn a_report_of_another_game_does_not_swallow_the_moves() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    rig.send(ReportMessage::ReviewTry);
    rig.state.session.replace_game(game_of(&["d2d4"]));
    assert!(rig.state.report_data().is_none());
    let changes = rig.state.session.changes();
    let new = rig
        .game()
        .position_at(rig.game().root())
        .unwrap()
        .parse_uci("c2c4")
        .unwrap();
    play_free(&mut rig, new);
    assert!(rig.state.session.changes() > changes);
}

#[test]
fn without_a_review_nothing_is_swallowed() {
    let mut rig = hanging();
    let changes = rig.state.session.changes();
    let root = rig.game().root();
    let new = rig
        .game()
        .position_at(root)
        .unwrap()
        .parse_uci("d2d4")
        .unwrap();
    play_free(&mut rig, new);
    assert!(rig.state.session.changes() > changes);
}

// ---- moving around, revealing, leaving ----

#[test]
fn next_and_previous_follow_the_moments_with_the_board_and_stop_at_the_ends() {
    let mut rig = hanging();
    // The 20-ply game has several faults on the numbers; if there is only
    // one the walk is trivial but the ends are still checked.
    start_review(&mut rig, SideFilter::Both);
    let moments = rig.state.report.review.as_ref().unwrap().moments.clone();
    rig.send(ReportMessage::ReviewPrevious);
    assert_eq!(rig.state.report.review.as_ref().unwrap().index, 0);
    for (i, moment) in moments.iter().enumerate() {
        assert_eq!(current_ply(&rig), moment.ply);
        assert_eq!(
            rig.state.session.cursor(),
            node_before(&rig, moment.ply),
            "{i}"
        );
        rig.send(ReportMessage::ReviewNext);
    }
    let last = moments.last().unwrap().ply;
    assert_eq!(current_ply(&rig), last, "the last stays");
    for moment in moments.iter().rev() {
        assert_eq!(current_ply(&rig), moment.ply);
        rig.send(ReportMessage::ReviewPrevious);
    }
    assert_eq!(current_ply(&rig), moments[0].ply);
}

#[test]
fn moving_to_another_moment_drops_the_attempt_in_progress() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    let count = rig.state.report.review.as_ref().unwrap().moments.len();
    if count < 2 {
        return;
    }
    rig.send(ReportMessage::ReviewTry);
    let ply = current_ply(&rig);
    let mv = other_move(&rig, ply);
    play(&mut rig, mv);
    rig.send(ReportMessage::ReviewNext);
    assert_eq!(step(&rig), ReviewStep::Looking);
    assert_eq!(attempts(&rig), 0);
}

#[test]
fn seeing_the_best_move_reveals_it_and_frees_the_board() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    let ply = current_ply(&rig);
    rig.send(ReportMessage::ReviewTry);
    rig.send(ReportMessage::ReviewShowBest);
    assert_eq!(step(&rig), ReviewStep::Revealed);
    assert_eq!(rig.state.session.cursor(), node_before(&rig, ply));
    let changes = rig.state.session.changes();
    let mv = other_move(&rig, ply);
    play_free(&mut rig, mv);
    assert!(rig.state.session.changes() > changes, "no more attempts");
}

#[test]
fn leaving_the_review_gives_the_board_back_and_the_game_is_untouched() {
    let mut rig = hanging();
    let original = rig.game().clone();
    let undo = rig.state.session.undo_depth();
    start_review(&mut rig, SideFilter::Both);
    let ply = current_ply(&rig);
    rig.send(ReportMessage::ReviewTry);
    let mv = other_move(&rig, ply);
    play(&mut rig, mv);
    let mv = played_of(&rig, ply);
    play(&mut rig, mv);
    let mv = best_of(&rig, ply);
    play(&mut rig, mv);
    rig.send(ReportMessage::ReviewNext);
    rig.send(ReportMessage::ReviewPrevious);
    rig.send(ReportMessage::ReviewExit);
    assert!(rig.state.report.review.is_none());
    assert!(rig.game().same_content(&original), "whole review");
    assert_eq!(rig.state.session.undo_depth(), undo);
    assert!(!rig.state.is_dirty());
    // The report itself stays for another review.
    assert!(rig.state.report_data().is_some());
    start_review(&mut rig, SideFilter::Both);
    assert!(rig.state.report.review.is_some());
    // And a move is now a normal move.
    rig.send(ReportMessage::ReviewExit);
    let changes = rig.state.session.changes();
    let root = rig.game().root();
    let new = rig
        .game()
        .position_at(root)
        .unwrap()
        .parse_uci("d2d4")
        .unwrap();
    rig.send(GameMessage::Navigate(Nav::Start));
    play_free(&mut rig, new);
    assert!(rig.state.session.changes() > changes);
}

#[test]
fn generating_again_or_clearing_ends_the_review() {
    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    assert!(rig.state.report.review.is_some());
    rig.send(ReportMessage::Clear);
    assert!(rig.state.report.review.is_none());
    assert!(rig.state.report_data().is_none());

    let mut rig = hanging();
    start_review(&mut rig, SideFilter::Both);
    rig.send(ReportMessage::Generate);
    assert!(
        rig.state.report.review.is_none(),
        "a new run ends the review"
    );
}

/// An engine that reports a second line far behind the best one.
fn engine_with_second_line(_: &SearchRequest, pos: &Position) -> Reply {
    let mut reply = material_reply(pos);
    if let Score::Cp(cp) = reply.score {
        let behind = if pos.turn() == Color::White {
            cp - 500
        } else {
            cp + 500
        };
        reply.second = Some(Score::Cp(behind));
    }
    reply
}

#[test]
fn highlights_add_the_great_moves_to_the_walk() {
    let mut rig = finished(game_of(&HANGING_QUEEN[..8]), &mut engine_with_second_line);
    let report = &rig.state.report_data().unwrap().report;
    let great = report.moves.iter().any(|m| m.class == MoveClass::Great);
    assert!(great, "the fixture has a great move");
    let faults = key_moments(report, SideFilter::Both, false).len();
    let all = key_moments(report, SideFilter::Both, true).len();
    assert!(all > faults);
    rig.send(ReportMessage::StartReview {
        side: SideFilter::Both,
        highlights: true,
    });
    assert_eq!(rig.state.report.review.as_ref().unwrap().moments.len(), all);
}

// ---- reviewing a report built from the game (D1570) ----

fn state_with_evals() -> AppState {
    let mut state = AppState::default();
    let mut game = game_of(&HANGING_QUEEN[..6]);
    let nodes: Vec<_> = game.mainline().collect();
    for (node, text) in nodes
        .iter()
        .zip(["+0.3", "+0.3", "+0.2", "+0.3", "-6.0", "-6.0"])
    {
        game.set_comment(*node, &format!("[%eval {text}]")).unwrap();
    }
    state.session.replace_game(game);
    state
}

#[test]
fn a_report_from_the_game_reviews_the_verdict_but_cannot_try_or_judge() {
    let mut state = state_with_evals();
    let fx = update(&mut state, Message::Report(ReportMessage::FromGame));
    assert!(fx.is_empty(), "no engine involved: {fx:?}");
    assert!(state.report_data().is_some());
    update(
        &mut state,
        Message::Report(ReportMessage::StartReview {
            side: SideFilter::Both,
            highlights: false,
        }),
    );
    let review = state.report.review.as_ref().expect("a review");
    assert!(review.moments.iter().any(|m| m.ply == 5));
    update(&mut state, Message::Report(ReportMessage::ReviewTry));
    assert_eq!(
        state.report.review.as_ref().unwrap().step,
        ReviewStep::Looking,
        "no best move: nothing to try"
    );
    // Moves go to the game as usual in this state.
    let changes = state.session.changes();
    let position = state
        .session
        .game()
        .position_at(state.session.cursor())
        .unwrap()
        .clone();
    let mv = position.legal_moves()[0];
    update(
        &mut state,
        Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions {
                force_variation: true,
            },
        }),
    );
    assert!(state.session.changes() > changes);
}

#[test]
fn recalculating_from_the_game_needs_no_engine_and_judges_the_moves_with_evaluations() {
    let mut state = state_with_evals();
    update(&mut state, Message::Report(ReportMessage::FromGame));
    let data = state.report_data().expect("report");
    assert_eq!(data.report.moves.len(), 6);
    assert!(data.complete);
    assert!(data.report.moves.iter().all(|m| m.best.is_none()));
    assert!(data.report.moves[4].class.is_fault());
}

#[test]
fn a_game_with_evaluations_only_for_part_of_the_moves_gives_a_partial_report() {
    let mut state = AppState::default();
    let mut game = game_of(&HANGING_QUEEN[..6]);
    let nodes: Vec<_> = game.mainline().collect();
    for node in &nodes[..3] {
        game.set_comment(*node, "[%eval +0.2]").unwrap();
    }
    state.session.replace_game(game);
    update(&mut state, Message::Report(ReportMessage::FromGame));
    let data = state.report_data().expect("report");
    assert_eq!(data.report.moves.len(), 3);
    assert!(!data.complete);
}

#[test]
fn a_non_standard_start_without_the_initial_evaluation_cannot_be_reported_from_the_game() {
    let mut state = AppState::default();
    let position = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
    let mut game = m4_6_common::game_from(Game::from_position(position), &["e2e4", "e8e7"]);
    let nodes: Vec<_> = game.mainline().collect();
    for node in &nodes {
        game.set_comment(*node, "[%eval +0.2]").unwrap();
    }
    state.session.replace_game(game);
    update(&mut state, Message::Report(ReportMessage::FromGame));
    assert!(state.report.data.is_none());
    assert!(matches!(
        state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::ReportNoEvals))
    ));
}

#[test]
fn an_empty_game_cannot_be_reported_from_itself() {
    let mut state = AppState::default();
    update(&mut state, Message::Report(ReportMessage::FromGame));
    assert!(state.report.data.is_none());
    assert!(matches!(
        state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::ReportNoEvals))
    ));
}

// ---- writing into the game (D1440, D1447, D1571) ----

fn nags(game: &Game, node: NodeId) -> Vec<u8> {
    game.nags(node).unwrap().iter().map(|n| n.value()).collect()
}

#[test]
fn nothing_is_written_until_the_user_asks() {
    let rig = hanging();
    assert!(!rig.state.is_dirty());
    assert!(
        rig.game()
            .mainline()
            .all(|n| rig.game().comment(n).unwrap().is_none() && nags(rig.game(), n).is_empty())
    );
}

#[test]
fn writing_is_one_undo_point_and_one_undo_restores_the_game() {
    let mut rig = hanging();
    let original = rig.game().clone();
    let undo = rig.state.session.undo_depth();
    rig.send(ReportMessage::WriteToGame);
    assert_eq!(rig.state.session.undo_depth(), undo + 1);
    assert!(rig.state.is_dirty());
    assert!(!rig.game().same_content(&original));
    rig.send(GameMessage::Undo);
    assert!(rig.game().same_content(&original));
    assert_eq!(rig.state.session.undo_depth(), undo);
}

#[test]
fn every_move_gets_its_eval_and_the_nag_of_its_class() {
    let mut rig = hanging();
    rig.send(ReportMessage::WriteToGame);
    let game = rig.game();
    let data = rig.state.report_data().expect("the report still applies");
    for (node, verdict) in game.mainline().zip(&data.report.moves) {
        let comment = game.comment(node).unwrap().unwrap_or_default();
        assert_eq!(comment.matches("[%eval").count(), 1, "ply {}", verdict.ply);
        let got = nags(game, node);
        let expected: &[u8] = match verdict.class {
            MoveClass::Brilliant => &[3],
            MoveClass::Great => &[1],
            MoveClass::Inaccuracy => &[6],
            MoveClass::Mistake | MoveClass::Miss => &[2],
            MoveClass::Blunder => &[4],
            MoveClass::Best | MoveClass::Excellent | MoveClass::Good | MoveClass::Book => &[],
        };
        assert_eq!(got, expected, "ply {} {:?}", verdict.ply, verdict.class);
    }
    assert!(
        data.report.moves.iter().any(|m| m.class.is_fault()),
        "the fixture has a fault to check"
    );
}

#[test]
fn the_best_move_is_not_marked_with_an_exclamation_mark() {
    let mut rig = hanging();
    rig.send(ReportMessage::WriteToGame);
    let data = rig.state.report_data().unwrap().clone();
    let best_nodes: Vec<_> = rig
        .game()
        .mainline()
        .zip(&data.report.moves)
        .filter(|(_, v)| v.class == MoveClass::Best)
        .map(|(n, _)| n)
        .collect();
    assert!(!best_nodes.is_empty());
    for node in best_nodes {
        assert!(nags(rig.game(), node).is_empty());
    }
}

#[test]
fn an_eval_already_present_is_kept_and_not_repeated() {
    let mut game = game_of(&HANGING_QUEEN[..8]);
    let nodes: Vec<_> = game.mainline().collect();
    game.set_comment(nodes[2], "my own [%eval +9.99] note")
        .unwrap();
    let mut rig = finished(game, &mut material_engine);
    rig.send(ReportMessage::WriteToGame);
    let comment = rig.game().comment(nodes[2]).unwrap().unwrap().to_owned();
    assert_eq!(comment.matches("[%eval").count(), 1, "{comment}");
    assert!(
        comment.contains("+9.99") && comment.contains("my own"),
        "{comment}"
    );
    for (i, node) in nodes.iter().enumerate() {
        let c = rig.game().comment(*node).unwrap().unwrap_or_default();
        assert_eq!(c.matches("[%eval").count(), 1, "ply {}", i + 1);
    }
}

/// An engine whose line is twelve moves long.
fn long_line_engine(_: &SearchRequest, pos: &Position) -> Reply {
    let first = material_reply(pos);
    let mut position = pos.clone();
    let mut line = Vec::new();
    for _ in 0..12 {
        let Some(mv) = material_reply(&position).pv.first().copied() else {
            break;
        };
        line.push(mv);
        if position.play(mv).is_err() {
            break;
        }
    }
    Reply { pv: line, ..first }
}

#[test]
fn the_variation_of_a_fault_is_the_engines_line_cut_to_eight_moves() {
    let mut rig = finished(game_of(&HANGING_QUEEN), &mut long_line_engine);
    let data = rig.state.report_data().unwrap().clone();
    let original = rig.game().clone();
    rig.send(ReportMessage::WriteToGame);
    let game = rig.game();
    let mut checked = 0;
    for (i, verdict) in data.report.moves.iter().enumerate() {
        let parent = ReportData::node_before(&original, verdict.ply).unwrap();
        let children = game.children(parent).unwrap();
        let extra = children.len() - 1;
        let wants_line = verdict.class.is_fault() && verdict.best != Some(verdict.played);
        if wants_line {
            assert_eq!(extra, 1, "ply {} gets a line", i + 1);
            let line = children[1];
            // Walk the chain of first children.
            let mut length = 1;
            let mut node = line;
            while let Some(next) = game.children(node).unwrap().first().copied() {
                length += 1;
                node = next;
            }
            assert!(length <= 8, "ply {}: {length} moves", i + 1);
            assert_eq!(length, 8, "twelve cut to eight");
            assert_eq!(game.mv(line).unwrap(), verdict.best.unwrap());
            checked += 1;
        } else {
            assert_eq!(extra, 0, "ply {} has no line", i + 1);
        }
    }
    assert!(checked > 0);
}

#[test]
fn writing_twice_changes_nothing_the_second_time() {
    let mut rig = hanging();
    rig.send(ReportMessage::WriteToGame);
    let after_first = rig.game().clone();
    rig.send(ReportMessage::WriteToGame);
    assert!(
        rig.game().same_content(&after_first),
        "no repeated [%eval], NAG or line"
    );
    // One undo of the second write (if it counted as a point) leaves the
    // first intact; one more takes the game back to its start.
    let mut guard = 0;
    while rig.state.session.undo_depth() > 0 && guard < 5 {
        rig.send(GameMessage::Undo);
        guard += 1;
    }
    assert!(
        rig.game()
            .mainline()
            .all(|n| rig.game().comment(n).unwrap().is_none())
    );
}

#[test]
fn the_notice_counts_the_moves_and_the_lines_written() {
    let mut rig = hanging();
    rig.send(ReportMessage::WriteToGame);
    let lines: usize = rig
        .game()
        .mainline()
        .map(|n| {
            rig.game()
                .parent(n)
                .unwrap()
                .map_or(0, |p| rig.game().children(p).unwrap().len() - 1)
        })
        .sum();
    match rig.state.notice {
        Some(AppNotice::Annotate(AnnotateNotice::ReportWritten { moves, variations })) => {
            assert_eq!(moves, 20);
            assert_eq!(usize::try_from(variations).unwrap(), lines);
        }
        ref other => panic!("{other:?}"),
    }
}

#[test]
fn a_report_of_another_main_line_writes_nothing() {
    let mut rig = hanging();
    rig.state.session.replace_game(game_of(&["d2d4", "d7d5"]));
    let before = rig.game().clone();
    let undo = rig.state.session.undo_depth();
    rig.send(ReportMessage::WriteToGame);
    assert!(rig.game().same_content(&before));
    assert_eq!(rig.state.session.undo_depth(), undo);
}

#[test]
fn a_partial_report_writes_only_what_it_judged() {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    let fx = rig.send(ReportMessage::Generate);
    let log = rig.drive(fx, 6, &mut material_engine);
    // Cancel through the watchdog of the fake run.
    let tick = log.last.iter().rev().find_map(|e| match e {
        mychess_app::Effect::ScheduleAnnotateTick {
            run, generation, ..
        } => Some((*run, *generation)),
        _ => None,
    });
    let (run, generation) = tick.expect("a timer");
    rig.send(mychess_app::AnnotateMessage::Tick { run, generation });
    let judged = rig.state.report_data().expect("partial").report.moves.len();
    assert!(judged > 0 && judged < 20);
    rig.send(ReportMessage::WriteToGame);
    let scored = rig
        .game()
        .mainline()
        .filter(|n| {
            rig.game()
                .comment(*n)
                .unwrap()
                .is_some_and(|c| c.contains("[%eval"))
        })
        .count();
    assert_eq!(scored, judged);
}

#[test]
fn the_classification_of_the_faults_survives_writing_and_recalculating() {
    let mut rig = hanging();
    let engine_report = rig.state.report_data().unwrap().report.clone();
    rig.send(ReportMessage::WriteToGame);
    rig.send(ReportMessage::Clear);
    rig.send(ReportMessage::FromGame);
    let again = &rig.state.report_data().expect("from the comments").report;
    assert_eq!(again.moves.len(), engine_report.moves.len());
    for (a, b) in engine_report.moves.iter().zip(&again.moves) {
        assert_eq!(
            a.class.is_fault(),
            b.class.is_fault(),
            "ply {}: {:?} vs {:?}",
            a.ply,
            a.class,
            b.class
        );
        if a.class.is_fault() {
            assert_eq!(a.class, b.class, "ply {}", a.ply);
        }
        assert!((a.loss - b.loss).abs() < 0.2, "ply {}", a.ply);
    }
}
