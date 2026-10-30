//! The opening report: the controller around the engine of
//! `mychess_db::report` (spec `docs/specs/m7-2-relatorio-de-abertura.md`,
//! slice M7-2b, decisions D3600-D3650; Scid `tcl/tools/optable.tcl`).
//!
//! - `message`: [`OpeningReportMessage`], [`OpeningReportEffect`], the
//!   answers and the [`OpeningReportNotice`] with its Fluent keys;
//! - `work`: the blocking work (the search of the games, the report, the
//!   documents and the files), run by [`perform`](crate::perform);
//! - `favorites`: [`Favorites`] and the file `report_favorites.toml`;
//! - `prefs`: [`OpeningReportPrefs`] (`[opening_report]`);
//! - `labels`: the words of the documents, filled from Fluent.
//!
//! The flow (one window, one job, the stale test, the filter by click, the
//! save, the favorites, the batch and the merge) is in
//! `state/opening_report_flow.rs`. The report never changes the filter of
//! the slot except when the user clicks on an element of the text (D3601).

mod favorites;
mod labels;
mod message;
mod prefs;
mod work;

use std::path::PathBuf;

use mychess_core::PartialDate;
use mychess_db::report::{OpeningReport, ReportOptions};

use crate::bases::{OpenControl, SharedSource, SlotId};
use crate::error::RequestId;

pub use favorites::{
    FAVORITES_VERSION, Favorite, FavoriteError, Favorites, FavoritesIssue, LoadedFavorites,
    MAX_FAVORITES, MAX_MOVES_CHARS, MAX_NAME_CHARS, favorites_path, parse_favorites,
    render_favorites,
};
pub use labels::{LabelLookup, label_keys, report_labels};
pub use message::{
    BatchFailure, ComputeOutcome, FavoriteMessage, OpeningReportEffect, OpeningReportMessage,
    OpeningReportNotice, Reference, ReportFailure, ReportSelector, ReportsDat, SaveFailure,
    SharedReport, SharedTemp,
};
/// The words of the documents (the front-end fills them from Fluent with
/// [`report_labels`]).
pub use mychess_db::report::ReportLabels;

/// The data of a report, for a window to read (M7-2c): the front-end never
/// calls the database, it only reads the finished, shared report.
pub mod data {
    pub use mychess_db::filter_stats::StatsQuery;
    pub use mychess_db::report::{
        BestKind, COLUMNS, EndTheme, MoveOrder, NUM_THEMES, OpeningReport, RANGES, ReportCore,
        ReportLine, ReportOptions, SectionSet, THEME_PLIES,
    };
}
pub use prefs::{
    BOARD_SIZE_RANGE, DEFAULT_BOARD_SIZE, OpeningReportPrefs, ReportFormat, ReportPart,
};

pub(crate) use message::{BatchJob, ComputeJob, RenderJob, SaveJob};
pub(crate) use work::perform;
#[cfg(test)]
pub(crate) use work::run_compute as run_compute_for_tests;

/// How many games "Merge games" takes (`sc_report opening best a 50`).
pub const MERGE_GAMES: usize = 50;

/// What a report was made for. The report is stale when the slot, the
/// database or its contents are no longer these (D3621).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReportKey {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    pub(crate) data_rev: u64,
}

/// The report on the window.
#[derive(Debug)]
pub(crate) struct Shown {
    pub(crate) key: ReportKey,
    pub(crate) report: SharedReport,
    pub(crate) options: ReportOptions,
    pub(crate) generated: PartialDate,
    pub(crate) database_name: String,
}

/// The generation in flight.
#[derive(Debug)]
pub(crate) struct Job {
    pub(crate) request: RequestId,
    pub(crate) key: ReportKey,
    pub(crate) options: ReportOptions,
    pub(crate) database_name: String,
    pub(crate) search: OpenControl,
    pub(crate) generate: OpenControl,
}

impl Job {
    pub(crate) fn cancel(&self) {
        self.search.cancel();
        self.generate.cancel();
    }
}

/// The generation of the reports of the favorites in flight (D3620).
#[derive(Debug)]
pub(crate) struct Batch {
    pub(crate) request: RequestId,
    pub(crate) key: ReportKey,
    pub(crate) items: Vec<Favorite>,
    pub(crate) next: usize,
    pub(crate) dir: PathBuf,
    pub(crate) part: ReportPart,
    pub(crate) format: ReportFormat,
    pub(crate) database_name: String,
    /// The options and the words of the moment the batch started: changing
    /// them while it runs does not change the later reports (D3661).
    pub(crate) options: ReportOptions,
    pub(crate) labels: Box<ReportLabels>,
    pub(crate) written: u32,
    pub(crate) failed: u32,
    pub(crate) used: Vec<String>,
    pub(crate) control: OpenControl,
}

/// The favorites and the requests that touch their file.
#[derive(Debug, Default)]
pub(crate) struct FavoritesState {
    pub(crate) list: Favorites,
    /// The file was read (or there is none to read): edits are allowed. A
    /// failed read leaves this false, so the list is never written over a
    /// file that could not be read (D3660).
    pub(crate) loaded: bool,
    /// The file holds something the list does not (a newer version or
    /// entries that were dropped): the next write keeps a `.bak` copy.
    pub(crate) keep_backup: bool,
    pub(crate) loading: Option<RequestId>,
    pub(crate) saving: Option<RequestId>,
    pub(crate) resave: bool,
    pub(crate) importing: Option<RequestId>,
}

/// "View HTML" in flight.
#[derive(Debug)]
pub(crate) struct ViewState {
    pub(crate) pending: Option<RequestId>,
    pub(crate) dir: Option<SharedTemp>,
}

/// The state of the opening report.
#[derive(Debug)]
pub struct OpeningReportUi {
    pub(crate) open: bool,
    pub(crate) job: Option<Job>,
    pub(crate) shown: Option<Shown>,
    pub(crate) reference: Option<Reference>,
    pub(crate) search_interrupted: bool,
    pub(crate) failure: Option<ReportFailure>,
    pub(crate) exclude: Option<String>,
    pub(crate) labels: ReportLabels,
    pub(crate) fav: FavoritesState,
    pub(crate) batch: Option<Batch>,
    pub(crate) saving: Option<(RequestId, ReportPart, ReportFormat)>,
    pub(crate) view: ViewState,
    pub(crate) today: Option<PartialDate>,
}

impl Default for OpeningReportUi {
    fn default() -> OpeningReportUi {
        OpeningReportUi {
            open: false,
            job: None,
            shown: None,
            reference: None,
            search_interrupted: false,
            failure: None,
            exclude: None,
            labels: ReportLabels::default(),
            fav: FavoritesState::default(),
            batch: None,
            saving: None,
            view: ViewState {
                pending: None,
                dir: None,
            },
            today: None,
        }
    }
}

impl OpeningReportUi {
    /// Whether the window is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether a report is being made.
    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    /// Stops the job and the batch and forgets their requests, so a late
    /// answer is ignored (the program is ending, or the slot went).
    pub(crate) fn cancel(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel();
        }
        if let Some(batch) = self.batch.take() {
            batch.control.cancel();
        }
    }
}

/// Where the window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReportPhase {
    /// Nothing asked yet.
    Idle,
    /// The games are being searched (phase 1) and the report made (phase
    /// 2); the numbers are `(done, total)` of each bar.
    Generating {
        /// The search of the games.
        search: (u64, u64),
        /// The generation of the report.
        generate: (u64, u64),
    },
    /// A report is on the window.
    Ready,
    /// The search was stopped: no report ("Search interrupted").
    SearchInterrupted,
    /// The report could not be made.
    Failed(ReportFailure),
}

/// The progress of the generation of the favorites' reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchView {
    /// The report in progress, from 1.
    pub current: usize,
    /// How many favorites.
    pub total: usize,
    /// The name of the favorite in progress.
    pub name: String,
}

/// What the window draws.
#[derive(Debug, Clone)]
pub struct OpeningReportView<'a> {
    /// The window is open.
    pub open: bool,
    /// What the window shows.
    pub phase: ReportPhase,
    /// The report on the window, if any.
    pub report: Option<&'a OpeningReport>,
    /// The same report, shared (to hand to the documents).
    pub shared: Option<SharedReport>,
    /// The options the report was made with.
    pub options: Option<&'a ReportOptions>,
    /// The report no longer matches the database ("the report is out of
    /// date", with an Update button; D3621).
    pub stale: bool,
    /// The options of the preferences.
    pub prefs: &'a OpeningReportPrefs,
    /// The move left out of the theory table.
    pub exclude: Option<&'a str>,
    /// The favorites.
    pub favorites: &'a [Favorite],
    /// The generation of the favorites' reports, if running.
    pub batch: Option<BatchView>,
    /// The name of the database of the report.
    pub database_name: Option<&'a str>,
}
