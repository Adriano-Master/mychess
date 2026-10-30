//! The EPD window in the state (spec M7-5, D3800-D3849).
//!
//! `update` here only does the O(1) edits of one record. Reading, writing,
//! ordering, searching and the batch edits of the books are the effects of
//! [`crate::epd_window::EpdEffect`], run by `perform` off the interface thread; their
//! answers carry the [`RequestId`] of the request and an old one is dropped.
//! While a job holds the book of a file (it shares it, read-only) the file
//! refuses edits, so the book is never copied on the interface thread.
//!
//! The analysis of the positions reuses the engine runs of the engine
//! management, like the annotation of games does: it takes a running UCI
//! engine (or starts one), stops its board analysis and gives it back at the
//! end. The machine of the run is [`crate::epd_window`]'s `run` module.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mychess_core::Game;
use mychess_engine::{EngineCommand, EngineEvent, Protocol};
use mychess_pgn::epd::{EpdBook, EpdIndex, EpdRecord};

use super::AppState;
use super::replace::game_from_position;
use crate::effect::Effect;
use crate::engines::{CommandKind, EngineRunState, Hotkey, RunId};
use crate::epd_window::{
    ActiveRun, AnnotateConfig, BatchDone, BatchOp, EpdCloseChoice, EpdEffect, EpdFile, EpdFileId,
    EpdMessage, EpdNotice, EpdPrefs, EpdRowView, EpdRun, EpdSort, EpdUndo, EpdView, Job, JobKind,
    LoadReport, MAX_EPD_FILES, OpenedEpd, PendingOpen, PickPurpose, RunAction, RunError,
    RunOutcome, RunOutput, RunStage, SaveFailure, SavedEpd, SharedBook, SharedRows, is_epd_tag,
    put_analysis, put_operation, with_epd_extension,
};
use crate::error::{FileError, RequestId};
use crate::files::GameOrigin;
use crate::flow::{AppNotice, Replacement};
use crate::host::{PickKind, PickSpec};
use crate::prefs::{PrefChange, PrefsMessage};
use crate::session::{GameMessage, Nav};
use crate::shared::SharedGame;

/// How long past its time limit a search may stay silent before the run
/// gives up on the engine.
const STALL_GRACE: Duration = Duration::from_secs(30);

/// How long the engine has to answer the `stop` of a cancelled run.
const CANCEL_GRACE: Duration = Duration::from_secs(10);

/// The oldest searches of ended runs kept in `EpdUi::stale`.
const STALE_LIMIT: usize = 16;

/// Removed records after which the book is compacted by a job.
const COMPACT_AT: usize = 1000;

/// Whether two paths name the same file on a system that may ignore case.
fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

impl AppState {
    fn epd_notice(&mut self, notice: EpdNotice) {
        self.notice = Some(AppNotice::Epd(notice));
    }

    /// Whether an analysis of positions is running (its engine starting
    /// included).
    pub fn epd_running(&self) -> bool {
        self.epd.run.is_some()
    }

    /// Whether `run` is the engine of the analysis of positions: the
    /// board's analysis does not drive it meanwhile.
    pub(crate) fn epd_uses(&self, run: RunId) -> bool {
        self.epd.run.as_ref().is_some_and(|r| r.engine_run == run)
    }

    /// The window: tabs, status and the state of the analysis. Cheap: it
    /// does not walk the records.
    pub fn epd_view(&self) -> EpdView {
        let board = self.session.position();
        // The tab shown (the active one, or the first): only it needs the
        // costly parts of the view.
        let shown = self
            .epd
            .active
            .filter(|id| self.epd.file(*id).is_some())
            .or_else(|| self.epd.files.first().map(|f| f.id));
        EpdView {
            tabs: self
                .epd
                .files
                .iter()
                .map(|f| crate::epd_window::tab_view(f, &self.epd, board, Some(f.id) == shown))
                .collect(),
            active: self.epd.active,
            max_files: MAX_EPD_FILES,
            opening: self.epd.opening.is_some(),
            close_prompt: self.epd.close_prompt,
            recent: self.prefs.epd.recent.clone(),
        }
    }

    /// Whether a question about a file (save before closing, remove a
    /// position) covers the window. Much cheaper than [`Self::epd_view`].
    pub fn epd_layer_open(&self) -> bool {
        self.epd.layer_open()
    }

    /// The rows `range` of the list of a file (for a virtualized list).
    pub fn epd_rows(&self, id: EpdFileId, range: std::ops::Range<usize>) -> Vec<EpdRowView> {
        self.epd
            .file(id)
            .map(|f| crate::epd_window::row_views(f, range))
            .unwrap_or_default()
    }

    /// What was found on the lines of a file when it was read, and how many
    /// more were counted but not kept.
    pub fn epd_diagnostics(
        &self,
        id: EpdFileId,
    ) -> Option<(&[mychess_pgn::epd::EpdDiagnostic], u64)> {
        self.epd
            .file(id)
            .map(|f| (f.report.diagnostics.as_slice(), f.report.dropped))
    }

    /// The files that would lose work if the program quit now: the
    /// dirty ones, the ones with an editor draft not yet confirmed, the ones
    /// a batch edit works on and the one an analysis writes into.
    pub fn epd_unsaved_files(&self) -> Vec<EpdFileId> {
        self.epd.unsaved_files()
    }

    /// The files of the window with unsaved changes (the front-end asks
    /// about them before it sends [`Message::Quit`](crate::Message::Quit)).
    pub fn epd_dirty_files(&self) -> Vec<EpdFileId> {
        self.epd.dirty_files()
    }

    // ---- dispatch ----

    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(crate) fn epd_message(&mut self, message: EpdMessage, effects: &mut Vec<Effect>) {
        match message {
            EpdMessage::Open => self.epd_ask(PickPurpose::Open, effects),
            EpdMessage::New => self.epd_ask(PickPurpose::New, effects),
            EpdMessage::OpenPath(path) => self.epd_start_open(path, false, effects),
            EpdMessage::CancelOpen => self.epd.opening = None,
            EpdMessage::Save => {
                if let Some(id) = self.epd.active {
                    self.epd_save(id, false, None, effects);
                } else {
                    self.epd_notice(EpdNotice::NoFile);
                }
            }
            EpdMessage::SaveAs => {
                if self.epd.active.is_some() {
                    self.epd_ask(PickPurpose::SaveAs, effects);
                } else {
                    self.epd_notice(EpdNotice::NoFile);
                }
            }
            EpdMessage::Close(id) => self.epd_close(id, effects),
            EpdMessage::AnswerClose(choice) => self.epd_answer_close(choice, effects),
            EpdMessage::Activate(id) => {
                if self.epd.file(id).is_some() && self.epd.active != Some(id) {
                    self.epd.active = Some(id);
                    self.epd.last_board = None;
                }
            }
            EpdMessage::ClearRecent => {
                self.epd_change_prefs(effects, |p| p.recent.clear());
            }
            EpdMessage::SetSort(sort) => self.epd_set_sort(sort),
            EpdMessage::SetFilter(text) => self.epd_set_filter(text),
            EpdMessage::Select(index) => {
                if let Some(id) = self.epd.active {
                    self.epd_select(id, Some(index));
                }
            }
            EpdMessage::GoTo(index) => self.epd_go_to(index, effects),
            EpdMessage::First => self.epd_walk(Walk::First, effects),
            EpdMessage::Last => self.epd_walk(Walk::Last, effects),
            EpdMessage::Next => self.epd_walk(Walk::Next, effects),
            EpdMessage::Prev => self.epd_walk(Walk::Prev, effects),
            EpdMessage::Random(n) => self.epd_walk(Walk::Random(n), effects),
            EpdMessage::Edit(text) => self.epd_edit(text),
            EpdMessage::Commit => {
                if let Some(id) = self.epd.active
                    && !self.epd_commit(id)
                {
                    self.epd_notice(EpdNotice::Busy);
                }
            }
            EpdMessage::AddPosition => self.epd_add_position(),
            EpdMessage::RemovePosition => self.epd_ask_remove(),
            EpdMessage::ConfirmRemove(yes) => self.epd_confirm_remove(yes, effects),
            EpdMessage::SortRecord => self.epd_sort_record(),
            EpdMessage::SortAll => self.epd_batch(BatchOp::SortAll, effects),
            EpdMessage::StripOpcode(opcode) => {
                let opcode = opcode.trim().to_owned();
                if !opcode.is_empty() {
                    self.epd_batch(BatchOp::Strip(opcode), effects);
                }
            }
            EpdMessage::PasteAnalysis(run) => self.epd_paste_analysis(run),
            EpdMessage::Deepest => self.epd_deepest(),
            EpdMessage::Find(text, forward) => self.epd_find(text, forward, effects),
            EpdMessage::Annotate(config) => self.epd_annotate_start(&config, effects),
            EpdMessage::StopAnnotate => self.epd_cancel(effects),
            EpdMessage::UndoRun => self.epd_undo_run(),
            EpdMessage::Tick { run, generation } => self.epd_tick(run, generation, effects),
            EpdMessage::Opened { request, result } => self.epd_opened(request, result, effects),
            EpdMessage::Saved { request, result } => self.epd_saved(request, result, effects),
            EpdMessage::Rows { request, rows } => self.epd_rows_done(request, rows),
            EpdMessage::Found { request, hit } => self.epd_found(request, hit),
            EpdMessage::Batch { request, result } => self.epd_batch_done(request, result),
        }
        self.epd_flush_rows(effects);
    }

    // ---- files ----

    /// Asks for a file name with the native dialog.
    fn epd_ask(&mut self, purpose: PickPurpose, effects: &mut Vec<Effect>) {
        if self.epd.pick.is_some() {
            return;
        }
        if purpose != PickPurpose::SaveAs && self.epd.files.len() >= MAX_EPD_FILES {
            return self.epd_notice(EpdNotice::TooMany { max: MAX_EPD_FILES });
        }
        let request = self.new_request();
        self.epd.pick = Some((request, purpose));
        let kind = match purpose {
            PickPurpose::Open => PickKind::OpenEpd,
            PickPurpose::New => PickKind::NewEpd,
            PickPurpose::SaveAs => PickKind::SaveEpd,
        };
        let initial_dir = self
            .prefs
            .epd
            .last_dir
            .clone()
            .or_else(|| self.prefs.files.last_open_dir.clone());
        let file_name = match purpose {
            PickPurpose::SaveAs => self
                .epd
                .active_file()
                .and_then(|f| f.path.file_name())
                .map(|n| n.to_string_lossy().into_owned()),
            PickPurpose::Open => None,
            PickPurpose::New => Some("positions.epd".to_owned()),
        };
        effects.push(Effect::PickFile(PickSpec {
            request,
            kind,
            initial_dir,
            file_name,
        }));
    }

    /// The answer of a file dialog. Returns whether it was the window's.
    pub(crate) fn epd_file_picked(
        &mut self,
        request: RequestId,
        path: Option<PathBuf>,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let Some((pending, purpose)) = self.epd.pick else {
            return false;
        };
        if pending != request {
            return false;
        }
        self.epd.pick = None;
        let Some(path) = path else {
            return true;
        };
        // Only the names typed for a file that does not exist yet get the
        // extension: an existing file picked to open is opened as it is.
        match purpose {
            PickPurpose::Open => self.epd_start_open(path, false, effects),
            PickPurpose::New => self.epd_start_open(with_epd_extension(path), true, effects),
            PickPurpose::SaveAs => {
                if let Some(id) = self.epd.active {
                    self.epd_save(id, false, Some(with_epd_extension(path)), effects);
                }
            }
        }
        true
    }

    /// Opens (or creates) a file: checks the limits, then reads it off the
    /// interface thread.
    pub(crate) fn epd_start_open(
        &mut self,
        path: PathBuf,
        create: bool,
        effects: &mut Vec<Effect>,
    ) {
        if self.epd.opening.is_some() {
            return self.epd_notice(EpdNotice::Busy);
        }
        if let Some(open) = self.epd.files.iter().find(|f| same_path(&f.path, &path)) {
            let id = open.id;
            self.epd.active = Some(id);
            return self.epd_notice(EpdNotice::AlreadyOpen);
        }
        if self.epd.files.len() >= MAX_EPD_FILES {
            return self.epd_notice(EpdNotice::TooMany { max: MAX_EPD_FILES });
        }
        let request = self.new_request();
        self.epd.opening = Some(PendingOpen {
            request,
            path: path.clone(),
        });
        effects.push(Effect::Epd(EpdEffect::Open {
            request,
            path,
            options: self.prefs.epd.dialect.options(),
            create,
        }));
    }

    fn epd_opened(
        &mut self,
        request: RequestId,
        result: Result<Box<OpenedEpd>, FileError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.epd.opening.as_ref().map(|o| o.request) != Some(request) {
            return;
        }
        self.epd.opening = None;
        let opened = match result {
            Ok(opened) => *opened,
            Err(error) => return self.epd_notice(EpdNotice::OpenFailed(error)),
        };
        // The same file by another name (a link, another case).
        if let Some(open) = self
            .epd
            .files
            .iter()
            .find(|f| same_path(&f.path, &opened.path))
        {
            let id = open.id;
            self.epd.active = Some(id);
            return self.epd_notice(EpdNotice::AlreadyOpen);
        }
        if self.epd.files.len() >= MAX_EPD_FILES {
            return self.epd_notice(EpdNotice::TooMany { max: MAX_EPD_FILES });
        }
        let skipped = u64::try_from(opened.diagnostics.len())
            .unwrap_or(u64::MAX)
            .saturating_add(opened.dropped);
        let id = self.epd.new_id();
        let path = opened.path.clone();
        self.epd.files.push(EpdFile::new(
            id,
            opened.path,
            Arc::clone(&opened.book.0),
            opened.read_only,
            Arc::clone(&opened.rows.0),
            LoadReport {
                diagnostics: opened.diagnostics,
                dropped: opened.dropped,
            },
        ));
        self.epd.active = Some(id);
        self.epd.last_board = None;
        self.epd_remember(&path, effects);
        if skipped > 0 {
            self.epd_notice(EpdNotice::SkippedLines { count: skipped });
        }
    }

    fn epd_remember(&mut self, path: &Path, effects: &mut Vec<Effect>) {
        let path = path.to_path_buf();
        self.epd_change_prefs(effects, move |p| p.remember(&path));
    }

    /// Changes the `[epd]` preferences (saved by the usual way).
    fn epd_change_prefs(&mut self, effects: &mut Vec<Effect>, change: impl FnOnce(&mut EpdPrefs)) {
        let mut prefs = (*self.prefs.epd).clone();
        change(&mut prefs);
        if prefs != *self.prefs.epd {
            self.prefs_message(PrefsMessage::Set(PrefChange::Epd(Box::new(prefs))), effects);
        }
    }

    /// Whether a change of the book of `id` may start; sets the notice
    /// when it may not.
    fn epd_can_edit(&mut self, id: EpdFileId) -> bool {
        let Some(file) = self.epd.file(id) else {
            self.epd_notice(EpdNotice::NoFile);
            return false;
        };
        let refusal = if file.read_only {
            Some(EpdNotice::ReadOnly)
        } else if self.epd.run.as_ref().is_some_and(|r| r.file == id) {
            Some(EpdNotice::RunActive)
        } else if file.locked() {
            Some(EpdNotice::Busy)
        } else {
            None
        };
        match refusal {
            Some(notice) => {
                self.epd_notice(notice);
                false
            }
            None => true,
        }
    }

    fn epd_save(
        &mut self,
        id: EpdFileId,
        then_close: bool,
        as_path: Option<PathBuf>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(file) = self.epd.file(id) else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        // "Save" over a read-only file is refused; "Save as" is not.
        if as_path.is_none() && file.read_only {
            return self.epd_notice(EpdNotice::ReadOnly);
        }
        if self.epd.run.as_ref().is_some_and(|r| r.file == id) {
            return self.epd_notice(EpdNotice::RunActive);
        }
        if file.locked() {
            return self.epd_notice(EpdNotice::Busy);
        }
        if let Some(other) = &as_path
            && self
                .epd
                .files
                .iter()
                .any(|f| f.id != id && same_path(&f.path, other))
        {
            return self.epd_notice(EpdNotice::AlreadyOpen);
        }
        if !self.epd_commit(id) {
            return self.epd_notice(EpdNotice::Busy);
        }
        let request = self.new_request();
        let options = self.prefs.epd.dialect.options();
        let Some(file) = self.epd.file_mut(id) else {
            return;
        };
        let path = as_path.clone().unwrap_or_else(|| file.path.clone());
        file.job = Some(Job {
            request,
            kind: JobKind::Save {
                then_close,
                as_path,
            },
        });
        effects.push(Effect::Epd(EpdEffect::Save {
            request,
            path,
            book: SharedBook(Arc::clone(&file.book)),
            options,
            compact: file.holes > 0,
        }));
    }

    fn epd_saved(
        &mut self,
        request: RequestId,
        result: Result<SavedEpd, SaveFailure>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(file) = self
            .epd
            .files
            .iter_mut()
            .find(|f| f.job.as_ref().is_some_and(|j| j.request == request))
        else {
            return;
        };
        let id = file.id;
        let Some(Job {
            kind: JobKind::Save {
                then_close,
                as_path,
            },
            ..
        }) = file.job.take()
        else {
            return;
        };
        match result {
            Err(SaveFailure::File(error)) => self.epd_notice(EpdNotice::SaveFailed(error)),
            Err(SaveFailure::Unwritable { opcode, kind }) => {
                self.epd_notice(EpdNotice::Unwritable { opcode, kind });
            }
            Ok(saved) => {
                let mut renamed = None;
                if let Some(file) = self.epd.file_mut(id) {
                    if let Some(book) = saved.book {
                        file.book = book.0;
                        file.remap(&saved.moved);
                        file.find_hit = None;
                        file.rows_stale = true;
                    }
                    if as_path.is_some() {
                        file.path = saved.path.clone();
                        file.read_only = false;
                        renamed = Some(saved.path.clone());
                    }
                    file.dirty = false;
                    file.undo = None;
                }
                if let Some(path) = renamed {
                    self.epd_remember(&path, effects);
                }
                if then_close {
                    self.epd_close_now(id, effects);
                }
            }
        }
    }

    fn epd_close(&mut self, id: EpdFileId, effects: &mut Vec<Effect>) {
        let Some(file) = self.epd.file(id) else {
            return;
        };
        if file.dirty && !file.read_only {
            self.epd.close_prompt = Some(id);
        } else {
            self.epd_close_now(id, effects);
        }
    }

    fn epd_answer_close(&mut self, choice: EpdCloseChoice, effects: &mut Vec<Effect>) {
        let Some(id) = self.epd.close_prompt.take() else {
            return;
        };
        match choice {
            EpdCloseChoice::Cancel => {}
            EpdCloseChoice::Discard => self.epd_close_now(id, effects),
            EpdCloseChoice::Save => self.epd_save(id, true, None, effects),
        }
    }

    fn epd_close_now(&mut self, id: EpdFileId, effects: &mut Vec<Effect>) {
        if self.epd.run.as_ref().is_some_and(|r| r.file == id) {
            self.epd_run_end(RunOutcome::Interrupted, true, effects);
        }
        let Some(at) = self.epd.files.iter().position(|f| f.id == id) else {
            return;
        };
        // Dropping the file drops its book: nothing stays open (Windows
        // refuses to rename or delete a file that is).
        self.epd.files.remove(at);
        if self.epd.close_prompt == Some(id) {
            self.epd.close_prompt = None;
        }
        if self.epd.active == Some(id) {
            let next = at.min(self.epd.files.len().saturating_sub(1));
            self.epd.active = self.epd.files.get(next).map(|f| f.id);
            self.epd.last_board = None;
        }
    }

    // ---- the list ----

    fn epd_set_sort(&mut self, sort: EpdSort) {
        if let Some(file) = self.epd.active_file_mut()
            && file.sort != sort
        {
            file.sort = sort;
            file.rows_stale = true;
        }
    }

    fn epd_set_filter(&mut self, text: String) {
        if let Some(file) = self.epd.active_file_mut()
            && file.filter != text
        {
            file.filter = text;
            file.rows_stale = true;
        }
    }

    /// Starts the job that orders and filters the rows of the files that
    /// need it (one at a time per file, none while a job or the analysis
    /// holds the file).
    fn epd_flush_rows(&mut self, effects: &mut Vec<Effect>) {
        let running = self.epd.run.as_ref().map(|r| r.file);
        let due: Vec<EpdFileId> = self
            .epd
            .files
            .iter()
            .filter(|f| f.rows_stale && f.job.is_none() && running != Some(f.id))
            .map(|f| f.id)
            .collect();
        for id in due {
            let request = self.new_request();
            let Some(file) = self.epd.file_mut(id) else {
                continue;
            };
            file.rows_stale = false;
            file.rows_request = Some(request);
            effects.push(Effect::Epd(EpdEffect::Rows {
                request,
                book: SharedBook(Arc::clone(&file.book)),
                sort: file.sort,
                filter: file.filter.clone(),
            }));
        }
    }

    fn epd_rows_done(&mut self, request: RequestId, rows: SharedRows) {
        if let Some(file) = self
            .epd
            .files
            .iter_mut()
            .find(|f| f.rows_request == Some(request))
        {
            file.rows_request = None;
            file.rows = rows.0;
        }
    }

    /// Selects a record (or none), writing the draft of the one left
    /// first. Returns whether the selection changed or was already there.
    fn epd_select(&mut self, id: EpdFileId, index: Option<EpdIndex>) -> bool {
        let Some(file) = self.epd.file(id) else {
            return false;
        };
        if let Some(index) = index
            && file.book.get_by_index(index).is_none()
        {
            return false;
        }
        if file.selected == index {
            return true;
        }
        if file.draft.is_some() && !self.epd_commit(id) {
            self.epd_notice(EpdNotice::Busy);
            return false;
        }
        let Some(file) = self.epd.file_mut(id) else {
            return false;
        };
        // The commit may have created the record that was typed for.
        file.selected = index;
        file.find_hit = None;
        file.pending_remove = None;
        file.warnings.clear();
        true
    }

    /// Selects a record and loads its position as a new game.
    fn epd_go_to(&mut self, index: EpdIndex, effects: &mut Vec<Effect>) {
        let Some(id) = self.epd.active else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        if !self.epd_select(id, Some(index)) {
            return;
        }
        let Some(position) = self
            .epd
            .file(id)
            .and_then(|f| f.book.get_by_index(index))
            .map(|r| r.position().clone())
        else {
            return;
        };
        let game: Game = game_from_position(position);
        self.begin_replace(
            Replacement::Loaded {
                game: SharedGame::new(game),
                origin: GameOrigin::EpdRecord { file: id, index },
                dirty: false,
                collection: None,
            },
            effects,
        );
    }

    fn epd_walk(&mut self, walk: Walk, effects: &mut Vec<Effect>) {
        let Some(file) = self.epd.active_file() else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        let rows = &file.rows;
        let target = match (walk, file.selected.and_then(|s| file.row_of(s))) {
            (Walk::First, _) | (Walk::Next, None) => rows.first().copied(),
            (Walk::Last, _) | (Walk::Prev, None) => rows.last().copied(),
            (Walk::Next, Some(row)) => rows.get(row + 1).or_else(|| rows.first()).copied(),
            (Walk::Prev, Some(row)) => row
                .checked_sub(1)
                .and_then(|r| rows.get(r))
                .or_else(|| rows.last())
                .copied(),
            (Walk::Random(n), _) => {
                let len = u64::try_from(rows.len()).unwrap_or(u64::MAX);
                n.checked_rem(len)
                    .and_then(|r| usize::try_from(r).ok())
                    .and_then(|r| rows.get(r))
                    .copied()
            }
        };
        match target {
            Some(index) => self.epd_go_to(index, effects),
            None => self.epd_notice(EpdNotice::NoRecord),
        }
    }

    // ---- the record ----

    fn epd_edit(&mut self, text: String) {
        let Some(id) = self.epd.active else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        let board = self.session.position().clone();
        let running = self.epd.run.as_ref().is_some_and(|r| r.file == id);
        let Some(file) = self.epd.file_mut(id) else {
            return;
        };
        if file.read_only {
            return self.epd_notice(EpdNotice::ReadOnly);
        }
        if running {
            return self.epd_notice(EpdNotice::RunActive);
        }
        if file.draft.is_none() && file.selected.is_none() {
            file.draft_anchor = Some(board);
        }
        file.draft = Some(text);
    }

    /// Writes the draft of `id` into its record, or creates the record when
    /// none is selected (the Scid's `sc_epd set`). Returns `false` when a
    /// job holds the book and nothing was done.
    fn epd_commit(&mut self, id: EpdFileId) -> bool {
        let board = self.session.position().clone();
        let Some(file) = self.epd.file_mut(id) else {
            return true;
        };
        let Some(text) = file.draft.clone() else {
            return true;
        };
        if file.read_only {
            file.draft = None;
            file.draft_anchor = None;
            return true;
        }
        if file.locked() {
            return false;
        }
        file.draft = None;
        let anchor = file.draft_anchor.take();
        let position = file
            .selected_position()
            .cloned()
            .or(anchor)
            .unwrap_or(board);
        let natural = file.natural_view();
        let book = Arc::make_mut(&mut file.book);
        let mut changed = false;
        let mut created = None;
        if let Some(record) = book.get_mut(&position) {
            let before = record.operations().to_vec();
            file.warnings = record.set_operations_text(&text);
            changed = record.operations() != before.as_slice();
        } else if !text.trim().is_empty() {
            let mut record = EpdRecord::new(position.clone());
            file.warnings = record.set_operations_text(&text);
            book.insert(position.clone(), record.operations().to_vec());
            created = book.index_of(&position);
            changed = true;
        }
        if let Some(index) = created {
            file.selected = Some(index);
            if natural {
                Arc::make_mut(&mut file.rows).push(index);
            } else {
                file.rows_stale = true;
            }
        }
        if changed {
            file.dirty = true;
            file.undo = None;
            if file.ops_affect_rows() {
                file.rows_stale = true;
            }
        }
        if let Some(opcode) = file.warnings.iter().find_map(|w| match w {
            mychess_pgn::epd::TextWarning::NonStandardOpcode { opcode, .. } => Some(opcode.clone()),
            _ => None,
        }) {
            self.epd_notice(EpdNotice::NonStandardOpcode { opcode });
        }
        true
    }

    fn epd_add_position(&mut self) {
        let Some(id) = self.epd.active else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        if !self.epd_can_edit(id) {
            return;
        }
        if !self.epd_commit(id) {
            return self.epd_notice(EpdNotice::Busy);
        }
        let position = self.session.position().clone();
        let Some(file) = self.epd.file_mut(id) else {
            return;
        };
        if let Some(existing) = file.book.index_of(&position) {
            file.selected = Some(existing);
            file.find_hit = None;
            return;
        }
        let natural = file.natural_view();
        Arc::make_mut(&mut file.book).append_position(&position);
        file.dirty = true;
        file.undo = None;
        file.find_hit = None;
        file.selected = file.book.index_of(&position);
        match (natural, file.selected) {
            (true, Some(index)) => Arc::make_mut(&mut file.rows).push(index),
            _ => file.rows_stale = true,
        }
        self.epd.last_board = Some(position);
    }

    fn epd_ask_remove(&mut self) {
        let Some(id) = self.epd.active else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        if !self.epd_can_edit(id) {
            return;
        }
        let Some(file) = self.epd.file_mut(id) else {
            return;
        };
        match file.selected {
            Some(index) => file.pending_remove = Some(index),
            None => self.epd_notice(EpdNotice::NoRecord),
        }
    }

    fn epd_confirm_remove(&mut self, yes: bool, effects: &mut Vec<Effect>) {
        let Some(id) = self.epd.active else {
            return;
        };
        let pending = self.epd.file_mut(id).and_then(|f| f.pending_remove.take());
        let Some(index) = pending.filter(|_| yes) else {
            return;
        };
        if !self.epd_can_edit(id) {
            return;
        }
        let request = self.new_request();
        let Some(file) = self.epd.file_mut(id) else {
            return;
        };
        // The record that takes its place on the screen.
        let row = file.row_of(index);
        let neighbour = row.and_then(|r| {
            file.rows
                .get(r + 1)
                .or_else(|| r.checked_sub(1).and_then(|p| file.rows.get(p)))
                .copied()
        });
        if Arc::make_mut(&mut file.book).remove_at(index).is_none() {
            return;
        }
        file.holes += 1;
        file.dirty = true;
        file.undo = None;
        file.find_hit = None;
        file.draft = None;
        file.draft_anchor = None;
        Arc::make_mut(&mut file.rows).retain(|i| *i != index);
        file.selected = neighbour;
        // Many removals: reclaim the holes off the interface thread.
        if file.holes >= COMPACT_AT {
            file.job = Some(Job {
                request,
                kind: JobKind::Batch(BatchOp::Compact),
            });
            effects.push(Effect::Epd(EpdEffect::Batch {
                request,
                book: SharedBook(Arc::clone(&file.book)),
                op: BatchOp::Compact,
            }));
        }
    }

    fn epd_sort_record(&mut self) {
        let Some(id) = self.epd.active else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        if !self.epd_can_edit(id) {
            return;
        }
        if !self.epd_commit(id) {
            return self.epd_notice(EpdNotice::Busy);
        }
        let Some(file) = self.epd.file_mut(id) else {
            return;
        };
        let Some(position) = file.selected_position().cloned() else {
            return self.epd_notice(EpdNotice::NoRecord);
        };
        // The order of the book's own sort, on a book of one record.
        let Some(before) = file.book.get(&position).map(|r| r.operations().to_vec()) else {
            return;
        };
        let mut single = EpdBook::new();
        single.insert(position.clone(), before);
        let changed = single.sort_operations();
        if changed > 0
            && let Some(sorted) = single.get(&position).map(|r| r.operations().to_vec())
            && let Some(record) = Arc::make_mut(&mut file.book).get_mut(&position)
        {
            *record.operations_mut() = sorted;
            file.dirty = true;
            file.undo = None;
        }
        self.epd_notice(EpdNotice::Sorted { count: changed });
    }

    /// A batch edit of every record, on a copy, off the interface thread.
    fn epd_batch(&mut self, op: BatchOp, effects: &mut Vec<Effect>) {
        let Some(id) = self.epd.active else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        if !self.epd_can_edit(id) {
            return;
        }
        if !self.epd_commit(id) {
            return self.epd_notice(EpdNotice::Busy);
        }
        let request = self.new_request();
        let Some(file) = self.epd.file_mut(id) else {
            return;
        };
        file.job = Some(Job {
            request,
            kind: JobKind::Batch(op.clone()),
        });
        effects.push(Effect::Epd(EpdEffect::Batch {
            request,
            book: SharedBook(Arc::clone(&file.book)),
            op,
        }));
    }

    fn epd_batch_done(&mut self, request: RequestId, result: Result<BatchDone, FileError>) {
        let Some(file) = self
            .epd
            .files
            .iter_mut()
            .find(|f| f.job.as_ref().is_some_and(|j| j.request == request))
        else {
            return;
        };
        let Some(Job {
            kind: JobKind::Batch(op),
            ..
        }) = file.job.take()
        else {
            return;
        };
        let Ok(done) = result else {
            return self.epd_notice(EpdNotice::JobFailed);
        };
        let compact = matches!(op, BatchOp::Compact);
        if let Some(book) = done.book {
            file.book = book.0;
            if compact {
                file.remap(&done.moved);
                file.find_hit = None;
                file.rows_stale = true;
            } else if done.changed > 0 {
                file.dirty = true;
                file.undo = None;
                file.draft = None;
                if file.ops_affect_rows() {
                    file.rows_stale = true;
                }
            }
        }
        let notice = match op {
            BatchOp::SortAll => Some(EpdNotice::Sorted {
                count: done.changed,
            }),
            BatchOp::Strip(opcode) => Some(EpdNotice::Stripped {
                opcode,
                count: done.changed,
            }),
            BatchOp::Compact => None,
        };
        if let Some(notice) = notice {
            self.epd_notice(notice);
        }
    }

    // ---- paste the analysis ----

    fn epd_paste_analysis(&mut self, wanted: Option<RunId>) {
        let Some(id) = self.epd.active else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        if !self.epd_can_edit(id) {
            return;
        }
        if !self.epd_commit(id) {
            return self.epd_notice(EpdNotice::Busy);
        }
        let Some(position) = self
            .epd
            .file(id)
            .and_then(|f| f.selected_position().cloned())
        else {
            return self.epd_notice(EpdNotice::NoRecord);
        };
        // The best line of the analysis of this very position.
        let line =
            self.engine_runs
                .as_slice()
                .iter()
                .filter(|r| wanted.is_none_or(|w| w == r.id))
                .filter_map(|r| self.analysis.get(r.id))
                .filter(|view| view.position() == Some(&position))
                .find_map(|view| {
                    view.lines.iter().find(|l| l.multipv == 1).map(|l| {
                        crate::epd_window::EngineLine {
                            score: l.score,
                            depth: l.depth,
                            nodes: view.stats.nodes,
                            pv: l.pv.clone(),
                        }
                    })
                });
        let Some(line) = line else {
            return self.epd_notice(EpdNotice::NoAnalysis);
        };
        let operations = crate::epd_window::analysis_operations(&position, &line);
        let Some(file) = self.epd.file_mut(id) else {
            return;
        };
        if let Some(record) = Arc::make_mut(&mut file.book).get_mut(&position)
            && put_analysis(record, operations)
        {
            file.dirty = true;
            file.undo = None;
            if file.ops_affect_rows() {
                file.rows_stale = true;
            }
        }
    }

    /// Goes to the deepest position of the game that the file has.
    fn epd_deepest(&mut self) {
        let Some(file) = self.epd.active_file() else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        let Some(ply) = file.book.deepest_match(self.session.game()) else {
            return self.epd_notice(EpdNotice::NoDeepest);
        };
        if self.refuse_edit() || self.modal_open() {
            return;
        }
        drop(
            self.session
                .update(GameMessage::Navigate(Nav::GoToMainlinePly(ply))),
        );
    }

    // ---- searching ----

    fn epd_find(&mut self, text: String, forward: bool, effects: &mut Vec<Effect>) {
        let Some(id) = self.epd.active else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        if self.epd.file(id).is_some_and(EpdFile::locked) {
            return self.epd_notice(EpdNotice::Busy);
        }
        if self.epd.run.as_ref().is_some_and(|r| r.file == id) {
            return self.epd_notice(EpdNotice::RunActive);
        }
        if !self.epd_commit(id) {
            return self.epd_notice(EpdNotice::Busy);
        }
        let request = self.new_request();
        let Some(file) = self.epd.file_mut(id) else {
            return;
        };
        file.find_text.clone_from(&text);
        file.find_hit = None;
        if text.is_empty() {
            return;
        }
        file.job = Some(Job {
            request,
            kind: JobKind::Find,
        });
        effects.push(Effect::Epd(EpdEffect::Find {
            request,
            book: SharedBook(Arc::clone(&file.book)),
            from: file.selected,
            needle: text,
            forward,
        }));
    }

    fn epd_found(&mut self, request: RequestId, hit: Option<(EpdIndex, std::ops::Range<usize>)>) {
        let Some(file) = self
            .epd
            .files
            .iter_mut()
            .find(|f| f.job.as_ref().is_some_and(|j| j.request == request))
        else {
            return;
        };
        let id = file.id;
        file.job = None;
        match hit {
            None => {
                file.find_hit = None;
                self.epd_notice(EpdNotice::NotFound);
            }
            Some((index, range)) => {
                if self.epd_select(id, Some(index))
                    && let Some(file) = self.epd.file_mut(id)
                {
                    file.find_hit = Some(range);
                }
            }
        }
    }

    // ---- undo of the analysis ----

    fn epd_undo_run(&mut self) {
        let Some(id) = self.epd.active else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        if self.epd.file(id).is_some_and(|f| f.undo.is_none()) {
            return self.epd_notice(EpdNotice::NoRunToUndo);
        }
        if !self.epd_can_edit(id) {
            return;
        }
        let Some(file) = self.epd.file_mut(id) else {
            return;
        };
        let Some(undo) = file.undo.take() else {
            return;
        };
        let book = Arc::make_mut(&mut file.book);
        for (index, operations) in undo.entries {
            let Some(position) = book.get_by_index(index).map(|r| r.position().clone()) else {
                continue;
            };
            if let Some(record) = book.get_mut(&position) {
                *record.operations_mut() = operations;
            }
        }
        file.dirty = undo.dirty_before;
        file.draft = None;
        if file.ops_affect_rows() {
            file.rows_stale = true;
        }
        self.epd_notice(EpdNotice::RunUndone);
    }

    // ---- the selection follows the board ----

    /// Selects the record of the position of the board (none when the file
    /// does not have it) whenever the board changes (spec section 2).
    pub(crate) fn refresh_epd(&mut self, effects: &mut Vec<Effect>) {
        if self.epd.files.is_empty() {
            return;
        }
        self.epd_sync_board();
        self.epd_flush_rows(effects);
    }

    fn epd_sync_board(&mut self) {
        if self.epd.run.is_some() {
            return;
        }
        let board = self.session.position();
        if self.epd.last_board.as_ref() == Some(board) {
            return;
        }
        let Some(id) = self.epd.active else {
            return;
        };
        let board = board.clone();
        // Write what was typed for the record being left; if a job holds
        // the book, try again at the next message.
        if self.epd.file(id).is_some_and(|f| f.draft.is_some()) && !self.epd_commit(id) {
            return;
        }
        if let Some(file) = self.epd.file_mut(id) {
            let found = file.book.index_of(&board);
            if file.selected != found {
                file.selected = found;
                file.find_hit = None;
                file.pending_remove = None;
            }
        }
        self.epd.last_board = Some(board);
    }

    // ---- quit ----

    /// The program is quitting: the analysis stops and the answers of the
    /// jobs in flight are dropped.
    pub(crate) fn epd_quit(&mut self, effects: &mut Vec<Effect>) {
        if self.epd.run.is_some() {
            self.epd_run_end(RunOutcome::Interrupted, true, effects);
        }
        self.epd.opening = None;
        self.epd.pick = None;
        for file in &mut self.epd.files {
            file.job = None;
            file.rows_request = None;
        }
    }

    // ---- the analysis of the positions ----

    fn epd_annotate_start(&mut self, config: &AnnotateConfig, effects: &mut Vec<Effect>) {
        if self.epd.run.is_some() {
            return self.epd_notice(EpdNotice::RunActive);
        }
        let Some(id) = self.epd.active else {
            return self.epd_notice(EpdNotice::NoFile);
        };
        let writes = config.mode.annotates();
        if writes && !self.epd_can_edit(id) {
            return;
        }
        if self.epd.file(id).is_some_and(EpdFile::locked) {
            return self.epd_notice(EpdNotice::Busy);
        }
        // The engines belong to one use at a time.
        if self.annotation_running()
            || self.play_live()
            || self.tournament_live()
            || self.training_playing()
        {
            return self.epd_notice(EpdNotice::EngineBusy);
        }
        if !self.epd_commit(id) {
            return self.epd_notice(EpdNotice::Busy);
        }
        if self.epd.file(id).is_some_and(|f| f.book.is_empty()) {
            return self.epd_notice(EpdNotice::NothingToAnalyze);
        }
        let seconds = config.seconds.clamp(
            *crate::epd_window::SECONDS_RANGE.start(),
            *crate::epd_window::SECONDS_RANGE.end(),
        );
        let config = AnnotateConfig {
            seconds,
            ..config.clone()
        };
        let engine_run = match self.epd_pick_engine(config.engine, effects) {
            Ok(run) => run,
            Err(notice) => return self.epd_notice(notice),
        };
        let engine_name = self
            .engine_runs
            .get(engine_run)
            .and_then(|r| self.engines.get(r.engine))
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let ready = self
            .engine_runs
            .get(engine_run)
            .is_some_and(|r| r.state == EngineRunState::Running);
        self.epd.run = Some(ActiveRun {
            file: id,
            engine_run,
            engine_name,
            config: config.clone(),
            stage: RunStage::Starting,
            watch: 0,
            progress: 0,
            seen: 0,
            remaining: Duration::ZERO,
            period: Duration::ZERO,
            analyzing: false,
        });
        let engine = config.engine.map(|e| e.0);
        let (mode, secs) = (config.mode, config.seconds);
        self.epd_change_prefs(effects, move |p| {
            p.annotate_mode = mode;
            p.annotate_seconds = secs;
            if engine.is_some() {
                p.annotate_engine = engine;
            }
        });
        if ready {
            self.epd_begin(effects);
        }
    }

    /// The execution of the engine the analysis will use: a running UCI
    /// one (the wanted one first), or a new one of the wanted engine or of
    /// F2.
    fn epd_pick_engine(
        &mut self,
        wanted: Option<crate::engines::EngineId>,
        effects: &mut Vec<Effect>,
    ) -> Result<RunId, EpdNotice> {
        let free = |state: &AppState, run: &crate::engines::EngineRun| {
            !state.play_uses(run.id)
                && !state.training_uses(run.id)
                && !state.annotation_uses(run.id)
        };
        let uci = |state: &AppState, engine| {
            state
                .engines
                .get(engine)
                .is_some_and(|e| e.protocol == Protocol::Uci)
        };
        if let Some(engine) = wanted
            && self.engines.get(engine).is_some()
            && !uci(self, engine)
        {
            return Err(EpdNotice::UciOnly);
        }
        let running = |state: &AppState, engine| {
            state
                .engine_runs
                .of_engine(engine)
                .filter(|r| r.state == EngineRunState::Running && free(state, r))
                .map(|r| r.id)
        };
        let ready = wanted.and_then(|e| running(self, e)).or_else(|| {
            if wanted.is_some() {
                return None;
            }
            self.engines
                .engines()
                .iter()
                .filter(|e| uci(self, e.id))
                .find_map(|e| running(self, e.id))
        });
        if let Some(run) = ready {
            return Ok(run);
        }
        let entry = match wanted {
            Some(id) => self.engines.get(id).cloned(),
            None => self
                .engines
                .hotkey_owner(Hotkey::F2)
                .and_then(|id| self.engines.get(id))
                .cloned(),
        };
        let Some(entry) = entry else {
            return Err(EpdNotice::NoEngine);
        };
        if entry.protocol != Protocol::Uci {
            return Err(EpdNotice::UciOnly);
        }
        let existing = self
            .engine_runs
            .of_engine(entry.id)
            .map(|r| (r.id, r.state.clone()));
        match existing {
            Some((run, EngineRunState::Starting)) => Ok(run),
            Some((run, EngineRunState::Failed(_))) => {
                drop(self.engine_runs.remove(run));
                let defaults = self.prefs.analysis.run_defaults();
                Ok(self.begin_run(entry, false, defaults, effects))
            }
            Some(_) => Err(EpdNotice::EngineBusy),
            None => {
                let defaults = self.prefs.analysis.run_defaults();
                Ok(self.begin_run(entry, false, defaults, effects))
            }
        }
    }

    /// The engine of a run in `Starting` is ready.
    pub(crate) fn epd_engine_ready(&mut self, run: RunId, effects: &mut Vec<Effect>) {
        let waiting = self
            .epd
            .run
            .as_ref()
            .is_some_and(|a| a.engine_run == run && matches!(a.stage, RunStage::Starting));
        if waiting {
            self.epd_begin(effects);
        }
    }

    /// Takes the engine and sends the first search.
    fn epd_begin(&mut self, effects: &mut Vec<Effect>) {
        let Some(active) = self.epd.run.as_ref() else {
            return;
        };
        let (engine_run, file_id, config) = (active.engine_run, active.file, active.config.clone());
        let engine_name = active.engine_name.clone();
        let engine_ok = self
            .engine_runs
            .get(engine_run)
            .is_some_and(|r| r.state == EngineRunState::Running);
        let file_ready = self
            .epd
            .file(file_id)
            .is_some_and(|f| !(f.locked() || config.mode.annotates() && f.read_only));
        if !engine_ok {
            self.epd.run = None;
            return self.epd_notice(EpdNotice::NoEngine);
        }
        if !file_ready {
            self.epd.run = None;
            return self.epd_notice(EpdNotice::Busy);
        }
        let analyzing = self
            .engine_runs
            .get(engine_run)
            .is_some_and(|r| r.analyzing);
        // No analysis of the board meanwhile; it comes back at the end.
        if let Some(r) = self.engine_runs.get_mut(engine_run) {
            r.analyzing = false;
            r.sent = None;
        }
        let Some(file) = self.epd.file_mut(file_id) else {
            self.epd.run = None;
            return;
        };
        file.last_run = None;
        // The analysis that writes can be undone, one level (D3817).
        file.undo = config.mode.annotates().then(|| EpdUndo {
            dirty_before: file.dirty,
            ..EpdUndo::default()
        });
        let mut run = EpdRun::new(&file.book, &config, &engine_name, file.selected);
        if run.progress().total == 0 {
            file.undo = None;
            if let Some(r) = self.engine_runs.get_mut(engine_run) {
                r.analyzing = analyzing;
            }
            self.epd.run = None;
            return self.epd_notice(EpdNotice::NothingToAnalyze);
        }
        let outputs = run.start(&file.book);
        if let Some(active) = self.epd.run.as_mut() {
            active.analyzing = analyzing;
            active.stage = RunStage::Running(Box::new(run));
        }
        self.epd_outputs(outputs, effects);
    }

    fn epd_send(&self, run: RunId, command: EngineCommand, effects: &mut Vec<Effect>) {
        effects.push(Effect::EngineCommand {
            run,
            command: Box::new(command),
            hub: self.engine_hub.clone(),
        });
    }

    fn epd_outputs(&mut self, outputs: Vec<RunOutput>, effects: &mut Vec<Effect>) {
        for output in outputs {
            let Some(engine_run) = self.epd.run.as_ref().map(|a| a.engine_run) else {
                return;
            };
            match output {
                RunOutput::Send(command) => {
                    let limit = match &*command {
                        EngineCommand::Search(request) => Some(request.limits.movetime),
                        _ => None,
                    };
                    self.epd_send(engine_run, *command, effects);
                    if let Some(movetime) = limit {
                        let total = movetime.unwrap_or_default().saturating_add(STALL_GRACE);
                        if let Some(active) = self.epd.run.as_mut() {
                            active.remaining = total;
                        }
                        self.epd_arm(total, effects);
                    }
                }
                RunOutput::Apply(actions) => self.epd_apply(&actions),
                RunOutput::Done(outcome) => return self.epd_run_end(outcome, true, effects),
            }
        }
        self.epd_follow_run();
    }

    /// The record being analysed is the selected one.
    fn epd_follow_run(&mut self) {
        let Some(active) = self.epd.run.as_ref() else {
            return;
        };
        let RunStage::Running(run) = &active.stage else {
            return;
        };
        let (id, current) = (active.file, run.current());
        if let (Some(file), Some(current)) = (self.epd.file_mut(id), current) {
            file.selected = Some(current);
            file.draft = None;
            file.find_hit = None;
        }
    }

    /// Writes what the run decided into the records, keeping what they
    /// were for the "Undo analysis".
    fn epd_apply(&mut self, actions: &[RunAction]) {
        let Some(id) = self.epd.run.as_ref().map(|a| a.file) else {
            return;
        };
        let Some(file) = self.epd.file_mut(id) else {
            return;
        };
        let affects_rows = file.ops_affect_rows();
        let book = Arc::make_mut(&mut file.book);
        let mut any = false;
        for action in actions {
            let (index, annotate) = match action {
                RunAction::Annotate { index, .. } | RunAction::Result { index, .. } => {
                    (*index, matches!(action, RunAction::Annotate { .. }))
                }
            };
            let Some(position) = book.get_by_index(index).map(|r| r.position().clone()) else {
                continue;
            };
            let Some(record) = book.get_mut(&position) else {
                continue;
            };
            if let Some(undo) = file.undo.as_mut()
                && undo.touched.insert(index)
            {
                undo.entries.push((index, record.operations().to_vec()));
            }
            any |= match action {
                RunAction::Annotate { operations, .. } if annotate => {
                    put_analysis(record, operations.clone())
                }
                RunAction::Result { operation, .. } => put_operation(record, operation.clone()),
                RunAction::Annotate { .. } => false,
            };
        }
        if any {
            file.dirty = true;
            if affects_rows {
                file.rows_stale = true;
            }
        }
    }

    /// Ends the run: gives the engine back (when it is alive), keeps the
    /// numbers and reports.
    pub(crate) fn epd_run_end(
        &mut self,
        outcome: RunOutcome,
        engine_alive: bool,
        effects: &mut Vec<Effect>,
    ) {
        let Some(active) = self.epd.run.take() else {
            return;
        };
        let engine_run = active.engine_run;
        let ready = engine_alive
            && self
                .engine_runs
                .get(engine_run)
                .is_some_and(|r| r.state == EngineRunState::Running);
        let progress = match &active.stage {
            RunStage::Running(run) => run.progress(),
            RunStage::Starting => crate::epd_window::RunProgress::default(),
        };
        if ready && let RunStage::Running(run) = &active.stage {
            if !run.is_finished() {
                // What the stopped search still reports is not for the
                // analysis window.
                match run.pending_search() {
                    Some(id) => self.epd_remember_stale(engine_run, id),
                    None if run.awaits_start() => self.epd.untagged = Some(engine_run),
                    None => {}
                }
                self.epd_send(engine_run, EngineCommand::Stop, effects);
            }
            if let Some(r) = self.engine_runs.get_mut(engine_run) {
                // The board's analysis comes back: the next refresh sends it.
                r.analyzing = active.analyzing;
                r.sent = None;
            }
        }
        if let Some(file) = self.epd.file_mut(active.file) {
            if matches!(active.stage, RunStage::Running(_)) {
                file.last_run = Some((outcome, progress));
            }
            file.rows_stale |= !file.natural_view();
        }
        let notice = match outcome {
            RunOutcome::Finished => EpdNotice::RunDone {
                done: progress.done,
                found: progress.found,
                counted: progress.counted,
                illegal: progress.illegal,
            },
            RunOutcome::Interrupted => EpdNotice::RunInterrupted {
                done: progress.done,
                total: progress.total,
            },
            RunOutcome::Failed(RunError::EngineFailed | RunError::NoScore) => {
                EpdNotice::EngineFailed
            }
        };
        self.epd_notice(notice);
    }

    /// The user stopped the analysis.
    fn epd_cancel(&mut self, effects: &mut Vec<Effect>) {
        let Some(active) = self.epd.run.as_mut() else {
            return;
        };
        match &mut active.stage {
            // The engine is still starting: there is nothing to undo.
            RunStage::Starting => self.epd_run_end(RunOutcome::Interrupted, true, effects),
            RunStage::Running(run) => {
                let outputs = run.cancel();
                self.epd_outputs(outputs, effects);
                // A `stop` the engine never answers must not hold the run.
                if self.epd.run.is_some() {
                    self.epd_arm(CANCEL_GRACE, effects);
                }
            }
        }
    }

    // ---- the watchdog ----

    fn epd_arm(&mut self, delay: Duration, effects: &mut Vec<Effect>) {
        let generation = self.new_request().0;
        let Some(active) = self.epd.run.as_mut() else {
            return;
        };
        active.watch = generation;
        active.seen = active.progress;
        active.period = delay;
        effects.push(Effect::Epd(EpdEffect::Tick {
            run: active.engine_run,
            generation,
            delay,
        }));
    }

    /// The timer fired: if the engine said nothing since it was set, it is
    /// given up; otherwise the wait starts again.
    fn epd_tick(&mut self, run: RunId, generation: u64, effects: &mut Vec<Effect>) {
        let Some(active) = self.epd.run.as_ref() else {
            return;
        };
        if active.engine_run != run || active.watch != generation {
            return;
        }
        let cancelling = matches!(&active.stage, RunStage::Running(r) if r.is_cancelling());
        // The only timer alive after a cancel is the one of `CANCEL_GRACE`.
        if cancelling {
            return self.epd_run_end(RunOutcome::Interrupted, true, effects);
        }
        let remaining = active.remaining.saturating_sub(active.period);
        if active.progress == active.seen || remaining.is_zero() {
            let outcome = RunOutcome::Failed(RunError::EngineFailed);
            return self.epd_run_end(outcome, true, effects);
        }
        if let Some(active) = self.epd.run.as_mut() {
            active.remaining = remaining;
        }
        self.epd_arm(remaining.min(Duration::from_secs(120)), effects);
    }

    // ---- engine events ----

    /// Gives an event of an engine to the run. Returns whether the run took
    /// it (then it is not for the analysis window).
    pub(crate) fn epd_event(
        &mut self,
        run: RunId,
        event: &EngineEvent,
        effects: &mut Vec<Effect>,
    ) -> bool {
        if self.epd_is_stale(run, event) {
            return true;
        }
        self.epd_live_event(run, event, effects) || self.epd_note_stale(run, event)
    }

    fn epd_live_event(
        &mut self,
        run: RunId,
        event: &EngineEvent,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let crate::epd_window::EpdUi {
            files, run: active, ..
        } = &mut self.epd;
        let Some(active) = active.as_mut() else {
            return false;
        };
        if active.engine_run != run {
            return false;
        }
        let RunStage::Running(machine) = &mut active.stage else {
            return false;
        };
        let Some(file) = files.iter().find(|f| f.id == active.file) else {
            return false;
        };
        let Some(outputs) = machine.on_event(&file.book, event) else {
            return false;
        };
        active.progress = active.progress.wrapping_add(1);
        self.epd_outputs(outputs, effects);
        true
    }

    fn epd_note_stale(&mut self, run: RunId, event: &EngineEvent) -> bool {
        let EngineEvent::SearchTagged { search, tag } = event else {
            return false;
        };
        if !is_epd_tag(*tag) {
            // A search of the analysis window is coming: an untagged start
            // can no longer be the stopped one's.
            if self.epd.untagged == Some(run) {
                self.epd.untagged = None;
            }
            return false;
        }
        self.epd_remember_stale(run, *search);
        true
    }

    fn epd_remember_stale(&mut self, run: RunId, search: mychess_engine::SearchId) {
        let stale = &mut self.epd.stale;
        if stale.len() >= STALE_LIMIT {
            stale.remove(0);
        }
        stale.push((run, search));
    }

    /// What a search of an ended run still reports is dropped.
    fn epd_is_stale(&mut self, run: RunId, event: &EngineEvent) -> bool {
        let id = match event {
            EngineEvent::SearchStarted(id) => *id,
            EngineEvent::Info(info) => info.search,
            EngineEvent::BestMove(best) => best.search,
            _ => return false,
        };
        let Some(at) = self.epd.stale.iter().position(|entry| *entry == (run, id)) else {
            // The engine that lost the label of the stopped search: its next
            // start without one is that search's.
            if matches!(event, EngineEvent::SearchStarted(_)) && self.epd.untagged == Some(run) {
                self.epd.untagged = None;
                self.epd_remember_stale(run, id);
                return true;
            }
            return false;
        };
        if matches!(event, EngineEvent::BestMove(_)) {
            self.epd.stale.remove(at);
        }
        true
    }

    /// The engine of the run ended, failed to start or was stopped.
    pub(crate) fn epd_engine_gone(&mut self, run: RunId, alive: bool, effects: &mut Vec<Effect>) {
        if self.epd_uses(run) {
            self.epd_run_end(RunOutcome::Failed(RunError::EngineFailed), alive, effects);
        }
    }

    /// A command to the engine of the run was refused or found its queue
    /// full. A search that never starts would hold the run for ever.
    pub(crate) fn epd_command_failed(
        &mut self,
        run: RunId,
        kind: CommandKind,
        effects: &mut Vec<Effect>,
    ) {
        if kind != CommandKind::Search || !self.epd_uses(run) {
            return;
        }
        let Some(ActiveRun {
            stage: RunStage::Running(machine),
            ..
        }) = self.epd.run.as_mut()
        else {
            return;
        };
        if machine.awaits_start() {
            let outputs = machine.refused();
            self.epd_outputs(outputs, effects);
        }
    }
}

/// Where the navigation goes.
#[derive(Debug, Clone, Copy)]
enum Walk {
    First,
    Last,
    Next,
    Prev,
    Random(u64),
}
