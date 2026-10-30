//! The extra formats of "Export games..." and the image of the board (spec
//! M7-4, slice b): the options, the questions about existing files, the
//! preview and the answers.
//!
//! Everything here is bookkeeping, which `update` may do. Reading the games,
//! drawing and writing the files are the effects
//! [`Effect::ExportDocs`] and [`Effect::RenderImage`], run by `perform`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mychess_core::{BoardMark, EmbeddedCommand, embedded_commands};
use mychess_db::CsvColumn;
use mychess_pgn::write::Newline;

use super::{AppState, PendingListJob};
use crate::bases::BaseError;
use crate::bases::{ListJobKind, OpenControl, SharedSource, SlotId};
use crate::board_image::{MAX_MARGIN_PX, SIZE_RANGE};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::export_docs::{
    ConfirmNeeded, ConfirmPrompt, CsvHeaders, ExportAssets, ExportConfirmView, ExportDocsJob,
    ExportDocsMessage, ExportDocsView, ExportEdit, ExportFormat, ExportImageView, ExportPrefs,
    ImageFailure, ImageJob, ImageOutcome, ImageStep, ImageStepView, ImageWindow, MAX_DIAGRAM_PX,
    MAX_PER_FILE, TemplateKind, fix_destination_ext,
};
use crate::flow::AppNotice;
use crate::games_export::{
    ExportList, ExportScope, ExportStep, GamesExportNotice, GamesExportReport, GamesExportResult,
};
use crate::host::{PickKind, PickSpec};
use crate::prefs::{PrefChange, PrefsMessage};
use mychess_db::export::game_list::DEFAULT_LIST_FORMAT;

/// The game-list columns that have a CSV column (D3739).
fn csv_column_of(column: crate::bases::ListColumn) -> Option<CsvColumn> {
    use crate::bases::ListColumn as L;
    Some(match column {
        L::Number => CsvColumn::Number,
        L::White => CsvColumn::White,
        L::Black => CsvColumn::Black,
        L::Result => CsvColumn::Result,
        L::Length => CsvColumn::Moves,
        L::Date => CsvColumn::Date,
        L::Event => CsvColumn::Event,
        L::WhiteElo => CsvColumn::WhiteElo,
        L::BlackElo => CsvColumn::BlackElo,
        L::Round => CsvColumn::Round,
        L::Site => CsvColumn::Site,
        L::Eco => CsvColumn::Eco,
        L::Flags => CsvColumn::Flags,
        _ => return None,
    })
}

/// `prefs` with `edit` applied. Numbers are limited to their ranges.
fn apply_edit(mut prefs: ExportPrefs, edit: ExportEdit) -> ExportPrefs {
    let d = ExportPrefs::default();
    match edit {
        ExportEdit::Format(format) => prefs.format = format,
        ExportEdit::HtmlComments(on) => prefs.html.comments = on,
        ExportEdit::HtmlVariations(on) => prefs.html.variations = on,
        ExportEdit::HtmlSymbols(on) => prefs.html.symbols = on,
        ExportEdit::HtmlDiagramPx(px) => prefs.html.diagram_px = px.min(MAX_DIAGRAM_PX),
        ExportEdit::ViewerPerFile(n) => prefs.viewer.per_file = n.clamp(1, MAX_PER_FILE),
        ExportEdit::ViewerIncludePgn(on) => prefs.viewer.include_pgn = on,
        ExportEdit::ViewerOpenAfter(on) => prefs.viewer.open_after = on,
        ExportEdit::LatexComments(on) => prefs.latex.comments = on,
        ExportEdit::LatexVariations(on) => prefs.latex.variations = on,
        ExportEdit::LatexFlip(on) => prefs.latex.flip = on,
        ExportEdit::LatexNullMoves(on) => prefs.latex.convert_null_moves = on,
        ExportEdit::EpdDedupe(on) => prefs.epd.dedupe = on,
        ExportEdit::EpdFinalPosition(on) => prefs.epd.final_position = on,
        ExportEdit::EpdHmvcFmvn(on) => prefs.epd.hmvc_fmvn = on,
        ExportEdit::EpdId(on) => prefs.epd.id = on,
        ExportEdit::EpdGameNumber(on) => prefs.epd.game_number = on,
        ExportEdit::ListFormat(text) => prefs.list_format = text,
        ExportEdit::ResetListFormat => DEFAULT_LIST_FORMAT.clone_into(&mut prefs.list_format),
        ExportEdit::CsvHeader(on) => prefs.csv.header = on,
        ExportEdit::CsvSeparator(separator) => prefs.csv.separator = separator,
        ExportEdit::CsvBom(on) => prefs.csv.bom = on,
        ExportEdit::Template(kind, text) => *template_mut(&mut prefs, kind) = text,
        ExportEdit::ResetTemplate(kind) => {
            let mut default = d;
            *template_mut(&mut prefs, kind) = std::mem::take(template_mut(&mut default, kind));
        }
        ExportEdit::ImageFormat(format) => prefs.image.format = format,
        ExportEdit::ImageSize(px) => {
            prefs.image.size_px = px.clamp(*SIZE_RANGE.start(), *SIZE_RANGE.end());
        }
        ExportEdit::ImageCoordinates(on) => prefs.image.coordinates = on,
        ExportEdit::ImageLastMove(on) => prefs.image.last_move = on,
        ExportEdit::ImageMarks(on) => prefs.image.marks = on,
        ExportEdit::ImageTransparent(on) => prefs.image.transparent = on,
        ExportEdit::ImageMargin(px) => prefs.image.margin_px = px.min(MAX_MARGIN_PX),
        // The orientation lives in the window, not in the preferences.
        _ => {}
    }
    prefs
}

fn template_mut(prefs: &mut ExportPrefs, kind: TemplateKind) -> &mut String {
    match kind {
        TemplateKind::HtmlStart => &mut prefs.html_start,
        TemplateKind::HtmlEnd => &mut prefs.html_end,
        TemplateKind::LatexStart => &mut prefs.latex_start,
        _ => &mut prefs.latex_end,
    }
}

fn is_image_edit(edit: &ExportEdit) -> bool {
    matches!(
        edit,
        ExportEdit::ImageFormat(_)
            | ExportEdit::ImageSize(_)
            | ExportEdit::ImageCoordinates(_)
            | ExportEdit::ImageLastMove(_)
            | ExportEdit::ImageMarks(_)
            | ExportEdit::ImageTransparent(_)
            | ExportEdit::ImageMargin(_)
            | ExportEdit::ImageFlip(_)
    )
}

impl AppState {
    fn docs_notice(&mut self, notice: GamesExportNotice) {
        self.notice = Some(AppNotice::GamesExport(notice));
    }

    /// Gives the host the piece drawings, the colors and the font that the
    /// viewer, the diagrams and the image need (D3723).
    pub fn set_export_assets(&mut self, assets: Option<ExportAssets>) {
        self.export_docs.assets = assets.map(Arc::new);
    }

    /// Gives the host's translations of the CSV header (see
    /// [`csv_column_key`](crate::export_docs::csv_column_key)). A column
    /// without text is named after its key.
    pub fn set_export_csv_headers(&mut self, headers: CsvHeaders) {
        self.export_docs.csv_headers = headers;
    }

    /// Handles an [`ExportDocsMessage`].
    pub(crate) fn export_docs_message(
        &mut self,
        message: ExportDocsMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            ExportDocsMessage::SetFormat(format) => self.export_set_format(format, effects),
            ExportDocsMessage::Edit(edit) => self.export_edit(edit, effects),
            ExportDocsMessage::Confirm(yes) => self.export_confirm(yes, effects),
            ExportDocsMessage::OpenImage => self.export_image_open(effects),
            ExportDocsMessage::ImageSave => self.export_image_save(effects),
            ExportDocsMessage::ImageClose => {
                // While the file is written only the answer closes it.
                if self
                    .export_docs
                    .image
                    .as_ref()
                    .is_some_and(|w| !matches!(w.step, ImageStep::Saving(_)))
                {
                    self.export_docs.image = None;
                }
            }
            ExportDocsMessage::ImageRendered { request, result } => {
                self.export_image_rendered(request, result);
            }
            ExportDocsMessage::FileOpened { ok } => {
                if !ok {
                    self.docs_notice(GamesExportNotice::OpenFailed);
                }
            }
        }
    }

    /// Applies one change to the preferences and saves them. While the file
    /// is still being read the change is only remembered and applied over
    /// the loaded values (nothing is written before the load, D3751).
    fn export_change(&mut self, edit: ExportEdit, effects: &mut Vec<Effect>) {
        let prefs = apply_edit((*self.prefs.export_docs).clone(), edit.clone()).sanitized();
        if prefs == *self.prefs.export_docs {
            return;
        }
        if self.flow.prefs_loading {
            self.export_docs.early_edits.push(edit);
            *self.prefs.export_docs = prefs;
        } else {
            self.prefs_message(
                PrefsMessage::Set(PrefChange::ExportDocs(Box::new(prefs))),
                effects,
            );
        }
    }

    /// The changes made while the preferences were loading.
    pub(super) fn export_docs_take_early(&mut self) -> Vec<ExportEdit> {
        std::mem::take(&mut self.export_docs.early_edits)
    }

    /// Applies the changes taken by [`AppState::export_docs_take_early`]
    /// over the loaded preferences.
    pub(super) fn export_docs_replay(&mut self, edits: Vec<ExportEdit>) {
        for edit in edits {
            *self.prefs.export_docs =
                apply_edit((*self.prefs.export_docs).clone(), edit).sanitized();
        }
    }

    /// Whether the dialog of the export waits for its options.
    fn export_options_open(&self) -> bool {
        self.export_docs.confirm.is_none()
            && self
                .games_export
                .window
                .as_ref()
                .is_some_and(|w| w.step == ExportStep::Options)
    }

    fn export_set_format(&mut self, format: ExportFormat, effects: &mut Vec<Effect>) {
        if !self.export_options_open() {
            return;
        }
        self.export_change(ExportEdit::Format(format), effects);
        // The lists need a database; the viewer cannot append.
        if let Some(window) = self.games_export.window.as_mut() {
            if format.needs_database()
                && window.draft.scope == ExportScope::Current
                && let Some(scope) = [
                    ExportScope::Selection,
                    ExportScope::Filter,
                    ExportScope::All,
                ]
                .into_iter()
                .find(|s| window.counts.available(*s))
            {
                window.draft.scope = scope;
            }
            if !format.can_append() {
                window.draft.append = false;
            }
        }
    }

    fn export_edit(&mut self, edit: ExportEdit, effects: &mut Vec<Effect>) {
        if is_image_edit(&edit) {
            let Some(window) = self.export_docs.image.as_mut() else {
                return;
            };
            if window.step != ImageStep::Options {
                return;
            }
            if let ExportEdit::ImageFlip(on) = edit {
                window.flip = on;
            } else {
                self.export_change(edit, effects);
            }
            self.export_image_preview(effects);
            return;
        }
        if !self.export_options_open() {
            return;
        }
        self.export_change(edit, effects);
    }

    // ---- the job ----

    /// Starts the job of a format other than PGN (called by the PGN flow
    /// when the format is not PGN, with the file already chosen).
    pub(super) fn export_docs_start(
        &mut self,
        chosen: &Path,
        list: ExportList,
        source: SharedSource,
        slot: SlotId,
        protected: Vec<PathBuf>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(window) = self.games_export.window.as_ref() else {
            return;
        };
        let draft = window.draft;
        let base_name = window.base_name.clone();
        // Frozen now: later edits of the preferences do not touch the job.
        let options = Box::new((*self.prefs.export_docs).clone());
        let format = options.format;
        let Some(dest) =
            fix_destination_ext(chosen, format.accepted_extensions(), format.extension())
        else {
            let error = BaseError::File(crate::error::FileError::new(
                crate::error::FileErrorKind::Io(std::io::ErrorKind::InvalidInput),
                Some(chosen.to_path_buf()),
            ));
            if let Some(window) = self.games_export.window.as_mut() {
                window.step = ExportStep::Done(Box::new(GamesExportResult {
                    path: chosen.to_path_buf(),
                    report: GamesExportReport::default(),
                    error: Some(error.clone()),
                }));
            }
            self.docs_notice(GamesExportNotice::Failed(error));
            return;
        };
        let mut columns: Vec<CsvColumn> = self
            .prefs
            .game_list
            .columns
            .iter()
            .filter_map(|setup| csv_column_of(setup.column))
            .collect();
        if columns.is_empty() {
            columns = vec![
                CsvColumn::Number,
                CsvColumn::White,
                CsvColumn::Black,
                CsvColumn::Result,
                CsvColumn::Date,
            ];
        }
        let csv_columns = columns
            .into_iter()
            .map(|column| {
                let text = self
                    .export_docs
                    .csv_headers
                    .iter()
                    .find(|(c, _)| *c == column)
                    .map_or_else(
                        || crate::export_docs::csv_column_key(column).to_owned(),
                        |(_, text)| text.clone(),
                    );
                (column, text)
            })
            .collect();
        let write = self
            .prefs
            .pgn_export
            .write_options()
            .with_comments(draft.comments)
            .with_variations(draft.variations)
            .with_strip_marks(!draft.marks)
            .with_newline(Newline::Lf);
        let job = ExportDocsJob {
            source,
            list,
            only_tactics: draft.only_tactics && draft.scope != ExportScope::Current,
            format,
            dialog_confirmed: dest == chosen,
            confirmed: false,
            append: draft.append && format.can_append(),
            dest,
            options,
            write,
            csv_columns,
            assets: self.export_docs.assets.clone(),
            title: if base_name.is_empty() {
                "MyChess".to_owned()
            } else {
                base_name
            },
            protected,
        };
        self.export_docs_launch(job, slot, effects);
    }

    fn export_docs_launch(&mut self, job: ExportDocsJob, slot: SlotId, effects: &mut Vec<Effect>) {
        if let Some(window) = self.games_export.window.as_mut() {
            window.step = ExportStep::Running {
                path: job.dest.clone(),
            };
        }
        let request = self.new_request();
        let control = OpenControl::new();
        // The file stays reserved while the job outlives a closed slot.
        self.flow.list_job = Some(PendingListJob {
            request,
            slot,
            source: job.source.clone(),
            kind: ListJobKind::ExportGames,
            target: None,
            target_path: Some(job.dest.clone()),
            closing: false,
            control: control.clone(),
        });
        self.export_docs.last_job = Some(Box::new(job.clone()));
        effects.push(Effect::ExportDocs {
            request,
            job: Box::new(job),
            control,
        });
    }

    /// The job asked first: the file exists. Opens the question.
    pub(super) fn export_docs_ask(&mut self, needed: ConfirmNeeded) {
        let Some(job) = self.export_docs.last_job.take() else {
            return;
        };
        if let Some(window) = self.games_export.window.as_mut() {
            window.step = ExportStep::Options;
        }
        let slot = self.bases.current();
        self.export_docs.confirm = Some(ConfirmPrompt { needed, job, slot });
    }

    fn export_confirm(&mut self, yes: bool, effects: &mut Vec<Effect>) {
        // The question of the image has its own step.
        let image_path = self.export_docs.image.as_ref().and_then(|w| match &w.step {
            ImageStep::Confirm(path) => Some(path.clone()),
            _ => None,
        });
        if let Some(path) = image_path {
            if yes {
                self.export_image_write(Some(path), true, effects);
            } else if let Some(window) = self.export_docs.image.as_mut() {
                window.step = ImageStep::Options;
            }
            return;
        }
        let Some(prompt) = self.export_docs.confirm.take() else {
            return;
        };
        if !yes || self.games_export.window.is_none() || self.list_busy() {
            return;
        }
        let mut job = *prompt.job;
        job.confirmed = true;
        self.export_docs_launch(job, prompt.slot, effects);
    }

    // ---- views ----

    /// What the dialog of the export shows besides the options of the M3-9.
    pub fn export_docs_view(&self) -> ExportDocsView<'_> {
        let format = self.prefs.export_docs.format;
        ExportDocsView {
            prefs: &self.prefs.export_docs,
            has_assets: self.export_docs.assets.is_some(),
            confirm: self
                .export_docs
                .confirm
                .as_ref()
                .map(|prompt| ExportConfirmView {
                    kind: prompt.needed.kind,
                    format,
                    files: &prompt.needed.files,
                }),
        }
    }

    /// What the window of the board image shows, if it is open.
    pub fn export_image_view(&self) -> Option<ExportImageView<'_>> {
        let window = self.export_docs.image.as_ref()?;
        Some(ExportImageView {
            prefs: &self.prefs.export_docs.image,
            flip: window.flip,
            step: match &window.step {
                ImageStep::Options => ImageStepView::Options,
                ImageStep::Picking(_) => ImageStepView::Picking,
                ImageStep::Saving(_) => ImageStepView::Saving,
                ImageStep::Confirm(path) => ImageStepView::Confirm(path),
            },
            preview: window.preview.as_ref(),
            preview_error: window.preview_error.as_ref(),
            position: &window.position,
            last_move: window.last_move,
            marks: &window.marks,
        })
    }

    // ---- the image ----

    fn export_image_open(&mut self, effects: &mut Vec<Effect>) {
        if self.modal_open() || self.export_docs.image.is_some() {
            return;
        }
        if self.export_docs.assets.is_none() {
            self.docs_notice(GamesExportNotice::Problem(
                crate::export_docs::ExportProblem::AssetsMissing,
            ));
            return;
        }
        // The position shown (D3724, rule 29), not the one of the database.
        let game = self.session.game();
        let cursor = self.session.cursor();
        let last_move = game
            .mv(cursor)
            .ok()
            .and_then(|m| Some((m.from()?, m.to()?)));
        let marks: Vec<BoardMark> = game
            .comment(cursor)
            .ok()
            .flatten()
            .map(|text| {
                embedded_commands(text)
                    .into_iter()
                    .filter_map(|c| match c {
                        EmbeddedCommand::Mark(mark) => Some(mark),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let base_name = self
            .bases
            .current_slot()
            .and_then(|slot| slot.path.as_deref())
            .and_then(Path::file_stem)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.export_docs.image = Some(ImageWindow {
            position: self.session.position().clone(),
            last_move,
            marks,
            flip: self.board_flipped,
            base_name,
            step: ImageStep::Options,
            preview_request: None,
            preview: None,
            preview_error: None,
        });
        self.export_image_preview(effects);
    }

    /// The files whose names must never be taken by an image (D2013).
    fn export_protected(&self) -> Vec<PathBuf> {
        self.bases
            .iter()
            .filter_map(|(_, slot)| slot.path.clone())
            .collect()
    }

    fn image_job(&self, dest: Option<PathBuf>, confirmed: bool) -> Option<ImageJob> {
        let window = self.export_docs.image.as_ref()?;
        let assets = self.export_docs.assets.clone()?;
        Some(ImageJob {
            position: window.position.clone(),
            flip: window.flip,
            last_move: window.last_move,
            marks: window.marks.clone(),
            options: self.prefs.export_docs.image.clone(),
            assets,
            dest,
            confirmed,
            protected: self.export_protected(),
        })
    }

    /// Asks for a new preview; the older answer is dropped (D3728).
    fn export_image_preview(&mut self, effects: &mut Vec<Effect>) {
        let Some(job) = self.image_job(None, false) else {
            return;
        };
        let request = self.new_request();
        if let Some(window) = self.export_docs.image.as_mut() {
            window.preview_request = Some(request);
        }
        effects.push(Effect::RenderImage {
            request,
            job: Box::new(job),
        });
    }

    fn export_image_save(&mut self, effects: &mut Vec<Effect>) {
        let Some(window) = self.export_docs.image.as_ref() else {
            return;
        };
        if window.step != ImageStep::Options {
            return;
        }
        let format = self.prefs.export_docs.image.format;
        let stem = if window.base_name.is_empty() {
            "board".to_owned()
        } else {
            window.base_name.clone()
        };
        let file_name =
            fix_destination_ext(Path::new(&stem), &[format.extension()], format.extension())
                .map(|p| p.to_string_lossy().into_owned());
        let request = self.new_request();
        if let Some(window) = self.export_docs.image.as_mut() {
            window.step = ImageStep::Picking(request);
        }
        effects.push(Effect::PickFile(PickSpec {
            request,
            kind: PickKind::ExportImage { format },
            initial_dir: self.prefs.files.last_open_dir.clone(),
            file_name,
        }));
    }

    /// The answer of the file dialog. Returns whether it was the image's.
    pub(super) fn export_image_file_picked(
        &mut self,
        request: RequestId,
        path: Option<&Path>,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let is_ours = self
            .export_docs
            .image
            .as_ref()
            .is_some_and(|w| w.step == ImageStep::Picking(request));
        if !is_ours {
            return false;
        }
        match path {
            None => {
                if let Some(window) = self.export_docs.image.as_mut() {
                    window.step = ImageStep::Options;
                }
            }
            Some(chosen) => {
                let format = self.prefs.export_docs.image.format;
                let fixed = fix_destination_ext(chosen, &[format.extension()], format.extension());
                if let Some(dest) = fixed {
                    let confirmed = dest == chosen;
                    self.export_image_write(Some(dest), confirmed, effects);
                } else {
                    if let Some(window) = self.export_docs.image.as_mut() {
                        window.step = ImageStep::Options;
                    }
                    self.docs_notice(GamesExportNotice::ImageFailed(ImageFailure::File(
                        crate::error::FileError::new(
                            crate::error::FileErrorKind::Io(std::io::ErrorKind::InvalidInput),
                            Some(chosen.to_path_buf()),
                        ),
                    )));
                }
            }
        }
        true
    }

    fn export_image_write(
        &mut self,
        dest: Option<PathBuf>,
        confirmed: bool,
        effects: &mut Vec<Effect>,
    ) {
        let Some(job) = self.image_job(dest, confirmed) else {
            return;
        };
        let request = self.new_request();
        if let Some(window) = self.export_docs.image.as_mut() {
            window.step = ImageStep::Saving(request);
        }
        effects.push(Effect::RenderImage {
            request,
            job: Box::new(job),
        });
    }

    fn export_image_rendered(
        &mut self,
        request: RequestId,
        result: Result<ImageOutcome, ImageFailure>,
    ) {
        let Some(window) = self.export_docs.image.as_mut() else {
            return;
        };
        if window.step == ImageStep::Saving(request) {
            window.step = ImageStep::Options;
            match result {
                Ok(ImageOutcome::Saved(path)) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.export_docs.image = None;
                    self.docs_notice(GamesExportNotice::ImageSaved { name });
                }
                Ok(ImageOutcome::NeedsConfirm(path)) => window.step = ImageStep::Confirm(path),
                Ok(_) => {}
                Err(failure) => self.docs_notice(GamesExportNotice::ImageFailed(failure)),
            }
            return;
        }
        // A preview: only the newest counts.
        if window.preview_request != Some(request) {
            return;
        }
        match result {
            Ok(ImageOutcome::Preview(png)) => {
                window.preview = Some(png);
                window.preview_error = None;
            }
            Err(failure) => {
                window.preview = None;
                window.preview_error = Some(failure);
            }
            Ok(_) => {}
        }
    }

    /// The effects to run after a successful viewer export: open it, if the
    /// user asked (D3708).
    pub(super) fn export_docs_after(&mut self, path: &Path, effects: &mut Vec<Effect>) {
        if self
            .export_docs
            .last_job
            .as_ref()
            .is_some_and(|j| j.format == ExportFormat::Viewer && j.options.viewer.open_after)
        {
            effects.push(Effect::OpenExported {
                path: path.to_path_buf(),
            });
        }
    }
}
