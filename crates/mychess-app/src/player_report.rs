//! The player report: the controller around the engine of
//! `mychess_db::report::player_report` (spec `docs/specs/m7-3-relatorio-de-jogador.md`,
//! slice M7-3b, decisions M7-3/D3650-D3699; Scid `tcl/tools/preport.tcl`).
//!
//! - `message`: [`PlayerReportMessage`], [`PlayerReportEffect`], the answers
//!   and the [`PlayerReportNotice`] with its Fluent keys;
//! - `work`: the blocking work (the selection of the games, the report, the
//!   documents and the files), run by [`perform`](crate::perform);
//! - `prefs`: [`PlayerReportPrefs`] (`[player_report]`);
//! - `labels`: the words of the documents, filled from Fluent.
//!
//! The flow (one window, one job, the stale test, the filter by click, the
//! Clipbase, the save) is in `state/player_report_flow.rs`. The report never
//! changes the filter of the slot except when the user clicks on an element
//! of the text (M7-3/D3653). The window and the dialog are the front-end's
//! (slice c): it reads [`PlayerReportView`] and [`PlayerDialogView`].

mod labels;
mod message;
mod prefs;
mod work;

use std::path::PathBuf;

use mychess_core::PartialDate;
use mychess_db::report::{PlayerReport, PlayerReportOptions};

use crate::bases::OpenControl;
use crate::error::RequestId;
use crate::opening_report::{ReportKey, ViewState};

pub use labels::{player_label_keys, player_labels};
pub use message::{
    PlayerComputeJob, PlayerFailure, PlayerOutcome, PlayerRenderJob, PlayerReportEffect,
    PlayerReportMessage, PlayerReportNotice, PlayerRequest, PlayerSaveJob, SharedPlayerReport,
};
/// The words of the documents (the front-end fills them from Fluent with
/// [`player_labels`]).
pub use mychess_db::report::PlayerLabels;
pub use prefs::{NameMatching, PlayerColor, PlayerReportPrefs, PlayerStart};

pub use crate::opening_report::{ReportFormat, ReportPart};
pub(crate) use work::perform;

/// The data of a report, for a window to read (slice c): the front-end never
/// calls the database, it only reads the finished, shared report.
pub mod data {
    pub use mychess_db::report::{
        BestKind, COLUMNS, EcoGroupShare, EndTheme, FrequentPlayer, NUM_THEMES, PlayerReport,
        PlayerReportOptions, ReportCore, ReportLine, ScoreCounts, THEME_PLIES, natural_name,
    };
}

/// The most names the dialog offers from the history.
pub const HISTORY_SHOWN: usize = 10;

/// What a report was made for: the choices of the dialog, kept for
/// "Update" (M7-3/D3659).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Params {
    pub(crate) name: String,
    pub(crate) color: PlayerColor,
    pub(crate) matching: NameMatching,
    pub(crate) start: PlayerStart,
    /// The line to the position of the board when `start` is the position.
    pub(crate) reference: Option<crate::opening_report::Reference>,
}

/// The report on the window.
#[derive(Debug)]
pub(crate) struct Shown {
    pub(crate) key: ReportKey,
    pub(crate) report: SharedPlayerReport,
    pub(crate) options: PlayerReportOptions,
    pub(crate) generated: PartialDate,
    pub(crate) database_name: String,
}

/// The generation in flight.
#[derive(Debug)]
pub(crate) struct Job {
    pub(crate) request: RequestId,
    pub(crate) key: ReportKey,
    pub(crate) options: PlayerReportOptions,
    pub(crate) database_name: String,
    pub(crate) control: OpenControl,
    /// Empty the Clipbase and copy the games when the report is done.
    pub(crate) copy_to_clipbase: bool,
}

/// The state of the player report.
#[derive(Debug)]
pub struct PlayerReportUi {
    pub(crate) open: bool,
    /// The dialog is open with these choices.
    pub(crate) dialog: Option<PlayerRequest>,
    /// The Clipbase has games: waiting for the answer to "it will be
    /// emptied".
    pub(crate) confirm: Option<PlayerRequest>,
    pub(crate) job: Option<Job>,
    pub(crate) shown: Option<Shown>,
    pub(crate) params: Option<Params>,
    pub(crate) failure: Option<PlayerFailure>,
    pub(crate) labels: PlayerLabels,
    pub(crate) saving: Option<(RequestId, ReportPart, ReportFormat)>,
    /// The file to save exists: waiting for the answer to "replace it?".
    pub(crate) confirm_save: Option<(PathBuf, ReportPart, ReportFormat)>,
    pub(crate) view: ViewState,
    pub(crate) today: Option<PartialDate>,
}

impl Default for PlayerReportUi {
    fn default() -> PlayerReportUi {
        PlayerReportUi {
            open: false,
            dialog: None,
            confirm: None,
            job: None,
            shown: None,
            params: None,
            failure: None,
            labels: PlayerLabels::default(),
            saving: None,
            confirm_save: None,
            view: ViewState {
                pending: None,
                dir: None,
            },
            today: None,
        }
    }
}

impl PlayerReportUi {
    /// Whether the window is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the dialog is open.
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// Whether a report is being made.
    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    /// Stops the job and forgets its request, so a late answer is ignored
    /// (the program is ending, or the slot went).
    pub(crate) fn cancel(&mut self) {
        if let Some(job) = self.job.take() {
            job.control.cancel();
        }
    }
}

/// Where the window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlayerPhase {
    /// Nothing asked yet.
    Idle,
    /// The report is being made; `(done, total)` of the single bar.
    Generating((u64, u64)),
    /// A report is on the window.
    Ready,
    /// The report could not be made.
    Failed(PlayerFailure),
}

/// What the window draws.
#[derive(Debug, Clone)]
pub struct PlayerReportView<'a> {
    /// The window is open.
    pub open: bool,
    /// What the window shows.
    pub phase: PlayerPhase,
    /// The report on the window, if any.
    pub report: Option<&'a PlayerReport>,
    /// The same report, shared (to hand to the documents).
    pub shared: Option<SharedPlayerReport>,
    /// The options the report was made with.
    pub options: Option<&'a PlayerReportOptions>,
    /// The report no longer matches the database ("the report is out of
    /// date", with an Update button; M7-3/D3659).
    pub stale: bool,
    /// The options of the preferences.
    pub prefs: &'a PlayerReportPrefs,
    /// The name of the database of the report.
    pub database_name: Option<&'a str>,
    /// The question "the Clipbase will be emptied" is open.
    pub confirm_clipbase: bool,
    /// The file the question "replace it?" is about, when open.
    pub confirm_overwrite: Option<&'a std::path::Path>,
}

/// What the dialog "Generate player report" shows (spec section 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerDialogView {
    /// The choices on show.
    pub request: PlayerRequest,
    /// The recently used names (at most [`HISTORY_SHOWN`]).
    pub history: Vec<String>,
    /// The name of White in the game on the board, if it is usable (not
    /// empty and not `?`): the button "White (name)".
    pub white_name: Option<String>,
    /// The name of Black, likewise.
    pub black_name: Option<String>,
    /// The Clipbase option can be used (off when the database in use is the
    /// Clipbase).
    pub clipbase_enabled: bool,
}
