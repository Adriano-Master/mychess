//! M8-1a contract tests of how a game ends (spec D1226-D1229, D1233): mate,
//! stalemate, threefold repetition (board + side + castling), 50 moves,
//! insufficient material, time (with the draw exception), resignation.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1_common;

use m8_1_common::*;
use mychess_app::{PlayEnd, PlayPhase, TimeMode};
use mychess_core::{Color, GameResult};
use rstest::rstest;

const QUIET: TimeMode = TimeMode::Depth(1);

// ---- result mapping ----

#[rstest]
#[case(PlayEnd::Checkmate { winner: Color::White }, GameResult::White)]
#[case(PlayEnd::Checkmate { winner: Color::Black }, GameResult::Black)]
#[case(PlayEnd::Timeout { loser: Color::White }, GameResult::Black)]
#[case(PlayEnd::Timeout { loser: Color::Black }, GameResult::White)]
#[case(PlayEnd::Resignation { loser: Color::White }, GameResult::Black)]
#[case(PlayEnd::Resignation { loser: Color::Black }, GameResult::White)]
#[case(PlayEnd::Stalemate, GameResult::Draw)]
#[case(PlayEnd::InsufficientMaterial, GameResult::Draw)]
#[case(PlayEnd::FiftyMoves, GameResult::Draw)]
#[case(PlayEnd::ThreefoldRepetition, GameResult::Draw)]
#[case(PlayEnd::TimeoutDraw, GameResult::Draw)]
#[case(PlayEnd::OpponentFailure, GameResult::Unknown)]
fn every_end_maps_to_its_result(#[case] end: PlayEnd, #[case] result: GameResult) {
    assert_eq!(end.result(), result);
}

#[test]
fn only_the_automatic_draws_carry_a_comment_with_the_scid_text() {
    assert_eq!(
        PlayEnd::ThreefoldRepetition.comment(),
        Some("3 fold repetition")
    );
    assert_eq!(PlayEnd::FiftyMoves.comment(), Some("50 move rule"));
    for end in [
        PlayEnd::Stalemate,
        PlayEnd::InsufficientMaterial,
        PlayEnd::TimeoutDraw,
        PlayEnd::OpponentFailure,
        PlayEnd::Checkmate {
            winner: Color::White,
        },
    ] {
        assert_eq!(end.comment(), None, "{end:?}");
    }
}

// ---- checkmate ----

#[test]
fn fools_mate_by_the_engine_gives_black_the_win_right_after_the_move() {
    let (mut c, _, t0) = begin(setup(Color::White, QUIET));
    let out = play_ucis(&mut c, &["f2f3", "e7e5", "g2g4", "d8h4"], t0);
    assert_eq!(
        ended(&out),
        Some(PlayEnd::Checkmate {
            winner: Color::Black
        })
    );
    assert_eq!(c.result(), GameResult::Black);
    assert_eq!(c.phase(), PlayPhase::Ended(c.end().unwrap()));
    assert!(!c.is_live());
    // D1229: no delay, no further request to the opponent.
    assert!(thinks(&out).is_empty());
}

#[test]
fn a_mate_given_by_the_player_gives_him_the_win() {
    let (mut c, _, t0) = begin(setup(Color::White, QUIET));
    play_ucis(
        &mut c,
        &["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6"],
        t0,
    );
    let out = play_ucis(&mut c, &["h5f7"], t0);
    assert_eq!(
        ended(&out),
        Some(PlayEnd::Checkmate {
            winner: Color::White
        })
    );
    assert_eq!(c.result(), GameResult::White);
}

#[test]
fn mate_on_the_hundredth_half_move_is_a_mate_not_a_fifty_move_draw() {
    let (mut c, _, t0) = begin(fen_setup(
        Color::White,
        QUIET,
        "7k/Q7/6K1/8/8/8/8/8 w - - 99 80",
    ));
    let out = play_ucis(&mut c, &["a7h7"], t0);
    assert_eq!(
        ended(&out),
        Some(PlayEnd::Checkmate {
            winner: Color::White
        })
    );
    assert_eq!(c.result(), GameResult::White);
}

#[test]
fn a_game_that_starts_in_a_mated_position_ends_at_once_without_panic() {
    // Fool's mate position, White to move and mated.
    let (c, out, _) = begin(fen_setup(
        Color::White,
        QUIET,
        "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3",
    ));
    assert_eq!(
        ended(&out),
        Some(PlayEnd::Checkmate {
            winner: Color::Black
        })
    );
    assert_eq!(c.result(), GameResult::Black);
    assert!(thinks(&out).is_empty());
}

// ---- stalemate ----

#[test]
fn stalemate_by_the_player_draws() {
    let (mut c, _, t0) = begin(fen_setup(
        Color::White,
        QUIET,
        "7k/8/5KQ1/8/8/8/8/8 w - - 0 1",
    ));
    let out = play_ucis(&mut c, &["g6f7"], t0);
    assert_eq!(ended(&out), Some(PlayEnd::Stalemate));
    assert_eq!(c.result(), GameResult::Draw);
}

#[test]
fn stalemate_by_the_engine_draws_even_though_the_player_has_material() {
    let (mut c, _, t0) = begin(fen_setup(
        Color::Black,
        QUIET,
        "7k/8/5KQ1/8/8/8/8/8 w - - 0 1",
    ));
    let out = c.opponent_moved(Some(uci(&c, "g6f7")), None, t0, None);
    assert_eq!(ended(&out), Some(PlayEnd::Stalemate));
    assert_eq!(c.result(), GameResult::Draw);
}

#[test]
fn a_game_that_starts_stalemated_ends_at_once() {
    let (c, out, _) = begin(fen_setup(
        Color::White,
        QUIET,
        "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1",
    ));
    assert_eq!(ended(&out), Some(PlayEnd::Stalemate));
    assert_eq!(c.result(), GameResult::Draw);
}

// ---- insufficient material (D1233) ----

#[rstest]
#[case::king_vs_king("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1", "e1e2")]
#[case::bishop_vs_king("4k3/8/8/8/8/8/4r3/3BK3 w - - 0 1", "e1e2")]
#[case::knight_vs_king("4k3/8/8/8/8/8/4r3/3NK3 w - - 0 1", "e1e2")]
fn dead_material_after_a_capture_draws(#[case] fen: &str, #[case] capture: &str) {
    let (mut c, _, t0) = begin(fen_setup(Color::White, QUIET, fen));
    let out = play_ucis(&mut c, &[capture], t0);
    assert_eq!(ended(&out), Some(PlayEnd::InsufficientMaterial));
    assert_eq!(c.result(), GameResult::Draw);
    assert!(c.game().comment(c.tip()).unwrap().is_none());
}

#[rstest]
#[case::rook("4k3/8/8/8/8/8/4r3/3RK3 w - - 0 1")]
#[case::pawn("4k3/8/8/8/8/P7/4r3/4K3 w - - 0 1")]
#[case::two_knights("4k3/8/8/8/8/8/4r3/2NNK3 w - - 0 1")]
#[case::bishop_and_knight("4k3/8/8/8/8/8/4r3/2BNK3 w - - 0 1")]
fn enough_material_does_not_end_the_game(#[case] fen: &str) {
    let (mut c, _, t0) = begin(fen_setup(Color::White, QUIET, fen));
    let out = play_ucis(&mut c, &["e1e2"], t0);
    assert_eq!(ended(&out), None);
    assert!(c.is_live());
}

// ---- fifty moves ----

#[test]
fn fifty_moves_end_at_the_hundredth_half_move_and_not_before() {
    let (mut c, _, t0) = begin(fen_setup(
        Color::White,
        QUIET,
        "3k4/8/8/8/8/8/4R3/4K3 w - - 97 80",
    ));
    // 98, 99: still going.
    let out = play_ucis(&mut c, &["e2e3", "d8c8"], t0);
    assert_eq!(ended(&out), None);
    assert_eq!(c.position().halfmove_clock(), 99);
    let out = play_ucis(&mut c, &["e3e4"], t0);
    assert_eq!(ended(&out), Some(PlayEnd::FiftyMoves));
    assert_eq!(c.result(), GameResult::Draw);
    assert_eq!(comment_at_tip(&c).as_deref(), Some("50 move rule"));
}

#[test]
fn a_pawn_move_resets_the_fifty_move_count() {
    let (mut c, _, t0) = begin(fen_setup(
        Color::White,
        QUIET,
        "3k4/8/8/8/8/8/P3R3/4K3 w - - 99 80",
    ));
    let out = play_ucis(&mut c, &["a2a3"], t0);
    assert_eq!(ended(&out), None);
    assert_eq!(c.position().halfmove_clock(), 0);
}

#[test]
fn a_capture_resets_the_fifty_move_count() {
    let (mut c, _, t0) = begin(fen_setup(
        Color::White,
        QUIET,
        "3k4/8/8/8/8/8/p3R3/4K3 w - - 99 80",
    ));
    let out = play_ucis(&mut c, &["e2a2"], t0);
    assert_eq!(ended(&out), None);
}

// ---- threefold repetition ----

#[test]
fn the_third_occurrence_of_the_start_position_ends_the_game_at_ply_eight() {
    let (mut c, _, t0) = begin(setup(Color::White, QUIET));
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let out = play_ucis(&mut c, &cycle, t0);
    assert_eq!(ended(&out), None, "two occurrences are not three");
    let out = play_ucis(&mut c, &["g1f3", "g8f6", "f3g1"], t0);
    assert_eq!(ended(&out), None);
    let out = play_ucis(&mut c, &["f6g8"], t0);
    assert_eq!(ended(&out), Some(PlayEnd::ThreefoldRepetition));
    assert_eq!(c.result(), GameResult::Draw);
    assert_eq!(comment_at_tip(&c).as_deref(), Some("3 fold repetition"));
    assert_eq!(c.game().mainline().count(), 8);
}

#[test]
fn a_repetition_of_a_position_reached_in_between_also_counts() {
    // The position after 1.e4 e5 occurs at plies 2, 6 and 10; the start
    // position occurs once.
    let (mut c, _, t0) = begin(setup(Color::White, QUIET));
    play_ucis(&mut c, &["e2e4", "e7e5"], t0);
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let out = play_ucis(&mut c, &cycle, t0);
    assert_eq!(ended(&out), None);
    let out = play_ucis(&mut c, &cycle, t0);
    assert_eq!(ended(&out), Some(PlayEnd::ThreefoldRepetition));
    assert_eq!(c.game().mainline().count(), 10);
}

#[test]
fn losing_a_castling_right_makes_the_same_board_a_different_position() {
    // Rooks leave and come back. Key after ply 1 (rook on a2, Black to
    // move) is `q` for plies 1 but "no rights" for plies 5 and 9, so with
    // the castling field taken into account the third repetition is the
    // position after ply 2, at ply 10; ignoring the field it would be
    // ply 9.
    let (mut c, _, t0) = begin(fen_setup(
        Color::White,
        QUIET,
        "r3k3/8/8/8/8/8/8/R3K3 w Qq - 0 1",
    ));
    let cycle = ["a1a2", "a8a7", "a2a1", "a7a8"];
    play_ucis(&mut c, &cycle, t0);
    play_ucis(&mut c, &cycle, t0);
    let out = play_ucis(&mut c, &["a1a2"], t0);
    assert_eq!(ended(&out), None, "ply 9: castling rights differ at ply 1");
    let out = play_ucis(&mut c, &["a8a7"], t0);
    assert_eq!(ended(&out), Some(PlayEnd::ThreefoldRepetition));
    assert_eq!(c.game().mainline().count(), 10);
}

#[test]
fn the_same_board_with_the_other_side_to_move_is_not_a_repetition() {
    // White's king walks a 3-square triangle, Black's king a 2-cycle: the
    // board of the start recurs at ply 5 with Black to move (not a
    // repetition of the start) and the third time of any position is only
    // at ply 24. Ignoring the side to move would end the game at ply 12.
    let (mut c, _, t0) = begin(fen_setup(
        Color::White,
        QUIET,
        "4k3/8/8/8/8/8/8/R3K3 w - - 0 1",
    ));
    let white = ["e1d1", "d1d2", "d2e1"];
    let black = ["e8d8", "d8e8"];
    for ply in 1..=24usize {
        let text = if ply % 2 == 1 {
            white[(ply / 2) % 3]
        } else {
            black[(ply / 2 - 1) % 2]
        };
        let out = play_ucis(&mut c, &[text], t0);
        if ply < 24 {
            assert_eq!(ended(&out), None, "ply {ply}");
        } else {
            assert_eq!(ended(&out), Some(PlayEnd::ThreefoldRepetition));
        }
    }
}

#[test]
fn an_en_passant_square_does_not_make_two_equal_positions_different() {
    // D1228: board, side and castling only. After 1...c5 the pawn on d5 can
    // capture en passant, but the later occurrences of the same board (the
    // kings walk and come back) have no such right: three occurrences all
    // the same, the game ends at ply 9.
    let (mut c, _, t0) = begin(fen_setup(
        Color::White,
        QUIET,
        "4k3/2p5/8/3P4/8/8/8/4K3 b - - 0 1",
    ));
    play_ucis(&mut c, &["c7c5"], t0);
    let walk = ["e1e2", "e8e7", "e2e1", "e7e8"];
    let out = play_ucis(&mut c, &walk, t0);
    assert_eq!(ended(&out), None);
    let out = play_ucis(&mut c, &walk, t0);
    assert_eq!(ended(&out), Some(PlayEnd::ThreefoldRepetition));
    assert_eq!(c.game().mainline().count(), 9);
}

// ---- time (D1226) ----

fn minute() -> TimeMode {
    timebonus(1, 0, 1, 0)
}

#[test]
fn the_flag_of_the_player_loses_the_game_on_a_tick() {
    let (mut c, _, t0) = begin(setup(Color::White, minute()));
    assert!(c.tick(t0 + 59 * SEC).is_empty());
    assert!(c.is_live());
    let out = c.tick(t0 + 60 * SEC);
    assert_eq!(
        ended(&out),
        Some(PlayEnd::Timeout {
            loser: Color::White
        })
    );
    assert_eq!(c.result(), GameResult::Black);
    assert!(!c.is_live());
    assert!(c.tick(t0 + 999 * SEC).is_empty(), "nothing after the end");
}

#[test]
fn the_flag_of_the_engine_wins_for_the_player_and_stops_its_search() {
    let (mut c, _, t0) = begin(setup(Color::White, minute()));
    play_ucis(&mut c, &["e2e4"], t0);
    assert_eq!(c.phase(), PlayPhase::Opponent);
    let out = c.tick(t0 + 61 * SEC);
    assert_eq!(
        ended(&out),
        Some(PlayEnd::Timeout {
            loser: Color::Black
        })
    );
    assert!(has_stop(&out), "the search must be told to stop");
    assert_eq!(c.result(), GameResult::White);
}

#[test]
fn a_move_that_arrives_after_the_flag_loses_and_is_not_played() {
    let (mut c, _, t0) = begin(setup(Color::White, minute()));
    let mv = uci(&c, "e2e4");
    let out = c.player_move(mv, t0 + 61 * SEC);
    assert_eq!(
        ended(&out),
        Some(PlayEnd::Timeout {
            loser: Color::White
        })
    );
    assert_eq!(moved_count(&out), 0);
    assert_eq!(c.game().mainline().count(), 0);
    assert_eq!(c.result(), GameResult::Black);
}

#[test]
fn a_late_move_of_the_engine_loses_for_the_engine() {
    let (mut c, _, t0) = begin(setup(Color::White, minute()));
    play_ucis(&mut c, &["e2e4"], t0);
    let mv = uci(&c, "e7e5");
    let out = c.opponent_moved(Some(mv), None, t0 + 61 * SEC, None);
    assert_eq!(
        ended(&out),
        Some(PlayEnd::Timeout {
            loser: Color::Black
        })
    );
    assert_eq!(c.result(), GameResult::White);
}

#[test]
fn an_engine_time_measured_outside_can_flag_it_even_if_the_instants_do_not() {
    let (mut c, _, t0) = begin(setup(Color::White, minute()));
    play_ucis(&mut c, &["e2e4"], t0);
    let mv = uci(&c, "e7e5");
    let out = c.opponent_moved(Some(mv), Some(70 * SEC), t0, None);
    assert_eq!(
        ended(&out),
        Some(PlayEnd::Timeout {
            loser: Color::Black
        })
    );
}

/// `first`: a move of White that lets the clock of Black run (otherwise
/// White's flag falls).
#[rstest]
#[case::lone_king("4k3/8/8/8/8/8/8/4K2R w - - 0 1", None, PlayEnd::TimeoutDraw)]
#[case::rook_can_mate("4k3/8/8/8/8/8/8/4K2R w - - 0 1", Some("h1h2"), PlayEnd::Timeout { loser: Color::Black })]
#[case::pawn_can_mate("4k3/7p/8/8/8/8/8/4K2R w - - 0 1", None, PlayEnd::Timeout { loser: Color::White })]
#[case::two_knights_can_mate("4k3/8/8/8/8/8/8/4Knn1 w - - 0 1", None, PlayEnd::Timeout { loser: Color::White })]
#[case::a_knight_against_a_rook_is_not_a_dead_position("4kn2/8/8/8/8/8/8/4K2R w - - 0 1", None, PlayEnd::Timeout { loser: Color::White })]
#[case::black_flags_against_a_bare_king(
    "4k2r/8/8/8/8/8/8/4K3 w - - 0 1",
    Some("e1e2"),
    PlayEnd::TimeoutDraw
)]
fn the_flag_draws_when_the_opponent_cannot_mate(
    #[case] fen: &str,
    #[case] first: Option<&str>,
    #[case] expected: PlayEnd,
) {
    let (mut c, _, t0) = begin(fen_setup(Color::White, minute(), fen));
    if let Some(first) = first {
        play_ucis(&mut c, &[first], t0);
    }
    let out = c.tick(t0 + 61 * SEC);
    assert_eq!(ended(&out), Some(expected));
}

#[rstest]
#[case(TimeMode::Depth(3))]
#[case(TimeMode::Nodes(500))]
#[case(TimeMode::MoveTime(5))]
fn the_clock_only_counts_outside_timebonus_modes(#[case] mode: TimeMode) {
    let (mut c, _, t0) = begin(setup(Color::White, mode));
    assert!(!c.clock().is_countdown());
    assert!(c.tick(t0 + 100_000 * SEC).is_empty());
    assert!(c.is_live());
    let mv = uci(&c, "e2e4");
    let out = c.player_move(mv, t0 + 100_000 * SEC);
    assert_eq!(ended(&out), None);
    assert_eq!(c.shown(Color::White, t0 + 100_000 * SEC), 100_000 * SEC);
}

#[test]
fn the_increment_accumulates_for_both_sides_over_a_game() {
    // 1 minute + 30 s: thirty seconds per move is sustainable forever.
    let (mut c, _, t0) = begin(setup(Color::White, timebonus(1, 30, 1, 30)));
    let line = [
        ("e2e4", "e7e5"),
        ("g1f3", "b8c6"),
        ("f1c4", "f8c5"),
        ("c2c3", "g8f6"),
        ("d2d3", "d7d6"),
        ("b1d2", "a7a6"),
    ];
    let mut now = t0;
    for (round, (a, b)) in line.iter().enumerate() {
        now += 30 * SEC;
        let out = c.player_move(uci(&c, a), now);
        assert_eq!(ended(&out), None, "round {round}");
        now += SEC;
        let out = c.opponent_moved(Some(uci(&c, b)), None, now, None);
        assert_eq!(ended(&out), None, "round {round}");
    }
    assert_eq!(c.shown(Color::White, now), 60 * SEC);
    // Black: 60 + 6 * (30 - 1) after six one-second moves.
    assert_eq!(c.shown(Color::Black, now), (60 + 6 * 29) * SEC);
    assert!(c.is_live());
}

// ---- resignation and failure ----

#[rstest]
#[case(Color::White, GameResult::Black)]
#[case(Color::Black, GameResult::White)]
fn resigning_records_a_loss_for_the_player(#[case] color: Color, #[case] result: GameResult) {
    let (mut c, _, t0) = begin(setup(color, QUIET));
    let out = c.resign(t0);
    assert_eq!(ended(&out), Some(PlayEnd::Resignation { loser: color }));
    assert_eq!(c.result(), result);
}

#[test]
fn resigning_while_the_engine_thinks_stops_the_search() {
    let (mut c, _, t0) = begin(setup(Color::Black, QUIET));
    assert_eq!(c.phase(), PlayPhase::Opponent);
    let out = c.resign(t0);
    assert!(has_stop(&out));
    assert_eq!(c.result(), GameResult::White);
}

#[test]
fn resigning_twice_does_nothing_the_second_time() {
    let (mut c, _, t0) = begin(setup(Color::White, QUIET));
    c.resign(t0);
    assert!(c.resign(t0).is_empty());
    assert_eq!(c.result(), GameResult::Black);
}

#[test]
fn no_end_can_be_overwritten_by_a_later_one() {
    let (mut c, _, t0) = begin(setup(Color::White, minute()));
    c.resign(t0);
    assert!(c.tick(t0 + 999 * SEC).is_empty());
    assert!(c.player_move(uci(&c, "e2e4"), t0).iter().all(|o| !matches!(
        o,
        mychess_app::PlayOutput::Moved(_) | mychess_app::PlayOutput::Ended(_)
    )));
    assert_eq!(c.result(), GameResult::Black);
}
