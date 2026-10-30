//! "Export games..." (spec M3-9, D2000-D2029; Scid: `exportGames` in
//! `tcl/end.tcl` and `sc_base export`).
//!
//! The flow (messages, dialogs, the answer of the job) is in
//! `state/games_export_flow.rs`; the blocking work is [`work`]. The export
//! is a job of the game list (`ListJobKind::ExportGames`), so it shares the
//! progress, the cancellation, the "one job at a time" rule and the quit
//! handling of the other list jobs. The databases are only read.

mod work;

use std::path::{Path, PathBuf};

use mychess_db::Filter;
use mychess_pgn::write::WriteOptions;

use crate::bases::{BaseError, SharedSource};
use crate::error::RequestId;
use crate::shared::SharedGame;

pub(crate) use work::{collect_numbers, export_run, fix_destination, is_protected};

/// The most skipped games listed in the report (D2004); the others are only
/// counted.
pub const MAX_SKIPPED_LINES: usize = 30;

/// What to export (D2001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExportScope {
    /// Every game of the database.
    All,
    /// The games of the current filter.
    Filter,
    /// The games selected in the list.
    Selection,
    /// The game on the board, with its unsaved edits.
    Current,
}

/// What the dialog of options holds (D2001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportDraft {
    /// What to export.
    pub scope: ExportScope,
    /// Write the comments.
    pub comments: bool,
    /// Write the variations.
    pub variations: bool,
    /// Keep the `[%...]` commands (`[%eval]`, `[%clk]`...) in the comments.
    pub marks: bool,
    /// Only the games with the Tactics (`T`) flag.
    pub only_tactics: bool,
    /// Add the games to the end of the file instead of replacing it.
    pub append: bool,
}

/// Events of the export of games.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GamesExportMessage {
    /// "Export games...": opens the dialog of options.
    Open,
    /// Chooses what to export (ignored when it has no game).
    SetScope(ExportScope),
    /// Comments in or out.
    SetComments(bool),
    /// Variations in or out.
    SetVariations(bool),
    /// `[%...]` commands in or out.
    SetMarks(bool),
    /// Only the games flagged `T`.
    SetOnlyTactics(bool),
    /// Append to the file.
    SetAppend(bool),
    /// "Choose destination...": asks for the file and starts the job.
    Choose,
    /// Closes the window (while the job runs only `ListMessage::CancelJob`
    /// acts).
    Close,
}

/// The games a job writes, in the order of the database (D2007).
#[derive(Debug, Clone, PartialEq)]
pub enum ExportList {
    /// The games of a filter, ascending.
    Filter(Box<Filter>),
    /// These games, ascending.
    Numbers(Vec<u32>),
    /// A game in memory.
    Current(SharedGame),
}

impl ExportList {
    /// How many games the list holds.
    pub fn count(&self) -> u64 {
        match self {
            ExportList::Filter(filter) => u64::from(filter.count()),
            ExportList::Numbers(numbers) => u64::try_from(numbers.len()).unwrap_or(u64::MAX),
            ExportList::Current(_) => 1,
        }
    }
}

/// What [`Effect::ExportGames`](crate::Effect::ExportGames) does.
#[derive(Debug, Clone, PartialEq)]
pub struct GamesExportJob {
    /// The database that holds the games (not used by `Current`).
    pub source: SharedSource,
    /// The games.
    pub list: ExportList,
    /// Keep only the games with the `T` flag (D2008).
    pub only_tactics: bool,
    /// The file, as the user chose it, already fixed by D2012.
    pub dest: PathBuf,
    /// The user confirmed exactly `dest` (the dialog asked about replacing
    /// it); if the name was changed, an existing file is never replaced.
    pub confirmed: bool,
    /// Add to the end of the file (D2011).
    pub append: bool,
    /// How each game is written (D2009).
    pub write: WriteOptions,
    /// The files of the databases that are open (D2013).
    pub protected: Vec<PathBuf>,
}

/// Why a game was not exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExportSkipCause {
    /// The game could not be read from the database.
    Unreadable,
    /// The game could not be written as PGN.
    Unwritable,
}

impl ExportSkipCause {
    /// The Fluent key of the cause.
    pub fn message_key(self) -> &'static str {
        match self {
            ExportSkipCause::Unreadable => "export-games-skip-unreadable",
            ExportSkipCause::Unwritable => "export-games-skip-unwritable",
        }
    }
}

/// The totals of an export and its first remarks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GamesExportReport {
    /// Games written.
    pub written: u64,
    /// Games left out because they have no `T` flag (D2008).
    pub excluded: u64,
    /// Games that could not be exported.
    pub skipped: u64,
    /// The first of them: number in the database and why (at most
    /// [`MAX_SKIPPED_LINES`]).
    pub lines: Vec<(u32, ExportSkipCause)>,
    /// The user stopped the job. A new file was then thrown away; an
    /// appended one keeps what was added (D2010, D2011).
    pub interrupted: bool,
    /// The games went to the end of an existing file.
    pub appended: bool,
    /// EPD: positions left out because an earlier game had the same one.
    pub repeated: u64,
    /// EPD: games left out because they have no position to give.
    pub not_selected: u64,
    /// The viewer: how many volume files were written.
    pub volumes: u32,
    /// Why nothing was written, besides the errors of the files (spec
    /// M7-4).
    pub problem: Option<crate::export_docs::ExportProblem>,
    /// The job wrote nothing because the file needs the user's word first
    /// (spec M7-4, D3750).
    pub confirm: Option<crate::export_docs::ConfirmNeeded>,
}

/// What the job did (the answer of the effect).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamesExportOutcome {
    /// The file written (or that would have been).
    pub path: PathBuf,
    /// The totals.
    pub report: GamesExportReport,
}

/// How an export ended, for the report window (D2004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamesExportResult {
    /// The file.
    pub path: PathBuf,
    /// The totals; empty when the job failed before writing.
    pub report: GamesExportReport,
    /// Why nothing was written, or the export ended early.
    pub error: Option<BaseError>,
}

/// How many games each choice of the dialog has (D2001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportCounts {
    /// Games of the database.
    pub all: u32,
    /// Games of the filter.
    pub filter: u32,
    /// Games selected.
    pub selection: u32,
}

impl ExportCounts {
    /// Whether `scope` has something to export. The game on the board is
    /// always there.
    pub fn available(&self, scope: ExportScope) -> bool {
        match scope {
            ExportScope::All => self.all > 0,
            ExportScope::Filter => self.filter > 0,
            ExportScope::Selection => self.selection > 0,
            ExportScope::Current => true,
        }
    }
}

/// The step the window is in (D2017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExportStep {
    /// The options.
    Options,
    /// The file dialog is open.
    Picking(RequestId),
    /// The job runs.
    Running {
        /// The file.
        path: PathBuf,
    },
    /// The job ended.
    Done(Box<GamesExportResult>),
}

/// The window of the export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GamesExportWindow {
    pub(crate) draft: ExportDraft,
    pub(crate) counts: ExportCounts,
    /// The name of the database, to propose a file name.
    pub(crate) base_name: String,
    pub(crate) step: ExportStep,
}

/// The state of the export in [`AppState`](crate::AppState).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GamesExportUi {
    pub(crate) window: Option<GamesExportWindow>,
}

impl GamesExportUi {
    /// Whether the window is open.
    pub fn is_open(&self) -> bool {
        self.window.is_some()
    }
}

/// What the window shows.
#[derive(Debug, Clone)]
pub enum GamesExportStepView<'a> {
    /// The options.
    Options,
    /// Waiting for the file dialog.
    Picking,
    /// The job runs.
    Running {
        /// The file.
        path: &'a Path,
        /// Games handled and games to handle, from the job.
        progress: (u64, u64),
    },
    /// The report.
    Done(&'a GamesExportResult),
}

/// What the window draws.
#[derive(Debug, Clone)]
pub struct GamesExportView<'a> {
    /// The options.
    pub draft: ExportDraft,
    /// The games of each choice.
    pub counts: ExportCounts,
    /// The step.
    pub step: GamesExportStepView<'a>,
}

/// What the status bar says (D2004).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GamesExportNotice {
    /// The games were written.
    Exported {
        /// How many.
        count: u64,
        /// The name of the file.
        name: String,
    },
    /// Written, but some games were left out.
    ExportedWithSkips {
        /// How many were written.
        count: u64,
        /// How many were not.
        skipped: u64,
        /// The name of the file.
        name: String,
    },
    /// The user stopped it; the new file was thrown away.
    Stopped,
    /// The user stopped an append; `count` games were already in the file.
    StoppedAppended {
        /// How many.
        count: u64,
    },
    /// Nothing to write (no game with the flag, for instance).
    NothingToExport,
    /// Every game was skipped: the file was left as it was (D2021).
    AllSkipped {
        /// How many games could not be exported.
        skipped: u64,
    },
    /// The export failed.
    Failed(BaseError),
    /// The viewer was written (spec M7-4).
    ViewerExported {
        /// How many volume files.
        volumes: u32,
        /// How many games.
        count: u64,
        /// The name of the first file.
        name: String,
    },
    /// EPD positions were written; `repeated` equal ones were left out.
    EpdExported {
        /// How many lines.
        count: u64,
        /// How many repeated positions were left out.
        repeated: u64,
        /// The name of the file.
        name: String,
    },
    /// The format could not go on and nothing was written (the file is as it
    /// was).
    Problem(crate::export_docs::ExportProblem),
    /// The image of the board was saved.
    ImageSaved {
        /// The name of the file.
        name: String,
    },
    /// The image of the board could not be made or saved.
    ImageFailed(crate::export_docs::ImageFailure),
    /// The system did not open the exported file.
    OpenFailed,
}

impl GamesExportNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            GamesExportNotice::Exported { .. } => "export-games-notice-exported",
            GamesExportNotice::ExportedWithSkips { .. } => "export-games-notice-skips",
            GamesExportNotice::Stopped => "export-games-notice-stopped",
            GamesExportNotice::StoppedAppended { .. } => "export-games-notice-stopped-appended",
            GamesExportNotice::NothingToExport => "export-games-notice-nothing",
            GamesExportNotice::AllSkipped { .. } => "export-games-notice-all-skipped",
            GamesExportNotice::Failed(error) => error.message_key(),
            GamesExportNotice::ViewerExported { .. } => "export-notice-viewer",
            GamesExportNotice::EpdExported { .. } => "export-notice-epd",
            GamesExportNotice::Problem(problem) => problem.message_key(),
            GamesExportNotice::ImageSaved { .. } => "export-notice-image-saved",
            GamesExportNotice::ImageFailed(failure) => failure.message_key(),
            GamesExportNotice::OpenFailed => "export-notice-open-failed",
        }
    }
}

#[cfg(test)]
mod tests;
