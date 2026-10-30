//! M7-1a1, criteria 5 to 11 and 22: `find_duplicates`
//! (`docs/specs/m7-1-manutencao.md`).
#![allow(clippy::unwrap_used, clippy::too_many_lines, clippy::assert_is_empty)]

#[path = "common/m7_1a1_support.rs"]
mod support;

use std::collections::BTreeSet;

use mychess_db::maintenance::{
    DeleteWhich, DuplicateCriteria, DuplicateScan, MaintError, Scope, TwinPair, find_duplicates,
    find_duplicates_whole_groups, find_duplicates_with_limit,
};
use mychess_db::{CancelToken, Filter, NoProgress};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use support::{QG4, RUY6, RUY8, S, build, open, s, twin_games};

fn scan(games: &[S], criteria: &DuplicateCriteria) -> DuplicateScan {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), games);
    let mut db = open(&base);
    find_duplicates(
        &mut db,
        Scope::All,
        criteria,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
}

fn twin_criteria() -> DuplicateCriteria {
    DuplicateCriteria {
        skip_short: false,
        ..DuplicateCriteria::default()
    }
}

fn pair(first: u32, second: u32) -> TwinPair {
    TwinPair { first, second }
}

#[test]
fn the_default_criteria_are_those_of_the_scid_dialog() {
    let c = DuplicateCriteria::default();
    assert!(c.exact_names && c.same_colors && c.same_site && c.same_round);
    assert!(c.same_year && c.same_month && c.same_moves && c.skip_short);
    assert!(c.keep_commented && c.keep_with_variations);
    assert!(!c.same_event && !c.same_day && !c.same_result && !c.same_eco && !c.copy_ratings);
    assert_eq!(c.delete, DeleteWhich::Shorter);
}

#[test]
fn shorter_deletes_the_short_game_and_protects_the_commented_one() {
    let result = scan(&twin_games(), &twin_criteria());
    assert_eq!(result.pairs, [pair(1, 2), pair(2, 4)]);
    assert_eq!(result.to_delete, [1]);
    assert_eq!(result.report.protected, 1);
    assert_eq!(result.report.examined, 5);
    assert_eq!(result.report.marked, 1);
    assert_eq!(result.report.pairs_found, 2);
    assert!(!result.report.interrupted);
}

#[test]
fn lower_and_higher_number_strategies_follow_the_specified_walk() {
    let lower = scan(
        &twin_games(),
        &DuplicateCriteria {
            delete: DeleteWhich::LowerNumber,
            ..twin_criteria()
        },
    );
    assert_eq!(lower.to_delete, [1, 2]);
    assert_eq!(lower.report.protected, 0);

    let higher = scan(
        &twin_games(),
        &DuplicateCriteria {
            delete: DeleteWhich::HigherNumber,
            ..twin_criteria()
        },
    );
    assert_eq!(higher.to_delete, [2]);
    assert_eq!(higher.report.protected, 1);

    let unprotected = scan(
        &twin_games(),
        &DuplicateCriteria {
            delete: DeleteWhich::HigherNumber,
            keep_commented: false,
            ..twin_criteria()
        },
    );
    assert_eq!(unprotected.to_delete, [2, 4]);
}

#[test]
fn a_game_with_a_variation_is_protected_too() {
    let games = [s(&RUY6), s(&RUY6).with_variation()];
    let result = scan(
        &games,
        &DuplicateCriteria {
            delete: DeleteWhich::HigherNumber,
            ..twin_criteria()
        },
    );
    assert_eq!(result.pairs, [pair(1, 2)]);
    assert!(result.to_delete.is_empty());
    assert_eq!(result.report.protected, 1);
    let free = scan(
        &games,
        &DuplicateCriteria {
            delete: DeleteWhich::HigherNumber,
            keep_with_variations: false,
            ..twin_criteria()
        },
    );
    assert_eq!(free.to_delete, [2]);
}

#[test]
fn equal_length_games_delete_the_lower_number() {
    let result = scan(&[s(&RUY6), s(&RUY6)], &twin_criteria());
    assert_eq!(result.to_delete, [1]);
}

#[test]
fn skipping_short_games_leaves_the_fixture_untouched() {
    let result = scan(&twin_games(), &DuplicateCriteria::default());
    assert_eq!(result.report.examined, 0);
    assert!(result.pairs.is_empty() && result.to_delete.is_empty());
}

#[test]
fn already_deleted_games_are_never_candidates() {
    let games = [s(&RUY6).flags(mychess_db::si4::GameFlags::DELETE), s(&RUY8)];
    let result = scan(&games, &twin_criteria());
    assert_eq!(result.report.examined, 1);
    assert!(result.pairs.is_empty());
}

#[test]
fn place_holder_games_match_without_comparing_moves() {
    let long = s(&["d2d4", "d7d5"]);
    let short = s(&["e2e4"]);
    let result = scan(&[long.clone(), short.clone()], &twin_criteria());
    assert_eq!(result.pairs, [pair(1, 2)]);

    let unknown_white = scan(
        &[
            long.clone().players("?", "Beta, B"),
            short.clone().players("?", "Beta, B"),
        ],
        &twin_criteria(),
    );
    assert!(unknown_white.pairs.is_empty());

    // Rule 18: one side with an unknown event is enough to refuse.
    let unknown_event = scan(&[long, short.event("?")], &twin_criteria());
    assert!(unknown_event.pairs.is_empty());
}

#[test]
fn the_place_holder_rule_still_needs_the_same_year_site_and_round() {
    let long = s(&["d2d4", "d7d5"]);
    for other in [
        s(&["e2e4"]).date("2019.01.10"),
        s(&["e2e4"]).site("Lima"),
        s(&["e2e4"]).round("2"),
    ] {
        let criteria = DuplicateCriteria {
            same_site: false,
            same_round: false,
            same_year: false,
            same_month: false,
            ..twin_criteria()
        };
        let result = scan(&[long.clone(), other], &criteria);
        assert!(result.pairs.is_empty());
    }
}

#[test]
fn month_day_and_color_criteria_are_applied_only_when_on() {
    let games = [s(&RUY6), s(&RUY6).date("2020.02.11")];
    assert!(scan(&games, &twin_criteria()).pairs.is_empty());
    let no_month = DuplicateCriteria {
        same_month: false,
        ..twin_criteria()
    };
    assert_eq!(scan(&games, &no_month).pairs, [pair(1, 2)]);
    let day = DuplicateCriteria {
        same_month: false,
        same_day: true,
        ..twin_criteria()
    };
    assert!(scan(&games, &day).pairs.is_empty());

    let swapped = [s(&RUY6), s(&RUY6).players("Beta, B", "Alfa, A")];
    assert!(scan(&swapped, &twin_criteria()).pairs.is_empty());
    let colors = DuplicateCriteria {
        same_colors: false,
        ..twin_criteria()
    };
    assert_eq!(scan(&swapped, &colors).pairs, [pair(1, 2)]);
}

#[test]
fn event_result_and_eco_criteria() {
    let mut with_eco = s(&RUY6);
    with_eco.eco = Some("C60");
    let games = [s(&RUY6).event("Open A"), s(&RUY6).event("Open B")];
    assert_eq!(scan(&games, &twin_criteria()).pairs, [pair(1, 2)]);
    let event = DuplicateCriteria {
        same_event: true,
        ..twin_criteria()
    };
    assert!(scan(&games, &event).pairs.is_empty());

    let results = [s(&RUY6), s(&RUY6).result(mychess_core::GameResult::Black)];
    let result = DuplicateCriteria {
        same_result: true,
        ..twin_criteria()
    };
    assert!(scan(&results, &result).pairs.is_empty());
    assert_eq!(scan(&results, &twin_criteria()).pairs.len(), 1);

    let ecos = [with_eco.clone(), s(&RUY6)];
    let eco = DuplicateCriteria {
        same_eco: true,
        ..twin_criteria()
    };
    assert!(scan(&ecos, &eco).pairs.is_empty());
    assert_eq!(scan(&[with_eco.clone(), with_eco], &eco).pairs.len(), 1);
}

#[test]
fn different_moves_are_not_twins_unless_moves_are_ignored() {
    let games = [s(&RUY6), s(&QG4)];
    assert!(scan(&games, &twin_criteria()).pairs.is_empty());
    let loose = DuplicateCriteria {
        same_moves: false,
        ..twin_criteria()
    };
    assert_eq!(scan(&games, &loose).pairs, [pair(1, 2)]);
}

#[test]
fn four_letter_names_compare_characters_not_bytes() {
    let inexact = DuplicateCriteria {
        exact_names: false,
        ..twin_criteria()
    };
    let same_prefix = [s(&RUY6), s(&RUY6).players("Alfa, Z", "Beta, B")];
    assert!(scan(&same_prefix, &twin_criteria()).pairs.is_empty());
    assert_eq!(scan(&same_prefix, &inexact).pairs, [pair(1, 2)]);

    let other_prefix = [s(&RUY6), s(&RUY6).players("Alfb, A", "Beta, B")];
    assert!(scan(&other_prefix, &inexact).pairs.is_empty());

    // "Á" is two bytes. Four characters are compared, not four bytes: "Álvaro"
    // and "Álvaxo" agree on "Álva" (five bytes), "Álvaro" and "Álvxro" do not.
    let accents = [
        s(&RUY6).players("Álvaro, A", "Beta, B"),
        s(&RUY6).players("Álvaro, B", "Beta, B"),
    ];
    assert_eq!(scan(&accents, &inexact).pairs, [pair(1, 2)]);
    let near = [
        s(&RUY6).players("Álvaro, A", "Beta, B"),
        s(&RUY6).players("Álvxro, A", "Beta, B"),
    ];
    assert!(scan(&near, &inexact).pairs.is_empty());
}

#[test]
fn four_letter_names_compare_case_sensitively_like_the_scid_hash() {
    // D3565: `hashName` of the Scid hashes the bytes of the first four
    // letters, so "Alfa" and "ALFA" are different names with exact_names off.
    let inexact = DuplicateCriteria {
        exact_names: false,
        ..twin_criteria()
    };
    let upper = [s(&RUY6), s(&RUY6).players("ALFA, Z", "Beta, B")];
    assert!(scan(&upper, &inexact).pairs.is_empty());
    let lower = [s(&RUY6), s(&RUY6).players("Alfa, Z", "Beta, B")];
    assert_eq!(scan(&lower, &inexact).pairs, [pair(1, 2)]);
    let mixed = [s(&RUY6), s(&RUY6).players("alfa, A", "Beta, B")];
    assert!(scan(&mixed, &inexact).pairs.is_empty());
}

#[test]
fn the_filter_scope_hides_pairs_outside_it() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &twin_games());
    let mut db = open(&base);
    let mut filter = Filter::new(5);
    filter.fill(0);
    for n in [1, 2, 3] {
        filter.set(n, 1);
    }
    let result = find_duplicates(
        &mut db,
        Scope::Filter(&filter),
        &twin_criteria(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(result.pairs, [pair(1, 2)]);
    assert_eq!(result.report.examined, 3);

    let mut empty = Filter::new(5);
    empty.fill(0);
    let none = find_duplicates(
        &mut db,
        Scope::Filter(&empty),
        &twin_criteria(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(none.pairs.is_empty() && none.report.examined == 0);

    let wrong = Filter::new(2);
    let error = find_duplicates(
        &mut db,
        Scope::Filter(&wrong),
        &twin_criteria(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        MaintError::FilterSize {
            filter: 2,
            games: 5
        }
    ));
}

#[test]
fn copy_ratings_gives_the_kept_game_the_missing_elo_and_only_then() {
    let games = [s(&RUY6).elo(2500, 2400), s(&RUY8).elo(0, 2000)];
    let copy = DuplicateCriteria {
        copy_ratings: true,
        ..twin_criteria()
    };
    let result = scan(&games, &copy);
    assert_eq!(result.to_delete, [1]);
    // Game 2 stays and has no White Elo; its Black Elo is kept.
    assert_eq!(result.rating_copies, [(2, Some((2500, 0)), None)]);

    let off = scan(&games, &twin_criteria());
    assert!(off.rating_copies.is_empty());

    let both_rated = scan(&[s(&RUY6).elo(2500, 2400), s(&RUY8).elo(2100, 2000)], &copy);
    assert!(both_rated.rating_copies.is_empty());
}

#[test]
fn an_empty_database_and_a_single_game_find_nothing() {
    for games in [vec![], vec![s(&RUY6)]] {
        let result = scan(&games, &twin_criteria());
        assert!(result.pairs.is_empty() && result.to_delete.is_empty());
    }
}

/// A database of `groups` twin pairs of different players.
fn many_pairs(groups: usize) -> Vec<S> {
    let mut games = Vec::new();
    for g in 0..groups {
        let white = format!("White{g:05}, W");
        games.push(s(&RUY6).players(&white, "Beta, B"));
        games.push(s(&RUY8).players(&white, "Beta, B"));
    }
    games
}

#[test]
fn cancelling_in_the_middle_returns_a_coherent_partial_result() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &many_pairs(60));
    let criteria = twin_criteria();
    let full = {
        let mut db = open(&base);
        find_duplicates(
            &mut db,
            Scope::All,
            &criteria,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap()
    };
    assert_eq!(full.pairs.len(), 60);

    let cancel = CancelToken::new();
    let token = cancel.clone();
    let mut seen = Vec::new();
    let mut sink = |done: u64, total: u64| {
        seen.push((done, total));
        // Phase 2 starts after `len` units: stop after ten groups.
        if done >= 120 + 10 {
            token.cancel();
        }
    };
    let mut db = open(&base);
    let partial = find_duplicates(&mut db, Scope::All, &criteria, &mut sink, &cancel).unwrap();
    assert!(partial.report.interrupted);
    assert!(!partial.pairs.is_empty() && partial.pairs.len() < 60);
    let all: BTreeSet<_> = full.pairs.iter().collect();
    assert!(partial.pairs.iter().all(|p| all.contains(p)));
    assert_eq!(partial.report.marked as usize, partial.to_delete.len());
    assert!(
        seen.windows(2).all(|w| w[0].0 <= w[1].0),
        "monotonic: {seen:?}"
    );
    assert!(seen.iter().all(|&(d, t)| d <= t));
}

#[test]
fn progress_is_monotonic_and_ends_at_the_total() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &many_pairs(20));
    let mut seen = Vec::new();
    let mut sink = |done: u64, total: u64| seen.push((done, total));
    let mut db = open(&base);
    find_duplicates(
        &mut db,
        Scope::All,
        &twin_criteria(),
        &mut sink,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
    let last = seen.last().unwrap();
    assert_eq!(last.0, last.1);
}

#[test]
fn the_result_does_not_depend_on_the_number_of_threads() {
    let dir = tempfile::tempdir().unwrap();
    let mut games = many_pairs(30);
    games.extend(twin_games());
    let base = build(dir.path(), &games);
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let mut db = open(&base);
            find_duplicates(
                &mut db,
                Scope::All,
                &twin_criteria(),
                &mut NoProgress,
                &CancelToken::new(),
            )
            .unwrap()
        })
    };
    assert_eq!(run(1), run(4));
}

#[test]
fn a_record_that_cannot_be_read_is_never_a_twin() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[s(&RUY6), s(&RUY8), s(&RUY6)]);
    // Overwrite the record of game 2 with garbage.
    let db = open(&base);
    let entry = db.entry(2).unwrap();
    drop(db);
    let path = base.with_extension("sg4");
    let mut bytes = std::fs::read(&path).unwrap();
    let start = entry.offset as usize;
    for b in &mut bytes[start..start + entry.length as usize] {
        *b = 0xFF;
    }
    std::fs::write(&path, bytes).unwrap();
    let mut db = open(&base);
    let result = find_duplicates(
        &mut db,
        Scope::All,
        &twin_criteria(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(result.pairs, [pair(1, 3)]);
}

fn run_with(
    base: &std::path::Path,
    criteria: &DuplicateCriteria,
    limit: Option<u64>,
) -> DuplicateScan {
    let mut db = open(base);
    match limit {
        Some(limit) => find_duplicates_with_limit(
            &mut db,
            Scope::All,
            criteria,
            limit,
            &mut NoProgress,
            &CancelToken::new(),
        ),
        None => find_duplicates(
            &mut db,
            Scope::All,
            criteria,
            &mut NoProgress,
            &CancelToken::new(),
        ),
    }
    .unwrap()
}

#[test]
fn twins_at_the_end_of_a_big_player_group_are_found() {
    // 2000 games of the same two players. The first 1998 differ by round, so
    // they are in the same player group but can never be twins of each other;
    // the last two are twins. A walk of the whole group stopped at 100,000
    // pairs long before reaching them.
    let dir = tempfile::tempdir().unwrap();
    let mut games: Vec<S> = (0..1998)
        .map(|i| s(&RUY6).round(&format!("r{i}")))
        .collect();
    games.push(s(&RUY6).round("last"));
    games.push(s(&RUY6).round("last"));
    let base = build(dir.path(), &games);
    let criteria = twin_criteria();
    let one = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| run_with(&base, &criteria, None));
    assert_eq!(one.pairs, [pair(1999, 2000)]);
    assert_eq!(one.to_delete, [1999]);
    assert_eq!(one.report.groups_truncated, 0);
    assert_eq!(one.report.examined, 2000);
    let four = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap()
        .install(|| run_with(&base, &criteria, None));
    assert_eq!(one, four);
}

#[test]
fn the_walk_of_the_whole_group_misses_those_twins_and_says_so() {
    // The reference walk (and the old behavior) is cut at the pair limit: it
    // never reaches the twins at the end and reports the cut.
    let dir = tempfile::tempdir().unwrap();
    let mut games: Vec<S> = (0..1998)
        .map(|i| s(&RUY6).round(&format!("r{i}")))
        .collect();
    games.push(s(&RUY6).round("last"));
    games.push(s(&RUY6).round("last"));
    let base = build(dir.path(), &games);
    let mut db = open(&base);
    let whole = find_duplicates_whole_groups(
        &mut db,
        Scope::All,
        &twin_criteria(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(whole.pairs.is_empty());
    assert_eq!(whole.report.groups_truncated, 1);
}

#[test]
fn a_group_over_the_pair_limit_is_cut_and_counted() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<S> = (0..30).map(|_| s(&RUY6)).collect();
    let base = build(dir.path(), &games);
    let criteria = DuplicateCriteria {
        same_moves: false,
        keep_commented: false,
        keep_with_variations: false,
        delete: DeleteWhich::LowerNumber,
        ..twin_criteria()
    };
    let full = run_with(&base, &criteria, None);
    assert_eq!(full.report.groups_truncated, 0);
    assert_eq!(full.to_delete.len(), 29);
    let cut = run_with(&base, &criteria, Some(10));
    assert_eq!(cut.report.groups_truncated, 1);
    assert!(cut.to_delete.len() < 29, "{:?}", cut.to_delete);
    assert!(!cut.to_delete.is_empty());
    // The cut result is a prefix of the full one.
    assert!(cut.to_delete.iter().all(|n| full.to_delete.contains(n)));
}

#[test]
fn an_unreadable_index_entry_is_skipped_and_counted() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[s(&RUY6), s(&RUY6), s(&RUY6)]);
    // A result above 3 makes the entry of game 2 unreadable.
    let index = base.with_extension("si4");
    let mut bytes = std::fs::read(&index).unwrap();
    bytes[182 + 47 + 21] |= 0xF0;
    std::fs::write(&index, &bytes).unwrap();
    let result = run_with(&base, &twin_criteria(), None);
    assert_eq!(result.report.unreadable_entries, 1);
    assert_eq!(result.report.examined, 2);
    assert_eq!(result.pairs, [pair(1, 3)]);
}

#[test]
#[ignore = "builds 100,000 games: run with --ignored"]
fn a_hundred_thousand_games_are_scanned() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &many_pairs(50_000));
    let mut db = open(&base);
    let result = find_duplicates(
        &mut db,
        Scope::All,
        &twin_criteria(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(result.pairs.len(), 50_000);
}

// ---- properties ----

const NAMES: [&str; 3] = ["Alfa, A", "Beta, B", "Gama, G"];
const LINES: [&[&str]; 3] = [&RUY6, &RUY8, &QG4];

fn game_from(spec: (usize, usize, usize, usize, bool)) -> S {
    let (w, b, line, site, commented) = spec;
    let g = s(LINES[line])
        .players(NAMES[w], NAMES[b])
        .site(["Rio", "Lima"][site]);
    if commented { g.commented() } else { g }
}

fn criteria_from(bits: u16, delete: usize) -> DuplicateCriteria {
    let bit = |i: u32| bits & (1 << i) != 0;
    DuplicateCriteria {
        exact_names: bit(0),
        same_colors: bit(1),
        same_event: bit(2),
        same_site: bit(3),
        same_round: bit(4),
        same_year: bit(5),
        same_month: bit(6),
        same_day: bit(7),
        same_result: bit(8),
        same_eco: bit(9),
        same_moves: bit(10),
        skip_short: false,
        keep_commented: bit(11),
        keep_with_variations: bit(12),
        copy_ratings: bit(13),
        delete: [
            DeleteWhich::Shorter,
            DeleteWhich::LowerNumber,
            DeleteWhich::HigherNumber,
        ][delete],
    }
}

fn spec() -> impl Strategy<Value = (usize, usize, usize, usize, bool)> {
    (0..3usize, 0..3usize, 0..3usize, 0..2usize, any::<bool>())
}

type WideSpec = (
    usize,
    usize,
    usize,
    usize,
    bool,
    usize,
    usize,
    usize,
    usize,
    usize,
);

fn wide_spec() -> impl Strategy<Value = WideSpec> {
    (
        0..3usize,
        0..3usize,
        0..3usize,
        0..2usize,
        any::<bool>(),
        0..2usize,
        0..4usize,
        0..2usize,
        0..3usize,
        0..2usize,
    )
}

fn wide_game(spec: WideSpec) -> S {
    let (w, b, line, site, commented, round, date, event, result, elo) = spec;
    let game = s(LINES[line])
        .players(NAMES[w], NAMES[b])
        .site(["Rio", "Lima"][site])
        .round(["1", "2"][round])
        .date(["2020.01.10", "2020.01.11", "2020.02.10", "2020.??.??"][date])
        .event(["Open", "Cup"][event])
        .result(
            [
                mychess_core::GameResult::White,
                mychess_core::GameResult::Draw,
                mychess_core::GameResult::Unknown,
            ][result],
        )
        .elo([0, 2400][elo], 0);
    if commented { game.commented() } else { game }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn splitting_the_groups_equals_the_walk_of_the_whole_group(
        specs in proptest::collection::vec(wide_spec(), 0..16),
        bits in any::<u16>(),
        delete in 0..3usize,
    ) {
        let games: Vec<S> = specs.iter().copied().map(wide_game).collect();
        let criteria = criteria_from(bits, delete);
        let dir = tempfile::tempdir().unwrap();
        let base = build(dir.path(), &games);
        let split = run_with(&base, &criteria, None);
        let mut db = open(&base);
        let whole = find_duplicates_whole_groups(
            &mut db,
            Scope::All,
            &criteria,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        prop_assert_eq!(split, whole);
    }

    #[test]
    fn invariants_hold_for_random_databases_and_criteria(
        specs in proptest::collection::vec(spec(), 0..10),
        bits in any::<u16>(),
        delete in 0..3usize,
    ) {
        let games: Vec<S> = specs.iter().copied().map(game_from).collect();
        let criteria = criteria_from(bits, delete);
        let result = scan(&games, &criteria);
        for p in &result.pairs {
            prop_assert!(p.first < p.second);
        }
        let mut sorted = result.pairs.clone();
        sorted.sort_unstable();
        sorted.dedup();
        prop_assert_eq!(&sorted, &result.pairs);
        let mut marked = result.to_delete.clone();
        marked.sort_unstable();
        marked.dedup();
        prop_assert_eq!(&marked, &result.to_delete, "a game is marked twice");
        prop_assert!(result.to_delete.iter().all(|&n| n >= 1 && n as usize <= games.len()));
        // Deterministic.
        prop_assert_eq!(scan(&games, &criteria), result);
    }

    #[test]
    fn the_relation_is_symmetric_for_a_pair(
        a in spec(),
        b in spec(),
        bits in any::<u16>(),
    ) {
        let criteria = criteria_from(bits, 0);
        let forward = scan(&[game_from(a), game_from(b)], &criteria).pairs.len();
        let backward = scan(&[game_from(b), game_from(a)], &criteria).pairs.len();
        prop_assert_eq!(forward, backward);
    }

    #[test]
    fn equivalence_classes_leave_one_game_each(
        specs in proptest::collection::vec(spec(), 0..10),
    ) {
        // Moves ignored, all games long enough, nothing protected, and the
        // lower number deleted: each class of equal (players, site) keeps
        // only its last game.
        let criteria = DuplicateCriteria {
            same_moves: false,
            keep_commented: false,
            keep_with_variations: false,
            delete: DeleteWhich::LowerNumber,
            ..twin_criteria()
        };
        let games: Vec<S> = specs.iter().copied().map(game_from).collect();
        let result = scan(&games, &criteria);
        let mut classes = std::collections::HashMap::new();
        for sp in &specs {
            *classes.entry((sp.0, sp.1, sp.3)).or_insert(0usize) += 1;
        }
        let expected: usize = classes.values().map(|k| k - 1).sum();
        prop_assert_eq!(result.to_delete.len(), expected);
    }
}
