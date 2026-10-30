//! M7-1a3: the report of the Cleaner built from real step reports (spec M7-1,
//! rule 44, D3525, D3581).
#![allow(clippy::unwrap_used)]

#[path = "common/m7_1a2_support.rs"]
mod support;

use mychess_db::si4::{FlagOp, GameFlags, Si4Writer, compact_games, compact_names};
use mychess_db::{CleanerCounts, CleanerReport, CleanerStep, StepRecord, StepStatus};
use proptest::prelude::*;
use support::{build_games, plain, rich};

#[test]
fn the_reports_of_real_steps_sum_into_one() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[rich(), plain(), rich(), plain()]);
    let mut writer = Si4Writer::open(&base).unwrap();
    writer.set_flag(2, GameFlags::DELETE, FlagOp::Set).unwrap();
    writer.set_flag(4, GameFlags::DELETE, FlagOp::Set).unwrap();
    let (writer, games) = compact_games(writer).unwrap();
    let (_writer, names) = compact_names(writer).unwrap();
    let mut report = CleanerReport::new();
    // Pushed out of order on purpose.
    report.push(StepRecord::done(
        CleanerStep::CompactGames,
        CleanerCounts::from(&games),
    ));
    report.push(StepRecord::done(
        CleanerStep::CompactNames,
        CleanerCounts::from(&names),
    ));
    report.push(StepRecord::with_status(
        CleanerStep::Players,
        StepStatus::Skipped,
        "spelling file not loaded",
    ));
    let steps: Vec<_> = report.records().iter().map(|r| r.step).collect();
    assert_eq!(
        steps,
        [
            CleanerStep::Players,
            CleanerStep::CompactNames,
            CleanerStep::CompactGames
        ]
    );
    let totals = report.totals();
    assert_eq!(totals.games_removed, 2);
    assert!(totals.bytes_freed > 0);
    assert_eq!(report.worst_status(), Some(StepStatus::Done));
    assert!(!report.has_failures());
}

#[test]
fn steps_are_in_the_order_of_the_scid() {
    let mut sorted = CleanerStep::ALL;
    sorted.sort();
    assert_eq!(sorted, CleanerStep::ALL);
    assert_eq!(CleanerStep::ALL[0], CleanerStep::Players);
    assert_eq!(CleanerStep::ALL[9], CleanerStep::Tree);
}

fn count_value() -> impl Strategy<Value = u64> {
    prop_oneof![0u64..1000, (u64::MAX - 1000)..=u64::MAX]
}

fn counts() -> impl Strategy<Value = CleanerCounts> {
    proptest::array::uniform11(count_value()).prop_map(|v| CleanerCounts {
        names_mapped: v[0],
        games_renamed: v[1],
        eco_changed: v[2],
        elo_games: v[3],
        duplicates_marked: v[4],
        duplicates_protected: v[5],
        names_removed: v[6],
        games_removed: v[7],
        bytes_freed: v[8],
        tree_searches: v[9],
        skipped_items: v[10],
    })
}

fn record() -> impl Strategy<Value = StepRecord> {
    (
        0usize..CleanerStep::ALL.len(),
        prop_oneof![
            Just(StepStatus::Skipped),
            Just(StepStatus::Done),
            Just(StepStatus::Interrupted),
            Just(StepStatus::Failed)
        ],
        counts(),
        prop_oneof![Just(""), Just("a"), Just("b"), Just("boom")],
    )
        .prop_map(|(step, status, counts, detail)| StepRecord {
            step: CleanerStep::ALL[step],
            status,
            counts,
            detail: detail.to_owned(),
        })
}

fn report() -> impl Strategy<Value = CleanerReport> {
    proptest::collection::vec(record(), 0..12).prop_map(|records| {
        let mut r = CleanerReport::new();
        for record in records {
            r.push(record);
        }
        r
    })
}

fn merged(mut a: CleanerReport, b: &CleanerReport) -> CleanerReport {
    a.merge(b.clone());
    a
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Merging reports is commutative and associative (also with counts near
    /// `u64::MAX`, which saturate), and the result stays sorted by step.
    #[test]
    fn merging_reports_is_commutative_and_associative(
        a in report(), b in report(), c in report(),
    ) {
        prop_assert_eq!(merged(a.clone(), &b), merged(b.clone(), &a));
        prop_assert_eq!(
            merged(merged(a.clone(), &b), &c),
            merged(a.clone(), &merged(b.clone(), &c))
        );
        let steps: Vec<_> = merged(a, &b).records().iter().map(|r| r.step).collect();
        prop_assert!(steps.windows(2).all(|w| w[0] < w[1]));
    }
}

#[test]
fn the_worst_status_of_an_empty_report_is_none_and_failed_beats_interrupted() {
    let mut report = CleanerReport::new();
    assert_eq!(report.worst_status(), None);
    assert!(!report.has_failures());
    report.push(StepRecord::with_status(
        CleanerStep::Elo,
        StepStatus::Interrupted,
        "stopped",
    ));
    assert_eq!(report.worst_status(), Some(StepStatus::Interrupted));
    report.push(StepRecord::with_status(
        CleanerStep::Eco,
        StepStatus::Failed,
        "boom",
    ));
    assert_eq!(report.worst_status(), Some(StepStatus::Failed));
    assert!(report.has_failures());
}
