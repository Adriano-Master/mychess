//! Contract tests of `Filter` (spec M3-3, "Filtro": behaviour 1-6, rules
//! 1-12 and 14, D324-D327, D343, D363, D374) and of the progress types.
#![allow(
    clippy::assertions_on_result_states,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::needless_range_loop
)]

#[path = "common/m3_3a_support.rs"]
mod support;

use mychess_db::{CancelToken, FILTER_LAST_MOVE, Filter, FilterOp, NoProgress, ProgressSink};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;
use support::{B, build};

// ------------------------------------------------------------ naive model

fn m_rank(m: &[u8], g: u32) -> u32 {
    if g == 0 || g as usize > m.len() {
        return 0;
    }
    m[..g as usize].iter().filter(|v| **v != 0).count() as u32
}

fn m_nth(m: &[u8], n: u32) -> Option<u32> {
    if n == 0 {
        return None;
    }
    m.iter()
        .enumerate()
        .filter(|(_, v)| **v != 0)
        .nth(n as usize - 1)
        .map(|(i, _)| i as u32 + 1)
}

fn m_next(m: &[u8], g: u32) -> Option<u32> {
    (g as usize + 1..=m.len())
        .find(|i| m[i - 1] != 0)
        .map(|i| i as u32)
}

fn m_prev(m: &[u8], g: u32) -> Option<u32> {
    (1..g as usize)
        .rev()
        .find(|i| *i <= m.len() && m[i - 1] != 0)
        .map(|i| i as u32)
}

fn m_count(m: &[u8]) -> u32 {
    m.iter().filter(|v| **v != 0).count() as u32
}

fn assert_matches_model(f: &Filter, m: &[u8], probes: &[u32]) {
    assert_eq!(f.len() as usize, m.len());
    assert_eq!(f.is_empty(), m.is_empty());
    assert_eq!(f.count(), m_count(m));
    assert_eq!(f.first(), m_nth(m, 1));
    assert_eq!(f.last(), m_prev(m, m.len() as u32 + 1));
    assert_eq!(
        f.iter_games().collect::<Vec<_>>(),
        (1..=m.len() as u32)
            .filter(|g| m[*g as usize - 1] != 0)
            .collect::<Vec<_>>()
    );
    for &g in probes {
        let expect = if g >= 1 && g as usize <= m.len() {
            m[g as usize - 1]
        } else {
            0
        };
        assert_eq!(f.get(g), expect, "get({g})");
        assert_eq!(f.rank(g), m_rank(m, g), "rank({g})");
        assert_eq!(f.next_after(g), m_next(m, g), "next_after({g})");
        assert_eq!(f.prev_before(g), m_prev(m, g), "prev_before({g})");
    }
    for n in [
        0,
        1,
        2,
        f.count().saturating_sub(1),
        f.count(),
        f.count() + 1,
    ] {
        assert_eq!(f.nth(n), m_nth(m, n), "nth({n})");
    }
}

fn all_probes(n: usize) -> Vec<u32> {
    (0..=n as u32 + 3).collect()
}

// ------------------------------------------------------------ basics

#[test]
fn new_filter_has_every_game_in_with_value_one() {
    let f = Filter::new(7);
    assert_eq!(f.len(), 7);
    assert_eq!(f.count(), 7);
    for g in 1..=7 {
        assert_eq!(f.get(g), 1);
    }
}

#[test]
fn empty_filter_has_no_games_and_every_query_is_none() {
    let mut f = Filter::new(0);
    assert!(f.is_empty());
    assert_eq!((f.len(), f.count()), (0, 0));
    assert_eq!(
        (f.first(), f.last(), f.nth(0), f.nth(1)),
        (None, None, None, None)
    );
    assert_eq!(
        (f.next_after(0), f.prev_before(0), f.prev_before(1)),
        (None, None, None)
    );
    assert_eq!((f.rank(0), f.rank(1)), (0, 0));
    f.negate();
    f.end();
    f.fill(1);
    f.remove_range(1, 10);
    assert_eq!((f.len(), f.count()), (0, 0));
    assert_eq!(f.iter_games().count(), 0);
}

#[test]
fn single_game_filter_navigation() {
    let mut f = Filter::new(1);
    assert_eq!(
        (f.first(), f.last(), f.nth(1), f.nth(2)),
        (Some(1), Some(1), Some(1), None)
    );
    assert_eq!((f.next_after(0), f.next_after(1)), (Some(1), None));
    assert_eq!((f.prev_before(2), f.prev_before(1)), (Some(1), None));
    f.set(1, 0);
    assert_eq!((f.count(), f.first(), f.last()), (0, None, None));
}

#[test]
fn fill_zero_empties_the_count_and_fill_value_refills_it() {
    let mut f = Filter::new(10);
    f.fill(0);
    assert_eq!(f.count(), 0);
    assert_eq!(f.len(), 10);
    f.fill(9);
    assert_eq!(f.count(), 10);
    assert!((1..=10).all(|g| f.get(g) == 9));
}

#[test]
fn set_nonzero_then_zero_restores_the_previous_count() {
    let mut f = Filter::new(10);
    f.set(3, 0);
    let before = f.count();
    f.set(3, 5);
    assert_eq!(f.count(), before + 1);
    f.set(3, 0);
    assert_eq!(f.count(), before);
}

#[test]
fn set_between_two_nonzero_values_keeps_the_count() {
    let mut f = Filter::new(4);
    f.set(2, 77);
    f.set(2, 200);
    assert_eq!(f.count(), 4);
    assert_eq!(f.get(2), 200);
}

#[test]
fn set_out_of_range_is_ignored() {
    let mut f = Filter::new(3);
    assert!(!f.set(0, 5));
    assert!(!f.set(4, 5));
    assert!(f.set(3, 5));
    assert_eq!(f.count(), 3);
    assert_eq!((f.get(0), f.get(4), f.get(u32::MAX)), (0, 0, 0));
}

#[test]
fn append_grows_by_one_and_counts_only_nonzero() {
    let mut f = Filter::new(2);
    assert!(f.append(1));
    assert_eq!((f.len(), f.count()), (3, 3));
    assert!(f.append(0));
    assert_eq!((f.len(), f.count()), (4, 3));
    assert_eq!((f.get(3), f.get(4)), (1, 0));
    assert_eq!(f.last(), Some(3));
}

#[test]
fn append_crosses_a_block_boundary_keeping_rank_and_nth() {
    let mut f = Filter::new(4095);
    f.append(1);
    f.append(1);
    f.append(0);
    f.append(1);
    assert_eq!(f.len(), 4099);
    assert_eq!(f.count(), 4098);
    assert_eq!(f.nth(4098), Some(4099));
    assert_eq!(f.rank(4099), 4098);
    assert_eq!(f.rank(4098), 4097);
    assert_eq!(f.prev_before(4099), Some(4097));
}

#[test]
fn negate_swaps_membership_and_drops_the_ply() {
    let mut f = Filter::new(6);
    f.set(2, 0);
    f.set(4, 0);
    f.set(1, 40);
    f.set(6, 255);
    f.negate();
    let got: Vec<u8> = (1..=6).map(|g| f.get(g)).collect();
    assert_eq!(got, [0, 1, 0, 1, 0, 0]);
    assert_eq!(f.count(), 2);
}

#[test]
fn negate_twice_gives_membership_back_with_ply_one() {
    let mut f = Filter::new(5);
    f.set(1, 9);
    f.set(3, 0);
    f.negate();
    f.negate();
    let got: Vec<u8> = (1..=5).map(|g| f.get(g)).collect();
    assert_eq!(got, [1, 1, 0, 1, 1]);
}

#[test]
fn end_sets_255_only_on_games_that_are_in() {
    let mut f = Filter::new(5);
    f.set(2, 0);
    f.set(4, 17);
    f.end();
    let got: Vec<u8> = (1..=5).map(|g| f.get(g)).collect();
    assert_eq!(got, [255, 0, 255, 255, 255]);
    assert_eq!(f.count(), 4);
    assert_eq!(FILTER_LAST_MOVE, 255);
}

#[rstest]
#[case(0, 0, 0)] // from clamps to 1: removes game 1
#[case(0, 3, 3)]
#[case(2, 4, 3)]
#[case(4, 100, 2)] // to clamps to N
#[case(6, 9, 0)] // from > N: no-op, no error
#[case(100, 200, 0)]
#[case(4, 2, 0)] // from > to: no-op
#[case(1, 5, 5)]
fn remove_range_clamps_to_the_filter_and_never_fails(
    #[case] from: u32,
    #[case] to: u32,
    #[case] removed: u32,
) {
    let mut f = Filter::new(5);
    f.remove_range(from, to);
    // `0 0` clamps `from` to 1 and `to` stays 0, so it is an empty range.
    let expect = if (from, to) == (0, 0) { 0 } else { removed };
    assert_eq!(f.count(), 5 - expect);
}

#[test]
fn remove_range_keeps_the_ply_of_games_outside_the_range() {
    let mut f = Filter::new(5);
    f.set(5, 33);
    f.remove_range(2, 3);
    assert_eq!(f.get(5), 33);
    assert_eq!((f.get(2), f.get(3)), (0, 0));
}

#[test]
fn navigation_follows_the_scid_rules_with_a_hole_filter() {
    // in: 2, 3, 7, 9 of 10
    let mut f = Filter::new(10);
    f.fill(0);
    for g in [2, 3, 7, 9] {
        f.set(g, 1);
    }
    assert_eq!((f.first(), f.last()), (Some(2), Some(9)));
    assert_eq!(
        (1..=5).map(|n| f.nth(n)).collect::<Vec<_>>(),
        [Some(2), Some(3), Some(7), Some(9), None]
    );
    assert_eq!(f.nth(0), None);
    // locate: games of the filter with number <= g
    assert_eq!(
        (0..=12).map(|g| f.rank(g)).collect::<Vec<_>>(),
        [0, 0, 1, 2, 2, 2, 2, 3, 3, 4, 4, 0, 0]
    );
    assert_eq!(
        (
            f.next_after(0),
            f.next_after(3),
            f.next_after(9),
            f.next_after(50)
        ),
        (Some(2), Some(7), None, None)
    );
    assert_eq!(
        (
            f.prev_before(2),
            f.prev_before(3),
            f.prev_before(10),
            f.prev_before(500)
        ),
        (None, Some(2), Some(9), Some(9))
    );
}

#[test]
fn ply_shown_on_load_is_value_minus_one_and_zero_when_out() {
    let mut f = Filter::new(3);
    f.set(1, 1);
    f.set(2, 30);
    f.set(3, 0);
    assert_eq!((f.ply(1), f.ply(2), f.ply(3)), (0, 29, 0));
    f.end();
    // 255 means "last move": the loader clamps the ply to the line end.
    assert_eq!(f.ply(1), 254);
}

#[test]
fn filter_op_texts_follow_str_get_filter_op() {
    for t in ["a", "A", "0", "and", "And"] {
        assert_eq!(FilterOp::from_scid_str(t), FilterOp::And, "{t}");
    }
    for t in ["o", "O", "1", "or"] {
        assert_eq!(FilterOp::from_scid_str(t), FilterOp::Or, "{t}");
    }
    for t in ["r", "R", "2", "", "x", "9", "-"] {
        assert_eq!(FilterOp::from_scid_str(t), FilterOp::Reset, "{t}");
    }
    assert_eq!(FilterOp::default(), FilterOp::Reset);
}

// ------------------------------------------------------------ block borders

#[rstest]
#[case(4095)]
#[case(4096)]
#[case(4097)]
#[case(8192)]
#[case(8193)]
fn rank_and_nth_agree_with_the_model_at_block_borders(#[case] n: usize) {
    let mut model = vec![1u8; n];
    let mut f = Filter::new(n as u32);
    // punch holes around the borders
    for g in [1usize, 2, 4094, 4095, 4096, 4097, 4098, 8191, 8192, 8193] {
        if g <= n {
            f.set(g as u32, 0);
            model[g - 1] = 0;
        }
    }
    let probes: Vec<u32> = [
        0usize,
        1,
        2,
        3,
        4093,
        4094,
        4095,
        4096,
        4097,
        4098,
        4099,
        8191,
        8192,
        8193,
        8194,
        n,
        n + 1,
        n + 2,
    ]
    .iter()
    .map(|g| *g as u32)
    .collect();
    assert_matches_model(&f, &model, &probes);
}

#[test]
fn a_whole_empty_block_is_skipped_by_navigation() {
    let n = 3 * 4096 + 10;
    let mut f = Filter::new(n);
    f.fill(0);
    f.set(5, 1);
    f.set(2 * 4096 + 7, 1);
    assert_eq!(f.next_after(5), Some(2 * 4096 + 7));
    assert_eq!(f.prev_before(2 * 4096 + 7), Some(5));
    assert_eq!(f.nth(2), Some(2 * 4096 + 7));
    assert_eq!(f.rank(3 * 4096), 2);
    assert_eq!(f.last(), Some(2 * 4096 + 7));
}

#[test]
#[ignore = "slow: 5 million games (about 5 MB); run with --ignored"]
fn five_million_game_filter_keeps_count_rank_and_select() {
    let n = 5_000_000u32;
    let mut f = Filter::new(n);
    assert_eq!(f.count(), n);
    f.remove_range(1, 1_000_000);
    f.set(4_999_999, 0);
    assert_eq!(f.count(), n - 1_000_000 - 1);
    assert_eq!(f.first(), Some(1_000_001));
    assert_eq!(f.last(), Some(5_000_000));
    assert_eq!(f.nth(1), Some(1_000_001));
    assert_eq!(f.nth(f.count()), Some(5_000_000));
    assert_eq!(f.rank(5_000_000), f.count());
    assert_eq!(f.rank(1_000_000), 0);
    assert_eq!(f.prev_before(5_000_000), Some(4_999_998));
    f.negate();
    assert_eq!(f.count(), 1_000_001);
    assert_eq!(f.nth(1_000_001), Some(4_999_999));
    f.end();
    assert_eq!(f.get(4_999_999), 255);
}

// ------------------------------------------------------------ proptests

#[derive(Debug, Clone)]
enum Op {
    Set(u32, u8),
    Fill(u8),
    Append(u8),
    Negate,
    End,
    Remove(u32, u32),
}

fn op_strategy(n: u32) -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0..n + 3, any::<u8>()).prop_map(|(g, v)| Op::Set(g, v)),
        1 => any::<u8>().prop_map(Op::Fill),
        2 => any::<u8>().prop_map(Op::Append),
        1 => Just(Op::Negate),
        1 => Just(Op::End),
        2 => (0..n + 4, 0..n + 4).prop_map(|(a, b)| Op::Remove(a, b)),
    ]
}

fn apply_model(m: &mut Vec<u8>, op: &Op) {
    match *op {
        Op::Set(g, v) => {
            if g >= 1 && g as usize <= m.len() {
                m[g as usize - 1] = v;
            }
        }
        Op::Fill(v) => m.fill(v),
        Op::Append(v) => m.push(v),
        Op::Negate => m.iter_mut().for_each(|v| *v = u8::from(*v == 0)),
        Op::End => m.iter_mut().filter(|v| **v != 0).for_each(|v| *v = 255),
        Op::Remove(a, b) => {
            let from = a.max(1) as usize;
            let to = (b as usize).min(m.len());
            if from <= to {
                m[from - 1..to].fill(0);
            }
        }
    }
}

fn apply_filter(f: &mut Filter, op: &Op) {
    match *op {
        Op::Set(g, v) => {
            f.set(g, v);
        }
        Op::Fill(v) => f.fill(v),
        Op::Append(v) => {
            f.append(v);
        }
        Op::Negate => f.negate(),
        Op::End => f.end(),
        Op::Remove(a, b) => f.remove_range(a, b),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn filter_follows_the_naive_model_through_random_operations(
        n in 0u32..120,
        ops in proptest::collection::vec(op_strategy(120), 0..40),
    ) {
        let mut f = Filter::new(n);
        let mut m = vec![1u8; n as usize];
        for op in &ops {
            apply_model(&mut m, op);
            apply_filter(&mut f, op);
            assert_matches_model(&f, &m, &all_probes(m.len()));
        }
    }

    #[test]
    fn sparse_large_filter_matches_the_model_around_block_borders(
        n in 4000usize..9000,
        ins in proptest::collection::vec(1usize..9000, 0..12),
    ) {
        let mut f = Filter::new(n as u32);
        f.fill(0);
        let mut m = vec![0u8; n];
        for g in ins {
            if g <= n {
                f.set(g as u32, 3);
                m[g - 1] = 3;
            }
        }
        let probes: Vec<u32> = [0usize, 1, 4095, 4096, 4097, 8191, 8192, 8193, n, n + 1, n + 2]
            .iter()
            .chain(&[n / 2, n / 3, 4094, 4098])
            .map(|g| *g as u32)
            .collect();
        assert_matches_model(&f, &m, &probes);
    }

    #[test]
    fn negate_is_an_involution_on_membership_and_resets_the_ply(
        vals in proptest::collection::vec(any::<u8>(), 0..300),
    ) {
        let mut f = Filter::new(vals.len() as u32);
        for (i, v) in vals.iter().enumerate() {
            f.set(i as u32 + 1, *v);
        }
        let inside = m_count(&vals);
        f.negate();
        prop_assert_eq!(f.count() as usize, vals.len() - inside as usize);
        for (i, v) in vals.iter().enumerate() {
            prop_assert_eq!(f.get(i as u32 + 1), u8::from(*v == 0));
        }
        f.negate();
        for (i, v) in vals.iter().enumerate() {
            prop_assert_eq!(f.get(i as u32 + 1), u8::from(*v != 0));
        }
    }

    #[test]
    fn end_keeps_membership_and_makes_every_member_255(
        vals in proptest::collection::vec(any::<u8>(), 0..300),
    ) {
        let mut f = Filter::new(vals.len() as u32);
        for (i, v) in vals.iter().enumerate() {
            f.set(i as u32 + 1, *v);
        }
        let count = f.count();
        f.end();
        prop_assert_eq!(f.count(), count);
        for (i, v) in vals.iter().enumerate() {
            prop_assert_eq!(f.get(i as u32 + 1), if *v == 0 { 0 } else { 255 });
        }
    }
}

// ------------------------------------------------------------ text filter

fn text_db() -> support::Fixture {
    build(&[
        B::new("Kasparov, Garry", "Karpov, Anatoly")
            .event("Linares")
            .site("Spain")
            .round("Zorro")
            .date("1990.05.01")
            .done(),
        B::new("Smith, John", "Doe, Jane")
            .event("Open")
            .site("Dallas USA")
            .round("1")
            .date("2000.??.??")
            .done(),
        B::new("Doe, Jane", "kasparov, g")
            .event("Club")
            .site("Home")
            .round("2")
            .date("1985.12.24")
            .done(),
        B::new("Nobody", "Else")
            .event("ZZZ")
            .site("Nowhere")
            .round("3")
            .date("????.??.??")
            .done(),
    ])
}

#[rstest]
#[case("Linares", false, Some(1))] // event
#[case("Dallas", false, Some(2))] // site
#[case("Smith, John", false, Some(2))] // white; spaces count here
#[case("Smith,John", false, None)] // strContains does not ignore spaces
#[case("Karpov", false, Some(1))] // black
#[case("1985.12", false, Some(3))] // date as yyyy.mm.dd
#[case("2000.??.??", false, Some(2))] // unknown parts print as ?
#[case("Zorro", false, None)] // round is not examined
#[case("kasparov", false, Some(3))] // case sensitive: only the lowercase one
#[case("kasparov", true, Some(1))] // ignoring case: first hit is game 1
#[case("ZZZ", false, Some(4))]
#[case("nothing at all", true, None)]
fn textfind_from_zero_searches_white_black_event_site_and_date(
    #[case] text: &str,
    #[case] ignore_case: bool,
    #[case] expect: Option<u32>,
) {
    let fx = text_db();
    let db = fx.open();
    let f = Filter::new(db.len());
    assert_eq!(f.textfind(&db, text, ignore_case, 0), expect, "{text}");
}

#[test]
fn textfind_from_zero_examines_game_one() {
    // The Scid defect skipped game 1 (rule 13); MyChess must not (D343).
    let fx = text_db();
    let db = fx.open();
    let f = Filter::new(db.len());
    assert_eq!(f.textfind(&db, "Garry", false, 0), Some(1));
}

#[test]
fn textfind_continues_after_the_given_filter_position() {
    let fx = text_db();
    let db = fx.open();
    let f = Filter::new(db.len());
    assert_eq!(f.textfind(&db, "Doe", false, 0), Some(2));
    assert_eq!(f.textfind(&db, "Doe", false, 2), Some(3));
    assert_eq!(f.textfind(&db, "Doe", false, 3), None);
    assert_eq!(f.textfind(&db, "Doe", false, 99), None);
}

#[test]
fn textfind_returns_a_position_in_the_filter_not_a_game_number() {
    let fx = text_db();
    let db = fx.open();
    let mut f = Filter::new(db.len());
    f.set(1, 0);
    f.set(2, 0);
    // games 3 and 4 remain: game 4 is position 2
    assert_eq!(f.textfind(&db, "ZZZ", false, 0), Some(2));
    assert_eq!(
        f.textfind(&db, "Smith", false, 0),
        None,
        "out of the filter"
    );
}

#[test]
fn textfilter_removes_non_matching_games_keeps_their_ply_and_counts() {
    let fx = text_db();
    let db = fx.open();
    let mut f = Filter::new(db.len());
    f.set(3, 42);
    let removed = f.textfilter(&db, "Doe", false);
    assert_eq!(removed, 2);
    assert_eq!(f.count(), 2);
    assert_eq!((f.get(2), f.get(3)), (1, 42));
    assert_eq!((f.get(1), f.get(4)), (0, 0));
}

#[test]
fn textfilter_counts_only_games_that_were_in_the_filter() {
    let fx = text_db();
    let db = fx.open();
    let mut f = Filter::new(db.len());
    f.set(4, 0);
    assert_eq!(f.textfilter(&db, "Doe", false), 1);
}

#[test]
fn textfilter_ignores_round_and_respects_the_case_flag() {
    let fx = text_db();
    let db = fx.open();
    let mut f = Filter::new(db.len());
    assert_eq!(f.textfilter(&db, "Zorro", true), 4);
    assert_eq!(f.count(), 0);
    let mut f = Filter::new(db.len());
    assert_eq!(f.textfilter(&db, "KASPAROV", true), 2);
    assert_eq!((f.get(1), f.get(3)), (1, 1));
}

#[test]
fn textfilter_with_empty_text_removes_nothing() {
    let fx = text_db();
    let db = fx.open();
    let mut f = Filter::new(db.len());
    assert_eq!(f.textfilter(&db, "", false), 0);
    assert_eq!(f.count(), 4);
}

// ------------------------------------------------------------ progress

#[test]
fn cancel_token_clones_share_the_flag_and_can_be_reset() {
    let t = CancelToken::new();
    let c = t.clone();
    assert!(!c.is_cancelled());
    t.cancel();
    assert!(c.is_cancelled());
    c.reset();
    assert!(!t.is_cancelled());
}

#[test]
fn closures_and_no_progress_are_progress_sinks() {
    let mut seen: Vec<(u64, u64)> = Vec::new();
    {
        let mut sink = |d: u64, t: u64| seen.push((d, t));
        ProgressSink::progress(&mut sink, 3, 10);
    }
    assert_eq!(seen, [(3, 10)]);
    NoProgress.progress(1, 2);
}
