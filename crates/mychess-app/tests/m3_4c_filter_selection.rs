//! Contract tests of the filter actions and the multiple selection of the
//! game list (spec M3-4, slice 4c: rules 9 and 10, D324, D557, D561; Scid
//! `gamelist.tcl` `Select` 676, `Remove` 913, `removeFromFilter` 1338,
//! `tkscid.cpp` `sc_filter negate` 5567, `remove` 5635, `reset` 5665).
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4c_common;

use std::collections::BTreeSet;

use m3_4c_common::*;
use proptest::prelude::*;

fn setup(n: usize) -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "s.pgn", &numbered(n));
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), n as u32);
    (rig, slot, dir)
}

// ---- Reset / Negate ----

#[test]
fn reset_puts_every_game_back_in_the_filter() {
    let (mut rig, slot, _d) = setup(6);
    rig.select_all(slot, &[2, 3, 5]);
    rig.send(l(ListMessage::KeepSelected { slot }));
    assert_eq!(rig.filter_numbers(slot), [2, 3, 5]);
    rig.send(l(ListMessage::ResetFilter { slot }));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5, 6]);
    assert_eq!(rig.state.bases.slot(slot).expect("slot").filtered(), 6);
}

#[test]
fn reset_keeps_the_selection_because_all_games_are_in_again() {
    let (mut rig, slot, _d) = setup(4);
    rig.select_all(slot, &[1, 2]);
    rig.send(l(ListMessage::ResetFilter { slot }));
    assert_eq!(rig.selection(slot), [1, 2]);
}

#[test]
fn negate_swaps_the_games_in_and_out() {
    let (mut rig, slot, _d) = setup(6);
    rig.select_all(slot, &[2, 3, 5]);
    rig.send(l(ListMessage::KeepSelected { slot }));
    rig.send(l(ListMessage::NegateFilter { slot }));
    assert_eq!(rig.filter_numbers(slot), [1, 4, 6]);
    rig.send(l(ListMessage::NegateFilter { slot }));
    assert_eq!(
        rig.filter_numbers(slot),
        [2, 3, 5],
        "negating twice restores"
    );
}

#[test]
fn negating_an_empty_filter_lists_every_game_and_negating_a_full_one_none() {
    let (mut rig, slot, _d) = setup(3);
    rig.send(l(ListMessage::NegateFilter { slot }));
    assert_eq!(rig.filter_numbers(slot), Vec::<u32>::new());
    assert_eq!(rig.view(slot), Vec::<u32>::new());
    rig.send(l(ListMessage::NegateFilter { slot }));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3]);
}

#[test]
fn negate_gives_the_entered_games_ply_one() {
    // D324: the ply of the filter is lost on negate (Scid sets 1).
    let (mut rig, slot, _d) = setup(4);
    rig.select_all(slot, &[1]);
    rig.send(l(ListMessage::KeepSelected { slot }));
    rig.send(l(ListMessage::NegateFilter { slot }));
    let filter = rig.state.bases.slot(slot).expect("slot").filter();
    assert_eq!((filter.get(1), filter.get(2), filter.get(3)), (0, 1, 1));
}

#[test]
fn negate_removes_selected_games_that_left_the_filter_from_the_selection() {
    let (mut rig, slot, _d) = setup(4);
    rig.select_all(slot, &[1, 2]);
    rig.send(l(ListMessage::KeepSelected { slot }));
    rig.send(l(ListMessage::NegateFilter { slot }));
    assert_eq!(rig.filter_numbers(slot), [3, 4]);
    assert_eq!(rig.selection(slot), Vec::<u32>::new());
    assert_eq!(rig.anchor(slot), None, "the anchor left the filter");
}

#[test]
fn negate_puts_the_scroll_position_back_in_range() {
    let (mut rig, slot, _d) = setup(10);
    rig.send(BasesMessage::SetListFirst { slot, first: 9 });
    rig.select_all(slot, &[1]);
    rig.send(l(ListMessage::KeepSelected { slot }));
    assert_eq!(rig.first(slot), 1);
    rig.send(l(ListMessage::NegateFilter { slot })); // 9 games
    rig.send(BasesMessage::SetListFirst { slot, first: 9 });
    rig.send(l(ListMessage::NegateFilter { slot })); // 1 game
    assert_eq!(rig.first(slot), 1, "clamped to [1, max(1, filtered)]");
}

// ---- Remove ----

#[test]
fn remove_selected_takes_only_the_selection_out_of_the_filter() {
    let (mut rig, slot, _d) = setup(6);
    rig.select_all(slot, &[2, 4, 5]);
    rig.send(l(ListMessage::RemoveSelected { slot }));
    assert_eq!(rig.filter_numbers(slot), [1, 3, 6]);
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 6);
    assert_eq!(rig.selection(slot), Vec::<u32>::new());
    assert_eq!(rig.anchor(slot), None);
}

#[test]
fn remove_selected_with_nothing_selected_changes_nothing() {
    let (mut rig, slot, _d) = setup(4);
    rig.send(l(ListMessage::RemoveSelected { slot }));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4]);
}

#[test]
fn remove_above_and_below_cut_the_view_around_a_game() {
    let (mut rig, slot, _d) = setup(7);
    rig.send(l(ListMessage::RemoveAbove { slot, number: 3 }));
    assert_eq!(rig.filter_numbers(slot), [3, 4, 5, 6, 7]);
    rig.send(l(ListMessage::RemoveBelow { slot, number: 6 }));
    assert_eq!(rig.filter_numbers(slot), [3, 4, 5, 6]);
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 7);
}

#[test]
fn remove_above_the_first_and_below_the_last_change_nothing() {
    let (mut rig, slot, _d) = setup(4);
    rig.send(l(ListMessage::RemoveAbove { slot, number: 1 }));
    rig.send(l(ListMessage::RemoveBelow { slot, number: 4 }));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4]);
}

#[test]
fn remove_above_a_game_outside_the_filter_or_out_of_range_is_ignored() {
    let (mut rig, slot, _d) = setup(5);
    rig.select_all(slot, &[3]);
    rig.send(l(ListMessage::RemoveSelected { slot }));
    for number in [3, 0, 99, u32::MAX] {
        rig.send(l(ListMessage::RemoveAbove { slot, number }));
        rig.send(l(ListMessage::RemoveBelow { slot, number }));
    }
    assert_eq!(rig.filter_numbers(slot), [1, 2, 4, 5]);
}

#[test]
fn remove_above_and_below_follow_the_sorted_view_not_the_database_order() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    // View by White ascending: C(2) D(4) A(1) B(3) -> names are
    // 1:"Delta", 2:"Alpha", 3:"Echo", 4:"Bravo".
    let games = vec![
        G::new("Delta", "x"),
        G::new("Alpha", "x"),
        G::new("Echo", "x"),
        G::new("Bravo", "x"),
    ];
    let slot = rig.open_games(dir.path(), "s.pgn", &games);
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    assert_eq!(rig.view(slot), [2, 4, 1, 3]);
    // Remove everything above Delta (game 1) in the view: Alpha and Bravo.
    rig.send(l(ListMessage::RemoveAbove { slot, number: 1 }));
    assert_eq!(rig.filter_numbers(slot), [1, 3]);
    assert_eq!(rig.view(slot), [1, 3]);
    rig.send(l(ListMessage::ResetFilter { slot }));
    // Remove everything below Bravo (game 4): Delta and Echo.
    rig.send(l(ListMessage::RemoveBelow { slot, number: 4 }));
    assert_eq!(rig.filter_numbers(slot), [2, 4]);
}

// ---- Set Filter ----

#[test]
fn keep_selected_leaves_only_the_selected_games() {
    let (mut rig, slot, _d) = setup(6);
    rig.select_all(slot, &[2, 5]);
    rig.send(BasesMessage::SetListFirst { slot, first: 4 });
    rig.send(l(ListMessage::KeepSelected { slot }));
    assert_eq!(rig.filter_numbers(slot), [2, 5]);
    assert_eq!(rig.selection(slot), [2, 5], "the selection stays");
    assert_eq!(rig.first(slot), 1, "the list goes back to the top");
    assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), 6);
}

#[test]
fn keep_selected_with_nothing_selected_says_so_and_keeps_the_filter() {
    let (mut rig, slot, _d) = setup(3);
    rig.send(l(ListMessage::KeepSelected { slot }));
    assert_eq!(rig.notice(), Some(&BasesNotice::NothingSelected));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3]);
}

#[test]
fn keep_selected_after_a_sort_still_leaves_exactly_the_selected_games() {
    let (mut rig, slot, _d) = setup(5);
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    rig.select_all(slot, &[5, 1]);
    rig.send(l(ListMessage::KeepSelected { slot }));
    assert_eq!(rig.filter_numbers(slot), [1, 5]);
    assert_eq!(rig.view(slot), [5, 1], "descending view of the two");
}

// ---- isolation ----

#[test]
fn filter_actions_of_one_slot_never_change_another() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &numbered(5));
    let b = rig.open_games(dir.path(), "b.pgn", &numbered(5));
    assert_ne!(a, b);
    rig.select_all(a, &[1, 2]);
    rig.send(l(ListMessage::RemoveSelected { slot: a }));
    rig.send(l(ListMessage::NegateFilter { slot: a }));
    rig.select_all(b, &[3]);
    assert_eq!(rig.filter_numbers(b), [1, 2, 3, 4, 5]);
    rig.send(l(ListMessage::RemoveAbove { slot: a, number: 2 }));
    assert_eq!(rig.filter_numbers(b), [1, 2, 3, 4, 5]);
    assert_eq!(rig.selection(b), [3], "the selection of b is its own");
    assert_eq!(rig.selection(a), Vec::<u32>::new());
}

// ---- selection ----

#[test]
fn a_plain_click_selects_only_that_game_and_anchors_on_it() {
    let (mut rig, slot, _d) = setup(5);
    rig.pick(slot, 2, PickMode::Only);
    rig.pick(slot, 4, PickMode::Only);
    assert_eq!(rig.selection(slot), [4]);
    assert_eq!(rig.anchor(slot), Some(4));
}

#[test]
fn ctrl_click_toggles_a_game_in_and_out() {
    let (mut rig, slot, _d) = setup(5);
    rig.pick(slot, 1, PickMode::Only);
    rig.pick(slot, 3, PickMode::Toggle);
    rig.pick(slot, 5, PickMode::Toggle);
    assert_eq!(rig.selection(slot), [1, 3, 5]);
    rig.pick(slot, 3, PickMode::Toggle);
    assert_eq!(rig.selection(slot), [1, 5]);
    let anchor = rig.anchor(slot).expect("anchor");
    assert!(
        rig.selection(slot).contains(&anchor),
        "anchor is always selected"
    );
}

#[test]
fn toggling_off_the_last_game_leaves_an_empty_selection_without_an_anchor() {
    let (mut rig, slot, _d) = setup(3);
    rig.pick(slot, 2, PickMode::Only);
    rig.pick(slot, 2, PickMode::Toggle);
    assert_eq!(rig.selection(slot), Vec::<u32>::new());
    assert_eq!(rig.anchor(slot), None);
}

#[test]
fn shift_click_selects_the_range_from_the_anchor_and_keeps_the_anchor() {
    let (mut rig, slot, _d) = setup(8);
    rig.pick(slot, 3, PickMode::Only);
    rig.pick(slot, 6, PickMode::Range);
    assert_eq!(rig.selection(slot), [3, 4, 5, 6]);
    assert_eq!(rig.anchor(slot), Some(3));
    // A second Shift+click reshapes the range around the same anchor.
    rig.pick(slot, 1, PickMode::Range);
    assert_eq!(rig.selection(slot), [1, 2, 3]);
    rig.pick(slot, 3, PickMode::Range);
    assert_eq!(rig.selection(slot), [3]);
}

#[test]
fn shift_click_without_an_anchor_selects_only_the_clicked_game() {
    let (mut rig, slot, _d) = setup(5);
    rig.pick(slot, 4, PickMode::Range);
    assert_eq!(rig.selection(slot), [4]);
}

#[test]
fn shift_click_ranges_over_the_view_order_of_a_sorted_list() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let games = vec![
        G::new("Delta", "x"),
        G::new("Alpha", "x"),
        G::new("Echo", "x"),
        G::new("Bravo", "x"),
        G::new("Charlie", "x"),
    ];
    let slot = rig.open_games(dir.path(), "s.pgn", &games);
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    // View: Alpha(2) Bravo(4) Charlie(5) Delta(1) Echo(3).
    assert_eq!(rig.view(slot), [2, 4, 5, 1, 3]);
    rig.pick(slot, 4, PickMode::Only);
    rig.pick(slot, 1, PickMode::Range);
    assert_eq!(rig.selection(slot), [1, 4, 5], "Bravo..Delta in view order");
}

#[test]
fn shift_click_in_a_partially_filtered_list_only_covers_listed_games() {
    let (mut rig, slot, _d) = setup(7);
    rig.select_all(slot, &[3, 4]);
    rig.send(l(ListMessage::RemoveSelected { slot }));
    rig.pick(slot, 2, PickMode::Only);
    rig.pick(slot, 5, PickMode::Range);
    assert_eq!(rig.selection(slot), [2, 5], "3 and 4 are not listed");
}

#[test]
fn clicks_on_games_outside_the_filter_or_the_database_are_ignored() {
    let (mut rig, slot, _d) = setup(4);
    rig.select_all(slot, &[2]);
    rig.send(l(ListMessage::RemoveSelected { slot }));
    rig.pick(slot, 1, PickMode::Only);
    for number in [2, 0, 5, u32::MAX] {
        for mode in [PickMode::Only, PickMode::Toggle, PickMode::Range] {
            rig.pick(slot, number, mode);
        }
    }
    assert_eq!(rig.selection(slot), [1]);
}

#[test]
fn a_click_on_an_empty_or_unknown_slot_does_not_panic() {
    let (mut rig, _slot, _d) = setup(2);
    for n in [1, 5, 8] {
        rig.pick(sel(n), 1, PickMode::Only);
    }
    rig.select_all(sel(7), &[1]);
}

#[test]
fn select_numbers_replaces_the_selection_and_drops_invalid_numbers() {
    let (mut rig, slot, _d) = setup(5);
    rig.select_all(slot, &[1]);
    rig.select_all(slot, &[2, 0, 99, 4, 2, u32::MAX]);
    assert_eq!(rig.selection(slot), [2, 4]);
    let anchor = rig.anchor(slot).expect("anchor");
    assert!(rig.selection(slot).contains(&anchor));
}

#[test]
fn select_numbers_leaves_out_games_that_are_not_in_the_filter() {
    let (mut rig, slot, _d) = setup(5);
    rig.select_all(slot, &[1]);
    rig.send(l(ListMessage::RemoveSelected { slot }));
    rig.select_all(slot, &[1, 2, 3]);
    assert_eq!(rig.selection(slot), [2, 3]);
}

#[test]
fn select_numbers_with_an_empty_list_clears_the_selection() {
    let (mut rig, slot, _d) = setup(3);
    rig.select_all(slot, &[1, 2]);
    rig.select_all(slot, &[]);
    assert_eq!(rig.selection(slot), Vec::<u32>::new());
    assert_eq!(rig.anchor(slot), None);
}

#[test]
fn select_numbers_works_on_a_sorted_view() {
    let (mut rig, slot, _d) = setup(5);
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    rig.send(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    let visible = rig.view(slot);
    rig.select_all(slot, &visible);
    assert_eq!(rig.selection(slot), [1, 2, 3, 4, 5]);
}

#[test]
fn the_selection_of_each_slot_survives_switching_tabs() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &numbered(5));
    rig.select_all(a, &[2, 3]);
    let b = rig.open_games(dir.path(), "b.pgn", &numbered(5));
    rig.select_all(b, &[5]);
    rig.send(BasesMessage::Select(a));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.send(BasesMessage::Select(b));
    assert_eq!(rig.selection(a), [2, 3]);
    assert_eq!(rig.selection(b), [5]);
}

#[test]
fn the_selection_is_empty_for_a_fresh_slot() {
    let (rig, slot, _d) = setup(3);
    assert_eq!(rig.selection(slot), Vec::<u32>::new());
    assert_eq!(rig.anchor(slot), None);
}

// ---- model-based property ----

#[derive(Debug, Clone)]
enum Op {
    Only(u32),
    Toggle(u32),
    Numbers(Vec<u32>),
    RemoveSelected,
    Negate,
    Reset,
    Keep,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u32..12).prop_map(Op::Only),
        (0u32..12).prop_map(Op::Toggle),
        proptest::collection::vec(0u32..12, 0..6).prop_map(Op::Numbers),
        Just(Op::RemoveSelected),
        Just(Op::Negate),
        Just(Op::Reset),
        Just(Op::Keep),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn filter_and_selection_follow_a_set_model(ops in proptest::collection::vec(op(), 1..20)) {
        const N: u32 = 9;
        let (mut rig, slot, _d) = setup(N as usize);
        let all: BTreeSet<u32> = (1..=N).collect();
        let mut filter = all.clone();
        let mut selection: BTreeSet<u32> = BTreeSet::new();
        for op in ops {
            match op {
                Op::Only(n) => {
                    rig.pick(slot, n, PickMode::Only);
                    if filter.contains(&n) {
                        selection = BTreeSet::from([n]);
                    }
                }
                Op::Toggle(n) => {
                    rig.pick(slot, n, PickMode::Toggle);
                    if filter.contains(&n) && !selection.remove(&n) {
                        selection.insert(n);
                    }
                }
                Op::Numbers(v) => {
                    rig.select_all(slot, &v);
                    selection = v.into_iter().filter(|n| filter.contains(n)).collect();
                }
                Op::RemoveSelected => {
                    rig.send(l(ListMessage::RemoveSelected { slot }));
                    filter = filter.difference(&selection).copied().collect();
                }
                Op::Negate => {
                    rig.send(l(ListMessage::NegateFilter { slot }));
                    filter = all.difference(&filter).copied().collect();
                }
                Op::Reset => {
                    rig.send(l(ListMessage::ResetFilter { slot }));
                    filter = all.clone();
                }
                Op::Keep => {
                    rig.send(l(ListMessage::KeepSelected { slot }));
                    if !selection.is_empty() {
                        filter = selection.clone();
                    }
                }
            }
            selection = selection.intersection(&filter).copied().collect();
            prop_assert_eq!(rig.filter_numbers(slot), filter.iter().copied().collect::<Vec<_>>());
            prop_assert_eq!(rig.selection(slot), selection.iter().copied().collect::<Vec<_>>());
            if let Some(a) = rig.anchor(slot) {
                prop_assert!(selection.contains(&a), "anchor {} outside selection", a);
            }
            let first = rig.first(slot);
            prop_assert!(first >= 1 && first <= (filter.len() as u32).max(1));
            prop_assert_eq!(rig.state.bases.slot(slot).expect("slot").total(), N);
        }
    }
}
