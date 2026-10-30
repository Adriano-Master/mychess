//! M4-4 contract tests of the pure parts: score/PV/comment formatting
//! (D439, D441, D795), `short_nodes`, `parse_exclusion` (D445) and the notice
//! keys (D790). Derived from the spec and from the Scid's `scoreToMate`,
//! `addMoveNumbers` and `calculateNodes`.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string
)]

mod m4_4_common;

use std::collections::HashSet;

use mychess_app::{
    AnalysisNotice, AnalysisPrefs, ExcludeError, NumberStyle, comment_score, format_comment,
    format_pv, format_pv_limited, format_score, format_wdl, nodes_per_second, parse_exclusion,
    search_moves_without, short_nodes, wdl_percent,
};
use mychess_core::Position;
use mychess_engine::{Bound, Score, Wdl};
use proptest::prelude::*;
use rstest::rstest;

fn fen(text: &str) -> Position {
    Position::from_fen(text).expect("fen")
}

// ---- format_score ----

#[rstest]
#[case(Score::Cp(35), "+0.35")]
#[case(Score::Cp(-120), "-1.20")]
#[case(Score::Cp(-5), "-0.05")]
#[case(Score::Cp(100), "+1.00")]
#[case(Score::Cp(32_700), "+327.00")]
#[case(Score::Cp(-32_700), "-327.00")]
#[case(Score::Mate(3), "+M3")]
#[case(Score::Mate(-3), "-M3")]
#[case(Score::Mate(1), "+M1")]
fn exact_scores_follow_the_spec_table(#[case] score: Score, #[case] text: &str) {
    assert_eq!(format_score(score, Bound::Exact), text);
}

#[test]
fn a_zero_score_is_zero_point_zero_zero() {
    let s = format_score(Score::Cp(0), Bound::Exact);
    assert!(s == "0.00" || s == "+0.00", "got {s}");
}

#[test]
fn bounds_get_a_prefix_and_exact_gets_none() {
    assert_eq!(format_score(Score::Cp(35), Bound::Lower), "\u{2265}+0.35");
    assert_eq!(format_score(Score::Cp(-35), Bound::Upper), "\u{2264}-0.35");
    assert_eq!(format_score(Score::Mate(2), Bound::Lower), "\u{2265}+M2");
    assert_eq!(format_score(Score::Mate(-2), Bound::Upper), "\u{2264}-M2");
}

#[test]
fn extreme_scores_do_not_panic() {
    for s in [
        Score::Cp(i32::MAX),
        Score::Cp(i32::MIN),
        Score::Mate(i32::MAX),
        Score::Mate(i32::MIN),
        Score::Mate(0),
    ] {
        for b in [Bound::Exact, Bound::Lower, Bound::Upper] {
            assert!(!format_score(s, b).is_empty());
            drop(comment_score(s));
        }
    }
}

proptest! {
    #[test]
    fn centipawn_text_round_trips_through_its_digits(cp in -1_000_000i32..1_000_000) {
        let text = format_score(Score::Cp(cp), Bound::Exact);
        let value: f64 = text.parse().expect("number");
        prop_assert!((value * 100.0 - f64::from(cp)).abs() < 0.5, "{text} vs {cp}");
    }
}

// ---- format_comment (D441, D795) ----

fn prefs(name_in_comments: bool) -> AnalysisPrefs {
    AnalysisPrefs {
        name_in_comments,
        ..AnalysisPrefs::default()
    }
}

#[test]
fn comment_has_name_depth_and_score() {
    assert_eq!(
        format_comment(&prefs(true), "Stockfish", 20, Score::Cp(35), None),
        "[Stockfish] 20:+0.35"
    );
}

#[test]
fn comment_without_the_name_when_the_preference_is_off() {
    assert_eq!(
        format_comment(&prefs(false), "Stockfish", 20, Score::Cp(35), None),
        "20:+0.35"
    );
}

#[test]
fn comment_of_a_mate_keeps_the_scid_shape_without_plus() {
    assert_eq!(
        format_comment(&prefs(false), "x", 18, Score::Mate(3), None),
        "18:M3"
    );
    assert_eq!(
        format_comment(&prefs(false), "x", 18, Score::Mate(-3), None),
        "18:-M3"
    );
}

#[test]
fn comment_appends_wdl_as_percentages() {
    let wdl = Wdl {
        win: 450,
        draw: 300,
        loss: 250,
    };
    assert_eq!(
        format_comment(&prefs(false), "x", 12, Score::Cp(-120), Some(wdl)),
        "12:-1.20 W: 45% D: 30% B: 25%"
    );
}

#[test]
fn wdl_truncates_and_the_largest_takes_the_remainder() {
    // 333/333/334 per mille: 33/33/33, the largest (first in a tie... here the
    // loss is the largest at 33 too) takes the rest to 100.
    let (w, d, b) = wdl_percent(Wdl {
        win: 333,
        draw: 333,
        loss: 334,
    });
    assert_eq!(w + d + b, 100);
    let (w, d, b) = wdl_percent(Wdl {
        win: 999,
        draw: 1,
        loss: 0,
    });
    assert_eq!((w, d, b), (99 + 1, 0, 0), "largest (win) gets the rest");
    assert!(
        format_wdl(Wdl {
            win: 100,
            draw: 100,
            loss: 800
        })
        .contains("B: 80%")
    );
}

proptest! {
    #[test]
    fn wdl_percentages_always_total_one_hundred(w in 0u16..=1000, d in 0u16..=1000) {
        prop_assume!(w + d <= 1000);
        let l = 1000 - w - d;
        let (a, b, c) = wdl_percent(Wdl { win: w, draw: d, loss: l });
        prop_assert_eq!(a + b + c, 100);
    }

    #[test]
    fn wdl_percent_never_panics_on_hostile_values(w in any::<u16>(), d in any::<u16>(), l in any::<u16>()) {
        let _ = wdl_percent(Wdl { win: w, draw: d, loss: l });
        drop(format_wdl(Wdl { win: w, draw: d, loss: l }));
    }
}

// ---- format_pv (D439, D795) ----

fn ucis(position: &Position, text: &[&str]) -> Vec<mychess_core::Move> {
    m4_4_common::pv_of(position, text)
}

#[test]
fn pv_with_white_to_move_numbers_each_white_move() {
    let pos = Position::new();
    let pv = ucis(&pos, &["e2e4", "e7e5", "g1f3", "b8c6"]);
    assert_eq!(
        format_pv(&pos, &pv, NumberStyle::Tight),
        "1.e4 e5 2.Nf3 Nc6"
    );
    assert_eq!(
        format_pv(&pos, &pv, NumberStyle::Spaced),
        "1. e4 e5 2. Nf3 Nc6"
    );
}

#[test]
fn pv_with_black_to_move_starts_with_the_move_number_and_dots() {
    let pos = fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1");
    let pv = ucis(&pos, &["e7e5", "g1f3", "b8c6"]);
    // D795: `N... move` (three dots), not the Scid's accidental four.
    assert_eq!(
        format_pv(&pos, &pv, NumberStyle::Tight),
        "1... e5 2.Nf3 Nc6"
    );
    let spaced = format_pv(&pos, &pv, NumberStyle::Spaced);
    assert!(spaced.starts_with("1. ... e5"), "got {spaced}");
    assert!(spaced.contains("2. Nf3"), "got {spaced}");
}

#[test]
fn pv_numbering_uses_the_move_number_of_the_position() {
    let pos = fen("r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 2 7");
    let pv = ucis(&pos, &["g8f6", "b1c3"]);
    assert_eq!(format_pv(&pos, &pv, NumberStyle::Tight), "7... Nf6 8.Nc3");
}

#[test]
fn pv_of_one_move_and_empty_pv() {
    let pos = Position::new();
    assert_eq!(
        format_pv(&pos, &ucis(&pos, &["d2d4"]), NumberStyle::Tight),
        "1.d4"
    );
    assert_eq!(format_pv(&pos, &[], NumberStyle::Tight), "");
}

#[test]
fn pv_writes_castling_and_promotion_in_san() {
    let pos = fen("r3k2r/1P6/8/8/8/8/8/R3K2R w KQkq - 0 1");
    let pv = ucis(&pos, &["e1g1", "e8d8"]);
    assert_eq!(format_pv(&pos, &pv, NumberStyle::Tight), "1.O-O Kd8");
    let pv = ucis(&pos, &["e1c1"]);
    assert_eq!(format_pv(&pos, &pv, NumberStyle::Tight), "1.O-O-O");
    let pv = ucis(&pos, &["b7a8q"]);
    assert_eq!(format_pv(&pos, &pv, NumberStyle::Tight), "1.bxa8=Q+");
}

#[test]
fn pv_of_256_plies_is_limited_by_max_plies() {
    // Shuffle knights back and forth: 4 plies per cycle, 64 cycles (128 full moves).
    let mut pos = Position::new();
    let mut moves = Vec::new();
    for _ in 0..64 {
        for u in ["g1f3", "g8f6", "f3g1", "f6g8"] {
            let m = pos.parse_uci(u).expect("uci");
            pos.play(m).expect("play");
            moves.push(m);
        }
    }
    assert_eq!(moves.len(), 256);
    let start = Position::new();
    let cut = format_pv_limited(&start, &moves, NumberStyle::Tight, 6);
    assert_eq!(cut, "1.Nf3 Nf6 2.Ng1 Ng8 3.Nf3 Nf6");
    let all = format_pv(&start, &moves, NumberStyle::Tight);
    assert!(
        all.ends_with("128.Ng1 Ng8"),
        "tail was {}",
        &all[all.len().saturating_sub(20)..]
    );
}

#[test]
fn a_stale_pv_never_panics_and_keeps_the_legal_prefix() {
    let pos = Position::new();
    let good = ucis(&pos, &["e2e4", "e7e5"]);
    // The same moves from the position after 1.d4: the first is illegal there.
    let other = fen("rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq - 0 1");
    let text = format_pv(&other, &good, NumberStyle::Tight);
    assert!(text.is_empty() || !text.contains("e4"), "got {text}");
}

// ---- short_nodes / nps ----

#[rstest]
#[case(0, 0)]
#[case(999, 0)]
#[case(1000, 1)]
#[case(123_456, 123)]
#[case(u64::MAX, u64::MAX / 1000)]
fn short_nodes_drops_the_last_three_digits(#[case] n: u64, #[case] expected: u64) {
    assert_eq!(short_nodes(n), expected);
}

#[test]
fn nodes_per_second_is_zero_without_time_and_never_overflows() {
    assert_eq!(nodes_per_second(1_000_000, 0), 0);
    assert_eq!(nodes_per_second(1_000_000, 1000), 1_000_000);
    let _ = nodes_per_second(u64::MAX, 1);
}

// ---- parse_exclusion (D445) ----

#[test]
fn exclusion_reads_san_split_by_spaces_and_commas() {
    let pos = Position::new();
    let moves = parse_exclusion(&pos, "e4, d4  Nf3,c4").expect("legal");
    let got: HashSet<String> = moves.iter().map(|m| pos.uci(*m)).collect();
    let want: HashSet<String> = ["e2e4", "d2d4", "g1f3", "c2c4"].map(String::from).into();
    assert_eq!(got, want);
}

#[test]
fn empty_exclusion_text_clears() {
    let pos = Position::new();
    assert_eq!(parse_exclusion(&pos, "").expect("ok"), vec![]);
    assert_eq!(parse_exclusion(&pos, "  , ,").expect("ok"), vec![]);
}

#[test]
fn illegal_san_is_reported_with_the_token() {
    let pos = Position::new();
    match parse_exclusion(&pos, "e4 Ke2") {
        Err(ExcludeError::Invalid(t)) => assert_eq!(t, "Ke2"),
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert!(matches!(
        parse_exclusion(&pos, "e5"),
        Err(ExcludeError::Invalid(_))
    ));
}

#[test]
fn excluding_every_legal_move_is_refused() {
    let pos = Position::new();
    let all: Vec<String> = pos
        .legal_moves()
        .into_iter()
        .map(|m| pos.san(m).expect("san"))
        .collect();
    assert_eq!(all.len(), 20);
    assert_eq!(
        parse_exclusion(&pos, &all.join(" ")),
        Err(ExcludeError::All)
    );
    // 19 of 20 is accepted and leaves exactly one.
    let nineteen = all[..19].join(", ");
    let excluded = parse_exclusion(&pos, &nineteen).expect("ok");
    assert_eq!(search_moves_without(&pos, &excluded).len(), 1);
}

#[test]
fn hostile_exclusion_text_does_not_panic() {
    let pos = Position::new();
    let long = "x".repeat(100_000);
    match parse_exclusion(&pos, &long) {
        Err(ExcludeError::Invalid(t)) => assert!(t.chars().count() <= 64),
        other => panic!("expected Invalid, got {other:?}"),
    }
    for text in [
        "\u{0}",
        "é\u{301}",
        "e4\u{200b}",
        "O-O-O-O",
        "e4e4e4",
        "--",
        "+++",
        "😀 😀",
    ] {
        drop(parse_exclusion(&pos, text));
    }
}

proptest! {
    #[test]
    fn parse_exclusion_never_panics(text in ".{0,200}") {
        let pos = Position::new();
        drop(parse_exclusion(&pos, &text));
    }
}

// ---- notice keys (D790) ----

#[test]
fn notice_message_keys_are_unique_and_namespaced() {
    let all = [
        AnalysisNotice::AtGameEnd,
        AnalysisNotice::LineTruncated(2),
        AnalysisNotice::ExcludeInvalid("x".into()),
        AnalysisNotice::ExcludeAll,
        AnalysisNotice::ExcludeUnsupported,
        AnalysisNotice::MultiPvUnsupported,
        AnalysisNotice::Locked,
        AnalysisNotice::LogSaved,
    ];
    let keys: HashSet<&str> = all.iter().map(AnalysisNotice::message_key).collect();
    assert_eq!(keys.len(), all.len(), "keys must be unique");
    for k in &keys {
        assert!(k.starts_with("analysis-notice-"), "{k}");
        assert!(k.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{k}");
    }
    // Parameters do not change the key.
    assert_eq!(
        AnalysisNotice::LineTruncated(1).message_key(),
        AnalysisNotice::LineTruncated(9).message_key()
    );
}
