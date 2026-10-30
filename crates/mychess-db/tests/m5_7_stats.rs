//! Contract tests of the ECO statistics (M5-7a, spec
//! `docs/specs/m5-7-eco-e-novidade.md`, rule 2 and D1181 to D1185, D1197,
//! D1198). Expected values come from the spec formulas and Scid's
//! `sc_base_ecoStats`, never from the implementation.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::assert_is_empty,
    clippy::cast_possible_truncation
)]

#[path = "common/m5_7_support.rs"]
mod support;

use mychess_core::{Eco, GameResult};
use mychess_db::{EcoScope, EcoStats, EcoStatsTable, Filter, bar_scale};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;
use support::{Spec, open, spec, write_db};

const W: GameResult = GameResult::White;
const BL: GameResult = GameResult::Black;
const D: GameResult = GameResult::Draw;
const NONE: GameResult = GameResult::Unknown;

fn table(specs: &[Spec]) -> EcoStatsTable {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), specs);
    EcoStatsTable::build(&open(&base), EcoScope::WholeBase)
}

fn s(count: u32, white: u32, draw: u32, black: u32, none: u32) -> EcoStats {
    EcoStats {
        count,
        white,
        draw,
        black,
        none,
    }
}

fn count_of(t: &EcoStatsTable, prefix: &str) -> u32 {
    t.get(prefix).unwrap().count
}

// ------------------------------------------------------------ rule 2

#[test]
fn spec_example_counts_per_prefix_and_ignores_games_without_eco() {
    // Arrange: {A00a, B90a, B90a2, B91, no ECO}
    let t = table(&[
        spec(&["e4"]).eco("A00a"),
        spec(&["e4"]).eco("B90a"),
        spec(&["e4"]).eco("B90a2"),
        spec(&["e4"]).eco("B91"),
        spec(&["e4"]),
    ]);
    // Assert
    assert_eq!(count_of(&t, ""), 4);
    assert_eq!(count_of(&t, "A"), 1);
    assert_eq!(count_of(&t, "A0"), 1);
    assert_eq!(count_of(&t, "A00"), 1);
    assert_eq!(count_of(&t, "A00a"), 1);
    assert_eq!(count_of(&t, "B"), 3);
    assert_eq!(count_of(&t, "B9"), 3);
    assert_eq!(count_of(&t, "B90"), 2);
    assert_eq!(count_of(&t, "B90a"), 2);
    assert_eq!(count_of(&t, "B90a2"), 2, "same node as B90a");
    assert_eq!(count_of(&t, "B91"), 1);
    assert_eq!(
        count_of(&t, "B91a"),
        0,
        "B91 without extension has no letter node"
    );
    assert_eq!(count_of(&t, "C"), 0);
    assert_eq!(count_of(&t, "E99z"), 0);
}

#[test]
fn extension_digit_is_folded_into_the_letter_node() {
    let t = table(&[
        spec(&["e4"]).eco("B90a"),
        spec(&["e4"]).eco("B90a1"),
        spec(&["e4"]).eco("B90a4"),
        spec(&["e4"]).eco("B90b"),
    ]);
    assert_eq!(count_of(&t, "B90a"), 3);
    assert_eq!(count_of(&t, "B90b"), 1);
    assert_eq!(count_of(&t, "B90"), 4);
}

#[test]
fn extreme_codes_are_counted_in_the_last_nodes() {
    let t = table(&[spec(&["e4"]).eco("A00"), spec(&["e4"]).eco("E99z4")]);
    assert_eq!(count_of(&t, "A00"), 1);
    assert_eq!(count_of(&t, "E99z"), 1);
    assert_eq!(count_of(&t, "E99"), 1);
    assert_eq!(count_of(&t, "E"), 1);
    assert_eq!(count_of(&t, ""), 2);
}

#[test]
fn deleted_games_are_counted() {
    let t = table(&[
        spec(&["e4"]).eco("C20").result(W),
        spec(&["e4"]).eco("C20").result(BL).deleted(),
    ]);
    assert_eq!(t.get("C20").unwrap(), s(2, 1, 0, 1, 0));
}

#[test]
fn results_are_split_in_the_four_counters() {
    let t = table(&[
        spec(&["e4"]).eco("C20").result(W),
        spec(&["e4"]).eco("C20").result(W),
        spec(&["e4"]).eco("C20").result(D),
        spec(&["e4"]).eco("C20").result(BL),
        spec(&["e4"]).eco("C20").result(NONE),
        spec(&["e4"]).eco("C21").result(BL),
    ]);
    assert_eq!(t.get("C20").unwrap(), s(5, 2, 1, 1, 1));
    assert_eq!(t.get("C2").unwrap(), s(6, 2, 1, 2, 1));
    assert_eq!(t.get("").unwrap(), s(6, 2, 1, 2, 1));
}

#[test]
fn an_empty_database_gives_zero_everywhere() {
    let t = table(&[]);
    assert_eq!(t.get("").unwrap(), EcoStats::default());
    assert_eq!(t.unreadable(), 0);
    assert_eq!(
        EcoStatsTable::default().get("A00a").unwrap(),
        EcoStats::default()
    );
}

#[test]
fn filter_scope_counts_only_games_in_the_filter() {
    // Arrange: games 1 and 3 are in the filter.
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(
        dir.path(),
        &[
            spec(&["e4"]).eco("C20").result(W),
            spec(&["e4"]).eco("C20").result(BL),
            spec(&["e4"]).eco("B01").result(D),
            spec(&["e4"]).eco("B01").result(W).deleted(),
        ],
    );
    let db = open(&base);
    let mut filter = Filter::new(4);
    filter.set(2, 0);
    filter.set(4, 0);
    // Act
    let t = EcoStatsTable::build(&db, EcoScope::Filter(&filter));
    let whole = EcoStatsTable::build(&db, EcoScope::WholeBase);
    // Assert
    assert_eq!(t.get("C20").unwrap(), s(1, 1, 0, 0, 0));
    assert_eq!(t.get("B01").unwrap(), s(1, 0, 1, 0, 0));
    assert_eq!(t.get("").unwrap().count, 2);
    assert_eq!(whole.get("").unwrap().count, 4);
}

#[test]
fn filter_scope_with_an_empty_filter_counts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(
        dir.path(),
        &[spec(&["e4"]).eco("C20"), spec(&["d4"]).eco("A45")],
    );
    let mut filter = Filter::new(2);
    filter.fill(0);
    let t = EcoStatsTable::build(&open(&base), EcoScope::Filter(&filter));
    assert_eq!(t.get("").unwrap().count, 0);
}

// ------------------------------------------------------------ score

#[rstest]
#[case::two_wins_one_draw(s(3, 2, 1, 0, 0), 833)]
#[case::two_wins_one_loss(s(3, 2, 0, 1, 0), 666)]
#[case::empty(s(0, 0, 0, 0, 0), 0)]
#[case::single_unknown_is_half_a_point(s(1, 0, 0, 0, 1), 500)]
#[case::all_wins(s(4, 4, 0, 0, 0), 1000)]
#[case::all_losses(s(4, 0, 0, 4, 0), 0)]
#[case::one_win_two_losses_truncates(s(3, 1, 0, 2, 0), 333)]
#[case::draws_only(s(7, 0, 7, 0, 0), 500)]
#[case::mixed_with_unknown(s(4, 1, 1, 1, 1), 500)]
fn score_tenths_follows_the_scid_formula(#[case] stats: EcoStats, #[case] expected: u32) {
    assert_eq!(stats.score_tenths(), expected);
}

#[test]
fn score_tenths_does_not_overflow_with_huge_counts() {
    let big = s(u32::MAX, u32::MAX, 0, 0, 0);
    assert_eq!(big.score_tenths(), 1000);
}

#[test]
fn score_of_a_table_node_uses_the_table_counters() {
    let t = table(&[
        spec(&["e4"]).eco("C20").result(W),
        spec(&["e4"]).eco("C20").result(W),
        spec(&["e4"]).eco("C20").result(D),
    ]);
    assert_eq!(t.get("C20").unwrap().score_tenths(), 833);
}

// ------------------------------------------------------------ prefixes

#[rstest]
#[case("")]
#[case("A")]
#[case("E")]
#[case("A0")]
#[case("E99")]
#[case("A00a")]
#[case("E99z")]
#[case("A00a1")]
#[case::fifth_char_is_any("A00aX")]
#[case::fifth_char_is_any_punct("A00a?")]
fn get_accepts_valid_prefixes(#[case] prefix: &str) {
    let t = EcoStatsTable::default();
    assert!(t.get(prefix).is_ok(), "{prefix:?}");
    assert!(t.children(prefix).is_ok(), "{prefix:?}");
}

#[rstest]
#[case::too_long("A00a12")]
#[case::far_too_long("A00a1234567890")]
#[case::lowercase_letter("a")]
#[case::lowercase_full("a00")]
#[case::letter_f("F")]
#[case::letter_z("Z00")]
#[case::digit_first("0")]
#[case::second_not_digit("AB0")]
#[case::second_not_digit_single("AB")]
#[case::third_not_digit("A0x")]
#[case::fourth_digit("A001")]
#[case::fourth_uppercase("A00A")]
#[case::fourth_punct("A00!")]
#[case::space("A 0")]
#[case::multibyte_third("A0\u{e9}")]
#[case::multibyte_fourth("A00\u{e9}")]
#[case::multibyte_first("\u{c9}00")]
fn get_rejects_invalid_prefixes(#[case] prefix: &str) {
    let t = EcoStatsTable::default();
    assert!(t.get(prefix).is_err(), "get {prefix:?}");
    assert!(t.children(prefix).is_err(), "children {prefix:?}");
}

#[test]
fn length_counts_bytes_not_characters() {
    // 2 bytes each: 3 chars but 6 bytes -> too long, and invalid anyway.
    let t = EcoStatsTable::default();
    assert!(t.get("\u{e9}\u{e9}\u{e9}").is_err());
}

// ------------------------------------------------------------ children

#[test]
fn children_of_the_empty_prefix_are_the_five_letters() {
    let t = table(&[
        spec(&["e4"]).eco("A45"),
        spec(&["e4"]).eco("C20"),
        spec(&["e4"]).eco("C21"),
    ]);
    let kids = t.children("").unwrap();
    let names: Vec<&str> = kids.iter().map(|c| c.prefix.as_str()).collect();
    assert_eq!(names, ["A", "B", "C", "D", "E"]);
    assert_eq!(kids[0].stats.count, 1);
    assert_eq!(kids[1].stats.count, 0);
    assert_eq!(kids[2].stats.count, 2);
}

#[test]
fn children_of_letter_and_letter_digit_are_ten_digits() {
    let t = table(&[spec(&["e4"]).eco("B90"), spec(&["e4"]).eco("B95")]);
    let b = t.children("B").unwrap();
    let names: Vec<String> = b.iter().map(|c| c.prefix.clone()).collect();
    assert_eq!(
        names,
        ["B0", "B1", "B2", "B3", "B4", "B5", "B6", "B7", "B8", "B9"]
    );
    assert_eq!(b[9].stats.count, 2);
    let b9 = t.children("B9").unwrap();
    assert_eq!(b9.len(), 10);
    assert_eq!(b9[0].prefix, "B90");
    assert_eq!(b9[0].stats.count, 1);
    assert_eq!(b9[5].prefix, "B95");
    assert_eq!(b9[5].stats.count, 1);
}

#[test]
fn children_of_a_basic_code_are_the_26_subcode_letters() {
    let t = table(&[
        spec(&["e4"]).eco("B90a"),
        spec(&["e4"]).eco("B90a3"),
        spec(&["e4"]).eco("B90z4"),
        spec(&["e4"]).eco("B90"),
    ]);
    let kids = t.children("B90").unwrap();
    assert_eq!(kids.len(), 26);
    assert_eq!(kids[0].prefix, "B90a");
    assert_eq!(kids[0].stats.count, 2);
    assert_eq!(kids[25].prefix, "B90z");
    assert_eq!(kids[25].stats.count, 1);
    // The unextended B90 is in the parent but in no child.
    assert_eq!(t.get("B90").unwrap().count, 4);
    assert_eq!(kids.iter().map(|c| c.stats.count).sum::<u32>(), 3);
}

#[rstest]
#[case("B90a")]
#[case("B90a2")]
#[case("B90aX")]
fn children_at_four_or_more_characters_are_empty(#[case] prefix: &str) {
    assert!(
        EcoStatsTable::default()
            .children(prefix)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn each_child_has_the_stats_of_get_on_its_prefix() {
    let t = table(&[
        spec(&["e4"]).eco("C20").result(W),
        spec(&["e4"]).eco("C25").result(D),
        spec(&["e4"]).eco("D30").result(BL),
    ]);
    for parent in ["", "C", "C2", "D3"] {
        for child in t.children(parent).unwrap() {
            assert_eq!(
                child.stats,
                t.get(&child.prefix).unwrap(),
                "{}",
                child.prefix
            );
            assert!(child.prefix.starts_with(parent));
        }
    }
}

// ------------------------------------------------------------ bar_scale

#[rstest]
#[case(0, 5)]
#[case(1, 5)]
#[case(20, 5)]
#[case(21, 10)]
#[case(50, 10)]
#[case(51, 25)]
#[case(100, 25)]
#[case(101, 50)]
#[case(200, 50)]
#[case(201, 100)]
#[case(500, 100)]
#[case(501, 250)]
#[case(1000, 250)]
#[case(1001, 500)]
#[case(2000, 500)]
#[case(2001, 1000)]
#[case(5000, 1000)]
#[case(5001, 2500)]
#[case(10_000, 2500)]
#[case(10_001, 5000)]
#[case(20_000, 5000)]
#[case(20_001, 10_000)]
#[case(50_000, 10_000)]
#[case(50_001, 25_000)]
#[case(100_000, 25_000)]
#[case(100_001, 50_000)]
#[case(u32::MAX, 50_000)]
fn bar_scale_follows_the_grid_steps(#[case] max: u32, #[case] step: u32) {
    assert_eq!(bar_scale(max), step);
}

// ------------------------------------------------------------ apply_change

fn eco_of(code: u16) -> Option<Eco> {
    Eco::from_scid_code(code)
}

fn all_prefixes_of(codes: &[u16]) -> Vec<String> {
    let mut out = vec![String::new()];
    for &c in codes {
        let e = eco_of(c).unwrap();
        let text = e.to_string();
        for end in [1, 2, 3, 4] {
            if end <= text.len() {
                out.push(text[..end].to_owned());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn apply_change_moves_a_game_between_nodes_like_a_rebuild() {
    // Arrange
    let before = table(&[
        spec(&["e4"]).eco("C20").result(W),
        spec(&["e4"]).eco("C20").result(D),
    ]);
    let after = table(&[
        spec(&["e4"]).eco("C20").result(W),
        spec(&["e4"]).eco("B90a").result(D),
    ]);
    let mut t = before.clone();
    // Act
    t.apply_change(Some(support::eco("C20")), Some(support::eco("B90a")), D);
    // Assert
    for p in ["", "B", "B9", "B90", "B90a", "C", "C2", "C20"] {
        assert_eq!(t.get(p).unwrap(), after.get(p).unwrap(), "{p}");
    }
}

#[test]
fn apply_change_from_none_adds_and_to_none_removes() {
    let mut t = EcoStatsTable::default();
    t.apply_change(None, Some(support::eco("C20b")), W);
    assert_eq!(t.get("").unwrap(), s(1, 1, 0, 0, 0));
    assert_eq!(t.get("C20b").unwrap().count, 1);
    t.apply_change(Some(support::eco("C20b")), None, W);
    assert_eq!(t.get("").unwrap(), EcoStats::default());
    assert_eq!(t.get("C20b").unwrap(), EcoStats::default());
}

#[test]
fn apply_change_with_equal_codes_or_two_nones_is_a_no_op() {
    let mut t = table(&[spec(&["e4"]).eco("C20").result(W)]);
    let snapshot = t.get("").unwrap();
    t.apply_change(Some(support::eco("C20")), Some(support::eco("C20")), W);
    t.apply_change(None, None, W);
    assert_eq!(t.get("").unwrap(), snapshot);
    assert_eq!(t.get("C20").unwrap(), snapshot);
}

#[test]
fn apply_change_removing_an_absent_code_never_underflows() {
    let mut t = EcoStatsTable::default();
    t.apply_change(Some(support::eco("C20")), None, W);
    assert_eq!(t.get("").unwrap(), EcoStats::default());
    assert_eq!(t.get("C20").unwrap(), EcoStats::default());
}

#[test]
fn apply_change_matches_the_stats_after_set_eco_on_a_real_database() {
    // Arrange: a database, its table, then real set_eco calls.
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(
        dir.path(),
        &[
            spec(&["e4"]).eco("C20").result(W),
            spec(&["e4"]).result(D),
            spec(&["e4"]).eco("B90a2").result(BL),
            spec(&["e4"]).eco("B90").result(NONE).deleted(),
        ],
    );
    let mut t = EcoStatsTable::build(&open(&base), EcoScope::WholeBase);
    let results = [W, D, BL, NONE];
    let changes = [
        (1u32, Some(support::eco("C21"))),
        (2, Some(support::eco("E99z4"))),
        (3, None),
        (4, Some(support::eco("B90b"))),
    ];
    // Act
    {
        let mut writer = mychess_db::si4::Si4Writer::open(&base).unwrap();
        for (n, new) in changes {
            let old = open(&base).entry(n).unwrap().eco();
            writer.set_eco(n, new).unwrap();
            t.apply_change(old, new, results[usize::try_from(n).unwrap() - 1]);
        }
    }
    // Assert: same as a rebuild, at every node touched.
    let rebuilt = EcoStatsTable::build(&open(&base), EcoScope::WholeBase);
    for p in [
        "", "A", "B", "C", "E", "B9", "C2", "E9", "B90", "C21", "E99", "B90b", "E99z", "B90a",
    ] {
        assert_eq!(t.get(p).unwrap(), rebuilt.get(p).unwrap(), "{p}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn apply_change_sequence_equals_a_rebuild(
        initial in proptest::collection::vec((0u16..=65_500, 0usize..4), 1..8),
        edits in proptest::collection::vec((0usize..8, 0u16..=65_500), 0..10),
    ) {
        // Arrange: games with random codes (0 = none) and results.
        let results = [W, BL, D, NONE];
        let specs: Vec<Spec> = initial
            .iter()
            .map(|&(c, r)| {
                let mut sp = spec(&["e4"]).result(results[r]);
                sp.eco = eco_of(c);
                sp
            })
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let base = write_db(dir.path(), &specs);
        let mut t = EcoStatsTable::build(&open(&base), EcoScope::WholeBase);
        let mut current: Vec<u16> = initial.iter().map(|&(c, _)| c).collect();
        // Act
        for &(idx, new) in &edits {
            let i = idx % current.len();
            t.apply_change(eco_of(current[i]), eco_of(new), results[initial[i].1]);
            current[i] = new;
        }
        // Assert
        let final_specs: Vec<Spec> = initial
            .iter()
            .zip(&current)
            .map(|(&(_, r), &c)| {
                let mut sp = spec(&["e4"]).result(results[r]);
                sp.eco = eco_of(c);
                sp
            })
            .collect();
        let dir2 = tempfile::tempdir().unwrap();
        let base2 = write_db(dir2.path(), &final_specs);
        let rebuilt = EcoStatsTable::build(&open(&base2), EcoScope::WholeBase);
        let mut all: Vec<u16> = initial.iter().map(|&(c, _)| c).collect();
        all.extend(current.iter().copied());
        all.retain(|&c| c != 0);
        for p in all_prefixes_of(&all) {
            prop_assert_eq!(t.get(&p).unwrap(), rebuilt.get(&p).unwrap(), "prefix {}", p);
        }
    }

    #[test]
    fn counts_at_each_level_are_sums_of_the_children(
        codes in proptest::collection::vec(1u16..=65_500, 0..12),
    ) {
        let mut t = EcoStatsTable::default();
        for &c in &codes {
            t.apply_change(None, eco_of(c), W);
        }
        prop_assert_eq!(t.get("").unwrap().count as usize, codes.len());
        for parent in ["", "A", "B", "C", "D", "E", "A0", "C2", "E9"] {
            let kids = t.children(parent).unwrap();
            let sum: u32 = kids.iter().map(|k| k.stats.count).sum();
            prop_assert_eq!(sum, t.get(parent).unwrap().count, "parent {}", parent);
        }
    }
}
