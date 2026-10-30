//! The state of the open EPD files (spec M7-5, D3801).

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use mychess_core::Position;
use mychess_pgn::epd::{EpdBook, EpdDiagnostic, EpdIndex, EpdOperation, TextWarning};

use super::message::{BatchOp, EpdFileId, EpdSort, SortKey};
use super::run::{AnnotateConfig, EpdRun};
use crate::engines::RunId;
use crate::error::RequestId;

/// What a job in flight on a file will do with its answer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum JobKind {
    /// Writing the file; `then_close` closes the tab when it is done and
    /// `as_path` is the new name of a "Save as".
    Save {
        then_close: bool,
        as_path: Option<PathBuf>,
    },
    /// A batch edit.
    Batch(BatchOp),
    /// A text search.
    Find,
}

/// A job that holds the book of a file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Job {
    pub(crate) request: RequestId,
    pub(crate) kind: JobKind,
}

/// The records an analysis changed, as they were before (one level of
/// undo, D3817).
#[derive(Debug, Clone, Default)]
pub(crate) struct EpdUndo {
    pub(crate) entries: Vec<(EpdIndex, Vec<EpdOperation>)>,
    pub(crate) touched: HashSet<EpdIndex>,
    pub(crate) dirty_before: bool,
}

/// What was found on the lines of a file that were not plain records.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LoadReport {
    pub(crate) diagnostics: Vec<EpdDiagnostic>,
    pub(crate) dropped: u64,
}

/// An open EPD file.
#[derive(Debug)]
pub(crate) struct EpdFile {
    pub(crate) id: EpdFileId,
    pub(crate) path: PathBuf,
    pub(crate) book: Arc<EpdBook>,
    pub(crate) read_only: bool,
    pub(crate) dirty: bool,
    /// Records removed since the book was last compacted.
    pub(crate) holes: usize,
    pub(crate) selected: Option<EpdIndex>,
    /// The text of the editor when it differs from the record.
    pub(crate) draft: Option<String>,
    /// The position a draft typed with no record selected is about (the
    /// board when the typing began).
    pub(crate) draft_anchor: Option<Position>,
    pub(crate) sort: EpdSort,
    pub(crate) filter: String,
    pub(crate) rows: Arc<Vec<EpdIndex>>,
    pub(crate) rows_request: Option<RequestId>,
    /// The rows no longer match the sort and the filter: a job is due.
    pub(crate) rows_stale: bool,
    pub(crate) find_text: String,
    pub(crate) find_hit: Option<Range<usize>>,
    pub(crate) report: LoadReport,
    pub(crate) job: Option<Job>,
    pub(crate) undo: Option<EpdUndo>,
    pub(crate) pending_remove: Option<EpdIndex>,
    /// Opcodes of the last commit that are not standard (kept).
    pub(crate) warnings: Vec<TextWarning>,
    /// How the last analysis on this file ended.
    pub(crate) last_run: Option<(super::run::RunOutcome, super::run::RunProgress)>,
}

impl EpdFile {
    pub(crate) fn new(
        id: EpdFileId,
        path: PathBuf,
        book: Arc<EpdBook>,
        read_only: bool,
        rows: Arc<Vec<EpdIndex>>,
        report: LoadReport,
    ) -> EpdFile {
        EpdFile {
            id,
            path,
            book,
            read_only,
            dirty: false,
            holes: 0,
            selected: None,
            draft: None,
            draft_anchor: None,
            sort: EpdSort::default(),
            filter: String::new(),
            rows,
            rows_request: None,
            rows_stale: false,
            find_text: String::new(),
            find_hit: None,
            report,
            job: None,
            undo: None,
            pending_remove: None,
            warnings: Vec::new(),
            last_run: None,
        }
    }

    /// Whether the list shows every record in the order of the file.
    pub(crate) fn natural_view(&self) -> bool {
        self.sort == EpdSort::default() && self.filter.trim().is_empty()
    }

    /// Whether a change of the operations of a record can change the rows
    /// (a filter looks at them; so does the order by `id` or `ce`).
    pub(crate) fn ops_affect_rows(&self) -> bool {
        !self.filter.trim().is_empty() || matches!(self.sort.key, SortKey::Id | SortKey::Eval)
    }

    /// Whether the book is held by a job (or about to be), so it must not
    /// be changed: a change would copy all of it.
    pub(crate) fn locked(&self) -> bool {
        self.job.is_some() || self.rows_request.is_some()
    }

    /// The position of `index` in the list.
    pub(crate) fn row_of(&self, index: EpdIndex) -> Option<usize> {
        let rows = &self.rows;
        if self.sort.key == SortKey::File {
            let found = if self.sort.descending {
                rows.binary_search_by(|probe| index.cmp(probe))
            } else {
                rows.binary_search(&index)
            };
            // The filter keeps the order of the file, so the search holds.
            return found.ok();
        }
        rows.iter().position(|i| *i == index)
    }

    /// The record the editor is about.
    pub(crate) fn selected_position(&self) -> Option<&Position> {
        self.selected
            .and_then(|i| self.book.get_by_index(i))
            .map(mychess_pgn::epd::EpdRecord::position)
    }

    /// The text the editor shows.
    pub(crate) fn editor_text(&self) -> String {
        if let Some(draft) = &self.draft {
            return draft.clone();
        }
        self.selected
            .and_then(|i| self.book.get_by_index(i))
            .map(mychess_pgn::epd::EpdRecord::operations_text)
            .unwrap_or_default()
    }

    /// Remaps the handles held by the state after a compaction.
    pub(crate) fn remap(&mut self, moved: &[(EpdIndex, EpdIndex)]) {
        let map = |index: EpdIndex| {
            moved
                .iter()
                .find(|(old, _)| *old == index)
                .map_or(index, |(_, new)| *new)
        };
        self.selected = self.selected.map(map);
        self.pending_remove = self.pending_remove.map(map);
        self.holes = 0;
        // Every other handle (rows, find hit, undo) is recomputed or dropped
        // by the caller.
    }
}

/// The analysis in progress.
#[derive(Debug)]
pub(crate) struct ActiveRun {
    pub(crate) file: EpdFileId,
    pub(crate) engine_run: RunId,
    pub(crate) engine_name: String,
    pub(crate) config: AnnotateConfig,
    pub(crate) stage: RunStage,
    /// The generation of the watchdog timer in flight.
    pub(crate) watch: u64,
    /// Events of the run's searches seen so far, and at the last tick.
    pub(crate) progress: u64,
    pub(crate) seen: u64,
    pub(crate) remaining: std::time::Duration,
    pub(crate) period: std::time::Duration,
    /// Whether the engine was analysing the board when the run took it.
    pub(crate) analyzing: bool,
}

#[derive(Debug)]
pub(crate) enum RunStage {
    /// Waiting for the engine to finish its handshake.
    Starting,
    /// Walking the records.
    Running(Box<EpdRun>),
}

/// What a file dialog of the EPD window is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickPurpose {
    Open,
    New,
    SaveAs,
}

/// A file being read or created.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PendingOpen {
    pub(crate) request: RequestId,
    pub(crate) path: PathBuf,
}

/// The state of the EPD window.
#[derive(Debug, Default)]
pub struct EpdUi {
    pub(crate) files: Vec<EpdFile>,
    pub(crate) active: Option<EpdFileId>,
    pub(crate) next_id: u64,
    pub(crate) opening: Option<PendingOpen>,
    pub(crate) pick: Option<(RequestId, PickPurpose)>,
    pub(crate) close_prompt: Option<EpdFileId>,
    pub(crate) run: Option<ActiveRun>,
    /// The position of the board the selection was last synchronised with.
    pub(crate) last_board: Option<Position>,
    /// Searches of an ended run that may still report (their reports are
    /// dropped).
    pub(crate) stale: Vec<(RunId, mychess_engine::SearchId)>,
    /// An engine whose run ended with a `stop` pending for a search that had
    /// not been labelled: its next unlabelled start is dropped.
    pub(crate) untagged: Option<RunId>,
}

impl EpdUi {
    pub(crate) fn file(&self, id: EpdFileId) -> Option<&EpdFile> {
        self.files.iter().find(|f| f.id == id)
    }

    pub(crate) fn file_mut(&mut self, id: EpdFileId) -> Option<&mut EpdFile> {
        self.files.iter_mut().find(|f| f.id == id)
    }

    pub(crate) fn active_file(&self) -> Option<&EpdFile> {
        self.active.and_then(|id| self.file(id))
    }

    pub(crate) fn active_file_mut(&mut self) -> Option<&mut EpdFile> {
        let id = self.active?;
        self.file_mut(id)
    }

    pub(crate) fn new_id(&mut self) -> EpdFileId {
        self.next_id = self.next_id.wrapping_add(1);
        EpdFileId(self.next_id)
    }

    /// Whether any file is open.
    pub fn any_open(&self) -> bool {
        !self.files.is_empty()
    }

    /// The files with unsaved changes.
    pub fn dirty_files(&self) -> Vec<EpdFileId> {
        self.files
            .iter()
            .filter(|f| f.dirty)
            .map(|f| f.id)
            .collect()
    }

    /// The files that would lose work if the program ended now: the dirty
    /// ones, the ones with an editor draft not yet written to the record,
    /// the ones a batch edit is working on and the one an analysis writes
    /// into.
    pub fn unsaved_files(&self) -> Vec<EpdFileId> {
        self.files
            .iter()
            .filter(|f| {
                f.dirty
                    || f.draft.is_some()
                    || f.job
                        .as_ref()
                        .is_some_and(|j| matches!(j.kind, JobKind::Batch(_)))
                    || self.run.as_ref().is_some_and(|r| r.file == f.id)
            })
            .map(|f| f.id)
            .collect()
    }

    /// Whether a question about a file (close or remove) is open.
    pub fn layer_open(&self) -> bool {
        // A question about a file that is gone covers nothing: the view
        // draws no card for it, so it must not hold the keyboard either.
        self.close_prompt
            .is_some_and(|id| self.files.iter().any(|f| f.id == id))
            || self.files.iter().any(|f| f.pending_remove.is_some())
    }
}
