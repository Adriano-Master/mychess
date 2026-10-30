//! The import of PGN files into a database (spec M3-8, D1970-D1989): the
//! file dialogs, the job of the list, its answer and the window.
//!
//! Everything here is bookkeeping, which `update` may do. The reading of the
//! files and the writing of the database are the effect
//! [`Effect::ImportPgnFiles`], run by `perform`.

use std::path::{Path, PathBuf};

use mychess_db::{Filter, GameSource};

use super::{AppState, PendingListJob};
use crate::bases::{BaseError, BaseKind, BasesNotice, ListJobKind, OpenControl};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::file_import::{
    FileImportJob, FileImportMessage, FileImportNotice, FileImportOutcome, FileImportResult,
    FileImportTarget, FileImportView, FileImportWindow, ImportProgress, PendingFileImportPick,
    ensure_base_extension, suggest_base_path,
};
use crate::flow::AppNotice;
use crate::host::{PickKind, PickSpec};

impl AppState {
    fn file_import_notice(&mut self, notice: FileImportNotice) {
        self.notice = Some(AppNotice::FileImport(notice));
    }

    /// Handles a [`FileImportMessage`].
    pub(crate) fn file_import_message(
        &mut self,
        message: FileImportMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            FileImportMessage::Open => self.file_import_open(effects),
            FileImportMessage::Close => {
                // While the job runs only "stop" acts (D1985).
                if self
                    .file_import
                    .window
                    .as_ref()
                    .is_some_and(|w| w.result.is_some())
                {
                    self.file_import.window = None;
                }
            }
        }
    }

    /// "Import PGN file...": asks for the files (D1971).
    fn file_import_open(&mut self, effects: &mut Vec<Effect>) {
        if self.modal_open() || self.list_busy() {
            return;
        }
        let request = self.new_request();
        self.file_import.pending = Some(PendingFileImportPick {
            request,
            files: None,
        });
        effects.push(Effect::PickFile(PickSpec {
            request,
            kind: PickKind::ImportPgn,
            initial_dir: self.prefs.files.last_open_dir.clone(),
            file_name: None,
        }));
    }

    /// The answer of the dialog that picks the PGN files.
    pub(crate) fn files_picked(
        &mut self,
        request: RequestId,
        paths: Vec<PathBuf>,
        effects: &mut Vec<Effect>,
    ) {
        let current = self.file_import.pending.as_ref();
        if current.map(|p| (p.request, p.files.is_none())) != Some((request, true)) {
            return;
        }
        self.file_import.pending = None;
        let Some(first) = paths.first() else {
            return;
        };
        if self.modal_open() || self.list_busy() {
            return;
        }
        if let Some(target) = self.file_import_existing_target() {
            self.file_import_start(paths, target, effects);
            return;
        }
        // No database that takes games (D1973): ask for the name of a new
        // one, in the folder of the first file.
        let suggested = suggest_base_path(first);
        let request = self.new_request();
        let initial_dir = suggested.parent().map(Path::to_path_buf);
        let file_name = suggested
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        self.file_import.pending = Some(PendingFileImportPick {
            request,
            files: Some(paths),
        });
        effects.push(Effect::PickFile(PickSpec {
            request,
            kind: PickKind::NewBase,
            initial_dir,
            file_name,
        }));
    }

    /// The answer of the dialog that names the new database. Returns whether
    /// it was the import's.
    pub(crate) fn file_import_file_picked(
        &mut self,
        request: RequestId,
        path: Option<&Path>,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let Some(pending) = self.file_import.pending.as_ref() else {
            return false;
        };
        if pending.request != request || pending.files.is_none() {
            return false;
        }
        let files = self
            .file_import
            .pending
            .take()
            .and_then(|p| p.files)
            .unwrap_or_default();
        let Some(path) = path else {
            return true;
        };
        if files.is_empty() || self.modal_open() || self.list_busy() {
            return true;
        }
        let target = FileImportTarget::New {
            path: ensure_base_extension(path.to_path_buf()),
        };
        self.file_import_start(files, target, effects);
        true
    }

    /// The database of the current slot as a target, if it takes games
    /// (D1972).
    fn file_import_existing_target(&self) -> Option<FileImportTarget> {
        let id = self.bases.current();
        let slot = self.bases.slot(id)?;
        if id.is_clipbase() || slot.kind != BaseKind::Si4 || slot.read_only || slot.needs_reload {
            return None;
        }
        Some(FileImportTarget::Existing {
            path: slot.path.clone()?,
            expected_total: slot.total(),
            stamp: slot.stamp,
        })
    }

    fn file_import_start(
        &mut self,
        files: Vec<PathBuf>,
        target: FileImportTarget,
        effects: &mut Vec<Effect>,
    ) {
        let id = self.bases.current();
        let Some(source) = self.bases.slot(id).map(|s| s.source.clone()) else {
            return;
        };
        let progress = ImportProgress::default();
        self.file_import.window = Some(FileImportWindow {
            files: files.clone(),
            target: target.path().to_path_buf(),
            new_base: matches!(target, FileImportTarget::New { .. }),
            progress: progress.clone(),
            result: None,
        });
        let request = self.new_request();
        let control = OpenControl::new();
        // The file stays reserved while the job outlives a closed slot.
        self.flow.list_job = Some(PendingListJob {
            request,
            slot: id,
            source,
            kind: ListJobKind::ImportPgn,
            target: None,
            target_path: Some(target.path().to_path_buf()),
            closing: false,
            control: control.clone(),
        });
        effects.push(Effect::ImportPgnFiles {
            request,
            job: Box::new(FileImportJob {
                target,
                files,
                progress,
            }),
            control,
        });
    }

    /// The answer of the import.
    #[allow(clippy::too_many_lines)] // one flat decision on the outcome
    pub(crate) fn list_pgn_imported(
        &mut self,
        request: RequestId,
        result: Result<Box<FileImportOutcome>, BaseError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.list_job.as_ref().map(|j| j.request) != Some(request) {
            return;
        }
        let Some(job) = self.flow.list_job.take() else {
            return;
        };
        let outcome = match result {
            Ok(outcome) => *outcome,
            Err(error) => {
                // Nothing was written (D1978): a stop before the first byte
                // says nothing, a refusal says why.
                if error == BaseError::Cancelled {
                    self.file_import.window = None;
                    return;
                }
                if let Some(window) = self.file_import.window.as_mut() {
                    window.result = Some(FileImportResult {
                        report: crate::file_import::FileImportReport::default(),
                        error: Some(error.clone()),
                        opened: false,
                        kept: false,
                    });
                }
                self.file_import_notice(FileImportNotice::NotStarted(error));
                return;
            }
        };
        let added = outcome.report.games_added;
        let new_base = self.file_import.window.as_ref().is_some_and(|w| w.new_base);
        let (mut opened, mut kept) = (false, false);
        let mut reset = false;
        if new_base {
            if added > 0 {
                if outcome.source.is_some()
                    && !self.edits_blocked()
                    && self.flow.open_base.is_none()
                    && !job.closing
                {
                    self.start_open_base(outcome.path.clone(), effects);
                    opened = true;
                } else {
                    kept = true;
                }
            }
        } else if let Some(slot) = self.bases.slot_mut(job.slot)
            && slot.source == job.source
        {
            if let Some(source) = outcome.source.clone() {
                let total = GameSource::len(&*source.read());
                let old_total = slot.filter.len();
                slot.source = source;
                slot.stamp = outcome.stamp;
                // The new games enter the filter, like `append(1)` of the
                // Scid; filter, selection and position stay (D1981).
                let mut grown = total == old_total.saturating_add(added);
                if grown {
                    grown = (0..added).all(|_| slot.filter.append(1));
                    if added > 0 {
                        slot.clear_sort();
                    }
                }
                if !grown {
                    slot.filter = Filter::new(total);
                    slot.list.select_only(None);
                    slot.clear_sort();
                    reset = true;
                }
                slot.clamp_first();
            } else if added > 0 {
                // The games are on disk but the database could not be read
                // again: the slot refuses writes until it is opened again.
                slot.needs_reload = true;
            }
        }
        let report = outcome.report;
        let interrupted = report.interrupted;
        let clean = outcome.error.is_none()
            && !report.has_problems()
            && !interrupted
            && !kept
            && !outcome.removed
            && self
                .file_import
                .window
                .as_ref()
                .is_some_and(|w| w.files.len() == 1);
        let error = outcome.error.clone();
        let notice = if let Some(error) = error.clone() {
            FileImportNotice::Failed {
                count: added,
                error,
            }
        } else if interrupted {
            FileImportNotice::Interrupted { count: added }
        } else if added == 0 {
            FileImportNotice::NothingImported
        } else {
            FileImportNotice::Imported { count: added }
        };
        if clean {
            // D1984: no error and no warning, one file: the window goes.
            self.file_import.window = None;
        } else if let Some(window) = self.file_import.window.as_mut() {
            window.result = Some(FileImportResult {
                report,
                error,
                opened,
                kept,
            });
        }
        self.file_import_notice(notice);
        if reset {
            self.notice = Some(AppNotice::Bases(BasesNotice::ListReset));
        }
    }

    /// What the window draws, if it is open.
    pub fn file_import_view(&self) -> Option<FileImportView<'_>> {
        let window = self.file_import.window.as_ref()?;
        let running = window.result.is_none();
        let bytes = self
            .list_job()
            .filter(|(kind, _)| *kind == ListJobKind::ImportPgn)
            .map_or((0, 0), |(_, control)| control.progress());
        Some(FileImportView {
            files: &window.files,
            target: &window.target,
            new_base: window.new_base,
            running,
            file: window.progress.file(),
            games: window.progress.games(),
            bytes,
            result: window.result.as_ref(),
        })
    }
}
