//! M4-10b/c/d contract tests of the pure parts of the game report (spec
//! `m4-10-relatorio-da-partida.md`, D1560-D1575): the inputs of the core
//! (book through the ECO list, SAN, the conservative sacrifice), the
//! `[%eval]` reading and writing, the markers and the presets, and the
//! state machine of the guided review. No engine and no state.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::similar_names
)]

#[path = "m4_6_common/mod.rs"]
mod m4_6_common;

use m4_6_common::{game_from, game_of, pv_of};
use mychess_app::graphs::MarkerKind;
use mychess_app::report::{
    Attempt, GameReport, HINT_AFTER, MoveClass, MoveInput, ReportPreset, ReportThresholds, Review,
    ReviewStep, SideFilter, build_report, evals_from_game, inputs_from_game, key_moments,
    marker_kind, write_actions,
};
use mychess_app::{AnnotateAction, PositionEval, apply_actions};
use mychess_core::{Color, EcoBook, Game, Position};
use proptest::prelude::*;

fn ev(score: f32) -> PositionEval {
    PositionEval {
        score,
        mate: None,
        pv: Vec::new(),
        depth: 16,
        second: None,
    }
}

fn ev_pv(score: f32, pv: Vec<mychess_core::Move>) -> PositionEval {
    PositionEval { pv, ..ev(score) }
}

// ---- markers and presets ----

#[test]
fn only_the_six_notable_classes_draw_markers_each_with_its_own_kind() {
    let expected = [
        (MoveClass::Brilliant, Some(MarkerKind::Brilliant)),
        (MoveClass::Great, Some(MarkerKind::Great)),
        (MoveClass::Inaccuracy, Some(MarkerKind::Inaccuracy)),
        (MoveClass::Mistake, Some(MarkerKind::Mistake)),
        (MoveClass::Miss, Some(MarkerKind::Miss)),
        (MoveClass::Blunder, Some(MarkerKind::Blunder)),
        (MoveClass::Best, None),
        (MoveClass::Excellent, None),
        (MoveClass::Good, None),
        (MoveClass::Book, None),
    ];
    for (class, kind) in expected {
        assert_eq!(marker_kind(class), kind, "{class:?}");
    }
    // Every class is covered by the table above.
    assert_eq!(expected.len(), MoveClass::ALL.len());
}

#[test]
fn the_presets_are_fast_12_standard_16_deep_22_and_standard_is_the_default() {
    assert_eq!(ReportPreset::Fast.depth(), 12);
    assert_eq!(ReportPreset::Standard.depth(), 16);
    assert_eq!(ReportPreset::Deep.depth(), 22);
    assert_eq!(ReportPreset::default(), ReportPreset::Standard);
    assert_eq!(
        ReportPreset::ALL,
        [
            ReportPreset::Fast,
            ReportPreset::Standard,
            ReportPreset::Deep
        ]
    );
    let keys: Vec<_> = ReportPreset::ALL.iter().map(|p| p.label_key()).collect();
    assert_eq!(
        keys,
        [
            "report-preset-fast",
            "report-preset-standard",
            "report-preset-deep"
        ]
    );
}

// ---- inputs: book, SAN ----

const ECO_TEXT: &str = "B20 \"Sicilian Defence\" 1.e4 c5 *\nC20 \"King's Pawn Game\" 1.e4 e5 *\n";

#[test]
fn a_move_is_book_when_the_position_after_it_is_in_the_eco_list() {
    let game = game_of(&["e2e4", "c7c5", "g1f3", "d7d6"]);
    let evals: Vec<_> = (0..5).map(|_| ev(0.2)).collect();
    let book = EcoBook::parse(ECO_TEXT).unwrap();
    let inputs = inputs_from_game(&game, &evals, Some(&book));
    let flags: Vec<bool> = inputs.moves.iter().map(|m| m.book).collect();
    // 1.e4 c5 reaches the known line; 2.Nf3 and 2...d6 leave it.
    assert!(flags[1], "1...c5 is in the list: {flags:?}");
    assert!(!flags[2] && !flags[3], "{flags:?}");
    assert_eq!(inputs.sans, ["e4", "c5", "Nf3", "d6"]);
}

#[test]
fn without_a_loaded_eco_list_no_move_is_book() {
    let game = game_of(&["e2e4", "c7c5"]);
    let evals = [ev(0.2), ev(0.2), ev(0.2)];
    let inputs = inputs_from_game(&game, &evals, None);
    assert!(inputs.moves.iter().all(|m| !m.book));
}

#[test]
fn a_transposed_line_is_book_by_position_and_a_foreign_one_is_not() {
    let book = EcoBook::parse(ECO_TEXT).unwrap();
    // The same position reached by another order is still the same position.
    let evals: Vec<_> = (0..3).map(|_| ev(0.2)).collect();
    let d4 = game_of(&["d2d4", "d7d5"]);
    let d4 = inputs_from_game(&d4, &evals, Some(&book));
    assert!(d4.moves.iter().all(|m| !m.book), "1.d4 d5 is not listed");
    let e4 = game_of(&["e2e4", "e7e5"]);
    let e4 = inputs_from_game(&e4, &evals, Some(&book));
    assert!(e4.moves[1].book);
}

#[test]
fn a_book_move_wins_over_a_blunder_and_stays_out_of_the_accuracy() {
    let game = game_of(&["e2e4", "c7c5"]);
    let book = EcoBook::parse(ECO_TEXT).unwrap();
    // Black's c5 loses five pawns on the numbers, but it is theory.
    let evals = [ev(0.2), ev(0.2), ev(5.0)];
    let inputs = inputs_from_game(&game, &evals, Some(&book));
    let report = build_report(&inputs.moves, &ReportThresholds::default());
    assert_eq!(report.moves[1].class, MoveClass::Book);
    assert_eq!(report.black.accuracy, None, "only book moves: no accuracy");
    assert_eq!(report.black.count(MoveClass::Book), 1);
}

#[test]
fn inputs_stop_at_the_first_move_without_evaluations() {
    let game = game_of(&["e2e4", "e7e5", "g1f3"]);
    assert!(inputs_from_game(&game, &[], None).moves.is_empty());
    let one = inputs_from_game(&game, &[ev(0.1)], None);
    assert!(one.moves.is_empty() && one.sans.is_empty());
    let two = inputs_from_game(&game, &[ev(0.1), ev(0.1)], None);
    assert_eq!((two.moves.len(), two.sans.len()), (1, 1));
    // More evaluations than moves is harmless.
    let many: Vec<_> = (0..10).map(|_| ev(0.1)).collect();
    assert_eq!(inputs_from_game(&game, &many, None).moves.len(), 3);
}

#[test]
fn the_mover_alternates_and_a_game_from_a_black_to_move_fen_starts_with_black() {
    let position = Position::from_fen("4k3/8/8/8/8/8/p7/4K3 b - - 0 1").unwrap();
    let game = game_from(Game::from_position(position), &["a2a1q"]);
    let inputs = inputs_from_game(&game, &[ev(0.0), ev(-9.0)], None);
    assert_eq!(inputs.moves.len(), 1);
    assert_eq!(inputs.moves[0].mover, Color::Black);
    assert!(inputs.sans[0].contains("=Q"), "{}", inputs.sans[0]);
}

// ---- the conservative sacrifice (D1428, D1565) ----

/// 1.e4 d5 2.Bb5+ c6 3.Bxc6+ bxc6: White gives a bishop for a pawn.
fn bishop_sacrifice_game() -> Game {
    game_of(&["e2e4", "d7d5", "f1b5", "c7c6", "b5c6", "b7c6"])
}

#[test]
fn a_bishop_given_for_a_pawn_is_a_sacrifice_when_the_line_confirms_it() {
    let game = bishop_sacrifice_game();
    let nodes: Vec<_> = game.mainline().collect();
    // The position after 3.Bxc6+; the engine's line: ...bxc6 and 4.Nf3.
    let after = game.position_at(nodes[4]).unwrap().clone();
    let line = pv_of(&after, &["b7c6", "g1f3"]);
    let mut evals: Vec<_> = (0..7).map(|_| ev(0.3)).collect();
    evals[5] = ev_pv(0.3, line);
    let inputs = inputs_from_game(&game, &evals, None);
    let flags: Vec<bool> = inputs.moves.iter().map(|m| m.sacrifice).collect();
    assert_eq!(flags, [false, false, false, false, true, false]);
}

#[test]
fn a_line_that_is_too_short_never_makes_a_sacrifice() {
    let game = bishop_sacrifice_game();
    let nodes: Vec<_> = game.mainline().collect();
    let after = game.position_at(nodes[4]).unwrap().clone();
    let mut evals: Vec<_> = (0..7).map(|_| ev(0.3)).collect();
    evals[5] = ev_pv(0.3, pv_of(&after, &["b7c6"]));
    let inputs = inputs_from_game(&game, &evals, None);
    assert!(
        inputs.moves.iter().all(|m| !m.sacrifice),
        "one move proves nothing"
    );
    evals[5] = ev(0.3);
    let inputs = inputs_from_game(&game, &evals, None);
    assert!(inputs.moves.iter().all(|m| !m.sacrifice), "no line at all");
}

#[test]
fn a_reciprocal_capture_is_not_a_sacrifice() {
    // 1.e4 d5 2.exd5 Qxd5: a pawn for a pawn.
    let game = game_of(&["e2e4", "d7d5", "e4d5", "d8d5"]);
    let nodes: Vec<_> = game.mainline().collect();
    let after = game.position_at(nodes[2]).unwrap().clone();
    let mut evals: Vec<_> = (0..5).map(|_| ev(0.1)).collect();
    evals[3] = ev_pv(0.1, pv_of(&after, &["d8d5", "b1c3"]));
    let inputs = inputs_from_game(&game, &evals, None);
    assert!(inputs.moves.iter().all(|m| !m.sacrifice));
}

#[test]
fn a_single_pawn_given_up_is_below_the_two_point_threshold() {
    // 1.e4 e5 2.d4 exd4 3.Nf3: one pawn down after the reply and the move.
    let game = game_of(&["e2e4", "e7e5", "d2d4"]);
    let nodes: Vec<_> = game.mainline().collect();
    let after = game.position_at(nodes[2]).unwrap().clone();
    let mut evals: Vec<_> = (0..4).map(|_| ev(0.1)).collect();
    evals[3] = ev_pv(0.1, pv_of(&after, &["e5d4", "g1f3"]));
    let inputs = inputs_from_game(&game, &evals, None);
    assert!(!inputs.moves[2].sacrifice, "-1 is not enough");
}

#[test]
fn a_stale_or_illegal_line_gives_no_sacrifice_and_no_panic() {
    let game = bishop_sacrifice_game();
    let start = Position::new();
    // Legal from the start position, not after the move: the replay fails.
    let bogus = pv_of(&start, &["a2a3", "a7a6"]);
    let mut evals: Vec<_> = (0..7).map(|_| ev(0.3)).collect();
    for e in &mut evals {
        e.pv = bogus.clone();
    }
    let inputs = inputs_from_game(&game, &evals, None);
    assert_eq!(inputs.moves.len(), 6);
    assert!(inputs.moves.iter().all(|m| !m.sacrifice));
}

#[test]
fn a_flagged_sacrifice_that_keeps_the_position_is_brilliant_and_a_lost_one_is_not() {
    let game = bishop_sacrifice_game();
    let nodes: Vec<_> = game.mainline().collect();
    let after = game.position_at(nodes[4]).unwrap().clone();
    let line = pv_of(&after, &["b7c6", "g1f3"]);
    let before = game.position_at(nodes[3]).unwrap().clone();
    let best = pv_of(&before, &["b5c6"]);
    let mut evals: Vec<_> = (0..7).map(|_| ev(0.5)).collect();
    evals[4] = ev_pv(0.5, best.clone());
    evals[5] = ev_pv(0.5, line.clone());
    let inputs = inputs_from_game(&game, &evals, None);
    let report = build_report(&inputs.moves, &ReportThresholds::default());
    assert_eq!(report.moves[4].class, MoveClass::Brilliant);
    // The same sacrifice that leaves White lost (win probability < 40) is a
    // plain move of whatever class the loss says.
    evals[4] = ev_pv(-3.0, best);
    evals[5] = ev_pv(-3.0, line);
    let inputs = inputs_from_game(&game, &evals, None);
    let report = build_report(&inputs.moves, &ReportThresholds::default());
    assert_ne!(report.moves[4].class, MoveClass::Brilliant);
}

// ---- reading the evaluations of the game (D1572) ----

#[test]
fn the_standard_start_without_an_evaluation_counts_as_level() {
    let game = game_of(&["e2e4"]);
    let evals = evals_from_game(&game);
    assert_eq!(evals.len(), 1);
    assert_eq!(evals[0].score, 0.0);
    assert_eq!(evals[0].mate, None);
}

#[test]
fn another_start_without_an_evaluation_gives_no_report() {
    let position = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
    let mut game = game_from(Game::from_position(position), &["e2e4"]);
    let first = game.mainline().next().unwrap();
    game.set_comment(first, "[%eval +1.00]").unwrap();
    assert!(evals_from_game(&game).is_empty());
}

#[test]
fn another_start_with_an_evaluation_in_the_initial_comment_is_accepted() {
    let position = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
    let mut game = game_from(Game::from_position(position), &["e2e4"]);
    let root = game.root();
    game.set_comment(root, "[%eval +0.50]").unwrap();
    let first = game.mainline().next().unwrap();
    game.set_comment(first, "[%eval +0.75]").unwrap();
    let evals = evals_from_game(&game);
    assert_eq!(evals.len(), 2);
    assert_eq!((evals[0].score, evals[1].score), (0.5, 0.75));
}

#[test]
fn a_root_evaluation_wins_over_the_assumed_level_start() {
    let mut game = game_of(&["e2e4"]);
    let root = game.root();
    game.set_comment(root, "[%eval +0.30]").unwrap();
    let first = game.mainline().next().unwrap();
    game.set_comment(first, "[%eval +0.20]").unwrap();
    let evals = evals_from_game(&game);
    assert_eq!(evals[0].score, 0.3);
}

#[test]
fn evaluations_are_read_up_to_the_first_gap_only() {
    let mut game = game_of(&["e2e4", "e7e5", "g1f3", "b8c6"]);
    let nodes: Vec<_> = game.mainline().collect();
    game.set_comment(nodes[0], "[%eval +0.3]").unwrap();
    // nodes[1] has none: the later ones are not continuous.
    game.set_comment(nodes[2], "[%eval +0.4]").unwrap();
    game.set_comment(nodes[3], "[%eval +0.5]").unwrap();
    assert_eq!(evals_from_game(&game).len(), 2);
}

#[test]
fn an_evaluation_in_the_middle_of_text_and_a_mate_are_understood() {
    let mut game = game_of(&["e2e4", "e7e5", "g1f3"]);
    let nodes: Vec<_> = game.mainline().collect();
    game.set_comment(nodes[0], "good start [%clk 0:10:00] [%eval -0.25] more")
        .unwrap();
    game.set_comment(nodes[1], "[%eval #3]").unwrap();
    game.set_comment(nodes[2], "[%eval #-2]").unwrap();
    let evals = evals_from_game(&game);
    assert_eq!(evals.len(), 4);
    assert_eq!(evals[1].score, -0.25);
    assert_eq!(evals[1].mate, None);
    assert!(evals[2].mate.is_some_and(|m| m > 0), "{:?}", evals[2]);
    assert!(evals[2].score >= 300.0);
    assert!(evals[3].mate.is_some_and(|m| m < 0), "{:?}", evals[3]);
    assert!(evals[3].score <= -300.0);
}

#[test]
fn a_comment_that_is_a_game_result_is_not_an_evaluation() {
    let mut game = game_of(&["e2e4"]);
    let first = game.mainline().next().unwrap();
    game.set_comment(first, "1-0 resigns").unwrap();
    assert_eq!(evals_from_game(&game).len(), 1);
}

#[test]
fn the_report_rebuilt_from_the_game_has_no_best_move_no_great_and_no_brilliant() {
    let mut game = game_of(&["e2e4", "e7e5", "d1h5", "b8c6", "h5e5"]);
    let nodes: Vec<_> = game.mainline().collect();
    for (node, text) in nodes.iter().zip(["+0.3", "+0.3", "+0.2", "+0.3", "-6.0"]) {
        game.set_comment(*node, &format!("[%eval {text}]")).unwrap();
    }
    let evals = evals_from_game(&game);
    let inputs = inputs_from_game(&game, &evals, None);
    let report = build_report(&inputs.moves, &ReportThresholds::default());
    assert_eq!(report.moves.len(), 5);
    assert!(report.moves.iter().all(|m| m.best.is_none()));
    assert!(
        report
            .moves
            .iter()
            .all(|m| !matches!(m.class, MoveClass::Great | MoveClass::Brilliant))
    );
    assert!(report.moves[4].class.is_fault(), "{:?}", report.moves[4]);
}

// ---- writing the report (D1571) ----

/// 1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 with evaluations that give the classes
/// Inaccuracy, (Brilliant), Mistake, Miss, Blunder, (Great).
struct Fixture {
    game: Game,
    evals: Vec<PositionEval>,
    report: GameReport,
}

fn fixture() -> Fixture {
    let game = game_of(&["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6"]);
    let nodes: Vec<_> = game.mainline().collect();
    let position_before = |ply: usize| -> Position {
        let parent = if ply == 1 {
            game.root()
        } else {
            nodes[ply - 2]
        };
        game.position_at(parent).unwrap().clone()
    };
    let scores = [0.0, -1.0, -1.0, -3.0, 1.0, -3.0, -3.0];
    let mut evals: Vec<_> = scores.iter().map(|s| ev(*s)).collect();
    // Lines for the faults: something other than the move played.
    evals[0].pv = pv_of(&position_before(1), &["d2d4", "d7d5", "c2c4", "e7e6"]);
    evals[2].pv = pv_of(
        &position_before(3),
        &[
            "d2d4", "e5d4", "d1d4", "b8c6", "d4e3", "g8f6", "b1c3", "f8b4", "c1d2", "d7d6", "f1d3",
            "c8g4",
        ],
    );
    evals[3].pv = pv_of(&position_before(4), &["g8f6", "b1c3"]);
    evals[4].pv = pv_of(&position_before(5), &["d2d4"]);
    evals[5].pv = pv_of(&position_before(6), &["g8f6"]);
    let mut inputs = inputs_from_game(&game, &evals, None).moves;
    // Ply 2 is a sound sacrifice; ply 6 is the only move.
    inputs[1].sacrifice = true;
    inputs[5].before.second = Some(1.0);
    let report = build_report(&inputs, &ReportThresholds::default());
    Fixture {
        game,
        evals,
        report,
    }
}

fn nags_in(actions: &[AnnotateAction]) -> Vec<(mychess_core::NodeId, u8)> {
    actions
        .iter()
        .filter_map(|a| match a {
            AnnotateAction::Nag { node, nag } => Some((*node, nag.value())),
            _ => None,
        })
        .collect()
}

#[test]
fn the_fixture_produces_every_class_the_writing_tests_rely_on() {
    let f = fixture();
    let classes: Vec<_> = f.report.moves.iter().map(|m| m.class).collect();
    assert_eq!(
        classes,
        [
            MoveClass::Inaccuracy,
            MoveClass::Brilliant,
            MoveClass::Mistake,
            MoveClass::Miss,
            MoveClass::Blunder,
            MoveClass::Great,
        ]
    );
}

#[test]
fn each_class_writes_its_nag_and_the_best_and_quiet_classes_write_none() {
    let f = fixture();
    let nodes: Vec<_> = f.game.mainline().collect();
    let actions = write_actions(&f.game, &f.report, &f.evals);
    let nags = nags_in(&actions);
    let expected = [
        (nodes[0], 6), // inaccuracy: ?!
        (nodes[1], 3), // brilliant: !!
        (nodes[2], 2), // mistake: ?
        (nodes[3], 2), // miss: ?
        (nodes[4], 4), // blunder: ??
        (nodes[5], 1), // great: !
    ];
    assert_eq!(nags, expected);
}

#[test]
fn the_best_move_gets_no_exclamation_mark() {
    // D1571: `Best` is not marked, `Excellent` and `Good` neither.
    let game = game_of(&["e2e4", "e7e5", "g1f3", "b8c6"]);
    let evals = vec![ev(0.2), ev(0.2), ev(0.2), ev(0.2), ev(0.2)];
    let inputs = inputs_from_game(&game, &evals, None);
    let report = build_report(&inputs.moves, &ReportThresholds::default());
    assert!(report.moves.iter().all(|m| m.class == MoveClass::Best));
    let actions = write_actions(&game, &report, &evals);
    assert!(nags_in(&actions).is_empty());
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a, AnnotateAction::Score { .. }))
            .count(),
        4
    );
}

#[test]
fn only_faults_with_a_different_best_move_get_a_variation_of_at_most_eight_moves() {
    let f = fixture();
    let nodes: Vec<_> = f.game.mainline().collect();
    let actions = write_actions(&f.game, &f.report, &f.evals);
    let variations: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            AnnotateAction::Variation { parent, pv, .. } => Some((*parent, pv.clone())),
            _ => None,
        })
        .collect();
    // Faults: plies 1 (inaccuracy), 3 (mistake), 4 (miss), 5 (blunder).
    let parents: Vec<_> = variations.iter().map(|(p, _)| *p).collect();
    assert_eq!(
        parents,
        [f.game.root(), nodes[1], nodes[2], nodes[3]],
        "the line starts where the fault was played"
    );
    assert!(variations.iter().all(|(_, pv)| pv.len() <= 8));
    assert_eq!(variations[1].1.len(), 8, "twelve moves are cut to eight");
    assert_eq!(variations[1].1[..], f.evals[2].pv[..8]);
    assert_eq!(variations[0].1.len(), 4, "short lines are kept whole");
}

#[test]
fn a_fault_where_the_best_move_was_played_gets_no_variation() {
    let mut f = fixture();
    let played = f.report.moves[0].played;
    f.evals[0].pv = vec![played];
    let actions = write_actions(&f.game, &f.report, &f.evals);
    let first = actions.iter().filter(
        |a| matches!(a, AnnotateAction::Variation { parent, .. } if *parent == f.game.root()),
    );
    assert_eq!(first.count(), 0);
}

#[test]
fn a_fault_without_a_line_gets_no_variation() {
    let mut f = fixture();
    for e in &mut f.evals {
        e.pv.clear();
    }
    let actions = write_actions(&f.game, &f.report, &f.evals);
    assert!(
        actions
            .iter()
            .all(|a| !matches!(a, AnnotateAction::Variation { .. }))
    );
}

#[test]
fn an_eval_already_in_the_move_is_not_written_again_but_the_nag_still_is() {
    let mut f = fixture();
    let nodes: Vec<_> = f.game.mainline().collect();
    f.game
        .set_comment(nodes[4], "my note [%eval -3.00]")
        .unwrap();
    let actions = write_actions(&f.game, &f.report, &f.evals);
    let scored: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            AnnotateAction::Score { node, .. } => Some(*node),
            _ => None,
        })
        .collect();
    assert_eq!(scored.len(), 5);
    assert!(!scored.contains(&nodes[4]));
    assert!(nags_in(&actions).contains(&(nodes[4], 4)));
}

#[test]
fn writing_twice_changes_nothing_the_second_time() {
    let mut f = fixture();
    let first = write_actions(&f.game, &f.report, &f.evals);
    apply_actions(&mut f.game, &first).unwrap();
    let after_first = f.game.clone();
    let again = write_actions(&f.game, &f.report, &f.evals);
    assert!(
        again
            .iter()
            .all(|a| !matches!(a, AnnotateAction::Score { .. })),
        "every move already has its [%eval]"
    );
    apply_actions(&mut f.game, &again).unwrap();
    assert!(
        f.game.same_content(&after_first),
        "NAGs and lines are not duplicated"
    );
}

#[test]
fn the_comment_written_is_the_eval_command_and_survives_reading() {
    let mut f = fixture();
    let actions = write_actions(&f.game, &f.report, &f.evals);
    apply_actions(&mut f.game, &actions).unwrap();
    let read = evals_from_game(&f.game);
    assert_eq!(read.len(), 7);
    let scores: Vec<f32> = read.iter().map(|e| e.score).collect();
    assert_eq!(scores, [0.0, -1.0, -1.0, -3.0, 1.0, -3.0, -3.0]);
}

#[test]
fn a_report_that_does_not_match_the_game_writes_nothing() {
    let f = fixture();
    let other = game_of(&["d2d4", "d7d5", "c2c4", "e7e6", "b1c3", "g8f6"]);
    assert!(write_actions(&other, &f.report, &f.evals).is_empty());
    // A shorter game matches as far as it goes and writes only that.
    let shorter = game_of(&["e2e4", "e7e5"]);
    let actions = write_actions(&shorter, &f.report, &f.evals);
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a, AnnotateAction::Score { .. }))
            .count(),
        2
    );
}

#[test]
fn a_partial_report_writes_only_the_moves_it_judged() {
    let f = fixture();
    let inputs = inputs_from_game(&f.game, &f.evals[..4], None);
    let partial = build_report(&inputs.moves, &ReportThresholds::default());
    assert_eq!(partial.moves.len(), 3);
    let actions = write_actions(&f.game, &partial, &f.evals[..4]);
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a, AnnotateAction::Score { .. }))
            .count(),
        3
    );
}

proptest! {
    /// D1573: writing and reading again gives the same class for every
    /// move that does not depend on the engine's line.
    #[test]
    fn the_classes_survive_the_trip_through_the_comments(
        cps in proptest::collection::vec(-1500i32..1500, 1..24),
    ) {
        let ucis = ["g1f3", "g8f6", "f3g1", "f6g8"];
        let mut game = Game::new();
        let mut node = game.root();
        let mut position = Position::new();
        for i in 0..cps.len() {
            let mv = position.parse_uci(ucis[i % 4]).unwrap();
            position.play(mv).unwrap();
            node = game.add_move(node, mv).unwrap();
        }
        let mut evals = vec![ev(0.0)];
        evals.extend(cps.iter().map(|cp| ev(*cp as f32 / 100.0)));
        let inputs = inputs_from_game(&game, &evals, None);
        let report = build_report(&inputs.moves, &ReportThresholds::default());
        let actions = write_actions(&game, &report, &evals);
        apply_actions(&mut game, &actions).unwrap();
        let read = evals_from_game(&game);
        prop_assert_eq!(read.len(), evals.len());
        let again = inputs_from_game(&game, &read, None);
        let rebuilt = build_report(&again.moves, &ReportThresholds::default());
        let a: Vec<_> = report.moves.iter().map(|m| m.class).collect();
        let b: Vec<_> = rebuilt.moves.iter().map(|m| m.class).collect();
        prop_assert_eq!(a, b);
        prop_assert_eq!(report.white.accuracy, rebuilt.white.accuracy);
        let _ = node;
    }

    /// Whatever the evaluations, the inputs never panic and stay aligned.
    #[test]
    fn inputs_never_panic_and_stay_aligned_with_the_moves(
        plies in 0usize..12,
        evals in 0usize..16,
        score in -400.0f32..400.0,
    ) {
        let ucis = ["g1f3", "g8f6", "f3g1", "f6g8"];
        let game = game_of(&(0..plies).map(|i| ucis[i % 4]).collect::<Vec<_>>());
        let list: Vec<_> = (0..evals).map(|_| ev(score)).collect();
        let inputs = inputs_from_game(&game, &list, None);
        prop_assert_eq!(inputs.moves.len(), plies.min(evals.saturating_sub(1)));
        prop_assert_eq!(inputs.sans.len(), inputs.moves.len());
    }
}

// ---- the state machine of the review (D1435, D1569) ----

fn report_of(classes: &[MoveClass]) -> GameReport {
    // Real inputs with just the right losses; the classes come out of the
    // scale, so only unambiguous ones are used.
    let ucis = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let game = game_of(&(0..classes.len()).map(|i| ucis[i % 4]).collect::<Vec<_>>());
    let mut evals = vec![ev(0.0)];
    let mut white = 0.0f32; // White's point of view
    for (i, class) in classes.iter().enumerate() {
        let loss_pawns = match class {
            MoveClass::Blunder => 4.0,
            MoveClass::Mistake => 2.0,
            MoveClass::Inaccuracy => 1.0,
            _ => 0.0,
        };
        white += if i % 2 == 0 { -loss_pawns } else { loss_pawns };
        evals.push(ev(white));
    }
    let inputs = inputs_from_game(&game, &evals, None);
    build_report(&inputs.moves, &ReportThresholds::default())
}

#[test]
fn a_review_visits_only_the_moments_of_the_chosen_side() {
    let report = report_of(&[
        MoveClass::Blunder,
        MoveClass::Best,
        MoveClass::Best,
        MoveClass::Best,
        MoveClass::Best,
        MoveClass::Best,
    ]);
    let white = Review::new(&report, SideFilter::White, false).unwrap();
    assert_eq!(white.moments.iter().map(|m| m.ply).collect::<Vec<_>>(), [1]);
    // Nothing for Black: no review at all.
    assert!(Review::new(&report, SideFilter::Black, false).is_none());
    let both = Review::new(&report, SideFilter::Both, false).unwrap();
    assert_eq!(both.index, 0);
    assert_eq!(both.step, ReviewStep::Looking);
    assert_eq!(both.attempts, 0);
}

#[test]
fn a_report_without_faults_has_no_key_moments_even_with_highlights() {
    let report = report_of(&[MoveClass::Best; 4]);
    assert!(key_moments(&report, SideFilter::Both, true).is_empty());
    assert!(Review::new(&report, SideFilter::Both, true).is_none());
}

#[test]
fn only_the_engines_move_resolves_the_attempt_and_the_others_count() {
    let start = Position::new();
    let best = pv_of(&start, &["d2d4"])[0];
    let other = pv_of(&start, &["g1f3"])[0];
    let mut review =
        Review::new(&report_of(&[MoveClass::Blunder]), SideFilter::Both, false).unwrap();
    // Not yet trying.
    assert_eq!(review.judge(best, Some(best)), None);
    review.start_trying();
    assert_eq!(review.judge(other, Some(best)), Some(Attempt::Wrong));
    assert_eq!(review.attempts, 1);
    assert!(!review.hint_due(), "one miss is not enough");
    assert_eq!(review.judge(other, Some(best)), Some(Attempt::Wrong));
    assert_eq!(HINT_AFTER, 2);
    assert!(review.hint_due(), "after two misses");
    assert_eq!(review.judge(best, Some(best)), Some(Attempt::Correct));
    assert_eq!(review.step, ReviewStep::Solved);
    assert!(!review.hint_due(), "solved: no hint");
    // Solved: further attempts are not judged.
    assert_eq!(review.judge(other, Some(best)), None);
}

#[test]
fn trying_again_starts_the_attempt_count_from_zero_and_next_resets_the_moment() {
    let report = report_of(&[MoveClass::Blunder, MoveClass::Best, MoveClass::Mistake]);
    let mut review = Review::new(&report, SideFilter::Both, false).unwrap();
    let start = Position::new();
    let best = pv_of(&start, &["d2d4"])[0];
    let other = pv_of(&start, &["g1f3"])[0];
    review.start_trying();
    review.judge(other, Some(best));
    review.judge(other, Some(best));
    assert!(review.hint_due());
    review.start_trying();
    assert_eq!(review.attempts, 0);
    assert!(!review.hint_due());
    review.reveal();
    assert_eq!(review.step, ReviewStep::Revealed);
    assert_eq!(review.judge(best, Some(best)), None, "revealed: no judging");
    assert!(review.advance());
    assert_eq!((review.step, review.attempts), (ReviewStep::Looking, 0));
    assert_eq!(review.current().map(|m| m.ply), Some(3));
    assert!(!review.advance(), "the last moment");
    assert_eq!(review.current().map(|m| m.ply), Some(3));
    assert!(review.previous());
    assert!(!review.previous(), "the first moment");
    assert_eq!(review.index, 0);
}

#[test]
fn highlights_are_visited_only_when_asked() {
    let f = fixture();
    let without = key_moments(&f.report, SideFilter::Both, false);
    let with = key_moments(&f.report, SideFilter::Both, true);
    let plies = |m: &[mychess_app::report::Moment]| m.iter().map(|m| m.ply).collect::<Vec<_>>();
    assert_eq!(plies(&without), [1, 3, 4, 5]);
    assert_eq!(plies(&with), [1, 2, 3, 4, 5, 6]);
    let white = key_moments(&f.report, SideFilter::White, false);
    assert_eq!(plies(&white), [1, 3, 5]);
    let black = key_moments(&f.report, SideFilter::Black, true);
    assert_eq!(plies(&black), [2, 4, 6]);
}

#[test]
fn the_mover_of_each_judged_input_is_kept() {
    let f = fixture();
    let movers: Vec<_> = f.report.moves.iter().map(|m| m.mover).collect();
    assert_eq!(
        movers,
        [
            Color::White,
            Color::Black,
            Color::White,
            Color::Black,
            Color::White,
            Color::Black
        ]
    );
    // And the total of the table adds up to the moves of each side.
    assert_eq!(f.report.white.total(), 3);
    assert_eq!(f.report.black.total(), 3);
}

#[test]
fn move_input_fields_are_the_documented_ones() {
    // Compile-time contract of the public type used by the callers (D1530).
    let f = fixture();
    let inputs = inputs_from_game(&f.game, &f.evals, None);
    let first: &MoveInput = &inputs.moves[0];
    assert_eq!(first.mover, Color::White);
    assert!(!first.book && !first.sacrifice);
}
