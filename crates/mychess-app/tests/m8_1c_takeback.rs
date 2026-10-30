//! M8-1c (slice A) contract tests of "Take back" at the controller (spec
//! D1500-D1502, D1512, D1513 and the acceptance criteria): which phases
//! allow it, what it removes, how the clocks, the comments, the tags and
//! the repetition counters come back.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1c_common;

use std::time::Duration;

use m8_1c_common::*;
use mychess_app::{
    BookSelection, PlayEnd, PlayNotice, PlayOutput, PlayPhase, PlayQuestion, PlayRemark, TimeMode,
};
use mychess_core::{Color, GameResult, Position};
use proptest::prelude::*;
use rstest::rstest;

const DEPTH: TimeMode = TimeMode::Depth(2);

fn refused(out: &[PlayOutput]) -> bool {
    out == [PlayOutput::Notice(PlayRemark::NothingToTakeBack)]
}

// ---- what it removes ----

#[test]
fn taking_back_removes_the_move_of_the_player_and_the_answer() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_ucis(&mut c, &["e2e4", "e7e5"], t0);
    assert!(c.can_take_back());
    let out = c.take_back(t0);
    assert_eq!(out, [PlayOutput::TookBack]);
    assert_eq!(mainline_ucis(c.game()), Vec::<String>::new());
    assert_eq!(c.position(), &Position::new());
    assert_eq!(c.phase(), PlayPhase::Player);
    assert!(!c.can_take_back());
    assert_eq!(c.game().node_count(), 1, "only the root is left");
}

#[test]
fn taking_back_removes_only_the_last_pair() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_ucis(&mut c, &["e2e4", "e7e5", "g1f3", "b8c6"], t0);
    c.take_back(t0);
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5"]);
    assert_eq!(c.game().node_count(), 3);
    c.take_back(t0);
    assert_eq!(mainline_ucis(c.game()), Vec::<String>::new());
}

#[test]
fn after_taking_back_the_player_may_play_something_else_with_no_variation() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_ucis(&mut c, &["e2e4", "e7e5"], t0);
    c.take_back(t0);
    let out = c.player_move(uci(&c, "d2d4"), t0);
    assert_eq!(thinks(&out).len(), 1, "the engine is asked again");
    assert_eq!(thinks(&out)[0].moves.len(), 1);
    assert_eq!(mainline_ucis(c.game()), ["d2d4"]);
    assert_eq!(c.game().node_count(), 2, "no orphan node or variation");
    assert_eq!(c.game().children(c.game().root()).unwrap().len(), 1);
}

#[test]
fn a_black_player_takes_back_his_move_and_the_answer_but_not_the_first_move_of_the_engine() {
    let (mut c, _, t0) = begin(setup(Color::Black, DEPTH));
    assert!(!c.can_take_back(), "only the engine moved, nothing to undo");
    play_ucis(&mut c, &["e2e4", "e7e5", "g1f3"], t0);
    assert_eq!(c.phase(), PlayPhase::Player);
    let out = c.take_back(t0);
    assert_eq!(out, [PlayOutput::TookBack]);
    assert_eq!(mainline_ucis(c.game()), ["e2e4"]);
    assert_eq!(c.phase(), PlayPhase::Player);
    assert_eq!(c.position().turn(), Color::Black);
    // And now there is nothing left to undo.
    assert!(!c.can_take_back());
    assert!(refused(&c.take_back(t0)));
    assert_eq!(mainline_ucis(c.game()), ["e2e4"]);
}

#[test]
fn a_black_player_with_only_the_move_of_the_engine_gets_nothing_to_take_back() {
    let (mut c, _, t0) = begin(setup(Color::Black, DEPTH));
    play_ucis(&mut c, &["e2e4"], t0);
    assert_eq!(c.phase(), PlayPhase::Player);
    assert!(refused(&c.take_back(t0)));
    assert_eq!(mainline_ucis(c.game()), ["e2e4"]);
    assert_eq!(c.takebacks(), 0);
}

#[test]
fn the_free_book_answer_of_the_engine_goes_with_the_move_and_the_book_guides_again() {
    // Book 1.e4 e5 2.Nf3: the engine (White) opens, the player answers e5
    // and the engine closes the book for free with Nf3.
    let mut s = setup(Color::Black, DEPTH);
    s.book = Some(mini_book(BookSelection::Best));
    let (mut c, _, t0) = begin(s);
    c.player_move(uci(&c, "e7e5"), t0);
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5", "g1f3"]);
    assert!(!c.book_active());
    let out = c.take_back(t0);
    assert_eq!(out, [PlayOutput::TookBack]);
    assert_eq!(mainline_ucis(c.game()), ["e2e4"]);
    assert!(c.book_active(), "the book guides again (D1512)");
    assert_eq!(comment_at_tip(&c), None, "the comment of Nf3 left with it");
    // Playing e5 again brings the same free answer and comment back.
    c.player_move(uci(&c, "e7e5"), t0);
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5", "g1f3"]);
    assert_eq!(comment_at_tip(&c).as_deref(), Some("Last book move (book)"));
}

#[test]
fn the_free_opening_move_of_the_engine_goes_with_the_pair() {
    // The line is 1.e4 c5 2.Nf3; the player is White, so c5 is free.
    let mut s = setup(Color::White, DEPTH);
    s.config.start = mychess_app::StartFrom::Opening(
        mychess_app::OpeningLine::parse("Sicilian: e4 c5 Nf3").unwrap(),
    );
    let (mut c, _, t0) = begin(s);
    c.player_move(uci(&c, "e2e4"), t0);
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "c7c5"]);
    c.take_back(t0);
    assert_eq!(mainline_ucis(c.game()), Vec::<String>::new());
    // The line is still alive: the same move brings the same reply, free.
    c.player_move(uci(&c, "e2e4"), t0);
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "c7c5"]);
}

#[test]
fn the_line_state_is_restored_when_the_deviating_move_is_taken_back() {
    // Line 1.e4 c5; the player leaves it with d4 (answers "yes"), the
    // engine thinks; take back is not possible mid-think, so the engine
    // answers first.
    let mut s = setup(Color::White, DEPTH);
    s.config.start = mychess_app::StartFrom::Opening(
        mychess_app::OpeningLine::parse("Sicilian: e4 c5 Nf3").unwrap(),
    );
    let (mut c, _, t0) = begin(s);
    let d4 = uci(&c, "d2d4");
    let out = c.player_move(d4, t0);
    assert_eq!(out, [PlayOutput::Ask(PlayQuestion::LeaveLine { mv: d4 })]);
    c.answer(true, t0);
    c.opponent_moved(Some(uci(&c, "d7d5")), None, t0, None);
    c.take_back(t0);
    assert_eq!(mainline_ucis(c.game()), Vec::<String>::new());
    // The line asks again for a deviation: it was alive before the move.
    let out = c.player_move(uci(&c, "d2d4"), t0);
    assert!(matches!(out.as_slice(), [PlayOutput::Ask(_)]), "{out:?}");
}

// ---- where it is refused ----

#[test]
fn it_is_refused_while_the_engine_thinks_and_the_search_stays_valid() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    c.player_move(uci(&c, "e2e4"), t0);
    assert_eq!(c.phase(), PlayPhase::Opponent);
    assert!(!c.can_take_back());
    assert!(refused(&c.take_back(t0)));
    assert_eq!(mainline_ucis(c.game()), ["e2e4"]);
    // The answer of the engine is still taken.
    let out = c.opponent_moved(Some(uci(&c, "e7e5")), None, t0, None);
    assert_eq!(moved_count(&out), 1);
}

#[test]
fn it_is_refused_when_the_player_has_not_moved_yet() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    assert!(!c.can_take_back());
    assert!(refused(&c.take_back(t0)));
    assert_eq!(c.takebacks(), 0);
    assert_eq!(c.game().tags().get_extra("Takebacks"), None);
}

#[test]
fn it_is_refused_while_a_question_waits_and_the_question_stays() {
    let mut s = setup(Color::White, blitz());
    s.book = Some(mini_book(BookSelection::Best));
    let (mut c, _, t0) = begin(s);
    // 1.e4 is in the book and the engine answers e5 from it; Nf3 is in the
    // book too, so c3 leaves it.
    c.player_move(uci(&c, "e2e4"), t0);
    assert_eq!(
        mainline_ucis(c.game()),
        ["e2e4", "e7e5"],
        "e5 is a free book reply"
    );
    let c3 = uci(&c, "c2c3");
    let out = c.player_move(c3, t0);
    assert_eq!(out, [PlayOutput::Ask(PlayQuestion::LeaveBook { mv: c3 })]);
    let phase = c.phase();
    assert!(!c.can_take_back());
    assert!(refused(&c.take_back(t0)));
    assert_eq!(c.phase(), phase, "the question is kept");
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5"]);
    // Answering "no" still works afterwards.
    let out = c.answer(false, t0);
    assert!(out.contains(&PlayOutput::Notice(PlayRemark::MoveTakenBack)));
    assert_eq!(c.phase(), PlayPhase::Player);
}

#[test]
fn it_is_refused_while_paused() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_ucis(&mut c, &["e2e4", "e7e5"], t0);
    c.pause(t0);
    assert!(matches!(c.phase(), PlayPhase::Paused { .. }));
    assert!(!c.can_take_back());
    assert!(refused(&c.take_back(t0)));
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5"]);
}

#[test]
fn it_is_refused_while_paused_because_the_engine_was_lost() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    c.player_move(uci(&c, "e2e4"), t0);
    c.opponent_lost(t0);
    assert!(matches!(c.phase(), PlayPhase::Paused { lost: true }));
    assert!(refused(&c.take_back(t0)));
}

#[test]
fn it_is_refused_after_the_end_and_after_a_resignation() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_ucis(&mut c, &["e2e4", "e7e5"], t0);
    c.resign(t0);
    assert!(matches!(c.phase(), PlayPhase::Ended(_)));
    assert!(!c.can_take_back());
    assert!(refused(&c.take_back(t0)));
    assert_eq!(c.result(), GameResult::Black);
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5"]);
}

#[test]
fn it_is_refused_after_a_checkmate() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_ucis(&mut c, &["f2f3", "e7e5", "g2g4", "d8h4"], t0);
    assert!(matches!(
        c.phase(),
        PlayPhase::Ended(PlayEnd::Checkmate { .. })
    ));
    assert!(refused(&c.take_back(t0)));
    assert_eq!(c.result(), GameResult::Black);
}

// ---- clocks (D1501, D1512) ----

#[test]
fn the_clock_gives_back_the_time_and_takes_the_increment_away() {
    let (mut c, _, t0) = begin(setup(Color::White, timebonus(5, 3, 5, 3)));
    // The player spends 10 s, the engine 20 s.
    c.player_move(uci(&c, "e2e4"), t0 + 10 * SEC);
    c.opponent_moved(Some(uci(&c, "e7e5")), Some(20 * SEC), t0 + 30 * SEC, None);
    let now = t0 + 40 * SEC;
    assert_eq!(c.shown(Color::White, t0 + 30 * SEC), 293 * SEC);
    assert_eq!(c.shown(Color::Black, t0 + 30 * SEC), 283 * SEC);
    c.take_back(now);
    assert_eq!(c.shown(Color::White, now), 300 * SEC);
    assert_eq!(c.shown(Color::Black, now), 300 * SEC);
    assert_eq!(c.phase(), PlayPhase::Player);
    // The player's clock runs again from `now`.
    assert!(c.clock().is_running());
    assert_eq!(c.clock().turn(), Color::White);
    assert_eq!(c.shown(Color::White, now + 5 * SEC), 295 * SEC);
    assert_eq!(c.shown(Color::Black, now + 5 * SEC), 300 * SEC);
}

#[test]
fn a_flag_that_fell_before_the_take_back_still_loses_the_game() {
    let (mut c, _, t0) = begin(setup(Color::White, timebonus(5, 3, 5, 3)));
    c.player_move(uci(&c, "e2e4"), t0 + 10 * SEC);
    c.opponent_moved(Some(uci(&c, "e7e5")), Some(20 * SEC), t0 + 30 * SEC, None);
    // White has 293 s left from t0 + 30 s; no tick came before the click.
    let now = t0 + 30 * SEC + 294 * SEC;
    let out = c.take_back(now);
    assert_eq!(
        ended(&out),
        Some(PlayEnd::Timeout {
            loser: Color::White
        })
    );
    assert!(!out.contains(&PlayOutput::TookBack));
    assert_eq!(mainline_ucis(c.game()), ["e2e4", "e7e5"]);
}

#[test]
fn only_the_last_pair_comes_back_in_the_clock() {
    let (mut c, _, t0) = begin(setup(Color::White, timebonus(5, 3, 5, 3)));
    c.player_move(uci(&c, "e2e4"), t0 + 10 * SEC);
    c.opponent_moved(Some(uci(&c, "e7e5")), Some(20 * SEC), t0 + 30 * SEC, None);
    c.player_move(uci(&c, "g1f3"), t0 + 35 * SEC);
    c.opponent_moved(Some(uci(&c, "b8c6")), Some(2 * SEC), t0 + 40 * SEC, None);
    let now = t0 + 50 * SEC;
    c.take_back(now);
    // White: 300 - 10 + 3 after the first pair; Black: 300 - 20 + 3.
    assert_eq!(c.shown(Color::White, now), 293 * SEC);
    assert_eq!(c.shown(Color::Black, now), 283 * SEC);
}

#[test]
fn a_move_that_was_free_costs_nothing_to_give_back() {
    // Black player: e4 (free) and Nf3 (free) of the book; the player spent
    // 8 s on e5 with a 5+3 clock.
    let mut s = setup(Color::Black, blitz());
    s.book = Some(mini_book(BookSelection::Best));
    let (mut c, _, t0) = begin(s);
    c.player_move(uci(&c, "e7e5"), t0 + 8 * SEC);
    let now = t0 + 9 * SEC;
    c.take_back(now);
    assert_eq!(
        c.shown(Color::Black, now),
        300 * SEC,
        "the 8 s and the +3 s are gone"
    );
    assert_eq!(c.shown(Color::White, now), 300 * SEC);
    // Black had 300 and its clock is running again from `now`.
    let later = now + 4 * SEC;
    assert_eq!(c.shown(Color::Black, later), 296 * SEC);
}

#[test]
fn in_a_count_up_game_the_time_used_goes_back_too() {
    let (mut c, _, t0) = begin(setup(Color::White, TimeMode::MoveTime(5)));
    c.player_move(uci(&c, "e2e4"), t0 + 4 * SEC);
    c.opponent_moved(Some(uci(&c, "e7e5")), Some(3 * SEC), t0 + 7 * SEC, None);
    let now = t0 + 8 * SEC;
    c.take_back(now);
    assert_eq!(c.shown(Color::White, now), Duration::ZERO);
    assert_eq!(c.shown(Color::Black, now), Duration::ZERO);
    assert_eq!(c.shown(Color::White, now + 2 * SEC), 2 * SEC);
}

#[test]
fn the_question_clock_time_does_not_leak_into_the_take_back() {
    // The player takes 6 s deciding on a move that leaves the book, answers
    // yes, the engine answers, then the move is taken back.
    let mut s = setup(Color::White, blitz());
    s.book = Some(mini_book(BookSelection::Best));
    let (mut c, _, t0) = begin(s);
    c.player_move(uci(&c, "e2e4"), t0);
    assert_eq!(
        mainline_ucis(c.game()),
        ["e2e4", "e7e5"],
        "e5 is a free book reply"
    );
    let c3 = uci(&c, "c2c3");
    c.player_move(c3, t0 + 6 * SEC);
    c.answer(true, t0 + 20 * SEC);
    c.opponent_moved(Some(uci(&c, "g8f6")), Some(SEC), t0 + 22 * SEC, None);
    let now = t0 + 30 * SEC;
    c.take_back(now);
    // What is left is 1.e4 (0 s, +3 s) and the free e5: White 303, Black 300.
    assert_eq!(c.shown(Color::White, now), 303 * SEC);
    assert_eq!(c.shown(Color::Black, now), 300 * SEC);
}

// ---- comments, variations, tag ----

#[test]
fn the_comments_of_the_pair_leave_with_it_and_the_earlier_ones_stay() {
    let mut s = setup(Color::White, blitz());
    s.config.store_clock = true;
    let (mut c, _, t0) = begin(s);
    c.player_move(uci(&c, "e2e4"), t0 + 4 * SEC);
    c.opponent_moved(Some(uci(&c, "e7e5")), Some(2 * SEC), t0 + 6 * SEC, None);
    let first = c.tip();
    c.player_move(uci(&c, "g1f3"), t0 + 9 * SEC);
    c.opponent_moved(Some(uci(&c, "b8c6")), Some(3 * SEC), t0 + 12 * SEC, None);
    c.take_back(t0 + 13 * SEC);
    assert_eq!(c.tip(), first);
    assert!(comment_at_tip(&c).is_some_and(|t| t.contains("[%emt 0:00:02]")));
    let comments = c
        .game()
        .mainline()
        .filter_map(|id| c.game().comment(id).ok().flatten().map(str::to_owned))
        .count();
    assert_eq!(comments, 2, "one per move left, none of the pair");
    assert_eq!(c.game().node_count(), 3);
}

#[test]
fn the_takebacks_tag_counts_each_use_and_is_absent_before() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    assert_eq!(c.game().tags().get_extra("Takebacks"), None);
    play_ucis(&mut c, &["e2e4", "e7e5"], t0);
    c.take_back(t0);
    assert_eq!(c.game().tags().get_extra("Takebacks"), Some("1"));
    assert_eq!(c.takebacks(), 1);
    play_ucis(&mut c, &["d2d4", "d7d5"], t0);
    c.take_back(t0);
    assert_eq!(c.game().tags().get_extra("Takebacks"), Some("2"));
    // A refused take back does not count.
    c.take_back(t0);
    assert_eq!(c.takebacks(), 2);
    assert_eq!(c.game().tags().get_extra("Takebacks"), Some("2"));
}

#[test]
fn there_is_no_limit_to_the_number_of_takebacks() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    for _ in 0..50 {
        play_ucis(&mut c, &["e2e4", "e7e5"], t0);
        assert_eq!(c.take_back(t0), [PlayOutput::TookBack]);
    }
    assert_eq!(c.game().tags().get_extra("Takebacks"), Some("50"));
    assert_eq!(c.game().node_count(), 1);
}

#[test]
fn a_take_back_leaves_the_result_unknown_and_the_game_live() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_ucis(&mut c, &["e2e4", "e7e5"], t0);
    c.take_back(t0);
    assert_eq!(c.result(), GameResult::Unknown);
    assert!(c.is_live());
    assert_eq!(c.end(), None);
}

// ---- repetition and fifty moves come from the remaining position ----

#[test]
fn the_repetition_count_forgets_the_positions_that_were_taken_back() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    // The start position occurs a second time after Ng8.
    play_ucis(&mut c, &["g1f3", "g8f6", "f3g1", "f6g8"], t0);
    assert_eq!(c.phase(), PlayPhase::Player);
    c.take_back(t0);
    // Repeating again is only the second occurrence: no draw.
    let out = play_ucis(&mut c, &["f3g1", "f6g8"], t0);
    assert_eq!(ended(&out), None, "{out:?}");
    assert_eq!(c.phase(), PlayPhase::Player);
    // The third occurrence still ends the game.
    let out = play_ucis(&mut c, &["g1f3", "g8f6", "f3g1", "f6g8"], t0);
    assert_eq!(ended(&out), Some(PlayEnd::ThreefoldRepetition));
}

#[test]
fn the_move_that_would_repeat_for_the_third_time_is_not_counted_after_a_take_back() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_ucis(
        &mut c,
        &["g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1"],
        t0,
    );
    // Phase is Opponent; the engine is about to complete the third
    // repetition. Make it answer something else and take the pair back.
    let out = c.opponent_moved(Some(uci(&c, "b8c6")), None, t0, None);
    assert_eq!(ended(&out), None);
    c.take_back(t0);
    assert_eq!(c.phase(), PlayPhase::Player);
    assert_eq!(c.position().turn(), Color::White);
    assert_eq!(mainline_ucis(c.game()).len(), 6);
    // The engine can still complete the repetition with Ng8: it ends the
    // game once, with the stale Ng1/Nc6 positions not counted.
    let out = play_ucis(&mut c, &["f3g1", "f6g8"], t0);
    assert_eq!(ended(&out), Some(PlayEnd::ThreefoldRepetition));
}

#[test]
fn the_fifty_move_counter_comes_back_with_the_position() {
    // Rook and king against king; the clock is at 97 half-moves.
    let (mut c, _, t0) = begin(fen_setup(
        Color::White,
        DEPTH,
        "4k3/8/8/8/8/8/R7/4K3 w - - 97 80",
    ));
    play_ucis(&mut c, &["a2b2", "e8d8"], t0);
    assert_eq!(c.position().halfmove_clock(), 99);
    assert_eq!(c.phase(), PlayPhase::Player);
    c.take_back(t0);
    assert_eq!(c.position().halfmove_clock(), 97);
    // A different pair reaches 99 again without ending...
    let out = play_ucis(&mut c, &["a2c2", "e8f8"], t0);
    assert_eq!(ended(&out), None);
    // ...and the next player move makes 100 and ends the game.
    let out = play_ucis(&mut c, &["c2a2"], t0);
    assert_eq!(ended(&out), Some(PlayEnd::FiftyMoves));
}

// ---- notices ----

#[test]
fn the_notice_of_a_refused_take_back_has_its_key() {
    assert_eq!(
        PlayNotice::NothingToTakeBack.message_key(),
        "play-notice-nothing-to-take-back"
    );
}

// ---- properties ----

#[derive(Debug, Clone, Copy)]
enum Op {
    Pair(usize),
    Back,
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(
        prop_oneof![(0usize..1000).prop_map(Op::Pair), Just(Op::Back)],
        0..40,
    )
}

proptest! {
    /// Any mix of pairs and take backs keeps the game linear and equal to
    /// the model, the position equal to the replay and the clock equal to
    /// "initial + one increment per pair left".
    #[test]
    fn take_backs_keep_the_game_equal_to_a_stack_of_pairs(ops in ops()) {
        let (mut c, _, t0) = begin(setup(Color::White, timebonus(5, 3, 5, 3)));
        let mut model: Vec<(mychess_core::Move, mychess_core::Move)> = Vec::new();
        let mut pos_stack: Vec<Position> = vec![Position::new()];
        let mut backs = 0u32;
        for op in ops {
            match op {
                Op::Pair(k) => {
                    let pos = pos_stack.last().unwrap().clone();
                    let mut moves = pos.legal_moves();
                    moves.sort_by_key(|m| pos.uci(*m));
                    let mine = moves[k % moves.len()];
                    let after = pos.after(mine).unwrap();
                    let mut replies = after.legal_moves();
                    replies.sort_by_key(|m| after.uci(*m));
                    // Skip pairs that could end the game or repeat.
                    if replies.is_empty() || pos_stack.len() > 12 || after.is_check() {
                        continue;
                    }
                    let theirs = replies[(k / 7) % replies.len()];
                    let end = after.after(theirs).unwrap();
                    if end.is_check() || end.legal_moves().is_empty()
                        || end.is_insufficient_material()
                        || pos_stack.iter().any(|p| p.fen().split(' ').next() == end.fen().split(' ').next())
                    {
                        continue;
                    }
                    c.player_move(mine, t0);
                    c.opponent_moved(Some(theirs), None, t0, None);
                    model.push((mine, theirs));
                    pos_stack.push(end);
                }
                Op::Back => {
                    let out = c.take_back(t0);
                    if model.pop().is_some() {
                        pos_stack.pop();
                        backs += 1;
                        prop_assert_eq!(out, vec![PlayOutput::TookBack]);
                    } else {
                        prop_assert!(refused(&out));
                    }
                }
            }
            prop_assert_eq!(c.phase(), PlayPhase::Player);
            prop_assert_eq!(c.position(), pos_stack.last().unwrap());
            prop_assert_eq!(c.game().node_count(), 1 + 2 * model.len());
            let expected: Vec<String> = {
                let mut p = Position::new();
                let mut v = Vec::new();
                for (a, b) in &model {
                    v.push(p.uci(*a)); p.play(*a).unwrap();
                    v.push(p.uci(*b)); p.play(*b).unwrap();
                }
                v
            };
            prop_assert_eq!(mainline_ucis(c.game()), expected);
            let want = Duration::from_secs(300 + 3 * model.len() as u64);
            prop_assert_eq!(c.shown(Color::White, t0), want);
            prop_assert_eq!(c.shown(Color::Black, t0), want);
            prop_assert_eq!(c.takebacks(), backs);
        }
    }
}

#[rstest]
#[case(Color::White)]
#[case(Color::Black)]
fn can_take_back_agrees_with_the_phase(#[case] color: Color) {
    let (mut c, _, t0) = begin(setup(color, DEPTH));
    // Whoever starts, play until the player has moved once and the engine
    // answered.
    if color == Color::White {
        c.player_move(uci(&c, "e2e4"), t0);
        c.opponent_moved(Some(uci(&c, "e7e5")), None, t0, None);
    } else {
        c.opponent_moved(Some(uci(&c, "e2e4")), None, t0, None);
        c.player_move(uci(&c, "e7e5"), t0);
        c.opponent_moved(Some(uci(&c, "g1f3")), None, t0, None);
    }
    assert_eq!(c.phase(), PlayPhase::Player);
    assert!(c.can_take_back());
    c.pause(t0);
    assert!(!c.can_take_back());
}
