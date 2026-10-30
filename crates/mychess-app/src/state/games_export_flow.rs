//! The export of games to a PGN file (spec M3-9, D2000-D2029): the dialog
//! of options, the file dialog, the job of the list and its answer.
//!
//! Everything here is bookkeeping, which `update` may do. Reading the games
//! and writing the file are the effect [`Effect::ExportGames`], run by
//! `perform`.

use std::path::{Path, PathBuf};

use mychess_db::Filter;
use mychess_pgn::write::Newline;

use super::{AppState, PendingListJob};
use crate::bases::{BaseError, ListJobKind, OpenControl};
use crate::effect::Effect;
use crate::error::{FileError, FileErrorKind, RequestId};
use crate::export_docs::{ExportFormat, fix_destination_ext};
use crate::files::GameOrigin;
use crate::flow::AppNotice;
use crate::games_export::{
    ExportCounts, ExportDraft, ExportList, ExportScope, ExportStep, GamesExportJob,
    GamesExportMessage, GamesExportNotice, GamesExportOutcome, GamesExportReport,
    GamesExportResult, GamesExportStepView, GamesExportView, GamesExportWindow, fix_destination,
};
use crate::host::{PickKind, PickSpec};
use crate::shared::SharedGame;

impl AppState {
    fn games_export_notice(&mut self, notice: GamesExportNotice) {
        self.notice = Some(AppNotice::GamesExport(notice));
    }

    /// Handles a [`GamesExportMessage`].
    pub(crate) fn games_export_message(
        &mut self,
        message: GamesExportMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            GamesExportMessage::Open => self.games_export_open(),
            GamesExportMessage::Close => {
                // While the job runs only "stop" acts (D2017), and a question
                // about a file must be answered (spec M7-4).
                if self.export_docs.confirm.is_none()
                    && self
                        .games_export
                        .window
                        .as_ref()
                        .is_some_and(|w| !matches!(w.step, ExportStep::Running { .. }))
                {
                    self.games_export.window = None;
                }
            }
            GamesExportMessage::Choose => self.games_export_choose(effects),
            other => self.games_export_set(other),
        }
    }

    /// The changes of the options (only in the first step).
    fn games_export_set(&mut self, message: GamesExportMessage) {
        let Some(window) = self.games_export.window.as_mut() else {
            return;
        };
        if window.step != ExportStep::Options {
            return;
        }
        let draft = &mut window.draft;
        match message {
            GamesExportMessage::SetScope(scope) => {
                // The lists read the entry of a game of a database.
                let list = self.prefs.export_docs.format.needs_database();
                if window.counts.available(scope) && !(list && scope == ExportScope::Current) {
                    draft.scope = scope;
                }
            }
            GamesExportMessage::SetComments(on) => draft.comments = on,
            GamesExportMessage::SetVariations(on) => draft.variations = on,
            GamesExportMessage::SetMarks(on) => draft.marks = on,
            GamesExportMessage::SetOnlyTactics(on) => draft.only_tactics = on,
            GamesExportMessage::SetAppend(on) => draft.append = on,
            _ => {}
        }
    }

    /// "Export games...": the dialog of options (D2001).
    fn games_export_open(&mut self) {
        if self.modal_open() || self.list_busy() {
            return;
        }
        let id = self.bases.current();
        let (counts, base_name) = self.bases.slot(id).map_or_else(
            || (ExportCounts::default(), String::new()),
            |slot| {
                let counts = ExportCounts {
                    all: slot.total(),
                    filter: slot.filtered(),
                    selection: u32::try_from(slot.list.selection.len()).unwrap_or(u32::MAX),
                };
                let name = slot
                    .path
                    .as_deref()
                    .and_then(Path::file_stem)
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (counts, name)
            },
        );
        let scope = [
            ExportScope::Selection,
            ExportScope::Filter,
            ExportScope::All,
        ]
        .into_iter()
        .find(|scope| counts.available(*scope))
        .unwrap_or(ExportScope::Current);
        self.games_export.window = Some(GamesExportWindow {
            draft: ExportDraft {
                scope,
                comments: true,
                variations: true,
                marks: !self.prefs.pgn_export.strip_marks,
                only_tactics: false,
                append: false,
            },
            counts,
            base_name,
            step: ExportStep::Options,
        });
    }

    /// "Choose destination...": asks for the file (D2002).
    fn games_export_choose(&mut self, effects: &mut Vec<Effect>) {
        let Some(window) = self.games_export.window.as_ref() else {
            return;
        };
        if window.step != ExportStep::Options || !window.counts.available(window.draft.scope) {
            return;
        }
        let format = self.prefs.export_docs.format;
        if format.needs_database() && window.draft.scope == ExportScope::Current {
            return;
        }
        let append = window.draft.append && format.can_append();
        let kind = if format != ExportFormat::Pgn {
            PickKind::ExportDoc { format, append }
        } else if window.draft.append {
            PickKind::AppendPgn
        } else {
            PickKind::SavePgn
        };
        let stem = if window.base_name.is_empty() {
            "games".to_owned()
        } else {
            window.base_name.clone()
        };
        let file_name = Some(
            fix_destination_ext(
                Path::new(&stem),
                format.accepted_extensions(),
                format.extension(),
            )
            .unwrap_or_else(|| PathBuf::from(format!("games.{}", format.extension())))
            .to_string_lossy()
            .into_owned(),
        );
        let request = self.new_request();
        if let Some(window) = self.games_export.window.as_mut() {
            window.step = ExportStep::Picking(request);
        }
        effects.push(Effect::PickFile(PickSpec {
            request,
            kind,
            initial_dir: self.prefs.files.last_open_dir.clone(),
            file_name,
        }));
    }

    /// The answer of the file dialog. Returns whether it was the export's.
    pub(crate) fn games_export_file_picked(
        &mut self,
        request: RequestId,
        path: Option<&Path>,
        effects: &mut Vec<Effect>,
    ) -> bool {
        if self.export_image_file_picked(request, path, effects) {
            return true;
        }
        let is_ours = self
            .games_export
            .window
            .as_ref()
            .is_some_and(|w| w.step == ExportStep::Picking(request));
        if !is_ours {
            return false;
        }
        match path {
            None => {
                if let Some(window) = self.games_export.window.as_mut() {
                    window.step = ExportStep::Options;
                }
            }
            Some(path) => self.games_export_start(path, effects),
        }
        true
    }

    fn games_export_start(&mut self, chosen: &Path, effects: &mut Vec<Effect>) {
        let back_to_options = |state: &mut AppState| {
            if let Some(window) = state.games_export.window.as_mut() {
                window.step = ExportStep::Options;
            }
        };
        let Some(window) = self.games_export.window.as_ref() else {
            return;
        };
        let draft = window.draft;
        let id = self.bases.current();
        let Some(slot) = self.bases.slot(id) else {
            return back_to_options(self);
        };
        let list = match draft.scope {
            ExportScope::All => ExportList::Filter(Box::new(Filter::new(slot.total()))),
            ExportScope::Filter => ExportList::Filter(Box::new(slot.filter.clone())),
            ExportScope::Selection => {
                ExportList::Numbers(slot.list.selection.iter().copied().collect())
            }
            ExportScope::Current => {
                ExportList::Current(SharedGame::new(self.session.game().clone()))
            }
        };
        let source = slot.source.clone();
        // Everything the program reads or may write later (D2013): the open
        // databases, the file of the game on the board, the collection in
        // the chooser and the file of a save in flight.
        let mut protected: Vec<PathBuf> = self
            .bases
            .iter()
            .filter_map(|(_, slot)| slot.path.clone())
            .collect();
        if let GameOrigin::PgnFile { path, .. } | GameOrigin::SavedFile { path } = &self.origin {
            protected.push(path.clone());
        }
        if let Some(path) = self
            .chooser
            .as_ref()
            .and_then(|c| c.collection.path())
            .or_else(|| self.collection.as_deref().and_then(|c| c.path()))
        {
            protected.push(path.to_path_buf());
        }
        if let Some(save) = &self.flow.save {
            protected.push(save.path.clone());
        }
        if self.prefs.export_docs.format != ExportFormat::Pgn {
            return self.export_docs_start(chosen, list, source, id, protected, effects);
        }
        let Some(dest) = fix_destination(chosen) else {
            // A name that is not Unicode cannot be fixed (D2022).
            if let Some(window) = self.games_export.window.as_mut() {
                let error = BaseError::File(FileError::new(
                    FileErrorKind::Io(std::io::ErrorKind::InvalidInput),
                    Some(chosen.to_path_buf()),
                ));
                window.step = ExportStep::Done(Box::new(GamesExportResult {
                    path: chosen.to_path_buf(),
                    report: GamesExportReport::default(),
                    error: Some(error.clone()),
                }));
                self.notice = Some(AppNotice::GamesExport(GamesExportNotice::Failed(error)));
            }
            return;
        };
        let write = self
            .prefs
            .pgn_export
            .write_options()
            .with_comments(draft.comments)
            .with_variations(draft.variations)
            .with_strip_marks(!draft.marks)
            .with_newline(Newline::Lf);
        let job = GamesExportJob {
            source: source.clone(),
            list,
            only_tactics: draft.only_tactics && draft.scope != ExportScope::Current,
            confirmed: dest == chosen,
            dest: dest.clone(),
            append: draft.append,
            write,
            protected,
        };
        if let Some(window) = self.games_export.window.as_mut() {
            window.step = ExportStep::Running { path: dest.clone() };
        }
        let request = self.new_request();
        let control = OpenControl::new();
        // The file stays reserved while the job outlives a closed slot.
        self.flow.list_job = Some(PendingListJob {
            request,
            slot: id,
            source,
            kind: ListJobKind::ExportGames,
            target: None,
            target_path: Some(dest),
            closing: false,
            control: control.clone(),
        });
        effects.push(Effect::ExportGames {
            request,
            job: Box::new(job),
            control,
        });
    }

    /// The answer of the export.
    pub(crate) fn list_games_exported(
        &mut self,
        request: RequestId,
        result: Result<Box<GamesExportOutcome>, BaseError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.list_job.as_ref().map(|j| j.request) != Some(request) {
            return;
        }
        let Some(job) = self.flow.list_job.take() else {
            return;
        };
        let path = job.target_path.unwrap_or_default();
        let outcome = match result {
            Ok(outcome) => *outcome,
            Err(BaseError::Cancelled) => {
                self.games_export.window = None;
                return self.games_export_notice(GamesExportNotice::Stopped);
            }
            Err(error) => {
                if let Some(window) = self.games_export.window.as_mut() {
                    window.step = ExportStep::Done(Box::new(GamesExportResult {
                        path,
                        report: GamesExportReport::default(),
                        error: Some(error.clone()),
                    }));
                }
                return self.games_export_notice(GamesExportNotice::Failed(error));
            }
        };
        let report = outcome.report;
        // The file needs the user's word first: nothing was written (spec
        // M7-4, D3750).
        if let Some(needed) = report.confirm.clone() {
            return self.export_docs_ask(needed);
        }
        if let Some(problem) = report.problem {
            self.export_docs.last_job = None;
            if let Some(window) = self.games_export.window.as_mut() {
                window.step = ExportStep::Options;
            }
            return self.games_export_notice(GamesExportNotice::Problem(problem));
        }
        let name = outcome
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let notice = if report.interrupted {
            if report.appended {
                GamesExportNotice::StoppedAppended {
                    count: report.written,
                }
            } else {
                GamesExportNotice::Stopped
            }
        } else if report.written == 0 && report.skipped == 0 {
            GamesExportNotice::NothingToExport
        } else if report.written == 0 {
            GamesExportNotice::AllSkipped {
                skipped: report.skipped,
            }
        } else if report.skipped > 0 {
            GamesExportNotice::ExportedWithSkips {
                count: report.written,
                skipped: report.skipped,
                name,
            }
        } else if report.volumes > 0 {
            GamesExportNotice::ViewerExported {
                volumes: report.volumes,
                count: report.written,
                name,
            }
        } else if report.repeated > 0 {
            GamesExportNotice::EpdExported {
                count: report.written,
                repeated: report.repeated,
                name,
            }
        } else {
            GamesExportNotice::Exported {
                count: report.written,
                name,
            }
        };
        if !report.interrupted && report.written > 0 {
            self.export_docs_after(&outcome.path, effects);
        }
        self.export_docs.last_job = None;
        // A clean end, or one the user asked for, closes the window; a
        // report with problems stays (D2004).
        let keep = !report.interrupted && report.skipped > 0;
        if keep {
            if let Some(window) = self.games_export.window.as_mut() {
                window.step = ExportStep::Done(Box::new(GamesExportResult {
                    path: outcome.path,
                    report,
                    error: None,
                }));
            }
        } else {
            self.games_export.window = None;
        }
        self.games_export_notice(notice);
    }

    /// What the window draws, if it is open.
    pub fn games_export_view(&self) -> Option<GamesExportView<'_>> {
        let window = self.games_export.window.as_ref()?;
        let step = match &window.step {
            ExportStep::Options => GamesExportStepView::Options,
            ExportStep::Picking(_) => GamesExportStepView::Picking,
            ExportStep::Running { path } => GamesExportStepView::Running {
                path,
                progress: self
                    .list_job()
                    .filter(|(kind, _)| *kind == ListJobKind::ExportGames)
                    .map_or((0, 0), |(_, control)| control.progress()),
            },
            ExportStep::Done(result) => GamesExportStepView::Done(result),
        };
        Some(GamesExportView {
            draft: window.draft,
            counts: window.counts,
            step,
        })
    }
}
