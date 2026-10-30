//! Contract tests of the results and of the list of moves (spec m8-8,
//! D2553, D2556, D2557; acceptance criteria 1 and 2).

#![allow(clippy::assert_is_empty, clippy::unwrap_used, clippy::expect_used)]

use mychess_core::Position;
use mychess_tablebase::testing::{StubProbe, report};
use mychess_tablebase::{
    Category, Counts, Evaluation, OnlineProbe, PositionProbe, Source, position_key,
};
use pretty_assertions::assert_eq;

#[test]
fn negate_is_an_involution_and_flips_the_sign() {
    for category in Category::ALL {
        assert_eq!(category.negate().negate(), category);
        assert_eq!(category.negate().signum(), -category.signum());
    }
    assert_eq!(Category::CursedWin.negate(), Category::BlessedLoss);
    assert_eq!(Category::MaybeWin.negate(), Category::MaybeLoss);
    assert_eq!(Category::Draw.negate(), Category::Draw);
    assert_eq!(Category::Unknown.negate(), Category::Unknown);
}

#[test]
fn rank_orders_best_to_worst_and_unknown_last() {
    let ranks: Vec<u8> = Category::ALL.iter().map(|c| c.rank()).collect();
    let mut sorted = ranks.clone();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(ranks, sorted, "ALL is listed best first");
    assert_eq!(Category::Unknown.rank(), 0);
}

#[test]
fn lichess_names_round_trip_and_unknown_text_is_unknown() {
    for category in Category::ALL {
        assert_eq!(Category::from_lichess(category.as_str()), category);
    }
    assert_eq!(Category::from_lichess("surprise"), Category::Unknown);
    assert_eq!(Category::from_lichess(""), Category::Unknown);
}

#[test]
fn evaluation_negate_flips_distances() {
    let e = Evaluation {
        category: Category::Win,
        dtz: Some(12),
        dtz_rounded: true,
        dtm: Some(-7),
    };
    let n = e.negate();
    assert_eq!(n.category, Category::Loss);
    assert_eq!((n.dtz, n.dtm, n.dtz_rounded), (Some(-12), Some(7), true));
    assert_eq!(n.negate(), e);
}

fn start() -> Position {
    Position::new()
}

#[test]
fn moves_are_sorted_wins_first_shortest_first_losses_longest_first() {
    let pos = start();
    let r = report(
        &pos,
        Source::Local,
        Evaluation::of(Category::Win),
        &[
            ("e2e4", Category::Win, Some(9)),
            ("d2d4", Category::Win, Some(3)),
            ("g1f3", Category::Draw, Some(0)),
            ("b1c3", Category::Loss, Some(-4)),
            ("a2a3", Category::Loss, Some(-20)),
            ("h2h3", Category::CursedWin, Some(105)),
            ("c2c4", Category::BlessedLoss, Some(-105)),
        ],
    );
    let order: Vec<&str> = r.moves.iter().take(7).map(|m| m.uci.as_str()).collect();
    assert_eq!(
        order,
        ["d2d4", "e2e4", "h2h3", "g1f3", "c2c4", "a2a3", "b1c3"]
    );
    // The moves that were not listed are unknown and come last.
    assert!(
        r.moves[7..]
            .iter()
            .all(|m| m.evaluation.category == Category::Unknown)
    );
}

#[test]
fn ties_are_broken_by_san_deterministically() {
    let pos = start();
    let a = report(
        &pos,
        Source::Local,
        Evaluation::of(Category::Draw),
        &[
            ("e2e4", Category::Draw, None),
            ("d2d4", Category::Draw, None),
        ],
    );
    let b = report(
        &pos,
        Source::Local,
        Evaluation::of(Category::Draw),
        &[
            ("d2d4", Category::Draw, None),
            ("e2e4", Category::Draw, None),
        ],
    );
    assert_eq!(a, b);
    assert_eq!(a.moves[0].san, "d4");
    assert_eq!(a.moves[1].san, "e4");
}

#[test]
fn known_distances_come_before_unknown_ones() {
    let pos = start();
    let r = report(
        &pos,
        Source::Local,
        Evaluation::of(Category::Win),
        &[
            ("a2a3", Category::Win, None),
            ("e2e4", Category::Win, Some(50)),
        ],
    );
    assert_eq!(r.moves[0].uci, "e2e4");
    assert_eq!(r.moves[1].uci, "a2a3");
}

#[test]
fn counts_group_the_categories() {
    let pos = start();
    let r = report(
        &pos,
        Source::Local,
        Evaluation::of(Category::Win),
        &[
            ("e2e4", Category::Win, None),
            ("d2d4", Category::MaybeWin, None),
            ("g1f3", Category::Draw, None),
            ("b1c3", Category::CursedWin, None),
            ("a2a3", Category::Loss, None),
            ("h2h3", Category::MaybeLoss, None),
            ("c2c4", Category::BlessedLoss, None),
        ],
    );
    assert_eq!(
        r.counts(),
        Counts {
            wins: 2,
            draws: 3,
            losses: 2,
            unknown: 13
        }
    );
}

#[test]
fn best_group_and_recommended_are_deterministic() {
    let pos = start();
    let r = report(
        &pos,
        Source::Local,
        Evaluation::of(Category::Win),
        &[
            ("e2e4", Category::Win, Some(3)),
            ("d2d4", Category::Win, Some(3)),
            ("g1f3", Category::Win, Some(5)),
        ],
    );
    let group: Vec<&str> = r.best_group().iter().map(|m| m.san.as_str()).collect();
    assert_eq!(group, ["d4", "e4"]);
    assert_eq!(r.best().map(|m| m.san.as_str()), Some("d4"));
    assert_eq!(r.recommended(0).map(|m| m.san.as_str()), Some("d4"));
    assert_eq!(r.recommended(1).map(|m| m.san.as_str()), Some("e4"));
    assert_eq!(r.recommended(2).map(|m| m.san.as_str()), Some("d4"));
    assert_eq!(r.recommended(u64::MAX), r.recommended(u64::MAX));
}

#[test]
fn an_unknown_best_move_has_no_group() {
    let pos = start();
    let r = report(&pos, Source::Local, Evaluation::UNKNOWN, &[]);
    assert!(r.best_group().is_empty());
    assert!(r.best().is_none());
    assert!(r.recommended(7).is_none());
}

#[test]
fn has_dtm_only_when_a_distance_to_mate_is_there() {
    let pos = start();
    let r = report(&pos, Source::Local, Evaluation::of(Category::Draw), &[]);
    assert!(!r.has_dtm());
    let with = report(
        &pos,
        Source::Online,
        Evaluation {
            dtm: Some(5),
            ..Evaluation::of(Category::Win)
        },
        &[],
    );
    assert!(with.has_dtm());
}

#[test]
fn position_key_ignores_the_move_number_but_not_the_clock() {
    let a = Position::from_fen("8/8/8/8/8/2k5/8/KQ6 w - - 0 1").unwrap();
    let b = Position::from_fen("8/8/8/8/8/2k5/8/KQ6 w - - 0 40").unwrap();
    let c = Position::from_fen("8/8/8/8/8/2k5/8/KQ6 w - - 7 40").unwrap();
    assert_eq!(position_key(&a), position_key(&b));
    assert_ne!(position_key(&a), position_key(&c));
}

#[test]
fn the_stub_covers_what_it_says_and_counts_its_calls() {
    let stub = StubProbe::new(5);
    let kq = Position::from_fen("8/8/8/8/8/2k5/8/KQ6 w - - 0 1").unwrap();
    stub.answer(
        &kq,
        Ok(report(
            &kq,
            Source::Local,
            Evaluation::of(Category::Win),
            &[],
        )),
    );
    assert!(PositionProbe::probe(&stub, &kq).is_ok());
    assert_eq!(stub.calls(), 1);
    // Too many pieces for a stub of 3.
    let small = StubProbe::new(3);
    let start = Position::new();
    assert!(PositionProbe::probe(&small, &start).is_err());
    drop(OnlineProbe::probe(&stub, &kq));
}
