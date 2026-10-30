//! The Cleaner: the list of steps, in the order of the Scid, run one after
//! the other on the files of the database (spec M7-1, section 13 and rules
//! 44 to 47; decisions D3525, D3581).
//!
//! Every step is a call to the functions of `mychess-db` (no text goes
//! through the Scid commands). A step that fails is recorded and the next one
//! goes on, except when the database was changed from outside (`Stale`),
//! which stops everything. A stop of the user ends the run between steps and
//! inside the steps that look at the cancel flag; what was done stays in the
//! report. Steps that cannot apply (no spelling file, no ECO book) are
//! recorded as skipped, with a Fluent key as the detail.

use std::path::Path;

use mychess_db::maintenance::{
    CleanerCounts, CleanerReport, CleanerStep, DuplicateCriteria, Scope, StepRecord, StepStatus,
    find_duplicates,
};
use mychess_db::si4::{
    FlagOp, GameFlags, NameChange, NameKind, Si4Database, Si4Writer, compact_games, compact_names,
};
use mychess_db::spelling::check::{ScanOptions, scan_names};
use mychess_db::spelling::elo::{AddEloError, EloOptions, apply_add_elo, plan_add_elo};
use mychess_db::tree::{STANDARD_LINES, StcCache, TreeState, prime_with_lines};
use mychess_db::{ClassifySelection, Filter, classify_database};

use super::prefs::CleanerPrefs;
use crate::bases::{BaseError, FileStamp, OpenControl};
use crate::shared::SharedEco;
use crate::spelling_ui::SharedSpelling;

/// The detail of a step skipped because no spelling file is loaded.
pub const DETAIL_NO_SPELLING: &str = "maint-cleaner-detail-no-spelling";
/// The detail of a step skipped because the spelling file has no ratings.
pub const DETAIL_NO_ELO: &str = "maint-cleaner-detail-no-elo";
/// The detail of a step skipped because no ECO book is loaded.
pub const DETAIL_NO_ECO_BOOK: &str = "maint-cleaner-detail-no-eco-book";
/// The detail of a step that never ran because the user stopped the run.
pub const DETAIL_STOPPED: &str = "maint-cleaner-detail-stopped-before";
/// The detail of a step that failed (the error goes to the log, not to the
/// window: its text is English and technical).
pub const DETAIL_FAILED: &str = "maint-cleaner-detail-failed";
/// The detail of the steps left undone because the files changed.
pub const DETAIL_STALE: &str = "maint-cleaner-detail-stale";

/// What the Cleaner needs, resolved when it starts.
#[derive(Debug, Clone, PartialEq)]
pub struct CleanerJob {
    /// The steps that are on.
    pub(crate) steps: CleanerPrefs,
    /// The criteria of the duplicates.
    pub(crate) twins: DuplicateCriteria,
    /// Undelete every game before the search for duplicates.
    pub(crate) undelete_first: bool,
    /// The games the duplicates look at (`None`: all).
    pub(crate) twins_filter: Option<Box<Filter>>,
    /// The spelling file, if one is loaded.
    pub(crate) spelling: Option<SharedSpelling>,
    /// The ECO book, if one is loaded (also used to write the `.stc`).
    pub(crate) eco_book: Option<SharedEco>,
}

impl CleanerJob {
    /// Whether `step` is on.
    pub fn enabled(&self, step: CleanerStep) -> bool {
        let s = &self.steps;
        match step {
            CleanerStep::Players => s.players,
            CleanerStep::Events => s.events,
            CleanerStep::Sites => s.sites,
            CleanerStep::Rounds => s.rounds,
            CleanerStep::Eco => s.eco,
            CleanerStep::Elo => s.elo,
            CleanerStep::Twins => s.twins,
            CleanerStep::CompactNames => s.compact_names,
            CleanerStep::CompactGames => s.compact_games,
            CleanerStep::Tree => s.tree,
            _ => false,
        }
    }
}

/// How a run of the Cleaner ended.
#[derive(Debug)]
pub(crate) struct CleanerRun {
    pub(crate) report: CleanerReport,
    /// Some step wrote to the files.
    pub(crate) changed: bool,
    /// The `.stc` file was written by the last step.
    pub(crate) primed: bool,
    /// Why the run stopped early: `Cancelled` or `Stale`.
    pub(crate) error: Option<BaseError>,
}

/// What one step did.
struct StepResult {
    record: StepRecord,
    wrote: bool,
    primed: bool,
}

impl StepResult {
    fn skipped(step: CleanerStep, detail: &str) -> StepResult {
        StepResult {
            record: StepRecord::with_status(step, StepStatus::Skipped, detail),
            wrote: false,
            primed: false,
        }
    }

    fn failed(step: CleanerStep, error: &impl ToString, wrote: bool) -> StepResult {
        tracing::warn!(?step, error = %error.to_string(), "a step of the cleaner failed");
        StepResult {
            record: StepRecord::with_status(step, StepStatus::Failed, DETAIL_FAILED),
            wrote,
            primed: false,
        }
    }

    fn done(step: CleanerStep, counts: CleanerCounts, wrote: bool) -> StepResult {
        StepResult {
            record: StepRecord::done(step, counts),
            wrote,
            primed: false,
        }
    }

    fn interrupted(step: CleanerStep, counts: CleanerCounts, wrote: bool) -> StepResult {
        let mut record = StepRecord::with_status(step, StepStatus::Interrupted, "");
        record.counts = counts;
        StepResult {
            record,
            wrote,
            primed: false,
        }
    }
}

/// Runs the steps that are on. The database is closed for the slot while
/// this goes on, and `path` is the `.si4` file; `expected_total` is the
/// number of games the slot was loaded with.
pub(crate) fn run(
    job: &CleanerJob,
    path: &Path,
    expected_total: u32,
    control: &OpenControl,
) -> CleanerRun {
    let mut run = CleanerRun {
        report: CleanerReport::new(),
        changed: false,
        primed: false,
        error: None,
    };
    let token = control.token();
    match Si4Database::open(path) {
        Ok(db) if mychess_db::GameSource::len(&db) == expected_total => {}
        Ok(_) => {
            run.error = Some(BaseError::Stale);
            return run;
        }
        Err(error) => {
            run.error = Some(BaseError::from_si4(&error));
            return run;
        }
    }
    let mut stamp = FileStamp::of(path);
    // Why the steps that follow do not run, once something stopped the run.
    let mut stop: Option<(StepStatus, &'static str)> = None;
    for step in CleanerStep::ALL {
        if !job.enabled(step) {
            continue;
        }
        if let Some((status, detail)) = stop {
            run.report
                .push(StepRecord::with_status(step, status, detail));
            continue;
        }
        if token.is_cancelled() {
            stop = Some((StepStatus::Interrupted, DETAIL_STOPPED));
            run.error = Some(BaseError::Cancelled);
            run.report.push(StepRecord::with_status(
                step,
                StepStatus::Interrupted,
                DETAIL_STOPPED,
            ));
            continue;
        }
        if FileStamp::of(path) != stamp {
            stop = Some((StepStatus::Skipped, DETAIL_STALE));
            run.error = Some(BaseError::Stale);
            run.report.push(StepRecord::with_status(
                step,
                StepStatus::Failed,
                DETAIL_STALE,
            ));
            continue;
        }
        let result = run_step(job, step, path, control);
        if result.wrote {
            run.changed = true;
            stamp = FileStamp::of(path);
        }
        run.primed = result.primed;
        if result.record.status == StepStatus::Interrupted {
            stop = Some((StepStatus::Interrupted, DETAIL_STOPPED));
            run.error = Some(BaseError::Cancelled);
        }
        run.report.push(result.record);
    }
    run
}

fn run_step(job: &CleanerJob, step: CleanerStep, path: &Path, control: &OpenControl) -> StepResult {
    match step {
        CleanerStep::Players => names_step(job, step, NameKind::Player, path, control),
        CleanerStep::Events => names_step(job, step, NameKind::Event, path, control),
        CleanerStep::Sites => names_step(job, step, NameKind::Site, path, control),
        CleanerStep::Rounds => names_step(job, step, NameKind::Round, path, control),
        CleanerStep::Eco => eco_step(job, path, control),
        CleanerStep::Elo => elo_step(job, path, control),
        CleanerStep::Twins => twins_step(job, path, control),
        CleanerStep::CompactNames => compact_names_step(path),
        CleanerStep::CompactGames => compact_games_step(path),
        CleanerStep::Tree => tree_step(job, path, control),
        // A step added to the database crate later: nothing to run here.
        _ => StepResult::skipped(step, ""),
    }
}

/// Spelling of one kind of name: scan without surnames and without the
/// ambiguous proposals, apply them all (`doCleaner`).
fn names_step(
    job: &CleanerJob,
    step: CleanerStep,
    kind: NameKind,
    path: &Path,
    control: &OpenControl,
) -> StepResult {
    let Some(set) = &job.spelling else {
        return StepResult::skipped(step, DETAIL_NO_SPELLING);
    };
    let token = control.token();
    let mut progress = control.reporter();
    let scan = match Si4Database::open(path) {
        Ok(db) => scan_names(
            &db,
            set.checker(kind),
            ScanOptions::new(kind),
            &token,
            &mut progress,
        ),
        Err(error) => return StepResult::failed(step, &error, false),
    };
    if scan.interrupted {
        return StepResult::interrupted(step, CleanerCounts::default(), false);
    }
    let changes: Vec<NameChange> = scan
        .proposals
        .into_iter()
        .filter(|p| !p.ambiguous && !p.new.trim().is_empty() && p.new.trim() != p.old)
        .map(|p| NameChange {
            old: p.old,
            new: p.new.trim().to_owned(),
            born: p.born,
            died: p.died,
        })
        .collect();
    if changes.is_empty() {
        return StepResult::done(step, CleanerCounts::default(), false);
    }
    let mut writer = match Si4Writer::open(path) {
        Ok(writer) => writer,
        Err(error) => return StepResult::failed(step, &error, false),
    };
    match writer.apply_name_changes(kind, &changes, &token, &mut progress) {
        Ok(report) => {
            let counts = CleanerCounts::from(&report);
            if report.interrupted {
                StepResult::interrupted(step, counts, report.games_changed > 0)
            } else {
                StepResult::done(step, counts, report.games_changed > 0)
            }
        }
        Err(error) => StepResult::failed(step, &error, true),
    }
}

fn eco_step(job: &CleanerJob, path: &Path, control: &OpenControl) -> StepResult {
    let step = CleanerStep::Eco;
    let Some(book) = &job.eco_book else {
        return StepResult::skipped(step, DETAIL_NO_ECO_BOOK);
    };
    let token = control.token();
    let mut progress = control.reporter();
    let mut writer = match Si4Writer::open(path) {
        Ok(writer) => writer,
        Err(error) => return StepResult::failed(step, &error, false),
    };
    match classify_database(
        &mut writer,
        book,
        ClassifySelection::All,
        false,
        &token,
        &mut progress,
    ) {
        Ok(report) => {
            let counts = CleanerCounts::from(&report);
            if report.cancelled {
                StepResult::interrupted(step, counts, report.written > 0)
            } else {
                StepResult::done(step, counts, report.written > 0)
            }
        }
        Err(error) => StepResult::failed(step, &error, true),
    }
}

fn elo_step(job: &CleanerJob, path: &Path, control: &OpenControl) -> StepResult {
    let step = CleanerStep::Elo;
    let Some(set) = &job.spelling else {
        return StepResult::skipped(step, DETAIL_NO_SPELLING);
    };
    let token = control.token();
    let mut progress = control.reporter();
    let options = EloOptions {
        overwrite: false,
        only_filter: false,
        no_month: true,
        update: true,
    };
    let plan = match Si4Database::open(path) {
        Ok(db) => plan_add_elo(
            &db,
            set.checker(NameKind::Player),
            None,
            options,
            &token,
            &mut progress,
        ),
        Err(error) => return StepResult::failed(step, &error, false),
    };
    let plan = match plan {
        Ok(plan) => plan,
        Err(AddEloError::NoEloData) => return StepResult::skipped(step, DETAIL_NO_ELO),
        Err(error) => return StepResult::failed(step, &error, false),
    };
    if plan.report.interrupted {
        return StepResult::interrupted(step, CleanerCounts::default(), false);
    }
    let mut writer = match Si4Writer::open(path) {
        Ok(writer) => writer,
        Err(error) => return StepResult::failed(step, &error, false),
    };
    let (report, result) = apply_add_elo(&mut writer, &plan, &token, &mut progress);
    let counts = CleanerCounts::from(&report);
    match result {
        Ok(()) if report.interrupted => StepResult::interrupted(step, counts, report.games > 0),
        Ok(()) => StepResult::done(step, counts, report.games > 0),
        Err(error) => StepResult::failed(step, &error, report.games > 0),
    }
}

fn twins_step(job: &CleanerJob, path: &Path, control: &OpenControl) -> StepResult {
    let step = CleanerStep::Twins;
    let token = control.token();
    let mut progress = control.reporter();
    let mut wrote = false;
    if job.undelete_first {
        let mut writer = match Si4Writer::open(path) {
            Ok(writer) => writer,
            Err(error) => return StepResult::failed(step, &error, false),
        };
        let all: Vec<u32> = (1..=writer.len()).collect();
        match writer.set_flags_batch(&all, GameFlags::DELETE, FlagOp::Clear) {
            Ok(changed) => wrote = changed > 0,
            Err(error) => return StepResult::failed(step, &error, true),
        }
    }
    let scan = match Si4Database::open(path) {
        Ok(mut db) => {
            let scope = job
                .twins_filter
                .as_deref()
                .map_or(Scope::All, Scope::Filter);
            find_duplicates(&mut db, scope, &job.twins, &mut progress, &token)
        }
        Err(error) => return StepResult::failed(step, &error, wrote),
    };
    let scan = match scan {
        Ok(scan) => scan,
        Err(error) => return StepResult::failed(step, &error, wrote),
    };
    let counts = CleanerCounts::from(&scan.report);
    if scan.report.interrupted {
        // The pairs found so far are not marked: a stopped run changes
        // nothing beyond the undelete.
        return StepResult::interrupted(step, CleanerCounts::default(), wrote);
    }
    if scan.to_delete.is_empty() && scan.rating_copies.is_empty() {
        return StepResult::done(step, counts, wrote);
    }
    let mut writer = match Si4Writer::open(path) {
        Ok(writer) => writer,
        Err(error) => return StepResult::failed(step, &error, wrote),
    };
    if let Err(error) = writer.set_flags_batch(&scan.to_delete, GameFlags::DELETE, FlagOp::Set) {
        return StepResult::failed(step, &error, true);
    }
    if !scan.rating_copies.is_empty() {
        let (_, result) = writer.set_elo_batch(&scan.rating_copies);
        if let Err(error) = result {
            return StepResult::failed(step, &error, true);
        }
    }
    StepResult::done(step, counts, true)
}

fn compact_names_step(path: &Path) -> StepResult {
    let step = CleanerStep::CompactNames;
    let writer = match Si4Writer::open(path) {
        Ok(writer) => writer,
        Err(error) => return StepResult::failed(step, &error, false),
    };
    match compact_names(writer) {
        Ok((_writer, report)) => {
            StepResult::done(step, CleanerCounts::from(&report), report.changed)
        }
        Err(error) => StepResult::failed(step, &error, false),
    }
}

fn compact_games_step(path: &Path) -> StepResult {
    let step = CleanerStep::CompactGames;
    let writer = match Si4Writer::open(path) {
        Ok(writer) => writer,
        Err(error) => return StepResult::failed(step, &error, false),
    };
    match compact_games(writer) {
        Ok((_writer, report)) => StepResult::done(step, CleanerCounts::from(&report), true),
        Err(error) => StepResult::failed(step, &error, false),
    }
}

/// Fills the cache of the opening trees with the standard lines and writes
/// the `.stc` file (`prime`).
fn tree_step(job: &CleanerJob, path: &Path, control: &OpenControl) -> StepResult {
    let step = CleanerStep::Tree;
    let token = control.token();
    let mut progress = control.reporter();
    let mut db = match Si4Database::open(path) {
        Ok(db) => db,
        Err(error) => return StepResult::failed(step, &error, false),
    };
    let mut cache = StcCache::new(Some(path));
    let mut state = TreeState::default();
    let report = prime_with_lines(
        &mut db,
        &mut state,
        &mut cache,
        STANDARD_LINES,
        &token,
        &mut progress,
    );
    let report = match report {
        Ok(report) => report,
        Err(mychess_db::tree::PrimeError::Cancelled) => {
            return StepResult::interrupted(step, CleanerCounts::default(), false);
        }
        Err(error) => return StepResult::failed(step, &error, false),
    };
    let counts = CleanerCounts::from(&report);
    match cache.write(job.eco_book.as_deref()) {
        Ok(_) => StepResult {
            record: StepRecord::done(step, counts),
            wrote: false,
            primed: true,
        },
        Err(error) => StepResult::failed(step, &error, false),
    }
}
