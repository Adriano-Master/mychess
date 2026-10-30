//! M8-1c (slice A) contract tests of the draw offer at the controller (spec
//! D1503-D1505, D1509, D1511-D1513): the phases that allow it, the
//! acceptance criterion of the engine (three evaluations at 10 cp or less
//! from its point of view, ply 40), one offer per move of the player and the
//! end by agreement.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1c_common;

use std::time::Instant;

use m8_1c_common::*;
use mychess_app::{
    BookSelection, OpeningLine, PlayController, PlayEnd, PlayNotice, PlayOutput, PlayPhase,
    PlayRemark, StartFrom, TimeMode,
};
use mychess_core::{Color, GameResult};
use mychess_engine::Score;
use rstest::rstest;

const DEPTH: TimeMode = TimeMode::Depth(2);

fn declined(out: &[PlayOutput]) -> bool {
    out == [PlayOutput::Notice(PlayRemark::DrawDeclined)]
}

fn not_allowed(out: &[PlayOutput]) -> bool {
    out == [PlayOutput::Notice(PlayRemark::DrawNotAllowed)]
}

/// A game where the player (`color`) is to move after `plies` quiet moves;
/// every search of the engine says `engine_cp` for itself.
fn game_at(color: Color, plies: usize, engine_cp: i32) -> (PlayController, Instant) {
    let (mut c, _, t0) = begin(setup(color, DEPTH));
    play_quiet(&mut c, plies, t0, |_| Some(white_pov(color, engine_cp)));
    (c, t0)
}

// ---- the ply threshold ----

#[rstest]
#[case(Color::White, 38, false)]
#[case(Color::White, 40, true)]
#[case(Color::White, 42, true)]
#[case(Color::Black, 39, false)]
#[case(Color::Black, 41, true)]
fn the_engine_accepts_only_from_ply_40(
    #[case] color: Color,
    #[case] plies: usize,
    #[case] accepts: bool,
) {
    let (mut c, t0) = game_at(color, plies, 0);
    assert_eq!(c.phase(), PlayPhase::Player);
    assert_eq!(
        c.position().ply() as usize % 2,
        usize::from(color == Color::Black)
    );
    let out = c.offer_draw(t0);
    if accepts {
        assert_eq!(ended(&out), Some(PlayEnd::DrawAgreed), "{out:?}");
    } else {
        assert!(declined(&out), "{out:?}");
        assert_eq!(c.phase(), PlayPhase::Player);
    }
}

// ---- the evaluation threshold, from the point of view of the engine ----

#[rstest]
#[case::zero(0, true)]
#[case::ten(10, true)]
#[case::eleven(11, false)]
#[case::pawn_up(100, false)]
#[case::losing_a_little(-50, true)]
#[case::losing_a_lot(-900, true)]
fn the_threshold_is_10_cp_for_the_engine(#[case] engine_cp: i32, #[case] accepts: bool) {
    for color in [Color::White, Color::Black] {
        let plies = if color == Color::White { 40 } else { 41 };
        let (mut c, t0) = game_at(color, plies, engine_cp);
        let out = c.offer_draw(t0);
        assert_eq!(
            ended(&out) == Some(PlayEnd::DrawAgreed),
            accepts,
            "player {color:?}, engine {engine_cp} cp: {out:?}"
        );
    }
}

#[test]
fn scores_are_from_the_point_of_view_of_white_not_of_the_engine() {
    // Player White, engine Black: +300 for White means -300 for the engine,
    // which accepts; -300 for White means +300 for the engine, which does not.
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_quiet(&mut c, 40, t0, |_| Some(Score::Cp(300)));
    assert_eq!(ended(&c.offer_draw(t0)), Some(PlayEnd::DrawAgreed));
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_quiet(&mut c, 40, t0, |_| Some(Score::Cp(-300)));
    assert!(declined(&c.offer_draw(t0)));
    // Player Black, engine White: the signs flip.
    let (mut c, _, t0) = begin(setup(Color::Black, DEPTH));
    play_quiet(&mut c, 41, t0, |_| Some(Score::Cp(300)));
    assert!(declined(&c.offer_draw(t0)));
    let (mut c, _, t0) = begin(setup(Color::Black, DEPTH));
    play_quiet(&mut c, 41, t0, |_| Some(Score::Cp(-300)));
    assert_eq!(ended(&c.offer_draw(t0)), Some(PlayEnd::DrawAgreed));
}

#[test]
fn a_mate_against_the_engine_is_accepted_and_a_mate_for_it_is_not() {
    // Player White, engine Black: Mate(3) is White mating, Mate(-3) Black.
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_quiet(&mut c, 40, t0, |_| Some(Score::Mate(3)));
    assert_eq!(ended(&c.offer_draw(t0)), Some(PlayEnd::DrawAgreed));
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_quiet(&mut c, 40, t0, |_| Some(Score::Mate(-3)));
    assert!(declined(&c.offer_draw(t0)));
    // Player Black, engine White.
    let (mut c, _, t0) = begin(setup(Color::Black, DEPTH));
    play_quiet(&mut c, 41, t0, |_| Some(Score::Mate(-3)));
    assert_eq!(ended(&c.offer_draw(t0)), Some(PlayEnd::DrawAgreed));
    let (mut c, _, t0) = begin(setup(Color::Black, DEPTH));
    play_quiet(&mut c, 41, t0, |_| Some(Score::Mate(3)));
    assert!(declined(&c.offer_draw(t0)));
}

#[test]
fn only_the_last_three_searches_count() {
    // The engine was winning for a long time, then three quiet evaluations.
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    // 20 engine moves: the first 17 at +500 for the engine, the last 3 at 0.
    play_quiet(&mut c, 40, t0, |i| {
        Some(white_pov(Color::White, if i < 17 { 500 } else { 0 }))
    });
    assert_eq!(ended(&c.offer_draw(t0)), Some(PlayEnd::DrawAgreed));
}

#[test]
fn one_bad_evaluation_among_the_last_three_declines() {
    for bad in 17..20 {
        let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
        play_quiet(&mut c, 40, t0, |i| {
            Some(white_pov(Color::White, if i == bad { 11 } else { 0 }))
        });
        assert!(declined(&c.offer_draw(t0)), "bad evaluation at {bad}");
    }
}

#[test]
fn a_bad_evaluation_older_than_the_window_is_forgotten() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_quiet(&mut c, 40, t0, |i| {
        Some(white_pov(Color::White, if i == 16 { 900 } else { 0 }))
    });
    assert_eq!(ended(&c.offer_draw(t0)), Some(PlayEnd::DrawAgreed));
}

#[test]
fn a_search_without_an_exact_evaluation_counts_as_absent_and_declines() {
    for missing in 17..20 {
        let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
        play_quiet(&mut c, 40, t0, |i| {
            (i != missing).then(|| white_pov(Color::White, 0))
        });
        assert!(declined(&c.offer_draw(t0)), "missing at {missing}");
    }
}

#[test]
fn an_opponent_that_reports_no_scores_never_accepts() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    play_quiet(&mut c, 60, t0, |_| None);
    assert!(declined(&c.offer_draw(t0)));
}

#[test]
fn fewer_than_three_searches_decline_even_at_ply_40() {
    // A 40-ply opening line played for free by the engine leaves it with no
    // search of its own at ply 40; the free moves have no evaluation and do
    // not count (D1504).
    let text = quiet_opening("Quiet", 40);
    let mut s = setup(Color::White, DEPTH);
    s.config.start = StartFrom::Opening(OpeningLine::parse(&text).expect("line"));
    let (mut c, _, t0) = begin(s);
    let line = quiet_line(40);
    for pair in line.chunks(2) {
        c.player_move(pair[0], t0);
    }
    assert_eq!(c.position_moves().len(), 40);
    assert_eq!(c.phase(), PlayPhase::Player);
    assert!(declined(&c.offer_draw(t0)), "no search of the engine yet");
}

#[test]
fn the_free_moves_of_a_line_are_skipped_when_looking_for_the_last_three() {
    // After the 40-ply line, the engine searches three times with a quiet
    // evaluation; the first offer after the third one is accepted, and not
    // before the third.
    let text = quiet_opening("Quiet", 40);
    let mut s = setup(Color::White, DEPTH);
    s.config.start = StartFrom::Opening(OpeningLine::parse(&text).expect("line"));
    let (mut c, _, t0) = begin(s);
    for pair in quiet_line(40).chunks(2) {
        c.player_move(pair[0], t0);
    }
    for round in 1..=3 {
        assert!(declined(&c.offer_draw(t0)), "after {} searches", round - 1);
        play_quiet(&mut c, 2, t0, |_| Some(white_pov(Color::White, 0)));
    }
    assert_eq!(ended(&c.offer_draw(t0)), Some(PlayEnd::DrawAgreed));
}

#[test]
fn book_moves_do_not_count_as_evaluations() {
    // The engine (White) plays 1.e4 and 2.Nf3 from the book, for free, with
    // no evaluation; the searched moves that follow are enough at ply 41.
    let mut s = setup(Color::Black, DEPTH);
    s.book = Some(mini_book(BookSelection::Best));
    let (mut c, _, t0) = begin(s);
    c.player_move(uci(&c, "e7e5"), t0);
    assert_eq!(c.position_moves().len(), 3, "e4 and Nf3 came from the book");
    play_quiet(&mut c, 38, t0, |_| Some(white_pov(Color::Black, 0)));
    assert_eq!(c.position_moves().len(), 41);
    assert_eq!(c.phase(), PlayPhase::Player);
    assert_eq!(ended(&c.offer_draw(t0)), Some(PlayEnd::DrawAgreed));
}

// ---- one offer per move of the player ----

#[test]
fn a_second_offer_before_moving_is_not_allowed_and_a_new_move_allows_it() {
    let (mut c, t0) = game_at(Color::White, 40, 300);
    assert!(c.can_offer_draw());
    assert!(declined(&c.offer_draw(t0)));
    assert!(!c.can_offer_draw());
    assert!(not_allowed(&c.offer_draw(t0)));
    assert!(not_allowed(&c.offer_draw(t0)));
    // The player moves and the engine answers: the offer is open again.
    play_quiet(&mut c, 2, t0, |_| Some(white_pov(Color::White, 300)));
    assert!(c.can_offer_draw());
    assert!(declined(&c.offer_draw(t0)));
}

#[test]
fn a_declined_offer_changes_nothing_in_the_game_and_the_clock_runs_on() {
    let (mut c, _, t0) = begin(setup(Color::White, timebonus(5, 3, 5, 3)));
    play_quiet(&mut c, 40, t0, |_| Some(white_pov(Color::White, 300)));
    let before = mainline_ucis(c.game());
    let tip = c.tip();
    let out = c.offer_draw(t0 + 5 * SEC);
    assert!(declined(&out));
    assert_eq!(mainline_ucis(c.game()), before);
    assert_eq!(c.tip(), tip);
    assert_eq!(c.result(), GameResult::Unknown);
    assert!(c.clock().is_running());
    assert_eq!(c.shown(Color::White, t0 + 9 * SEC), (300 + 60 - 9) * SEC);
}

#[test]
fn an_offer_does_not_consume_the_right_to_play_and_take_back_gives_it_back() {
    let (mut c, t0) = game_at(Color::White, 40, 300);
    assert!(declined(&c.offer_draw(t0)));
    assert!(!c.can_offer_draw());
    c.take_back(t0);
    assert!(c.can_offer_draw(), "D1512: the offer is open again");
    assert!(declined(&c.offer_draw(t0)));
}

// ---- where it is not allowed ----

#[test]
fn offering_is_not_allowed_while_the_engine_thinks() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    c.player_move(uci(&c, "e2e4"), t0);
    assert_eq!(c.phase(), PlayPhase::Opponent);
    assert!(!c.can_offer_draw());
    assert!(not_allowed(&c.offer_draw(t0)));
    assert_eq!(c.phase(), PlayPhase::Opponent);
}

#[test]
fn offering_is_not_allowed_while_a_question_waits() {
    let mut s = setup(Color::White, DEPTH);
    s.book = Some(mini_book(BookSelection::Best));
    let (mut c, _, t0) = begin(s);
    let out = c.player_move(uci(&c, "c2c4"), t0);
    assert!(matches!(out.as_slice(), [PlayOutput::Ask(_)]));
    assert!(!c.can_offer_draw());
    assert!(not_allowed(&c.offer_draw(t0)));
    assert!(matches!(c.phase(), PlayPhase::Asking(_)));
}

#[test]
fn offering_is_not_allowed_while_paused_or_after_the_end() {
    let (mut c, t0) = game_at(Color::White, 40, 0);
    c.pause(t0);
    assert!(!c.can_offer_draw());
    assert!(not_allowed(&c.offer_draw(t0)));
    c.resume(t0);
    c.resign(t0);
    assert!(!c.can_offer_draw());
    assert!(not_allowed(&c.offer_draw(t0)));
    assert_eq!(c.result(), GameResult::Black, "the resignation stays");
}

#[test]
fn offering_is_not_allowed_after_the_engine_was_lost() {
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    c.player_move(uci(&c, "e2e4"), t0);
    c.opponent_lost(t0);
    assert!(not_allowed(&c.offer_draw(t0)));
}

#[test]
fn offering_before_the_first_move_is_declined_not_refused() {
    // The player is to move and nothing was offered yet: allowed, but ply 0.
    let (mut c, _, t0) = begin(setup(Color::White, DEPTH));
    assert!(c.can_offer_draw());
    assert!(declined(&c.offer_draw(t0)));
}

// ---- the agreement ----

#[test]
fn an_accepted_draw_ends_the_game_with_result_comment_and_view() {
    let (mut c, t0) = game_at(Color::White, 40, 0);
    let tip = c.tip();
    let out = c.offer_draw(t0);
    assert_eq!(ended(&out), Some(PlayEnd::DrawAgreed));
    assert_eq!(c.phase(), PlayPhase::Ended(PlayEnd::DrawAgreed));
    assert_eq!(c.end(), Some(PlayEnd::DrawAgreed));
    assert_eq!(c.result(), GameResult::Draw);
    assert!(!c.is_live());
    assert_eq!(c.tip(), tip, "no move was added");
    assert_eq!(comment_at_tip(&c).as_deref(), Some("Draw agreed"));
    assert!(!c.can_offer_draw());
    assert!(!c.can_take_back());
    assert!(!c.clock().is_running());
    assert_eq!(PlayEnd::DrawAgreed.result(), GameResult::Draw);
    assert_eq!(PlayEnd::DrawAgreed.comment(), Some("Draw agreed"));
}

#[test]
fn the_draw_comment_is_appended_to_the_comment_the_last_move_already_has() {
    // The last move of the engine carries the "Last book move" comment.
    let mut s = setup(Color::Black, DEPTH);
    s.book = Some(mini_book(BookSelection::Best));
    let (mut c, _, t0) = begin(s);
    c.player_move(uci(&c, "e7e5"), t0);
    assert_eq!(comment_at_tip(&c).as_deref(), Some("Last book move (book)"));
    // Too early to be accepted: confirm it is declined, comment untouched.
    assert!(declined(&c.offer_draw(t0)));
    assert_eq!(comment_at_tip(&c).as_deref(), Some("Last book move (book)"));
}

#[test]
fn the_notices_have_their_keys() {
    assert_eq!(
        PlayNotice::DrawNotAllowed.message_key(),
        "play-notice-draw-not-allowed"
    );
    assert_eq!(
        PlayNotice::DrawDeclined.message_key(),
        "play-notice-draw-declined"
    );
    assert_eq!(
        PlayNotice::Ended(PlayEnd::DrawAgreed).message_key(),
        "play-notice-end-draw-agreed"
    );
}
