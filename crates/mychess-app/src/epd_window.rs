//! The EPD window: up to four EPD files open, their records, the opcodes of
//! each position, the analysis by an engine, sorting and removing, and
//! searching (spec `docs/specs/m7-5-janela-epd.md`, slice M7-5b; Scid
//! `tcl/file/epd.tcl`).
//!
//! - `file`: the state of the open files ([`EpdUi`]) and of the analysis in
//!   progress;
//! - `message`: [`EpdMessage`], [`EpdEffect`], [`EpdNotice`] and the answers
//!   of the jobs;
//! - `work`: the blocking work (read, write, order, search, batch edits),
//!   run by [`perform`](crate::perform) off the interface thread;
//! - `run`: the pure state machine of the analysis of the positions;
//! - `paste`: the operations "Paste analysis" writes (`acd acn ce dm pv`);
//! - `prefs`: the `[epd]` table of the preferences;
//! - `view`: the snapshots the front-end draws.
//!
//! The flow is in `state/epd_flow.rs`. The books are read, written, ordered,
//! searched and edited in batches by effects; `update` only does the O(1)
//! edits of one record.

mod file;
mod message;
mod paste;
mod prefs;
mod run;
mod view;
mod work;

pub use file::EpdUi;
pub use message::{
    BatchDone, BatchOp, EpdCloseChoice, EpdEffect, EpdFileId, EpdMessage, EpdNotice, EpdSort,
    MAX_REPORT_LINES, OpenedEpd, SaveFailure, SavedEpd, SharedBook, SharedRows, SortKey,
    UnwritableKind,
};
/// The handle of a record and the diagnostics of a read, which the front-end
/// shows without depending on `mychess-pgn` (spec M7-5c).
pub use mychess_pgn::epd::{EpdDiagnostic, EpdDiagnosticKind, EpdIndex};
pub use paste::{
    ANALYSIS_OPCODES, EngineLine, analysis_operations, ce_and_dm, engine_opcode, pv_text,
};
pub use prefs::{AnnotateMode, DEFAULT_SECONDS, Dialect, EpdPrefs, MAX_RECENT_EPD, SECONDS_RANGE};
pub use run::{AnnotateConfig, RunError, RunOutcome, RunProgress, RunScope};
pub use view::{EpdRowView, EpdRunView, EpdTabView, EpdView};

pub(crate) use file::{
    ActiveRun, EpdFile, EpdUndo, Job, JobKind, LoadReport, PendingOpen, PickPurpose, RunStage,
};
pub(crate) use paste::{put_analysis, put_operation};
pub(crate) use run::{EpdRun, RunAction, RunOutput, is_epd_tag};
pub(crate) use view::{row_views, tab_view};
pub(crate) use work::perform;

/// The most EPD files open at once (`MAX_EPD` of the Scid, D3801).
pub const MAX_EPD_FILES: usize = 4;

/// `path` with `.epd` appended when the name has no extension (an empty one,
/// as in `foo.`, counts as none). Names with another extension (`foo.bar`,
/// `foo.EPD`, `foo.epd.gz`) stay as typed. It is for the names the user
/// types for a file that does not exist yet ("New" and "Save as"); an
/// existing file picked to be opened is never renamed.
///
/// Corner: a name that is only the extension (`.epd`, a hidden file) has
/// no extension for the standard library, so it becomes `.epd.epd`.
pub fn with_epd_extension(mut path: std::path::PathBuf) -> std::path::PathBuf {
    if path.extension().is_none_or(std::ffi::OsStr::is_empty) {
        path.set_extension("epd");
    }
    path
}

/// Whether `path` names an EPD file: `.epd` or `.epd.gz`, in any case
/// (spec M7-5, D3802).
pub fn is_epd_path(path: &std::path::Path) -> bool {
    let ext = |p: &std::path::Path| {
        p.extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
    };
    match ext(path).as_deref() {
        Some("epd") => true,
        Some("gz") => path
            .file_stem()
            .map(std::path::Path::new)
            .and_then(ext)
            .is_some_and(|e| e == "epd"),
        _ => false,
    }
}
