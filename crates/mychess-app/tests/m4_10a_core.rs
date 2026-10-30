//! M4-10a contract tests of the report core (spec
//! `m4-10-relatorio-da-partida.md`, D1423-D1432, D1530-D1533): win
//! probability, accuracy, classes by the loss scale, the special classes,
//! per-side totals and the key moments. Only plain numbers: no engine.
#![allow(
    clippy::unwrap_used,
    clippy::assert_is_empty,
    clippy::nonminimal_bool,
    clippy::redundant_closure_for_method_calls,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::too_many_lines
)]

use mychess_app::PositionEval;
use mychess_app::report::{
    MoveClass, MoveInput, ReportThresholds, SideFilter, accuracy_of_loss, build_report,
    key_moments, win_percent,
};
use mychess_core::{Color, Move, Position};
use proptest::prelude::*;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.3
}

/// A move legal from the start position, only needed as an identity.
fn mv(uci: &str) -> Move {
    let mut pos = Position::new();
    if let Ok(m) = pos.parse_uci(uci) {
        return m;
    }
    let first = pos.parse_uci("e2e4").expect("uci");
    pos.play(first).expect("play");
    pos.parse_uci(uci).expect("uci")
}

fn ev(pawns: f32) -> PositionEval {
    PositionEval {
        score: pawns,
        mate: None,
        pv: Vec::new(),
        depth: 16,
        second: None,
    }
}

fn mate(n: i32) -> PositionEval {
    PositionEval {
        score: if n > 0 { 327.0 } else { -327.0 },
        mate: Some(n),
        ..ev(0.0)
    }
}

fn input(mover: Color, before: PositionEval, after: PositionEval) -> MoveInput {
    MoveInput {
        mover,
        before,
        after,
        played: mv("e2e4"),
        book: false,
        sacrifice: false,
    }
}

/// One white move from an equal position to `after` pawns.
fn white_to(after: f32) -> MoveInput {
    input(Color::White, ev(0.0), ev(after))
}

fn class_of_single(i: MoveInput) -> MoveClass {
    build_report(&[i], &ReportThresholds::default()).moves[0].class
}

// ---- win_percent (D1423) ----

#[test]
fn win_percent_follows_the_published_formula() {
    assert!(close(win_percent(0.0), 50.0));
    for cp in [-1500.0f32, -300.0, -50.0, 20.0, 100.0, 400.0, 2000.0] {
        let want = 50.0 + 50.0 * (2.0 / (1.0 + (-0.003_682_08f32 * cp).exp()) - 1.0);
        assert!(close(win_percent(cp), want), "{cp}");
    }
    // 100 cp is about 59% (Lichess documents roughly this value).
    assert!(close(win_percent(100.0), 59.1), "{}", win_percent(100.0));
}

#[test]
fn win_percent_is_symmetric_monotone_and_bounded() {
    let mut last = -1.0;
    for cp in (-3000..=3000).step_by(50) {
        let cp = cp as f32;
        let w = win_percent(cp);
        assert!((0.0..=100.0).contains(&w), "{cp}: {w}");
        assert!(close(w + win_percent(-cp), 100.0), "{cp}");
        assert!(w >= last, "not monotone at {cp}");
        last = w;
    }
}

#[test]
fn win_percent_survives_hostile_numbers() {
    for cp in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e30, -1e30] {
        let w = win_percent(cp);
        assert!(w.is_finite() && (0.0..=100.0).contains(&w), "{cp}: {w}");
    }
}

// ---- accuracy_of_loss (D1430) ----

#[test]
fn accuracy_follows_the_published_formula() {
    assert!(accuracy_of_loss(0.0) > 99.9);
    for loss in [1.0f32, 5.0, 10.0, 20.0, 40.0] {
        let want = (103.1668 * (-0.04354 * loss).exp() - 3.1669).clamp(0.0, 100.0);
        assert!(close(accuracy_of_loss(loss), want), "{loss}");
    }
    assert_eq!(accuracy_of_loss(100.0), 0.0, "clamped at zero");
}

#[test]
fn accuracy_survives_hostile_numbers() {
    for loss in [f32::NAN, f32::INFINITY, -5.0, 1e30] {
        let a = accuracy_of_loss(loss);
        assert!(a.is_finite() && (0.0..=100.0).contains(&a), "{loss}: {a}");
    }
}

proptest! {
    #[test]
    fn accuracy_never_improves_with_a_bigger_loss(a in 0.0f32..100.0, b in 0.0f32..100.0) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(accuracy_of_loss(lo) >= accuracy_of_loss(hi));
    }
}

// ---- the loss scale (D1425, D1531) ----

#[test]
fn the_loss_scale_picks_the_class() {
    // (white's score after the move from 0.00, expected loss, class)
    let table = [
        (0.0f32, 0.0f32, MoveClass::Best),
        (-0.1, 0.9, MoveClass::Excellent),
        (-0.4, 3.7, MoveClass::Good),
        (-0.8, 7.3, MoveClass::Inaccuracy),
        (-1.5, 13.5, MoveClass::Mistake),
        (-3.0, 25.0, MoveClass::Blunder),
    ];
    for (after, loss, class) in table {
        let report = build_report(&[white_to(after)], &ReportThresholds::default());
        let m = &report.moves[0];
        assert!(close(m.loss, loss), "after {after}: loss {}", m.loss);
        assert_eq!(m.class, class, "after {after}");
        assert_eq!(m.ply, 1);
        assert_eq!(m.mover, Color::White);
        assert!(close(m.accuracy, accuracy_of_loss(m.loss)));
    }
}

#[test]
fn the_same_scale_applies_to_black_with_the_sign_reversed() {
    for (after, class) in [
        (0.0f32, MoveClass::Best),
        (0.4, MoveClass::Good),
        (0.8, MoveClass::Inaccuracy),
        (1.5, MoveClass::Mistake),
        (3.0, MoveClass::Blunder),
    ] {
        let i = input(Color::Black, ev(0.0), ev(after));
        assert_eq!(class_of_single(i), class, "after {after}");
    }
}

#[test]
fn a_move_that_improves_the_position_has_no_loss() {
    let report = build_report(
        &[white_to(2.0), input(Color::Black, ev(0.0), ev(-3.0))],
        &ReportThresholds::default(),
    );
    assert_eq!(report.moves[0].loss, 0.0);
    assert_eq!(report.moves[0].class, MoveClass::Best);
    assert_eq!(report.moves[1].loss, 0.0);
}

#[test]
fn the_engine_move_is_best_whatever_the_next_search_says() {
    let mut engine_move = input(Color::White, ev(0.0), ev(-0.1));
    engine_move.before.pv = vec![mv("e2e4")];
    assert_eq!(class_of_single(engine_move), MoveClass::Best);
    assert_eq!(
        class_of_single(white_to(-0.1)),
        MoveClass::Excellent,
        "not the engine move: only the Excellent tier"
    );
    let mut bad = input(Color::White, ev(0.0), ev(-0.4));
    bad.before.pv = vec![mv("e2e4")];
    assert_eq!(
        class_of_single(bad),
        MoveClass::Best,
        "the engine's own move: the loss is search noise (D1580)"
    );
    // The best move reported is the first of the pv.
    let mut with_pv = white_to(-1.5);
    with_pv.before.pv = vec![mv("d2d4"), mv("d7d5")];
    let report = build_report(&[with_pv], &ReportThresholds::default());
    assert_eq!(report.moves[0].best, Some(mv("d2d4")));
    assert_eq!(report.moves[0].played, mv("e2e4"));
}

#[test]
fn custom_thresholds_move_the_boundaries() {
    let tight = ReportThresholds {
        good: 1.0,
        ..ReportThresholds::default()
    };
    let report = build_report(&[white_to(-0.4)], &tight);
    assert_eq!(report.moves[0].class, MoveClass::Inaccuracy);
    let defaults = ReportThresholds::default();
    assert_eq!(
        (
            defaults.best,
            defaults.excellent,
            defaults.good,
            defaults.inaccuracy,
            defaults.mistake
        ),
        (0.5, 2.0, 5.0, 10.0, 20.0)
    );
    assert_eq!(
        (
            defaults.great_gap,
            defaults.miss_gain,
            defaults.brilliant_floor
        ),
        (15.0, 15.0, 40.0)
    );
}

proptest! {
    #[test]
    fn a_bigger_loss_never_gives_a_better_class_or_accuracy(a in 0.0f32..12.0, b in 0.0f32..12.0) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let t = ReportThresholds::default();
        let r_lo = build_report(&[white_to(-lo)], &t);
        let r_hi = build_report(&[white_to(-hi)], &t);
        prop_assert!(r_lo.moves[0].loss <= r_hi.moves[0].loss + 1e-4);
        prop_assert!(r_lo.moves[0].class <= r_hi.moves[0].class,
            "{:?} vs {:?}", r_lo.moves[0].class, r_hi.moves[0].class);
        prop_assert!(r_lo.moves[0].accuracy >= r_hi.moves[0].accuracy);
    }
}

// ---- mates and decided positions (D1531) ----

#[test]
fn mate_counts_as_a_certain_result_without_dividing_by_zero() {
    // White had mate in 3 and plays a move that allows mate against it.
    let report = build_report(
        &[input(Color::White, mate(3), mate(-2))],
        &ReportThresholds::default(),
    );
    let m = &report.moves[0];
    assert_eq!(m.loss, 100.0);
    assert_eq!(m.class, MoveClass::Blunder);
    assert_eq!(m.accuracy, 0.0);
    // Keeping the mate is no loss, even if it got longer.
    let kept = input(Color::White, mate(2), mate(3));
    let m = &build_report(&[kept], &ReportThresholds::default()).moves[0];
    assert_eq!(m.loss, 0.0);
    assert_eq!(m.class, MoveClass::Best);
    // Black mated: the sign of the mate is from White's point of view.
    let black = input(Color::Black, mate(-1), mate(2));
    assert_eq!(class_of_single(black), MoveClass::Blunder);
}

#[test]
fn a_decided_position_does_not_inflate_small_losses() {
    // +9.0 to +8.0 is a tiny change in win probability.
    let i = input(Color::White, ev(9.0), ev(8.0));
    let report = build_report(&[i], &ReportThresholds::default());
    assert!(report.moves[0].loss < 3.0, "{}", report.moves[0].loss);
}

// ---- special classes ----

#[test]
fn book_moves_win_over_everything_and_stay_out_of_the_accuracy() {
    let mut i = white_to(-9.0);
    i.book = true;
    let report = build_report(&[i], &ReportThresholds::default());
    assert_eq!(report.moves[0].class, MoveClass::Book);
    assert_eq!(report.white.count(MoveClass::Book), 1);
    assert_eq!(report.white.accuracy, None, "only book moves: no accuracy");
}

#[test]
fn a_book_blunder_does_not_lower_the_accuracy() {
    let mut book = white_to(-9.0);
    book.book = true;
    let clean = build_report(
        &[input(Color::White, ev(0.0), ev(0.0))],
        &ReportThresholds::default(),
    );
    let with_book = build_report(
        &[
            book,
            input(Color::Black, ev(-9.0), ev(-9.0)),
            input(Color::White, ev(-9.0), ev(-9.0)),
        ],
        &ReportThresholds::default(),
    );
    assert_eq!(
        with_book.white.accuracy.map(|a| a.round()),
        clean.white.accuracy.map(|a| a.round())
    );
}

#[test]
fn the_only_move_is_great_when_the_second_line_is_far_behind() {
    let mut i = input(Color::White, ev(0.5), ev(0.5));
    i.before.pv = vec![mv("e2e4")];
    i.before.second = Some(-1.5);
    assert_eq!(class_of_single(i.clone()), MoveClass::Great);
    // A close second line: just Best.
    i.before.second = Some(0.3);
    assert_eq!(class_of_single(i.clone()), MoveClass::Best);
    // Without a second line (no MultiPV): Best.
    i.before.second = None;
    assert_eq!(class_of_single(i), MoveClass::Best);
}

#[test]
fn the_only_move_is_not_great_in_an_already_decided_position() {
    let mut i = input(Color::White, ev(12.0), ev(12.0));
    i.before.pv = vec![mv("e2e4")];
    i.before.second = Some(0.0);
    assert_eq!(class_of_single(i), MoveClass::Best);
    let mut lost = input(Color::White, ev(-12.0), ev(-12.0));
    lost.before.pv = vec![mv("e2e4")];
    lost.before.second = Some(-20.0);
    assert_eq!(class_of_single(lost), MoveClass::Best);
}

#[test]
fn the_only_move_works_for_black_too() {
    let mut i = input(Color::Black, ev(-0.5), ev(-0.5));
    i.before.pv = vec![mv("e7e5")];
    // The second line (White's point of view) is much better for White.
    i.before.second = Some(1.5);
    assert_eq!(class_of_single(i), MoveClass::Great);
}

#[test]
fn a_sound_sacrifice_is_brilliant_only_when_the_position_stays_good() {
    let mut sound = input(Color::White, ev(0.5), ev(0.5));
    sound.before.pv = vec![mv("e2e4")];
    sound.sacrifice = true;
    assert_eq!(class_of_single(sound), MoveClass::Brilliant);
    // The sacrifice leaves the side lost (win < 40): not brilliant.
    let mut lost = input(Color::White, ev(-2.0), ev(-2.0));
    lost.before.pv = vec![mv("e2e4")];
    lost.sacrifice = true;
    assert_eq!(class_of_single(lost), MoveClass::Best);
    // A sacrifice that is also a loss is not brilliant.
    let mut bad = input(Color::White, ev(0.0), ev(-0.8));
    bad.sacrifice = true;
    assert_eq!(class_of_single(bad), MoveClass::Inaccuracy);
    let mut good = input(Color::White, ev(0.0), ev(-0.4));
    good.sacrifice = true;
    assert_eq!(class_of_single(good), MoveClass::Good);
    // Excellent (small loss) with a sacrifice still qualifies.
    let mut near = input(Color::White, ev(0.0), ev(-0.1));
    near.sacrifice = true;
    assert_eq!(class_of_single(near), MoveClass::Brilliant);
}

#[test]
fn a_wasted_chance_after_an_opponent_blunder_is_a_miss() {
    let t = ReportThresholds::default();
    // White blunders (0 -> -3.0); Black answers with an inaccuracy.
    let report = build_report(
        &[white_to(-3.0), input(Color::Black, ev(-3.0), ev(-2.2))],
        &t,
    );
    assert_eq!(report.moves[0].class, MoveClass::Blunder);
    assert_eq!(report.moves[1].class, MoveClass::Miss);
    assert!(report.moves[1].loss > 5.0);
    // The Miss replaces even a Blunder on the scale.
    let worse = build_report(
        &[white_to(-3.0), input(Color::Black, ev(-3.0), ev(0.5))],
        &t,
    );
    assert_eq!(worse.moves[1].class, MoveClass::Miss);
    // A small loss after the opponent's blunder is not a Miss.
    let fine = build_report(
        &[white_to(-3.0), input(Color::Black, ev(-3.0), ev(-2.9))],
        &t,
    );
    assert_ne!(fine.moves[1].class, MoveClass::Miss);
    assert_eq!(report.black.count(MoveClass::Miss), 1);
}

#[test]
fn no_miss_when_the_opponent_gave_less_than_fifteen_points() {
    let t = ReportThresholds::default();
    // White mistake of about 12 points, Black wastes a bit.
    let report = build_report(
        &[white_to(-1.33), input(Color::Black, ev(-1.33), ev(-0.5))],
        &t,
    );
    assert_eq!(report.moves[0].class, MoveClass::Mistake);
    assert_eq!(report.moves[1].class, MoveClass::Inaccuracy);
    // The opponent's move was only an inaccuracy.
    let report = build_report(
        &[white_to(-0.8), input(Color::Black, ev(-0.8), ev(0.5))],
        &t,
    );
    assert_ne!(report.moves[1].class, MoveClass::Miss);
}

#[test]
fn a_miss_needs_the_blunder_right_before() {
    let t = ReportThresholds::default();
    let report = build_report(
        &[
            white_to(-3.0),
            input(Color::Black, ev(-3.0), ev(-3.0)),
            input(Color::White, ev(-3.0), ev(-3.0)),
            input(Color::Black, ev(-3.0), ev(-2.2)),
        ],
        &t,
    );
    assert_eq!(report.moves[3].class, MoveClass::Inaccuracy);
}

// ---- totals and accuracy per side (D1530-D1532) ----

#[test]
fn an_empty_game_gives_an_empty_report() {
    let report = build_report(&[], &ReportThresholds::default());
    assert!(report.moves.is_empty());
    assert_eq!(report.white.accuracy, None);
    assert_eq!(report.black.accuracy, None);
    assert_eq!(report.white.total(), 0);
    assert_eq!(report.black.total(), 0);
    assert!(key_moments(&report, SideFilter::Both, true).is_empty());
}

#[test]
fn a_one_move_game_scores_only_white() {
    let report = build_report(&[white_to(0.0)], &ReportThresholds::default());
    assert_eq!(report.moves.len(), 1);
    assert!(report.white.accuracy.is_some());
    assert_eq!(report.black.accuracy, None);
    assert_eq!(report.white.total(), 1);
    assert_eq!(report.black.total(), 0);
    assert_eq!(report.side(Color::White), &report.white);
    assert_eq!(report.side(Color::Black), &report.black);
}

fn game(white: f32, black: f32, plies: usize) -> Vec<MoveInput> {
    // White's moves lose `white` pawns, Black's `black` pawns, cumulatively
    // from an equal start (the scores stay inside +-6).
    let mut score = 0.0f32;
    (0..plies)
        .map(|i| {
            let before = score;
            let (mover, delta) = if i % 2 == 0 {
                (Color::White, -white)
            } else {
                (Color::Black, black)
            };
            score = (score + delta).clamp(-6.0, 6.0);
            input(mover, ev(before), ev(score))
        })
        .collect()
}

#[test]
fn the_counts_of_each_side_add_up_to_its_moves() {
    let report = build_report(&game(0.5, 0.1, 21), &ReportThresholds::default());
    assert_eq!(report.moves.len(), 21);
    assert_eq!(report.white.total(), 11);
    assert_eq!(report.black.total(), 10);
    for (i, m) in report.moves.iter().enumerate() {
        assert_eq!(m.ply, i + 1);
        assert_eq!(
            m.mover,
            if i % 2 == 0 {
                Color::White
            } else {
                Color::Black
            }
        );
    }
}

#[test]
fn the_cleaner_side_has_the_higher_accuracy_and_both_are_bounded() {
    let report = build_report(&game(0.6, 0.0, 30), &ReportThresholds::default());
    let (w, b) = (
        report.white.accuracy.unwrap(),
        report.black.accuracy.unwrap(),
    );
    assert!(b > w, "black {b} vs white {w}");
    assert!((0.0..=100.0).contains(&w) && (0.0..=100.0).contains(&b));
    assert!(b > 95.0, "a flawless side is near 100: {b}");
}

#[test]
fn a_perfect_game_is_near_one_hundred_for_both() {
    let report = build_report(&game(0.0, 0.0, 40), &ReportThresholds::default());
    assert!(report.white.accuracy.unwrap() > 99.0);
    assert!(report.black.accuracy.unwrap() > 99.0);
}

#[test]
fn a_game_of_blunders_has_a_low_accuracy() {
    let inputs: Vec<MoveInput> = (0..20)
        .map(|i| {
            let (mover, before, after) = if i % 2 == 0 {
                (Color::White, 5.0, -5.0)
            } else {
                (Color::Black, -5.0, 5.0)
            };
            input(mover, ev(before), ev(after))
        })
        .collect();
    let report = build_report(&inputs, &ReportThresholds::default());
    assert!(report.white.accuracy.unwrap() < 20.0);
    assert!(report.white.count(MoveClass::Blunder) + report.white.count(MoveClass::Miss) >= 9);
}

#[test]
fn the_accuracy_is_stable_for_the_same_analysis() {
    let inputs = game(0.7, 0.3, 33);
    let t = ReportThresholds::default();
    let a = build_report(&inputs, &t);
    let b = build_report(&inputs, &t);
    assert_eq!(a.white.accuracy, b.white.accuracy);
    assert_eq!(a.black.accuracy, b.black.accuracy);
    assert_eq!(a.moves.len(), b.moves.len());
}

#[test]
fn non_finite_evaluations_do_not_poison_the_report() {
    let bad = |x: f32| PositionEval {
        score: x,
        ..ev(0.0)
    };
    let inputs = [
        input(Color::White, bad(f32::NAN), bad(f32::INFINITY)),
        input(Color::Black, bad(f32::NEG_INFINITY), bad(f32::NAN)),
        input(Color::White, bad(1e30), bad(-1e30)),
    ];
    let report = build_report(&inputs, &ReportThresholds::default());
    for m in &report.moves {
        assert!(m.loss.is_finite() && (0.0..=100.0).contains(&m.loss));
        assert!(m.accuracy.is_finite() && (0.0..=100.0).contains(&m.accuracy));
    }
    for side in [&report.white, &report.black] {
        assert!(
            side.accuracy
                .is_none_or(|a| a.is_finite() && (0.0..=100.0).contains(&a))
        );
    }
}

proptest! {
    #[test]
    fn any_game_gives_bounded_numbers_and_consistent_counts(
        steps in proptest::collection::vec((-15.0f32..15.0, any::<bool>(), any::<bool>(), proptest::option::of(-3i32..4)), 0..60),
    ) {
        let mut score = 0.0f32;
        let inputs: Vec<MoveInput> = steps
            .iter()
            .enumerate()
            .map(|(i, (next, book, sacrifice, m))| {
                let before = ev(score);
                let mut after = ev(*next);
                after.mate = m.filter(|n| *n != 0);
                score = *next;
                MoveInput {
                    mover: if i % 2 == 0 { Color::White } else { Color::Black },
                    before: PositionEval { mate: if i == 0 { None } else { before.mate }, ..before },
                    after,
                    played: mv("e2e4"),
                    book: *book,
                    sacrifice: *sacrifice,
                }
            })
            .collect();
        let report = build_report(&inputs, &ReportThresholds::default());
        prop_assert_eq!(report.moves.len(), inputs.len());
        prop_assert_eq!(
            (report.white.total() + report.black.total()) as usize,
            inputs.len()
        );
        for m in &report.moves {
            prop_assert!(m.loss.is_finite() && (0.0..=100.0).contains(&m.loss));
            prop_assert!((0.0..=100.0).contains(&m.accuracy));
        }
        for side in [&report.white, &report.black] {
            if let Some(a) = side.accuracy {
                prop_assert!(a.is_finite() && (0.0..=100.0).contains(&a));
            }
        }
    }
}

// ---- classes: names, translations ----

#[test]
fn every_class_has_a_distinct_fluent_key_defined_in_both_languages() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n");
    let en = std::fs::read_to_string(root.join("en-US").join("graph.ftl")).unwrap();
    let pt = std::fs::read_to_string(root.join("pt-BR").join("graph.ftl")).unwrap();
    let mut seen = std::collections::HashSet::new();
    for class in MoveClass::ALL {
        let key = class.name_key();
        assert!(key.starts_with("report-class-"), "{key}");
        assert!(seen.insert(key), "{key} repeated");
        assert!(en.contains(&format!("{key} =")), "{key} missing in en-US");
        assert!(pt.contains(&format!("{key} =")), "{key} missing in pt-BR");
    }
    assert_eq!(MoveClass::ALL.len(), 10);
}

#[test]
fn portuguese_names_follow_the_spec_vocabulary() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n");
    let pt = std::fs::read_to_string(root.join("pt-BR").join("graph.ftl")).unwrap();
    for (class, name) in [
        (MoveClass::Brilliant, "Brilhante"),
        (MoveClass::Great, "Excelente"),
        (MoveClass::Best, "Melhor"),
        (MoveClass::Excellent, "Ótimo"),
        (MoveClass::Good, "Bom"),
        (MoveClass::Book, "Livro"),
        (MoveClass::Inaccuracy, "Imprecisão"),
        (MoveClass::Mistake, "Erro"),
        (MoveClass::Miss, "Chance perdida"),
        (MoveClass::Blunder, "Capivarada"),
    ] {
        assert!(
            pt.contains(&format!("{} = {name}", class.name_key())),
            "{class:?}"
        );
    }
}

#[test]
fn the_faults_are_inaccuracy_mistake_miss_and_blunder() {
    let faults: Vec<MoveClass> = MoveClass::ALL
        .into_iter()
        .filter(|c| c.is_fault())
        .collect();
    assert_eq!(
        faults,
        vec![
            MoveClass::Inaccuracy,
            MoveClass::Mistake,
            MoveClass::Miss,
            MoveClass::Blunder
        ]
    );
}

// ---- key moments (D1435, D1436, D1533) ----

fn review_game() -> mychess_app::report::GameReport {
    let mut brilliant = input(Color::Black, ev(-0.8), ev(-0.8));
    brilliant.before.pv = vec![mv("e7e5")];
    brilliant.sacrifice = true;
    let inputs = vec![
        input(Color::White, ev(0.0), ev(0.0)),   // 1 best
        input(Color::Black, ev(0.0), ev(0.0)),   // 2 best
        input(Color::White, ev(0.0), ev(-0.8)),  // 3 inaccuracy
        brilliant,                               // 4 brilliant
        input(Color::White, ev(-0.8), ev(-3.0)), // 5 mistake or worse
        input(Color::Black, ev(-3.0), ev(-3.0)), // 6 best
    ];
    build_report(&inputs, &ReportThresholds::default())
}

#[test]
fn key_moments_list_the_faults_in_game_order() {
    let report = review_game();
    assert_eq!(report.moves[2].class, MoveClass::Inaccuracy);
    assert_eq!(report.moves[3].class, MoveClass::Brilliant);
    assert!(report.moves[4].class.is_fault());
    let both: Vec<usize> = key_moments(&report, SideFilter::Both, false)
        .iter()
        .map(|m| m.ply)
        .collect();
    assert_eq!(both, vec![3, 5]);
    let moments = key_moments(&report, SideFilter::Both, false);
    assert_eq!(moments[0].class, MoveClass::Inaccuracy);
    assert_eq!(moments[1].class, report.moves[4].class);
}

#[test]
fn key_moments_respect_the_side_and_the_highlights() {
    let report = review_game();
    let plies = |side, highlights| -> Vec<usize> {
        key_moments(&report, side, highlights)
            .iter()
            .map(|m| m.ply)
            .collect()
    };
    assert_eq!(plies(SideFilter::White, false), vec![3, 5]);
    assert_eq!(plies(SideFilter::Black, false), Vec::<usize>::new());
    assert_eq!(plies(SideFilter::Both, true), vec![3, 4, 5]);
    assert_eq!(plies(SideFilter::Black, true), vec![4]);
    assert_eq!(plies(SideFilter::White, true), vec![3, 5]);
}

#[test]
fn key_moments_never_include_book_good_or_best_moves() {
    let mut book = white_to(-9.0);
    book.book = true;
    let report = build_report(
        &[
            book,
            input(Color::Black, ev(-9.0), ev(-9.0)),
            white_to(-0.4),
        ],
        &ReportThresholds::default(),
    );
    assert!(key_moments(&report, SideFilter::Both, true).is_empty());
}
