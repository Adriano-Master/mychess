//! M8-2a contract tests of one game between engines (spec D1267, D1269,
//! D1270, D1274-D1277, D1292, D1297, D1302, D1309): limits sent to the
//! engines, clock, end of game, adjudication and the sign of the notes.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m8_2_common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use m8_2_common::*;
use mychess_app::tournament::{
    Adjudication, Adjudicator, DrawRule, GameSetup, Opening, Reply, ResignRule, Step,
    TablebaseProbe, TbOutcome, Termination, TimeControl, TournamentGame,
};
use mychess_app::{BookSelection, PlayBook, ThinkLimits};
use mychess_book::Book;
use mychess_core::{Color, GameResult, Move, Position};
use mychess_engine::Score;
use rstest::rstest;

fn setup(time: TimeControl) -> GameSetup {
    GameSetup {
        opening: Opening::initial(),
        opening_is_book: false,
        time,
        leeway_percent: 175,
        margin: Duration::from_secs(2),
        depth_limit: Duration::from_secs(600),
        claim_draws: true,
        adjudication: Adjudication::default(),
        book: None,
        book_ply_limit: None,
        ponder: [false, false],
        seed: 7,
        number: 1,
        tablebase: None,
    }
}

fn clock(base_ms: u64, increment_ms: u64) -> TimeControl {
    TimeControl::Clock {
        base_ms,
        increment_ms,
    }
}

fn from_fen(fen: &str) -> Opening {
    Opening {
        name: None,
        start: Position::from_fen(fen).expect("fen"),
        moves: Vec::new(),
    }
}

fn reply(position: &Position, uci: &str, spent_ms: u64, white_cp: Option<i32>) -> Reply {
    Reply {
        mv: Some(position.parse_uci(uci).expect("uci")),
        spent: Duration::from_millis(spent_ms),
        score: white_cp.map(Score::Cp),
        depth: Some(12),
        ponder: None,
    }
}

/// Asks for the turn and answers with `uci`.
fn play(game: &mut TournamentGame, uci: &str, spent_ms: u64, white_cp: Option<i32>, now: Instant) {
    assert!(
        matches!(game.advance(now), Step::Think(_)),
        "the game should be on: {:?}",
        game.end()
    );
    let r = reply(game.position(), uci, spent_ms, white_cp);
    game.reply(&r, now);
}

fn think(game: &mut TournamentGame, now: Instant) -> mychess_app::tournament::Turn {
    match game.advance(now) {
        Step::Think(turn) => *turn,
        Step::Over(end) => panic!("over: {end:?}"),
    }
}

// ---- limits sent to the engine ----

#[test]
fn clock_mode_sends_both_clocks_and_increments_and_the_deadline_is_time_plus_margin() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(clock(60_000, 1_000)), now);
    let turn = think(&mut game, now);
    let ThinkLimits::Clock(c) = turn.think.limits else {
        panic!("clock expected")
    };
    assert_eq!(c.white_time, Duration::from_secs(60));
    assert_eq!(c.black_time, Duration::from_secs(60));
    assert_eq!(c.white_increment, Duration::from_secs(1));
    assert_eq!(c.black_increment, Duration::from_secs(1));
    assert_eq!(turn.think.side, Color::White);
    assert_eq!(turn.deadline, Duration::from_secs(62));
}

#[test]
fn movetime_deadline_is_leeway_times_time_plus_margin() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(TimeControl::MoveTime { ms: 1_500 }), now);
    let turn = think(&mut game, now);
    assert_eq!(
        turn.think.limits,
        ThinkLimits::MoveTime(Duration::from_millis(1_500))
    );
    assert_eq!(turn.deadline, Duration::from_millis(2_625 + 2_000));
}

#[test]
fn depth_and_nodes_modes_use_the_per_move_ceiling_as_deadline() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(TimeControl::Depth(7)), now);
    let turn = think(&mut game, now);
    assert_eq!(turn.think.limits, ThinkLimits::Depth(7));
    assert_eq!(turn.deadline, Duration::from_secs(600));
    let mut game = TournamentGame::new(setup(TimeControl::Nodes(12_345)), now);
    let turn = think(&mut game, now);
    assert_eq!(turn.think.limits, ThinkLimits::Nodes(12_345));
}

#[test]
fn the_search_carries_the_start_position_and_every_move_so_far() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(TimeControl::Depth(3)), now);
    play(&mut game, "e2e4", 10, Some(0), now);
    play(&mut game, "e7e5", 10, Some(0), now);
    let turn = think(&mut game, now);
    assert_eq!(turn.think.start, Position::new());
    assert_eq!(turn.think.moves.len(), 2);
    assert_eq!(turn.think.side, Color::White);
}

// ---- the clock ----

#[test]
fn the_increment_is_added_on_the_first_move_of_each_side() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(clock(60_000, 1_000)), now);
    play(&mut game, "e2e4", 2_500, Some(0), now);
    assert_eq!(game.played()[0].left, Some(Duration::from_millis(58_500)));
    play(&mut game, "e7e5", 0, Some(0), now);
    assert_eq!(game.played()[1].left, Some(Duration::from_secs(61)));
}

#[test]
fn zero_base_with_increment_starts_with_the_increment_as_time() {
    // D1292: otherwise the first move always loses on time.
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(clock(0, 1_000)), now);
    let turn = think(&mut game, now);
    let ThinkLimits::Clock(c) = turn.think.limits else {
        panic!("clock")
    };
    assert_eq!(c.white_time, Duration::from_secs(1));
    let r = reply(game.position(), "e2e4", 900, Some(0));
    game.reply(&r, now);
    assert!(game.end().is_none(), "{:?}", game.end());
}

#[test]
fn opening_moves_cost_no_time_and_no_increment() {
    let now = Instant::now();
    let pos = Position::new();
    let e4 = pos.parse_uci("e2e4").unwrap();
    let after = pos.after(e4).unwrap();
    let e5 = after.parse_uci("e7e5").unwrap();
    let mut s = setup(clock(10_000, 1_000));
    s.opening = Opening {
        name: Some("Open game".into()),
        start: pos,
        moves: vec![e4, e5],
    };
    let game = TournamentGame::new(s, now);
    assert_eq!(game.moves().len(), 2);
    assert!(
        game.played()
            .iter()
            .all(|m| m.free && m.spent.is_none() && m.score.is_none())
    );
    assert_eq!(
        game.clock().shown(Color::White, now),
        Duration::from_secs(10)
    );
    assert_eq!(
        game.clock().shown(Color::Black, now),
        Duration::from_secs(10)
    );
}

#[test]
fn a_flag_that_falls_loses_on_time_to_the_opponent() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(clock(60_000, 0)), now);
    let r = reply(game.position(), "e2e4", 61_000, Some(0));
    assert!(matches!(game.advance(now), Step::Think(_)));
    game.reply(&r, now);
    let end = game.end().expect("over");
    assert_eq!(end.result, GameResult::Black);
    assert_eq!(end.termination, Termination::TimeForfeit);
    assert_eq!(end.comment.as_deref(), Some("White loses on time"));
    assert!(game.moves().is_empty(), "the late move is not played");
}

#[test]
fn flag_with_insufficient_mating_material_on_the_other_side_is_a_draw() {
    let now = Instant::now();
    let mut s = setup(clock(1_000, 0));
    s.opening = from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
    let mut game = TournamentGame::new(s, now);
    let r = reply(game.position(), "a1a2", 5_000, Some(0));
    assert!(matches!(game.advance(now), Step::Think(_)));
    game.reply(&r, now);
    let end = game.end().expect("over");
    assert_eq!(end.result, GameResult::Draw, "Black has a bare king");
    assert_eq!(end.termination, Termination::TimeForfeit);
}

#[test]
fn flag_of_the_side_that_cannot_be_mated_by_itself_still_loses_when_the_opponent_can_mate() {
    let now = Instant::now();
    let mut s = setup(clock(1_000, 0));
    s.opening = from_fen("4k3/8/8/8/8/8/8/R3K3 b - - 0 1");
    let mut game = TournamentGame::new(s, now);
    let r = reply(game.position(), "e8e7", 5_000, Some(0));
    assert!(matches!(game.advance(now), Step::Think(_)));
    game.reply(&r, now);
    assert_eq!(game.end().unwrap().result, GameResult::White);
}

#[rstest]
#[case(1_750, false)]
#[case(1_751, true)]
fn movetime_leeway_of_175_percent_is_the_limit(#[case] spent: u64, #[case] loses: bool) {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(TimeControl::MoveTime { ms: 1_000 }), now);
    play(&mut game, "e2e4", spent, Some(0), now);
    assert_eq!(game.end().is_some(), loses);
    if loses {
        assert_eq!(game.end().unwrap().termination, Termination::TimeForfeit);
    }
}

#[test]
fn leeway_is_configurable() {
    let now = Instant::now();
    let mut s = setup(TimeControl::MoveTime { ms: 1_000 });
    s.leeway_percent = 100;
    let mut game = TournamentGame::new(s, now);
    play(&mut game, "e2e4", 1_001, Some(0), now);
    assert!(game.end().is_some());
}

#[test]
fn depth_mode_never_loses_on_time_however_long_the_move_took() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(TimeControl::Depth(5)), now);
    play(&mut game, "e2e4", 3_600_000, Some(0), now);
    assert!(game.end().is_none());
}

// ---- illegal moves ----

#[test]
fn an_illegal_move_loses_the_game_for_the_engine_with_rules_infraction() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(TimeControl::Depth(5)), now);
    let other = Position::new()
        .after(Position::new().parse_uci("e2e4").unwrap())
        .unwrap();
    let illegal: Move = other.parse_uci("e7e5").unwrap();
    assert!(matches!(game.advance(now), Step::Think(_)));
    game.reply(
        &Reply {
            mv: Some(illegal),
            spent: Duration::ZERO,
            score: None,
            depth: None,
            ponder: None,
        },
        now,
    );
    let end = game.end().expect("over");
    assert_eq!(end.result, GameResult::Black);
    assert_eq!(end.termination, Termination::RulesInfraction);
    assert!(game.moves().is_empty());
}

#[test]
fn bestmove_none_in_a_position_with_moves_is_a_loss_not_a_pass() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(TimeControl::Depth(5)), now);
    play(&mut game, "e2e4", 1, None, now);
    assert!(matches!(game.advance(now), Step::Think(_)));
    game.reply(
        &Reply {
            mv: None,
            spent: Duration::ZERO,
            score: None,
            depth: None,
            ponder: None,
        },
        now,
    );
    let end = game.end().unwrap();
    assert_eq!(end.result, GameResult::White);
    assert_eq!(end.termination, Termination::RulesInfraction);
}

#[test]
fn replies_after_the_end_change_nothing() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(TimeControl::Depth(5)), now);
    game.adjudicate(GameResult::Draw);
    let r = reply(game.position(), "e2e4", 1, None);
    game.reply(&r, now);
    assert!(game.moves().is_empty());
    game.adjudicate(GameResult::White);
    assert_eq!(game.end().unwrap().result, GameResult::Draw);
}

// ---- end of game by the rules ----

#[test]
fn mate_ends_the_game_normally() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(TimeControl::Depth(5)), now);
    for (i, text) in ["f2f3", "e7e5", "g2g4", "d8h4"].iter().enumerate() {
        assert!(game.end().is_none(), "ply {i}");
        play(&mut game, text, 1, Some(0), now);
    }
    let end = game.end().unwrap();
    assert_eq!(
        (end.result, end.termination),
        (GameResult::Black, Termination::Normal)
    );
}

#[test]
fn stalemate_is_a_draw_at_once_with_its_comment() {
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(5));
    s.opening = from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    let mut game = TournamentGame::new(s, now);
    let Step::Over(end) = game.advance(now) else {
        panic!("over expected")
    };
    assert_eq!(end.result, GameResult::Draw);
    assert_eq!(end.comment.as_deref(), Some("Stalemate"));
}

#[test]
fn insufficient_material_draws_with_its_comment() {
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(5));
    s.opening = from_fen("4k3/8/8/8/8/8/8/2B1K3 w - - 0 1");
    let mut game = TournamentGame::new(s, now);
    let Step::Over(end) = game.advance(now) else {
        panic!("over")
    };
    assert_eq!(end.comment.as_deref(), Some("Insufficient material"));
    assert_eq!(end.result, GameResult::Draw);
}

#[test]
fn threefold_repetition_draws_on_the_third_occurrence_only() {
    let now = Instant::now();
    let mut game = TournamentGame::new(setup(TimeControl::Depth(5)), now);
    for (i, text) in SHUFFLE.iter().enumerate() {
        assert!(
            game.end().is_none(),
            "ply {i} ended early: {:?}",
            game.end()
        );
        play(&mut game, text, 1, Some(0), now);
    }
    let end = game.end().unwrap();
    assert_eq!(end.result, GameResult::Draw);
    assert_eq!(end.comment.as_deref(), Some("3 fold repetition"));
}

#[test]
fn repetition_needs_the_same_side_to_move_and_castling_rights() {
    // Same piece placement, different castling rights: not a repetition.
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(5));
    s.opening = from_fen("r3k3/8/8/8/8/8/8/R3K3 w Qq - 0 1");
    let mut game = TournamentGame::new(s, now);
    // Ra1-a2, Ra8-a7, Ra2-a1, Ra7-a8: pieces back, castling rights kept
    // (rooks moved: rights lost) -> never the same position as the start.
    for text in [
        "a1a2", "a8a7", "a2a1", "a7a8", "a1a2", "a8a7", "a2a1", "a7a8",
    ] {
        play(&mut game, text, 1, Some(0), now);
    }
    assert!(game.end().is_none(), "{:?}", game.end());
}

#[test]
fn fifty_move_rule_draws_when_the_counter_reaches_one_hundred_half_moves() {
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(5));
    s.opening = from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 99 80");
    let mut game = TournamentGame::new(s, now);
    play(&mut game, "a1a2", 1, Some(0), now);
    let end = game.end().expect("50 move rule");
    assert_eq!(end.result, GameResult::Draw);
    assert_eq!(end.comment.as_deref(), Some("50 move rule"));
}

#[test]
fn claim_draws_off_ignores_repetition_and_fifty_moves_but_not_mate() {
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(5));
    s.claim_draws = false;
    s.opening = from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 99 80");
    let mut game = TournamentGame::new(s, now);
    play(&mut game, "a1a2", 1, Some(0), now);
    assert!(game.end().is_none());
    let mut s = setup(TimeControl::Depth(5));
    s.claim_draws = false;
    let mut game = TournamentGame::new(s, now);
    for text in SHUFFLE {
        play(&mut game, text, 1, Some(0), now);
    }
    assert!(game.end().is_none());
    // A mate still ends it.
    let mut s = setup(TimeControl::Depth(5));
    s.claim_draws = false;
    let mut game = TournamentGame::new(s, now);
    for text in ["f2f3", "e7e5", "g2g4", "d8h4"] {
        play(&mut game, text, 1, Some(0), now);
    }
    assert_eq!(game.end().unwrap().result, GameResult::Black);
}

// ---- the sign of the notes (D1302) ----

fn resign(centipawns: u32, moves: u32, require_both: bool) -> Adjudication {
    Adjudication {
        resign: Some(ResignRule {
            centipawns,
            moves,
            require_both,
        }),
        ..Adjudication::default()
    }
}

/// Four plies: e4 e5 Nf3 Nc6 with the given White-point-of-view scores.
fn four_plies(adj: Adjudication, scores: [Option<i32>; 4]) -> TournamentGame {
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(5));
    s.adjudication = adj;
    let mut game = TournamentGame::new(s, now);
    for (text, cp) in ["e2e4", "e7e5", "g1f3", "b8c6"].iter().zip(scores) {
        if game.end().is_some() {
            break;
        }
        play(&mut game, text, 1, cp, now);
    }
    game
}

#[test]
fn white_pov_plus_700_on_the_moves_of_black_means_black_is_losing_and_resigns() {
    let game = four_plies(resign(600, 2, true), [Some(700); 4]);
    let end = game.end().expect("adjudicated");
    assert_eq!(end.result, GameResult::White);
    assert_eq!(end.termination, Termination::Adjudication);
    assert_eq!(end.comment.as_deref(), Some("Adjudicated: resign +7.00"));
}

#[test]
fn white_pov_minus_700_on_the_moves_of_black_means_black_is_winning() {
    let game = four_plies(resign(600, 2, true), [Some(-700); 4]);
    let end = game.end().expect("adjudicated");
    assert_eq!(end.result, GameResult::Black, "White is the one losing");
    assert_eq!(end.comment.as_deref(), Some("Adjudicated: resign +7.00"));
}

#[test]
fn played_move_notes_are_from_the_point_of_view_of_who_moved() {
    let game = four_plies(
        Adjudication::default(),
        [Some(30), Some(30), Some(-80), Some(-80)],
    );
    let notes: Vec<i32> = game
        .played()
        .iter()
        .map(|m| m.score.unwrap().score.as_centipawns())
        .collect();
    // White POV +30 on White's move = +30; on Black's move = -30 for Black.
    assert_eq!(notes, [30, -30, -80, 80]);
}

#[test]
fn mate_scores_use_32750_minus_n_and_trigger_the_resignation() {
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(5));
    s.adjudication = resign(600, 2, true);
    let mut game = TournamentGame::new(s, now);
    for text in ["e2e4", "e7e5", "g1f3", "b8c6"] {
        assert!(matches!(game.advance(now), Step::Think(_)));
        let mut r = reply(game.position(), text, 1, None);
        r.score = Some(Score::Mate(5)); // White mates in 5
        game.reply(&r, now);
        if game.end().is_some() {
            break;
        }
    }
    assert_eq!(game.end().expect("over").result, GameResult::White);
    assert_eq!(Score::Mate(3).as_centipawns(), 32_747);
    assert_eq!(Score::Mate(-3).as_centipawns(), -32_747);
}

#[test]
fn require_both_wants_the_winner_to_agree_with_its_last_note() {
    // Black's notes say -700 for it (+700 white POV); White reports nothing.
    let scores = [None, Some(700), None, Some(700)];
    assert!(four_plies(resign(600, 2, true), scores).end().is_none());
    let game = four_plies(resign(600, 2, false), scores);
    assert_eq!(game.end().expect("over").result, GameResult::White);
}

#[test]
fn the_winner_only_needs_its_last_note_not_a_streak() {
    // White: +100 then +700; Black always -700 for itself.
    let scores = [Some(100), Some(700), Some(700), Some(700)];
    let game = four_plies(resign(600, 2, true), scores);
    assert_eq!(game.end().expect("over").result, GameResult::White);
}

#[test]
fn a_note_that_is_not_bad_enough_breaks_the_streak() {
    // Black: -700, -100 (white POV +100), -700 ...: never 2 in a row.
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(5));
    s.adjudication = resign(600, 2, false);
    let mut game = TournamentGame::new(s, now);
    for (text, cp) in [
        ("e2e4", 700),
        ("e7e5", 700),
        ("g1f3", 700),
        ("b8c6", 100),
        ("f1c4", 700),
        ("g8f6", 700),
    ] {
        play(&mut game, text, 1, Some(cp), now);
    }
    assert!(game.end().is_none(), "{:?}", game.end());
}

#[test]
fn a_note_missing_breaks_the_streak() {
    let scores = [Some(700), Some(700), Some(700), None];
    assert!(four_plies(resign(600, 2, false), scores).end().is_none());
}

#[test]
fn zeroed_rules_are_ignored_and_never_fire_on_their_own() {
    // D1309.
    let adj = Adjudication {
        resign: Some(ResignRule {
            centipawns: 0,
            moves: 0,
            require_both: false,
        }),
        draw: Some(DrawRule {
            after_move: 0,
            centipawns: 0,
            moves: 0,
        }),
        max_plies: Some(0),
        ..Adjudication::default()
    };
    let game = four_plies(adj, [Some(900), Some(900), Some(900), Some(900)]);
    assert!(game.end().is_none(), "{:?}", game.end());
    let a = Adjudicator::new(adj);
    assert_eq!(a.verdict(500), None);
}

#[test]
fn draw_rule_waits_for_the_move_and_needs_quiet_notes_from_both_sides() {
    let adj = Adjudication {
        draw: Some(DrawRule {
            after_move: 2,
            centipawns: 10,
            moves: 2,
        }),
        ..Adjudication::default()
    };
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(5));
    s.adjudication = adj;
    let mut game = TournamentGame::new(s, now);
    for (i, text) in ["e2e4", "e7e5", "g1f3"].iter().enumerate() {
        play(&mut game, text, 1, Some(5), now);
        assert!(game.end().is_none(), "ply {}", i + 1);
    }
    play(&mut game, "b8c6", 1, Some(5), now);
    let end = game.end().expect("draw after move 2");
    assert_eq!(end.result, GameResult::Draw);
    assert_eq!(end.termination, Termination::Adjudication);
    assert_eq!(end.comment.as_deref(), Some("Adjudicated: draw"));
}

#[test]
fn draw_rule_does_not_fire_when_a_side_is_above_the_bound_or_silent() {
    let adj = Adjudication {
        draw: Some(DrawRule {
            after_move: 2,
            centipawns: 10,
            moves: 2,
        }),
        ..Adjudication::default()
    };
    assert!(
        four_plies(adj, [Some(5), Some(5), Some(5), Some(50)])
            .end()
            .is_none()
    );
    assert!(
        four_plies(adj, [Some(5), Some(5), Some(5), None])
            .end()
            .is_none()
    );
    assert!(
        four_plies(adj, [Some(5), Some(5), None, Some(5)])
            .end()
            .is_none()
    );
}

#[test]
fn max_plies_counts_the_plies_of_the_opening_too_and_draws() {
    let now = Instant::now();
    let pos = Position::new();
    let e4 = pos.parse_uci("e2e4").unwrap();
    let e5 = pos.after(e4).unwrap().parse_uci("e7e5").unwrap();
    let mut s = setup(TimeControl::Depth(5));
    s.opening = Opening {
        name: None,
        start: pos,
        moves: vec![e4, e5],
    };
    s.adjudication = Adjudication {
        max_plies: Some(4),
        ..Adjudication::default()
    };
    let mut game = TournamentGame::new(s, now);
    assert!(game.end().is_none());
    play(&mut game, "g1f3", 1, None, now);
    assert!(game.end().is_none());
    play(&mut game, "b8c6", 1, None, now);
    let end = game.end().expect("limit");
    assert_eq!(end.result, GameResult::Draw);
    assert_eq!(end.comment.as_deref(), Some("Adjudicated: move limit"));
}

#[test]
fn adjudicator_directly_resigns_on_threshold_boundaries() {
    let mut a = Adjudicator::new(resign(600, 2, false));
    a.record(Color::Black, Some(-600));
    a.record(Color::Black, Some(-599));
    assert!(a.verdict(10).is_none(), "-599 is above the threshold");
    a.record(Color::Black, Some(-600));
    assert!(a.verdict(10).is_none(), "the streak is broken by -599");
    a.record(Color::Black, Some(-600));
    assert_eq!(a.verdict(10).unwrap().result, GameResult::White);
}

#[derive(Debug)]
struct Probe(TbOutcome, u32);
impl TablebaseProbe for Probe {
    fn max_pieces(&self) -> u32 {
        self.1
    }
    fn probe(&self, _: &Position) -> Option<TbOutcome> {
        Some(self.0)
    }
}

#[rstest]
#[case(TbOutcome::Win, GameResult::White)]
#[case(TbOutcome::Loss, GameResult::Black)]
#[case(TbOutcome::CursedWin, GameResult::Draw)]
#[case(TbOutcome::BlessedLoss, GameResult::Draw)]
#[case(TbOutcome::Draw, GameResult::Draw)]
fn tablebase_verdict_follows_the_wdl_from_the_side_to_move(
    #[case] outcome: TbOutcome,
    #[case] expected: GameResult,
) {
    let adj = Adjudication {
        tablebase: true,
        tb_pieces: 5,
        ..Adjudication::default()
    };
    let a = Adjudicator::new(adj);
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
    let v = a.tablebase_verdict(Some(&Probe(outcome, 5)), &pos).unwrap();
    assert_eq!(v.result, expected);
    assert_eq!(v.reason, "TB adjudication");
}

#[test]
fn tablebase_is_off_without_probe_or_with_too_many_pieces_or_when_disabled() {
    let adj = Adjudication {
        tablebase: true,
        tb_pieces: 3,
        ..Adjudication::default()
    };
    let a = Adjudicator::new(adj);
    let four = Position::from_fen("4k3/8/8/8/8/8/8/R3K1N1 w - - 0 1").unwrap();
    assert!(a.tablebase_verdict(None, &four).is_none());
    assert!(
        a.tablebase_verdict(Some(&Probe(TbOutcome::Win, 7)), &four)
            .is_none()
    );
    let off = Adjudicator::new(Adjudication {
        tablebase: false,
        ..adj
    });
    let three = Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
    assert!(
        off.tablebase_verdict(Some(&Probe(TbOutcome::Win, 7)), &three)
            .is_none()
    );
}

#[test]
fn a_tablebase_result_ends_the_game_through_the_setup_probe() {
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(5));
    s.adjudication = Adjudication {
        tablebase: true,
        tb_pieces: 5,
        ..Adjudication::default()
    };
    s.tablebase = Some(Arc::new(Probe(TbOutcome::Loss, 5)));
    s.opening = from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
    let game = TournamentGame::new(s, now);
    let end = game.end().expect("TB verdict at the start");
    assert_eq!(end.result, GameResult::Black);
    assert_eq!(end.termination, Termination::Adjudication);
}

// ---- book ----

fn book(lines: &[(&[&str], u16)]) -> PlayBook {
    use std::collections::BTreeMap;
    let mut per: BTreeMap<String, (Position, Vec<(Move, u16)>)> = BTreeMap::new();
    for (line, weight) in lines {
        let mut pos = Position::new();
        for text in *line {
            let m = pos.parse_uci(text).unwrap();
            let e = per
                .entry(pos.fen())
                .or_insert_with(|| (pos.clone(), Vec::new()));
            if !e.1.iter().any(|(x, _)| *x == m) {
                e.1.push((m, *weight));
            }
            pos.play(m).unwrap();
        }
    }
    let mut b = Book::new();
    for (_, (pos, moves)) in per {
        b.set_position_moves(&pos, &moves).unwrap();
    }
    PlayBook::new(b, BookSelection::Weighted)
}

#[test]
fn book_moves_are_free_and_the_last_one_is_commented() {
    let now = Instant::now();
    let mut s = setup(clock(10_000, 1_000));
    s.book = Some(book(&[(&["e2e4", "e7e5"], 10)]));
    let mut game = TournamentGame::new(s, now);
    let turn = think(&mut game, now);
    assert_eq!(game.moves().len(), 2, "both book moves were played at once");
    assert!(game.played().iter().all(|m| m.free));
    assert!(
        game.played()[1]
            .note
            .as_deref()
            .unwrap()
            .starts_with("Last book move")
    );
    assert_eq!(
        game.clock().shown(Color::White, now),
        Duration::from_secs(10)
    );
    assert_eq!(turn.think.moves.len(), 2);
    assert_eq!(game.book_line().len(), 2);
}

#[test]
fn book_ply_limit_stops_the_book_early() {
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(3));
    s.book = Some(book(&[(&["e2e4", "e7e5", "g1f3"], 10)]));
    s.book_ply_limit = Some(1);
    let mut game = TournamentGame::new(s, now);
    think(&mut game, now);
    assert_eq!(game.moves().len(), 1);
}

#[test]
fn the_book_draw_is_reproducible_for_the_same_seed_and_game_number() {
    let now = Instant::now();
    let make = |seed: u64, number: u32| {
        let mut s = setup(TimeControl::Depth(3));
        s.book = Some(book(&[
            (&["e2e4"], 50),
            (&["d2d4"], 50),
            (&["c2c4"], 50),
            (&["g1f3"], 50),
        ]));
        s.seed = seed;
        s.number = number;
        let mut g = TournamentGame::new(s, now);
        think(&mut g, now);
        g.moves().to_vec()
    };
    assert_eq!(make(5, 3), make(5, 3));
    let variety: std::collections::HashSet<_> = (1..40).map(|n| make(5, n)).collect();
    assert!(variety.len() > 1, "different games draw different lines");
}

// ---- ponder ----

fn with_ponder(side_flags: [bool; 2]) -> (TournamentGame, Instant) {
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(3));
    s.ponder = side_flags;
    let mut game = TournamentGame::new(s, now);
    assert!(matches!(game.advance(now), Step::Think(_)));
    let pos = game.position().clone();
    let e4 = pos.parse_uci("e2e4").unwrap();
    let e5 = pos.after(e4).unwrap().parse_uci("e7e5").unwrap();
    game.reply(
        &Reply {
            mv: Some(e4),
            spent: Duration::from_millis(1),
            score: Some(Score::Cp(0)),
            depth: Some(1),
            ponder: Some(e5),
        },
        now,
    );
    (game, now)
}

#[test]
fn the_engine_that_just_moved_ponders_on_the_expected_reply() {
    let (mut game, now) = with_ponder([true, true]);
    let turn = think(&mut game, now);
    let plan = turn.ponder.expect("a ponder plan");
    assert_eq!(plan.side, Color::White);
    assert_eq!(plan.think.moves.len(), 2, "e4 and the expected e5");
    assert_eq!(plan.think.side, Color::White);
    assert_eq!(game.position().uci(plan.expected), "e7e5");
}

#[test]
fn no_ponder_plan_when_the_option_is_off_for_that_side_or_after_forgetting() {
    let (mut game, now) = with_ponder([false, true]);
    assert!(think(&mut game, now).ponder.is_none());
    let (mut game, now) = with_ponder([true, true]);
    game.forget_ponder();
    assert!(think(&mut game, now).ponder.is_none());
}

#[test]
fn an_expected_move_that_is_not_legal_gives_no_plan() {
    let now = Instant::now();
    let mut s = setup(TimeControl::Depth(3));
    s.ponder = [true, true];
    let mut game = TournamentGame::new(s, now);
    assert!(matches!(game.advance(now), Step::Think(_)));
    let pos = game.position().clone();
    let e4 = pos.parse_uci("e2e4").unwrap();
    // A white move as the "expected" reply of Black: illegal.
    let bogus = pos.parse_uci("d2d4").unwrap();
    game.reply(
        &Reply {
            mv: Some(e4),
            spent: Duration::ZERO,
            score: None,
            depth: None,
            ponder: Some(bogus),
        },
        now,
    );
    assert!(think(&mut game, now).ponder.is_none());
}
