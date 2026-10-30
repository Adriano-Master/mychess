//! Messages, effects, answers and notices of the player report (spec M7-3,
//! slice b, decisions M7-3/D3650-D3699).

use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use mychess_core::PartialDate;
use mychess_db::PlayerStats;
use mychess_db::report::{PlayerLabels, PlayerReport, PlayerReportOptions};

use super::prefs::{NameMatching, PlayerColor, PlayerReportPrefs, PlayerStart};
use crate::bases::{OpenControl, SharedSource};
use crate::charts::StatsPrefs;
use crate::error::RequestId;
use crate::opening_report::{
    Reference, ReportFormat, ReportPart, ReportSelector, SaveFailure, SharedTemp,
};
use crate::shared::SharedEco;
use crate::spelling_ui::Shared;

/// A finished report, shared between the state, the messages and the
/// effects (compared by identity).
#[derive(Debug, Clone)]
pub struct SharedPlayerReport(Arc<PlayerReport>);

impl SharedPlayerReport {
    pub(crate) fn new(report: PlayerReport) -> SharedPlayerReport {
        SharedPlayerReport(Arc::new(report))
    }
}

impl Deref for SharedPlayerReport {
    type Target = PlayerReport;

    fn deref(&self) -> &PlayerReport {
        &self.0
    }
}

impl PartialEq for SharedPlayerReport {
    fn eq(&self, other: &SharedPlayerReport) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SharedPlayerReport {}

/// What the dialog asks for ("OK" of "Generate player report").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRequest {
    /// The name typed (trimmed when the report is made).
    pub name: String,
    /// The colour of the player.
    pub color: PlayerColor,
    /// How the name is matched (M7-3/D3651).
    pub matching: NameMatching,
    /// The beginning of the games or the position of the board.
    pub start: PlayerStart,
    /// Empty the Clipbase and copy the games found to it (M7-3/D3658).
    pub copy_to_clipbase: bool,
}

/// An event of the player report.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PlayerReportMessage {
    /// "Player Report" of the Tools menu or the button of the player
    /// information: opens the dialog, filled with `player` (and the colour
    /// the game on the board gives it) or with the last choices.
    OpenDialog {
        /// The player to fill in; `None` takes the last one used.
        player: Option<String>,
    },
    /// "Cancel" of the dialog.
    CloseDialog,
    /// "OK" of the dialog: makes the report. With the Clipbase option and a
    /// Clipbase that has games, asks first
    /// ([`PlayerReportMessage::ConfirmClipbase`]).
    Generate(Box<PlayerRequest>),
    /// The answer to "the Clipbase will be emptied".
    ConfirmClipbase(bool),
    /// The answer to "replace the file?" asked when the extension added to
    /// the name of the file dialog hit an existing file (D4211).
    ConfirmSave(bool),
    /// "Update": makes the report again with the same choices, over the
    /// database in use now (M7-3/D3659).
    Refresh,
    /// "Stop" of the progress bar; the window opens with what was done.
    Stop,
    /// Closes the window and stops the job in flight.
    Close,
    /// The words of the documents, already translated.
    SetLabels(Box<PlayerLabels>),
    /// The options changed ("OK" of the options dialog); the report is not
    /// made again until [`PlayerReportMessage::Refresh`].
    SetPrefs(Box<PlayerReportPrefs>),
    /// "Defaults" of the options dialog.
    ResetPrefs,
    /// A click on an element of the text: the filter of the database of the
    /// report becomes the games of the selection (M7-3/D3653).
    Select(ReportSelector),
    /// A click on a game reference: loads game `n` at the position.
    LoadGame(u32),
    /// A click on the name of an opponent: opens his information (M7-3/D3661).
    OpenOpponent(String),
    /// Writes the report to `path` (the extension of the format is added
    /// unless the name has one).
    Save {
        /// The file chosen in the native dialog.
        path: PathBuf,
        /// The part of the report.
        part: ReportPart,
        /// The format.
        format: ReportFormat,
    },
    /// "View HTML": writes the report to a temporary file and opens it.
    ViewHtml,

    /// The answer of [`PlayerReportEffect::Compute`].
    Computed {
        /// The request it answers.
        request: RequestId,
        /// The report, or why there is none.
        outcome: Box<PlayerOutcome>,
    },
    /// The answer of [`PlayerReportEffect::Save`].
    Saved {
        /// The request it answers.
        request: RequestId,
        /// The file written, or why not.
        result: Result<PathBuf, SaveFailure>,
    },
    /// The answer of [`PlayerReportEffect::WriteHtml`].
    HtmlWritten {
        /// The request it answers.
        request: RequestId,
        /// The file, or `None` when it could not be written.
        result: Option<(PathBuf, SharedTemp)>,
    },
    /// The answer of [`PlayerReportEffect::OpenFile`].
    FileOpened {
        /// Whether the system took the file.
        ok: bool,
    },
}

/// Why a report could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlayerFailure {
    /// The name has no letter or digit.
    InvalidName,
    /// The position does not fit the search by position.
    UnsupportedPosition,
    /// The line to the position is not legal, or does not start from the
    /// standard position.
    BadLine,
    /// The database could not be read.
    Read,
    /// Something that should not happen (the thread failed).
    Internal,
}

/// What the generation came to.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PlayerOutcome {
    /// The report, complete or interrupted (`core.interrupted`).
    Ready {
        /// The report.
        report: SharedPlayerReport,
        /// The date of the generation.
        generated: PartialDate,
    },
    /// The report could not be made.
    Failed(PlayerFailure),
}

/// Everything the generation of a report needs, owned.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerComputeJob {
    pub(crate) request: RequestId,
    pub(crate) source: SharedSource,
    pub(crate) name: String,
    pub(crate) color: PlayerColor,
    pub(crate) matching: NameMatching,
    /// The line to the board position; `None` is the beginning of the games.
    pub(crate) reference: Option<Reference>,
    pub(crate) options: PlayerReportOptions,
    pub(crate) stats: StatsPrefs,
    pub(crate) eco: Option<SharedEco>,
    pub(crate) player_stats: Option<Shared<PlayerStats>>,
    pub(crate) today: Option<PartialDate>,
    /// Progress and cancellation (one bar, M7-3/D3669).
    pub(crate) control: OpenControl,
}

/// The data a document needs besides the report.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRenderJob {
    pub(crate) report: SharedPlayerReport,
    pub(crate) options: PlayerReportOptions,
    pub(crate) labels: Box<PlayerLabels>,
    pub(crate) part: ReportPart,
    pub(crate) format: ReportFormat,
    pub(crate) database_name: String,
    pub(crate) generated: PartialDate,
}

/// Writes a report to a file.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSaveJob {
    pub(crate) request: RequestId,
    pub(crate) path: PathBuf,
    /// The file dialog already confirmed the replacement of `path`.
    pub(crate) confirmed: bool,
    pub(crate) render: PlayerRenderJob,
    /// The files of the open databases, which are never overwritten.
    pub(crate) protected: Vec<PathBuf>,
}

/// What `perform` does for the player report.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PlayerReportEffect {
    /// Selects the games and makes the report on a blocking thread; answers
    /// with [`PlayerReportMessage::Computed`].
    Compute(Box<PlayerComputeJob>),
    /// Renders a document and writes it atomically; answers with
    /// [`PlayerReportMessage::Saved`].
    Save(Box<PlayerSaveJob>),
    /// Writes the HTML to a temporary file (in `dir`, or in a new temporary
    /// folder); answers with [`PlayerReportMessage::HtmlWritten`].
    WriteHtml {
        /// Correlates the answer.
        request: RequestId,
        /// The document.
        html: Box<PlayerRenderJob>,
        /// The folder of an earlier view, reused.
        dir: Option<SharedTemp>,
    },
    /// Opens a file with the program of the system; answers with
    /// [`PlayerReportMessage::FileOpened`].
    OpenFile {
        /// The file.
        path: PathBuf,
    },
}

/// A remark about the player report. The front-end translates from the
/// variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlayerReportNotice {
    /// The database is being reloaded (`preport-err-unavailable`).
    Unavailable,
    /// The database has no game (`preport-no-games`).
    NoGames,
    /// The name is empty or `?` (`preport-err-name`).
    InvalidName,
    /// There is no report on the window (`preport-err-no-report`).
    NoReport,
    /// The report no longer matches the database (`preport-stale`).
    Stale,
    /// Another operation holds the filter or the report (`preport-err-busy`).
    Busy,
    /// The report could not be made (`preport-err-failed`).
    Failed,
    /// The position has too many pieces for the search
    /// (`preport-err-position`).
    UnsupportedPosition,
    /// The line of the report is not legal (`preport-err-line`).
    BadLine,
    /// The report was interrupted (`preport-partial`).
    Interrupted,
    /// The filter was set (`preport-filter-set`).
    FilterSet,
    /// The report was written (`preport-saved`).
    Saved,
    /// The report was not written (`preport-err-save`).
    SaveFailed,
    /// The file is one of the open databases (`preport-err-save-base`).
    SaveProtected,
    /// The browser could not be started (`preport-err-view`).
    ViewFailed,
    /// The Clipbase option cannot be used on the Clipbase itself
    /// (`preport-err-clipbase`).
    ClipbaseSelf,
    /// The game on the board has no usable name (`preport-err-no-name`).
    NoGameName,
}

impl PlayerReportNotice {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            PlayerReportNotice::Unavailable => "preport-err-unavailable",
            PlayerReportNotice::NoGames => "preport-no-games",
            PlayerReportNotice::InvalidName => "preport-err-name",
            PlayerReportNotice::NoReport => "preport-err-no-report",
            PlayerReportNotice::Stale => "preport-stale",
            PlayerReportNotice::Busy => "preport-err-busy",
            PlayerReportNotice::Failed => "preport-err-failed",
            PlayerReportNotice::UnsupportedPosition => "preport-err-position",
            PlayerReportNotice::BadLine => "preport-err-line",
            PlayerReportNotice::Interrupted => "preport-partial",
            PlayerReportNotice::FilterSet => "preport-filter-set",
            PlayerReportNotice::Saved => "preport-saved",
            PlayerReportNotice::SaveFailed => "preport-err-save",
            PlayerReportNotice::SaveProtected => "preport-err-save-base",
            PlayerReportNotice::ViewFailed => "preport-err-view",
            PlayerReportNotice::ClipbaseSelf => "preport-err-clipbase",
            PlayerReportNotice::NoGameName => "preport-err-no-name",
        }
    }
}
