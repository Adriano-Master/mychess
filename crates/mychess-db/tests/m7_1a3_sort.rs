//! M7-1a3: `SortReport::order`, up to 15 criteria, stability, cancellation
//! (spec M7-1, rule 41, acceptance criterion 21, D3517, D3580).
#![allow(
    clippy::unwrap_used,
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::case_sensitive_file_extension_comparisons
)]

#[path = "common/m7_1a2_support.rs"]
mod support;

use std::fs;
use std::path::PathBuf;

use mychess_core::{Game, GameResult, PartialDate};
use mychess_db::si4::{
    FailPoint, MAX_SORT_KEYS, Si4Database, Si4Writer, SortKey, SortKeyError, sort_games,
    sort_games_controlled,
};
use mychess_db::{CancelToken, NoProgress};
use proptest::prelude::*;
use support::{build_games, line, snapshot};

fn dated(year: &str, white: &str, label: &str) -> Game {
    let mut g = line(&["e2e4", "e7e5"]);
    let t = g.tags_mut();
    t.date = PartialDate::parse_lenient(year);
    t.white = white.into();
    t.event = label.into();
    t.result = GameResult::White;
    g
}

/// Years 2003, 2001, 2002, 2001, 2003, 2001; the event says the original
/// number.
fn shuffled() -> Vec<Game> {
    ["2003", "2001", "2002", "2001", "2003", "2001"]
        .iter()
        .enumerate()
        .map(|(i, y)| {
            dated(
                &format!("{y}.01.01"),
                &format!("W{}", i % 2),
                &format!("g{}", i + 1),
            )
        })
        .collect()
}

fn events(base: &std::path::Path) -> Vec<String> {
    let mut db = Si4Database::open(base).unwrap();
    (1..=db.len())
        .map(|n| db.game(n).unwrap().tags().event.clone())
        .collect()
}

fn make(games: &[Game]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), games);
    (dir, base)
}

#[test]
fn order_gives_the_new_position_of_each_old_game() {
    let (_dir, base) = make(&shuffled());
    let writer = Si4Writer::open(&base).unwrap();
    let (writer, report) = sort_games(writer, &[SortKey::Year], false).unwrap();
    drop(writer);
    // Stable: 2001 (g2, g4, g6), 2002 (g3), 2003 (g1, g5).
    assert_eq!(events(&base), ["g2", "g4", "g6", "g3", "g1", "g5"]);
    assert_eq!(report.order, [4, 0, 3, 1, 5, 2]);
    assert_eq!(report.games, 6);
    assert_eq!(report.moved, 6);
    assert!(!report.interrupted);
}

#[test]
fn the_permutation_carries_a_selection_to_the_new_numbers() {
    let (_dir, base) = make(&shuffled());
    let writer = Si4Writer::open(&base).unwrap();
    let (_w, report) = sort_games(writer, &[SortKey::Year], false).unwrap();
    // A filter on the old games 1, 3 and 5 (0-based 0, 2, 4).
    let old_selected = [0usize, 2, 4];
    let mut new_selected: Vec<usize> = old_selected
        .iter()
        .map(|&i| report.order[i] as usize)
        .collect();
    new_selected.sort_unstable();
    let after = events(&base);
    let labels: Vec<&str> = new_selected.iter().map(|&i| after[i].as_str()).collect();
    assert_eq!(labels, ["g3", "g1", "g5"]);
}

#[test]
fn equal_games_keep_their_order_also_when_reversed() {
    let (_dir, base) = make(&shuffled());
    let writer = Si4Writer::open(&base).unwrap();
    let (_w, report) = sort_games(writer, &[SortKey::Year], true).unwrap();
    // Reversed comparison: 2003 first, ties still in the original order.
    assert_eq!(events(&base), ["g1", "g5", "g3", "g2", "g4", "g6"]);
    let mut seen = report.order.clone();
    seen.sort_unstable();
    assert_eq!(seen, [0, 1, 2, 3, 4, 5]);
}

#[test]
fn later_keys_break_the_ties_of_earlier_ones() {
    let (_dir, base) = make(&shuffled());
    let writer = Si4Writer::open(&base).unwrap();
    // Year, then white (W0 for g1 g3 g5; W1 for g2 g4 g6).
    let (_w, _) = sort_games(writer, &[SortKey::Year, SortKey::White], false).unwrap();
    // 2001: g2 W1, g4 W1, g6 W1; 2002: g3; 2003: g1 W0, g5 W0.
    assert_eq!(events(&base), ["g2", "g4", "g6", "g3", "g1", "g5"]);
    let (_dir2, base2) = make(&shuffled());
    let writer = Si4Writer::open(&base2).unwrap();
    let (_w, _) = sort_games(writer, &[SortKey::White, SortKey::Year], false).unwrap();
    // W0: g1(2003) g3(2002) g5(2003) -> g3 g1 g5; W1: g2 g4 g6 (2001).
    assert_eq!(events(&base2), ["g3", "g1", "g5", "g2", "g4", "g6"]);
}

#[test]
fn sorting_twice_by_every_criterion_moves_nothing_the_second_time() {
    for key in SortKey::ALL {
        let (_dir, base) = make(&shuffled());
        let writer = Si4Writer::open(&base).unwrap();
        let (writer, first) = sort_games(writer, &[key], false).unwrap();
        let mut seen = first.order.clone();
        seen.sort_unstable();
        assert_eq!(seen, [0, 1, 2, 3, 4, 5], "{key}: a permutation");
        let (_w, second) = sort_games(writer, &[key], false).unwrap();
        assert_eq!(second.moved, 0, "{key}");
        assert_eq!(second.order, [0, 1, 2, 3, 4, 5], "{key}");
    }
}

#[test]
fn fifteen_criteria_are_accepted_and_sixteen_are_not() {
    assert_eq!(MAX_SORT_KEYS, 15);
    let fifteen = vec!["date"; 15].join(",");
    assert_eq!(SortKey::parse_list(&fifteen).unwrap().len(), 15);
    let sixteen = vec!["date"; 16].join(",");
    assert_eq!(SortKey::parse_list(&sixteen), Err(SortKeyError::TooMany));
    // Fifteen criteria on a database run to the end.
    let (_dir, base) = make(&shuffled());
    let writer = Si4Writer::open(&base).unwrap();
    let keys = SortKey::parse_list(&fifteen).unwrap();
    let (_w, report) = sort_games(writer, &keys, false).unwrap();
    assert_eq!(report.games, 6);
}

#[test]
fn the_report_of_an_empty_database_has_an_empty_order() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("empty");
    let writer = Si4Writer::create(&base, 1).unwrap();
    let (_w, report) = sort_games(writer, &[SortKey::Date], false).unwrap();
    assert_eq!((report.games, report.moved), (0, 0));
    assert!(report.order.is_empty());
}

#[test]
fn a_cancelled_sort_changes_nothing_and_returns_a_usable_writer() {
    let games: Vec<Game> = (0..600)
        .map(|i| {
            dated(
                &format!("{}.01.01", 2000 + (600 - i) % 40),
                "W",
                &format!("g{i}"),
            )
        })
        .collect();
    let (dir, base) = make(&games);
    let before = snapshot(&base);
    let names_before: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    let writer = Si4Writer::open(&base).unwrap();
    let (writer, report) = sort_games_controlled(
        writer,
        &[SortKey::Year],
        false,
        &mut |done: u64, _total: u64| {
            if done > 1 {
                trigger.cancel();
            }
        },
        &cancel,
        &mut FailPoint::never(),
    )
    .unwrap();
    assert!(report.interrupted);
    assert_eq!(report.moved, 0);
    assert_eq!(report.order, (0..600).collect::<Vec<u32>>());
    assert_eq!(writer.len(), 600);
    drop(writer);
    assert_eq!(snapshot(&base), before);
    let mut names_after: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    let mut names_before = names_before;
    names_after.sort();
    names_before.sort();
    assert_eq!(names_after, names_before, "no temporary left");
    // And it sorts afterwards.
    let writer = Si4Writer::open(&base).unwrap();
    let (_w, report) = sort_games(writer, &[SortKey::Year], false).unwrap();
    assert!(report.moved > 0);
}

/// Cancels when `when(done)` first holds; the sort must change nothing.
fn cancelled_sort_leaves_everything(when: impl Fn(u64) -> bool) {
    let games: Vec<Game> = (0..600)
        .map(|i| {
            dated(
                &format!("{}.01.01", 2000 + (600 - i) % 40),
                "W",
                &format!("g{i}"),
            )
        })
        .collect();
    let (dir, base) = make(&games);
    let before = snapshot(&base);
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    let mut seen = Vec::new();
    let writer = Si4Writer::open(&base).unwrap();
    let (writer, report) = sort_games_controlled(
        writer,
        &[SortKey::Year],
        false,
        &mut |done: u64, _total: u64| {
            seen.push(done);
            if when(done) {
                trigger.cancel();
            }
        },
        &cancel,
        &mut FailPoint::never(),
    )
    .unwrap();
    assert!(report.interrupted, "{seen:?}");
    assert_eq!((report.moved, report.games), (0, 600));
    assert_eq!(report.order, (0..600).collect::<Vec<u32>>());
    drop(writer);
    assert_eq!(snapshot(&base), before);
    let leftovers: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".tmp") || n.ends_with(".txn"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_sort_cancelled_while_writing_changes_nothing() {
    // Totals are 1200: done > 600 is only reached by the writing phase.
    cancelled_sort_leaves_everything(|done| done > 600);
}

#[test]
fn a_sort_cancelled_right_after_the_comparison_changes_nothing() {
    // `done == 600` is the report that follows `sort_by` (the reading phase
    // reports 1, 257 and 513).
    cancelled_sort_leaves_everything(|done| done == 600);
}

#[test]
fn progress_is_monotonic_and_ends_at_the_total() {
    let (_dir, base) = make(&shuffled());
    let mut calls = Vec::new();
    let writer = Si4Writer::open(&base).unwrap();
    sort_games_controlled(
        writer,
        &[SortKey::Year],
        false,
        &mut |d: u64, t: u64| calls.push((d, t)),
        &CancelToken::new(),
        &mut FailPoint::never(),
    )
    .unwrap();
    assert!(calls.windows(2).all(|w| w[0].0 <= w[1].0), "{calls:?}");
    assert_eq!(*calls.last().unwrap(), (12, 12));
    let _ = NoProgress;
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Sorting keeps the set of games, the permutation is consistent with the
    /// records, and the order is deterministic.
    #[test]
    fn sorting_preserves_the_games_and_the_permutation_matches(
        years in proptest::collection::vec(1990u16..2000, 1..10),
        key in 0usize..19,
        reverse in any::<bool>(),
    ) {
        let games: Vec<Game> = years
            .iter()
            .enumerate()
            .map(|(i, y)| dated(&format!("{y}.01.01"), &format!("W{}", i % 3), &format!("g{i}")))
            .collect();
        let (_dir, base) = make(&games);
        let mut before = Si4Database::open(&base).unwrap();
        let records: Vec<Vec<u8>> = (1..=before.len())
            .map(|n| before.record_bytes(n).unwrap().to_vec())
            .collect();
        drop(before);
        let writer = Si4Writer::open(&base).unwrap();
        let (writer, report) = sort_games(writer, &[SortKey::ALL[key]], reverse).unwrap();
        drop(writer);
        let mut after = Si4Database::open(&base).unwrap();
        for (old, &new) in report.order.iter().enumerate() {
            let rec = after.record_bytes(new + 1).unwrap().to_vec();
            prop_assert_eq!(&rec, &records[old]);
        }
        let mut seen = report.order.clone();
        seen.sort_unstable();
        prop_assert_eq!(seen, (0..years.len() as u32).collect::<Vec<_>>());
    }
}
