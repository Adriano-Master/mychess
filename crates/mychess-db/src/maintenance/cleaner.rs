//! The report of the *Cleaner* (`::maint::doCleaner`, `maint.tcl` 2333-2501;
//! decisions D3525, D3581).
//!
//! The Cleaner runs up to ten steps in a fixed order. Each step ends with a
//! report of its own (name corrections, ECO, Elo, duplicates, compaction,
//! tree); this module turns them into [`CleanerCounts`] and sums them in one
//! [`CleanerReport`]. The steps are run by the application; here there is
//! only the arithmetic, so it is the same everywhere:
//!
//! - counts are `u64` and add with saturation, so merging never panics or
//!   wraps;
//! - the records are kept sorted by [`CleanerStep`] (the order of the Scid),
//!   whatever the order of the merges, and [`CleanerReport::merge`] is
//!   commutative and associative, so partial reports of a long run can be
//!   joined in any grouping.

use crate::eco::ClassifyReport;
use crate::si4::{CompactGamesReport, CompactNamesReport, NameChangeReport};
use crate::spelling::elo::EloReport;
use crate::tree::PrimeReport;

use super::DuplicateReport;

/// A step of the Cleaner, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum CleanerStep {
    /// Spelling of the player names.
    Players,
    /// Spelling of the event names.
    Events,
    /// Spelling of the site names.
    Sites,
    /// Spelling of the round names.
    Rounds,
    /// ECO classification.
    Eco,
    /// Add Elo ratings.
    Elo,
    /// Mark the duplicate games for deletion.
    Twins,
    /// Compact the name file.
    CompactNames,
    /// Compact the game file.
    CompactGames,
    /// Fill the tree cache (`.stc`).
    Tree,
}

impl CleanerStep {
    /// Every step, in execution order.
    pub const ALL: [CleanerStep; 10] = [
        CleanerStep::Players,
        CleanerStep::Events,
        CleanerStep::Sites,
        CleanerStep::Rounds,
        CleanerStep::Eco,
        CleanerStep::Elo,
        CleanerStep::Twins,
        CleanerStep::CompactNames,
        CleanerStep::CompactGames,
        CleanerStep::Tree,
    ];
}

/// How a step ended. Ordered from the mildest to the worst; merging two
/// records of a step keeps the worst, except that `Done` beats `Skipped`
/// (a step that ran in one part of the work did run).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StepStatus {
    /// Not run, for a reason the user chose or the setup implies (no
    /// spelling file loaded, option off, nothing to do).
    Skipped,
    /// Ran to the end.
    Done,
    /// Stopped by the user; the counts are partial.
    Interrupted,
    /// Failed; `detail` of the record says why. The next step still runs,
    /// except after a stale database (decision D3525).
    Failed,
}

impl StepStatus {
    fn severity(self) -> u8 {
        match self {
            StepStatus::Skipped => 0,
            StepStatus::Done => 1,
            StepStatus::Interrupted => 2,
            StepStatus::Failed => 3,
        }
    }
}

/// What the steps changed, as plain counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct CleanerCounts {
    /// Name corrections that map a name to another one.
    pub names_mapped: u64,
    /// Games whose name IDs were replaced.
    pub games_renamed: u64,
    /// Games whose ECO code changed.
    pub eco_changed: u64,
    /// Games that got at least one Elo.
    pub elo_games: u64,
    /// Games marked for deletion as duplicates.
    pub duplicates_marked: u64,
    /// Duplicate pairs protected by comments or variations.
    pub duplicates_protected: u64,
    /// Names removed by compaction.
    pub names_removed: u64,
    /// Games removed by compaction (deleted or unreadable).
    pub games_removed: u64,
    /// Bytes the game file lost in compaction.
    pub bytes_freed: u64,
    /// Searches run to fill the tree cache.
    pub tree_searches: u64,
    /// Items the steps left alone (unreadable, out of life span, name limit...).
    pub skipped_items: u64,
}

impl CleanerCounts {
    /// Adds `other` into `self` with saturation.
    pub fn add(&mut self, other: &CleanerCounts) {
        macro_rules! sum {
            ($($f:ident),*) => { $(self.$f = self.$f.saturating_add(other.$f);)* };
        }
        sum!(
            names_mapped,
            games_renamed,
            eco_changed,
            elo_games,
            duplicates_marked,
            duplicates_protected,
            names_removed,
            games_removed,
            bytes_freed,
            tree_searches,
            skipped_items
        );
    }
}

impl From<&NameChangeReport> for CleanerCounts {
    fn from(r: &NameChangeReport) -> Self {
        CleanerCounts {
            names_mapped: r.mapped.into(),
            games_renamed: r.games_changed.into(),
            skipped_items: u64::from(r.names_not_found)
                + u64::from(r.skipped_full)
                + u64::from(r.skipped_by_date),
            ..CleanerCounts::default()
        }
    }
}

impl From<&ClassifyReport> for CleanerCounts {
    fn from(r: &ClassifyReport) -> Self {
        CleanerCounts {
            eco_changed: r.changed.into(),
            skipped_items: r.unreadable.into(),
            ..CleanerCounts::default()
        }
    }
}

impl From<&EloReport> for CleanerCounts {
    fn from(r: &EloReport) -> Self {
        CleanerCounts {
            elo_games: r.games.into(),
            ..CleanerCounts::default()
        }
    }
}

impl From<&DuplicateReport> for CleanerCounts {
    fn from(r: &DuplicateReport) -> Self {
        CleanerCounts {
            duplicates_marked: r.marked.into(),
            duplicates_protected: r.protected.into(),
            skipped_items: u64::from(r.unreadable_entries),
            ..CleanerCounts::default()
        }
    }
}

impl From<&CompactNamesReport> for CleanerCounts {
    fn from(r: &CompactNamesReport) -> Self {
        CleanerCounts {
            names_removed: r.removed.iter().map(|&n| u64::from(n)).sum(),
            ..CleanerCounts::default()
        }
    }
}

impl From<&CompactGamesReport> for CleanerCounts {
    fn from(r: &CompactGamesReport) -> Self {
        CleanerCounts {
            games_removed: u64::from(r.deleted) + u64::from(r.corrupt),
            bytes_freed: r.games_bytes_before.saturating_sub(r.games_bytes_after),
            ..CleanerCounts::default()
        }
    }
}

impl From<&PrimeReport> for CleanerCounts {
    fn from(r: &PrimeReport) -> Self {
        CleanerCounts {
            tree_searches: r.searches.into(),
            skipped_items: r.skipped.into(),
            ..CleanerCounts::default()
        }
    }
}

/// The outcome of one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    /// The step.
    pub step: CleanerStep,
    /// How it ended.
    pub status: StepStatus,
    /// What it changed.
    pub counts: CleanerCounts,
    /// Why it failed or was skipped (log text, not for translation); empty
    /// when there is nothing to say.
    pub detail: String,
}

impl StepRecord {
    /// A record of a step that ran to the end.
    pub fn done(step: CleanerStep, counts: CleanerCounts) -> StepRecord {
        StepRecord {
            step,
            status: StepStatus::Done,
            counts,
            detail: String::new(),
        }
    }

    /// A record of a step that did not run or did not finish.
    pub fn with_status(
        step: CleanerStep,
        status: StepStatus,
        detail: impl Into<String>,
    ) -> StepRecord {
        StepRecord {
            step,
            status,
            counts: CleanerCounts::default(),
            detail: detail.into(),
        }
    }
}

/// The records of the steps, one per step, in [`CleanerStep`] order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CleanerReport {
    records: Vec<StepRecord>,
}

impl CleanerReport {
    /// An empty report.
    pub fn new() -> CleanerReport {
        CleanerReport::default()
    }

    /// The records, sorted by step.
    pub fn records(&self) -> &[StepRecord] {
        &self.records
    }

    /// The record of `step`, if the step was reported.
    pub fn record(&self, step: CleanerStep) -> Option<&StepRecord> {
        self.records.iter().find(|r| r.step == step)
    }

    /// Adds one record; when the step is already there the two are merged.
    pub fn push(&mut self, record: StepRecord) {
        match self.records.binary_search_by_key(&record.step, |r| r.step) {
            Ok(at) => {
                if let Some(mine) = self.records.get_mut(at) {
                    merge_record(mine, record);
                }
            }
            Err(at) => self.records.insert(at, record),
        }
    }

    /// Sums `other` into `self`: counts of the same step add with
    /// saturation, the status is the worst of the two (`Done` over
    /// `Skipped`), steps only one side has are copied. Commutative and
    /// associative, and the result is always sorted by step.
    pub fn merge(&mut self, other: CleanerReport) {
        for record in other.records {
            self.push(record);
        }
    }

    /// The sum of the counts of every step.
    pub fn totals(&self) -> CleanerCounts {
        let mut total = CleanerCounts::default();
        for record in &self.records {
            total.add(&record.counts);
        }
        total
    }

    /// The worst status of the report, `None` when it is empty.
    pub fn worst_status(&self) -> Option<StepStatus> {
        self.records
            .iter()
            .map(|r| r.status)
            .max_by_key(|s| s.severity())
    }

    /// Whether some step failed.
    pub fn has_failures(&self) -> bool {
        self.records.iter().any(|r| r.status == StepStatus::Failed)
    }
}

fn merge_record(mine: &mut StepRecord, other: StepRecord) {
    mine.counts.add(&other.counts);
    let (a, b) = (mine.status.severity(), other.status.severity());
    // The detail follows the worse status; on a tie the smaller text wins,
    // so the result does not depend on the order of the merge.
    let take_other_detail = match b.cmp(&a) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => {
            mine.detail.is_empty() || (!other.detail.is_empty() && other.detail < mine.detail)
        }
    };
    if b > a {
        mine.status = other.status;
    }
    if take_other_detail {
        mine.detail = other.detail;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(step: CleanerStep, status: StepStatus, marked: u64, detail: &str) -> StepRecord {
        let mut r = StepRecord::with_status(step, status, detail);
        r.counts.duplicates_marked = marked;
        r
    }

    #[test]
    fn records_stay_sorted_by_step_whatever_the_push_order() {
        let mut report = CleanerReport::new();
        report.push(StepRecord::done(
            CleanerStep::Tree,
            CleanerCounts::default(),
        ));
        report.push(StepRecord::done(
            CleanerStep::Players,
            CleanerCounts::default(),
        ));
        report.push(StepRecord::done(CleanerStep::Eco, CleanerCounts::default()));
        let steps: Vec<_> = report.records().iter().map(|r| r.step).collect();
        assert_eq!(
            steps,
            [CleanerStep::Players, CleanerStep::Eco, CleanerStep::Tree]
        );
    }

    #[test]
    fn merging_adds_counts_with_saturation() {
        let mut a = CleanerReport::new();
        a.push(rec(CleanerStep::Twins, StepStatus::Done, u64::MAX - 1, ""));
        let mut b = CleanerReport::new();
        b.push(rec(CleanerStep::Twins, StepStatus::Done, 5, ""));
        a.merge(b);
        assert_eq!(a.totals().duplicates_marked, u64::MAX);
    }

    #[test]
    fn merge_keeps_the_worst_status_and_done_beats_skipped() {
        let mut a = CleanerReport::new();
        a.push(rec(CleanerStep::Elo, StepStatus::Skipped, 0, "no file"));
        let mut b = CleanerReport::new();
        b.push(rec(CleanerStep::Elo, StepStatus::Done, 0, ""));
        a.merge(b);
        assert_eq!(
            a.record(CleanerStep::Elo).map(|r| r.status),
            Some(StepStatus::Done)
        );
        let mut c = CleanerReport::new();
        c.push(rec(CleanerStep::Elo, StepStatus::Failed, 0, "boom"));
        a.merge(c);
        assert!(a.has_failures());
        assert_eq!(
            a.record(CleanerStep::Elo).map(|r| r.detail.as_str()),
            Some("boom")
        );
    }

    #[test]
    fn merge_is_commutative_and_associative() {
        let parts = [
            rec(CleanerStep::Twins, StepStatus::Interrupted, 3, "b"),
            rec(CleanerStep::Twins, StepStatus::Interrupted, 4, "a"),
            rec(CleanerStep::Eco, StepStatus::Done, 1, ""),
        ];
        let make = |order: [usize; 3]| {
            let mut r = CleanerReport::new();
            for i in order {
                let mut one = CleanerReport::new();
                one.push(parts[i].clone());
                r.merge(one);
            }
            r
        };
        assert_eq!(make([0, 1, 2]), make([2, 1, 0]));
        assert_eq!(make([0, 1, 2]), make([1, 2, 0]));
        assert_eq!(
            make([0, 1, 2])
                .record(CleanerStep::Twins)
                .map(|r| r.detail.clone()),
            Some("a".to_owned())
        );
    }

    #[test]
    fn conversions_follow_the_step_reports() {
        let c = CleanerCounts::from(&CompactGamesReport {
            kept: 3,
            deleted: 2,
            corrupt: 1,
            half_moves_fixed: 0,
            games_bytes_before: 100,
            games_bytes_after: 40,
        });
        assert_eq!((c.games_removed, c.bytes_freed), (3, 60));
        let c = CleanerCounts::from(&CompactNamesReport {
            changed: true,
            removed: [1, 2, 0, 4],
        });
        assert_eq!(c.names_removed, 7);
    }
}
