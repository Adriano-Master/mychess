//! The maintenance of the databases (spec `docs/specs/m7-1-manutencao.md`,
//! slice M7-1b: the controller side over `mychess_db::maintenance` and the
//! writers of `mychess_db::si4`; Scid `tcl/file/maint.tcl`).
//!
//! - `message`: [`MaintenanceMessage`], [`MaintRequest`] (what the user can
//!   ask), the effects, the reports and the [`MaintNotice`] with its Fluent
//!   keys;
//! - `prefs`: the `[maintenance]` table ([`MaintenancePrefs`]);
//! - `cleaner`: the steps of the Cleaner, in the order of the Scid;
//! - `work`: what [`perform`](crate::perform) runs on a blocking thread.
//!
//! The flow (the window, the confirmations with the count of the games they
//! affect, the checks of read-only, PGN and Clipbase, the single job of the
//! list, the permutation of the filter, the selection and the open game after
//! a sort, the invalidation of the caches, the checker of duplicates) is in
//! `state/maintenance_flow.rs`. Every operation is a job of the game list: one
//! at a time, with progress and Stop, and the quit of the program waits for it
//! (M3-9/D2005). The windows are the front-end's (slice c): it reads
//! [`MaintenanceView`] and the reports.

mod cleaner;
mod message;
pub(crate) mod permute;
mod prefs;
mod view;
mod work;

use std::path::PathBuf;

use mychess_db::DuplicateScan;
use mychess_db::maintenance::{CheckReport, DatabaseStats, GameDiff, SharedTag};

use crate::bases::{OpenControl, SharedSource, SlotId};
use crate::error::RequestId;
use crate::spelling_ui::Shared;

pub use cleaner::{
    CleanerJob, DETAIL_FAILED, DETAIL_NO_ECO_BOOK, DETAIL_NO_ELO, DETAIL_NO_SPELLING, DETAIL_STALE,
    DETAIL_STOPPED,
};
pub use message::{
    CompactPreview, ConfirmKind, ConfirmView, FlagAction, FlagScope, HeaderEdit, MAINT_KEYS,
    MaintFlag, MaintJob, MaintJobKind, MaintNotice, MaintOutcome, MaintReport, MaintRequest,
    MaintenanceEffect, MaintenanceMessage, NameKindEdit, NameRequest, NameRequestKind, PreviewDone,
    PreviewJob, StripChoice, TagListView, TagRequest, TwinSide, WeakCriteria,
};
pub use prefs::{
    CleanerPrefs, DeleteChoice, MAX_SORT_CRITERIA, MaintenancePrefs, NameScopeChoice, ScopeChoice,
    SortCriterion, TwinPrefs, default_sort_keys,
};
pub use view::{HeaderView, MaintAccess, MaintenanceView, TwinPairView};
pub(crate) use work::perform;

pub(crate) use message::{GameSet, MaintOp, NameJobGames, PreviewResult, PreviewWork};

/// The data of the reports, for a window to read (slice c): the front-end
/// never calls the database, it only reads the finished, shared reports.
pub mod data {
    pub use mychess_db::maintenance::{
        CheckCause, CheckIssue, CheckReport, CleanerCounts, CleanerReport, CleanerStep,
        CompactionStats, DatabaseStats, DiffToken, DuplicateReport, DuplicateScan, GameDiff,
        NameUsage, RepairReport, SharedField, SharedTag, StepRecord, StepStatus, TagCount,
        TagListing, TwinPair,
    };
    pub use mychess_db::si4::{
        CompactGamesReport, CompactNamesReport, NameEditReport, RewriteReport,
    };
}

/// The statistics of one slot, made for the database of `source` when its
/// games were `data_rev`.
#[derive(Debug)]
pub(crate) struct StatsEntry {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) data_rev: u64,
    pub(crate) stats: Shared<DatabaseStats>,
}

/// The count of the statistics in flight.
#[derive(Debug)]
pub(crate) struct StatsRun {
    pub(crate) request: RequestId,
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) data_rev: u64,
    pub(crate) control: OpenControl,
}

/// What an operation waits for before it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// A count is being made for the confirmation.
    Previewing(RequestId),
    /// The confirmation is on screen.
    Confirming,
    /// The question about the altered game of the editor is on screen.
    Discarding,
}

/// What stands between the request and the job.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Gate {
    /// Nothing: the job starts.
    Open,
    /// A confirmation with a count.
    Confirm(ConfirmKind),
    /// A count is made first (the confirmation, or a notice if there is
    /// nothing to do, comes with the answer).
    Preview(PreviewWork, ConfirmKind),
}

/// An operation worked out from a request: the job, how to receive its
/// answer and what must happen before it starts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Prepared {
    pub(crate) kind: MaintJobKind,
    pub(crate) what: JobWhat,
    pub(crate) op: MaintOp,
    pub(crate) gate: Gate,
    /// The operation rewrites games or renumbers them: an altered game of
    /// the editor is asked about first (spec rule 3).
    pub(crate) rewrites: bool,
    /// The games the confirmation counts.
    pub(crate) count: u32,
    /// The user may ask for a copy of the three files first.
    pub(crate) backup_offered: bool,
    /// What to remember in the preferences when the job starts.
    pub(crate) remember: Remember,
    /// The weak-criteria warnings of the duplicates.
    pub(crate) weak: Vec<WeakCriteria>,
}

/// What a started operation leaves in the preferences.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Remember {
    Nothing,
    Sort {
        keys: Vec<SortCriterion>,
        reverse: bool,
        compact_after: bool,
    },
    StripScope(ScopeChoice),
    NameScope(NameScopeChoice),
}

/// An operation asked for and not started yet.
#[derive(Debug, Clone)]
pub(crate) struct Staged {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) prepared: Prepared,
    pub(crate) confirm: Option<ConfirmView>,
    pub(crate) stage: Stage,
    /// The user said "don't save" to the altered game of the editor: the
    /// editor gets a new game when the job ends.
    pub(crate) discarded: bool,
}

/// The job of the list that is a maintenance job.
#[derive(Debug, Clone)]
pub(crate) struct RunningJob {
    pub(crate) request: RequestId,
    pub(crate) kind: MaintJobKind,
    pub(crate) slot: SlotId,
    /// A repair has no slot of its own: the answer is kept whatever the slot
    /// holds.
    pub(crate) detached: bool,
    pub(crate) what: JobWhat,
    pub(crate) discarded: bool,
}

/// What the job in flight was, for the handling of its answer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum JobWhat {
    Flags { flag: MaintFlag },
    Header,
    DropDeleted,
    Twins,
    UndeleteFirst,
    UndeleteAll,
    MarkTwins,
    ShareTags,
    Check,
    Compact,
    Sort,
    Strip,
    TagList { scope: ScopeChoice },
    TagFilter,
    TagEdit,
    Names,
    Repair,
    Cleaner,
}

impl JobWhat {
    /// Whether the job writes the files (or the memory) of the database.
    pub(crate) fn writes(&self) -> bool {
        !matches!(
            self,
            JobWhat::DropDeleted
                | JobWhat::Twins
                | JobWhat::Check
                | JobWhat::TagList { .. }
                | JobWhat::TagFilter
                | JobWhat::Repair
        )
    }
}

/// The pairs of the last search for duplicates, valid while the database is
/// the one they were found in.
#[derive(Debug, Clone)]
pub(crate) struct TwinState {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) data_rev: u64,
    pub(crate) scan: Shared<DuplicateScan>,
    /// The marking was done (the games of `to_delete` have the flag).
    pub(crate) marked: bool,
}

/// The checker of duplicates: one pair at a time.
#[derive(Debug, Clone, Default)]
pub(crate) struct Checker {
    pub(crate) index: usize,
    pub(crate) diff: Option<Shared<GameDiff>>,
    pub(crate) diff_request: Option<RequestId>,
    pub(crate) share_request: Option<RequestId>,
    pub(crate) share: Option<Shared<Vec<SharedTag>>>,
}

/// The state of the maintenance tools.
#[derive(Debug, Default)]
pub struct MaintenanceUi {
    pub(crate) open: bool,
    pub(crate) stats: Option<StatsEntry>,
    pub(crate) stats_run: Option<StatsRun>,
    pub(crate) staged: Option<Staged>,
    /// The control of the preview that counts for a confirmation: Stop and
    /// the quit cancel it, the front-end reads its progress.
    pub(crate) preview: Option<OpenControl>,
    pub(crate) running: Option<RunningJob>,
    pub(crate) twins: Option<TwinState>,
    pub(crate) checker: Option<Checker>,
    pub(crate) check: Option<Shared<CheckReport>>,
    pub(crate) tags: Option<TagListView>,
    pub(crate) last: Option<MaintReport>,
    pub(crate) backups: Vec<PathBuf>,
    /// The file the repair that waits for its confirmation will work on.
    pub(crate) repair_path: Option<PathBuf>,
    /// Games undeleted by the step before the search for duplicates.
    pub(crate) undeleted_first: u32,
    /// The program is ending: the statistics were stopped and start again if
    /// the quit is given up.
    pub(crate) quitting: bool,
}

impl MaintenanceUi {
    /// Whether the window is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The report of the last operation, for the window.
    pub fn last_report(&self) -> Option<&MaintReport> {
        self.last.as_ref()
    }

    /// The copies of the three files made before the last operation (kept).
    pub fn backups(&self) -> &[PathBuf] {
        &self.backups
    }

    /// The report of the last check of the games.
    pub fn check_report(&self) -> Option<&Shared<CheckReport>> {
        self.check.as_ref()
    }

    /// The extra tags found by the last listing.
    pub fn tag_list(&self) -> Option<&TagListView> {
        self.tags.as_ref()
    }

    /// Stops what runs (the program is ending).
    pub(crate) fn cancel(&mut self) {
        self.quitting = true;
        if let Some(run) = &self.stats_run {
            run.control.cancel();
        }
        if let Some(control) = &self.preview {
            control.cancel();
        }
    }

    /// The progress `(done, total)` of the preview that runs, if one does.
    pub fn preview_progress(&self) -> Option<(u64, u64)> {
        self.preview.as_ref().map(OpenControl::progress)
    }
}
