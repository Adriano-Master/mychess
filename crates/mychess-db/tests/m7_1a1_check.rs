//! M7-1a1, criterion 14: `check_games` (`docs/specs/m7-1-manutencao.md`).
#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

#[path = "common/m7_1a1_support.rs"]
mod support;

use mychess_db::maintenance::{CheckCause, MAX_CHECK_ISSUES, Scope, check_games};
use mychess_db::{CancelToken, Filter, NoProgress};
use pretty_assertions::assert_eq;
use support::{QG4, RUY6, RUY8, S, build, open, s};

fn five() -> Vec<S> {
    vec![s(&RUY6), s(&RUY8), s(&QG4), s(&RUY6), s(&RUY8)]
}

fn run(base: &std::path::Path) -> mychess_db::CheckReport {
    let mut db = open(base);
    check_games(&mut db, Scope::All, &mut NoProgress, &CancelToken::new()).unwrap()
}

#[test]
fn a_healthy_database_has_no_issue() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &five());
    let report = run(&base);
    assert_eq!((report.examined, report.problems), (5, 0));
    assert!(report.issues.is_empty() && !report.interrupted);
}

#[test]
fn empty_database_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[]);
    assert_eq!(run(&base).examined, 0);
}

/// Patches entry `n` (base 1) of the `.si4`.
fn patch_entry(base: &std::path::Path, n: usize, edit: impl FnOnce(&mut [u8])) {
    let path = base.with_extension("si4");
    let mut bytes = std::fs::read(&path).unwrap();
    let start = 182 + 47 * (n - 1);
    edit(&mut bytes[start..start + 47]);
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn three_kinds_of_damage_are_reported_with_the_right_cause_and_number() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &five());
    let db = open(&base);
    let record3 = db.entry(4).unwrap();
    drop(db);
    // Game 2: length 0. Game 3: record beyond the end of the file. Game 4:
    // record bytes overwritten.
    patch_entry(&base, 2, |e| {
        e[4] = 0;
        e[5] = 0;
        e[6] &= !0x80;
    });
    patch_entry(&base, 3, |e| {
        e[0..4].copy_from_slice(&0x00FF_FF00u32.to_be_bytes());
    });
    let sg4 = base.with_extension("sg4");
    let mut bytes = std::fs::read(&sg4).unwrap();
    let start = record3.offset as usize;
    for b in &mut bytes[start..start + record3.length as usize] {
        *b = 0xFF;
    }
    std::fs::write(&sg4, bytes).unwrap();

    let report = run(&base);
    assert_eq!(report.examined, 5);
    assert_eq!(report.problems, 3);
    let found: Vec<(u32, CheckCause)> = report.issues.iter().map(|i| (i.game, i.cause)).collect();
    assert_eq!(
        found,
        [
            (2, CheckCause::Index),
            (3, CheckCause::Record),
            (4, CheckCause::Decode),
        ]
    );
    assert!(report.issues.iter().all(|i| !i.detail.is_empty()));
}

#[test]
fn the_filter_scope_checks_only_its_games() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &five());
    patch_entry(&base, 2, |e| {
        e[4] = 0;
        e[5] = 0;
        e[6] &= !0x80;
    });
    let mut filter = Filter::new(5);
    filter.fill(0);
    filter.set(1, 1);
    filter.set(3, 1);
    let mut db = open(&base);
    let report = check_games(
        &mut db,
        Scope::Filter(&filter),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!((report.examined, report.problems), (2, 0));
    let wrong = Filter::new(9);
    assert!(
        check_games(
            &mut db,
            Scope::Filter(&wrong),
            &mut NoProgress,
            &CancelToken::new()
        )
        .is_err()
    );
}

#[test]
fn cancelling_returns_a_coherent_partial_report() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<S> = (0..300).map(|_| s(&RUY6)).collect();
    let base = build(dir.path(), &games);
    // Break game 1 so there is something to report before the cancellation.
    patch_entry(&base, 1, |e| {
        e[4] = 0;
        e[5] = 0;
        e[6] &= !0x80;
    });
    let cancel = CancelToken::new();
    cancel.cancel();
    let mut db = open(&base);
    let report = check_games(&mut db, Scope::All, &mut NoProgress, &cancel).unwrap();
    // The token is looked at on the first tick, 5000 games or 100 ms away; the
    // run may finish first. Either way the report is consistent.
    assert!(report.examined <= 300);
    assert!(report.problems <= report.examined);
    if report.interrupted {
        assert!(report.examined < 300 || report.problems <= 1);
    }
}

#[test]
fn progress_never_goes_back_and_ends_at_the_total() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &five());
    let mut seen = Vec::new();
    let mut sink = |d: u64, t: u64| seen.push((d, t));
    let mut db = open(&base);
    check_games(&mut db, Scope::All, &mut sink, &CancelToken::new()).unwrap();
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
    assert_eq!(seen.last(), Some(&(5, 5)));
}

#[test]
fn the_list_is_limited_but_the_count_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<S> = (0..MAX_CHECK_ISSUES + 20).map(|_| s(&["e2e4"])).collect();
    let base = build(dir.path(), &games);
    let path = base.with_extension("si4");
    let mut bytes = std::fs::read(&path).unwrap();
    for n in 0..games.len() {
        let start = 182 + 47 * n;
        bytes[start + 4] = 0;
        bytes[start + 5] = 0;
        bytes[start + 6] &= !0x80;
    }
    std::fs::write(&path, bytes).unwrap();
    let report = run(&base);
    assert_eq!(report.problems as usize, games.len());
    assert_eq!(report.issues.len(), MAX_CHECK_ISSUES);
    assert_eq!(report.issues[0].game, 1);
    assert_eq!(
        report.issues.last().unwrap().game as usize,
        MAX_CHECK_ISSUES
    );
}

#[test]
fn cancelling_from_the_progress_sink_stops_in_the_middle() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<S> = (0..6000).map(|_| s(&["e2e4"])).collect();
    let base = build(dir.path(), &games);
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let mut sink = |_: u64, _: u64| token.cancel();
    let mut db = open(&base);
    let report = check_games(&mut db, Scope::All, &mut sink, &cancel).unwrap();
    assert!(report.interrupted);
    assert!(
        report.examined > 0 && report.examined < 6000,
        "{}",
        report.examined
    );
    assert_eq!(report.problems, 0);
}
