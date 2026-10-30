//! The glue of the opening report with [`AppState`] (spec M7-2, slice b):
//! one window, one job, the stale test, the filter by click, save and view,
//! the favorites, the batch of reports and the merge of the best games.
//!
//! Everything here is bookkeeping, which `update` may do; the search, the
//! report and the files are [`OpeningReportEffect`]s run by `perform`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use mychess_core::{Move, PartialDate, Position, SanSuffix};
use mychess_db::report::{BestKind, select_games};
use mychess_pgn::read::{ReadOptions, parse_movetext};

use super::AppState;
use crate::bases::{OpenControl, SlotId};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::files::{GameOrigin, sanitize_file_name};
use crate::flow::AppNotice;
use crate::merge_game::MergeMessage;
use crate::opening_report::{
    Batch, BatchFailure, BatchJob, BatchView, ComputeJob, ComputeOutcome, FavoriteError,
    FavoriteMessage, FavoritesIssue, Job, LoadedFavorites, MERGE_GAMES, OpeningReportEffect,
    OpeningReportMessage, OpeningReportNotice, OpeningReportPrefs, OpeningReportView, Reference,
    RenderJob, ReportFailure, ReportFormat, ReportKey, ReportPart, ReportPhase, ReportSelector,
    ReportsDat, SaveFailure, SaveJob, Shown, favorites_path,
};
use crate::prefs::{PrefChange, PrefsMessage};

/// The name of the Clipbase in the documents.
const CLIPBASE_NAME: &str = "Clipbase";

/// `path` with the extension of `format` added unless it already ends with
/// one the format accepts, in any case (D3616).
pub(crate) fn with_report_extension(path: &Path, format: ReportFormat) -> PathBuf {
    let accepted = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| format.accepts_extension(e));
    if accepted {
        return path.to_path_buf();
    }
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(format.extension());
    PathBuf::from(name)
}

/// The moves as the Scid's `sc_game moves` writes them: `1.e4 c5 2.Nf3`.
fn moves_text(start: &Position, stem: &[Move]) -> Option<String> {
    let mut position = start.clone();
    let mut out = String::new();
    for &mv in stem {
        let san = position.san_with(mv, SanSuffix::CheckOrMate).ok()?;
        if !out.is_empty() {
            out.push(' ');
        }
        if position.turn() == mychess_core::Color::White {
            let _ = write!(out, "{}.", position.fullmove_number());
        }
        out.push_str(&san);
        position = position.after(mv).ok()?;
    }
    Some(out)
}

/// The moves of a favorite, if the text is a legal line from the start.
fn parse_line(text: &str) -> Option<Vec<Move>> {
    let parsed = parse_movetext(text, &Position::new(), &ReadOptions::default());
    if !parsed.diagnostics.is_empty() {
        return None;
    }
    let game = &parsed.game;
    game.mainline().map(|id| game.mv(id).ok()).collect()
}

impl AppState {
    fn orep_notice(&mut self, notice: OpeningReportNotice) {
        self.notice = Some(AppNotice::OpeningReport(notice));
    }

    /// The state of the opening report.
    pub fn opening_report(&self) -> &crate::opening_report::OpeningReportUi {
        &self.opening_report
    }

    /// Sets the date the reports are made on (`None`: the clock). For tests
    /// and the host (D3608).
    pub fn set_opening_report_today(&mut self, today: Option<PartialDate>) {
        self.opening_report.today = today;
    }

    /// Stops the job and the batch (the program is ending).
    pub(crate) fn opening_report_cancel(&mut self) {
        self.opening_report.cancel();
    }

    pub(crate) fn slot_key(&self, id: SlotId) -> Option<ReportKey> {
        let slot = self.bases.slot(id).filter(|s| !s.needs_reload)?;
        Some(ReportKey {
            slot: id,
            source: slot.source.clone(),
            len: slot.total(),
            data_rev: slot.data_rev(),
        })
    }

    /// Whether the slot of `key` is the current one and still holds the
    /// same games.
    pub(crate) fn key_is_current(&self, key: &ReportKey) -> bool {
        key.slot == self.bases.current() && self.slot_key(key.slot).as_ref() == Some(key)
    }

    pub(crate) fn base_name(&self, id: SlotId) -> String {
        self.bases
            .slot(id)
            .and_then(|s| s.path.as_deref())
            .and_then(Path::file_name)
            .map_or_else(
                || CLIPBASE_NAME.to_owned(),
                |n| n.to_string_lossy().into_owned(),
            )
    }

    /// The line to the position of the board (the trial position in trial
    /// mode, which lives in the same game).
    pub(crate) fn board_reference(&self) -> Reference {
        let game = self.session.game();
        let stem: Vec<Move> = game
            .path_to(self.session.cursor())
            .map(|path| path.iter().filter_map(|id| game.mv(*id).ok()).collect())
            .unwrap_or_default();
        let start = game.start_position();
        Reference {
            start: (*start != Position::new()).then(|| start.clone()),
            stem,
        }
    }

    /// What the front-end needs to keep its model of the report in step:
    /// the shared report, the options it was made with and the database
    /// name. Cheap (no SAN is written), unlike [`AppState::opening_report_view`].
    pub fn opening_report_shown(
        &self,
    ) -> Option<(
        crate::opening_report::SharedReport,
        &mychess_db::report::ReportOptions,
        &str,
    )> {
        let shown = self.opening_report.shown.as_ref()?;
        Some((
            shown.report.clone(),
            &shown.options,
            shown.database_name.as_str(),
        ))
    }

    /// The moves of the position that "Exclude" offers, in SAN. Written on
    /// demand: only the form that lists them asks.
    pub fn opening_report_exclude_choices(&self) -> Vec<String> {
        self.opening_report
            .shown
            .as_ref()
            .map(|s| {
                let pos = &s.report.position;
                pos.legal_moves()
                    .into_iter()
                    .filter_map(|m| pos.san_with(m, SanSuffix::CheckOrMate).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The window as it is drawn.
    pub fn opening_report_view(&self) -> OpeningReportView<'_> {
        let ui = &self.opening_report;
        let phase = if let Some(job) = &ui.job {
            ReportPhase::Generating {
                search: job.search.progress(),
                generate: job.generate.progress(),
            }
        } else if ui.shown.is_some() {
            ReportPhase::Ready
        } else if ui.search_interrupted {
            ReportPhase::SearchInterrupted
        } else if let Some(failure) = ui.failure {
            ReportPhase::Failed(failure)
        } else {
            ReportPhase::Idle
        };
        let shown = ui.shown.as_ref();
        OpeningReportView {
            open: ui.open,
            phase,
            report: shown.map(|s| &*s.report),
            shared: shown.map(|s| s.report.clone()),
            options: shown.map(|s| &s.options),
            stale: shown.is_some_and(|s| !self.key_is_current(&s.key)),
            prefs: &self.prefs.opening_report,
            exclude: ui.exclude.as_deref(),
            favorites: ui.fav.list.items(),
            batch: ui.batch.as_ref().map(|b| BatchView {
                current: b.next.min(b.items.len()),
                total: b.items.len(),
                name: b
                    .next
                    .checked_sub(1)
                    .and_then(|i| b.items.get(i))
                    .map(|f| f.name.clone())
                    .unwrap_or_default(),
            }),
            database_name: shown.map(|s| s.database_name.as_str()),
        }
    }

    /// Cancels the job and the batch whose database is no longer the
    /// current one (D3621: switching, closing or changing the database
    /// stops the work). Called at the end of every `update`.
    pub(crate) fn refresh_opening_report(&mut self) {
        let stale_job = self
            .opening_report
            .job
            .as_ref()
            .is_some_and(|j| !self.key_is_current(&j.key));
        if stale_job && let Some(job) = self.opening_report.job.take() {
            job.cancel();
        }
        let stale_batch = self
            .opening_report
            .batch
            .as_ref()
            .is_some_and(|b| !self.key_is_current(&b.key));
        if stale_batch && let Some(batch) = self.opening_report.batch.take() {
            batch.control.cancel();
            self.orep_notice(OpeningReportNotice::BatchDone {
                written: batch.written,
                failed: batch.failed,
            });
        }
    }

    fn orep_options(&self, exclude: Option<String>) -> mychess_db::report::ReportOptions {
        let mut options = self.prefs.opening_report.options();
        options.exclude = exclude;
        options.normalized()
    }

    fn compute_job(
        &self,
        request: RequestId,
        id: SlotId,
        reference: Reference,
        options: mychess_db::report::ReportOptions,
        search: OpenControl,
        generate: OpenControl,
    ) -> Option<ComputeJob> {
        let slot = self.bases.slot(id)?;
        Some(ComputeJob {
            request,
            source: slot.source.clone(),
            reference,
            options,
            stats: (*self.prefs.stats).clone(),
            eco: self.eco_book.clone(),
            player_stats: self.player_stats(id),
            today: self.opening_report.today,
            search,
            generate,
        })
    }

    fn orep_start(&mut self, reference: &Reference, effects: &mut Vec<Effect>) {
        // A report while the batch of the favorites runs would compete with
        // it for the database (the "Update" button is off then too).
        if self.opening_report.batch.is_some() {
            self.orep_notice(OpeningReportNotice::Busy);
            return;
        }
        if let Some(job) = self.opening_report.job.take() {
            job.cancel();
        }
        let id = self.bases.current();
        let Some(key) = self.slot_key(id) else {
            self.orep_notice(OpeningReportNotice::Unavailable);
            return;
        };
        let request = self.new_request();
        let options = self.orep_options(self.opening_report.exclude.clone());
        let (search, generate) = (OpenControl::new(), OpenControl::new());
        let Some(job) = self.compute_job(
            request,
            id,
            reference.clone(),
            options.clone(),
            search.clone(),
            generate.clone(),
        ) else {
            return;
        };
        let database_name = self.base_name(id);
        let ui = &mut self.opening_report;
        ui.open = true;
        ui.search_interrupted = false;
        ui.failure = None;
        ui.reference = Some(reference.clone());
        ui.job = Some(Job {
            request,
            key,
            options,
            database_name,
            search,
            generate,
        });
        effects.push(Effect::OpeningReport(OpeningReportEffect::Compute(
            Box::new(job),
        )));
        self.ensure_favorites(effects);
    }

    /// Handles an [`OpeningReportMessage`].
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(crate) fn opening_report_message(
        &mut self,
        message: OpeningReportMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            OpeningReportMessage::Generate => {
                self.opening_report.exclude = None;
                let reference = self.board_reference();
                self.orep_start(&reference, effects);
            }
            OpeningReportMessage::OpenFavorite(index) => self.orep_open_favorite(index, effects),
            OpeningReportMessage::Refresh => {
                let reference = self
                    .opening_report
                    .reference
                    .clone()
                    .unwrap_or_else(|| self.board_reference());
                self.orep_start(&reference, effects);
            }
            OpeningReportMessage::Stop => {
                if let Some(job) = &self.opening_report.job {
                    job.cancel();
                }
                if let Some(batch) = &mut self.opening_report.batch {
                    batch.control.cancel();
                    batch.next = batch.items.len();
                }
            }
            OpeningReportMessage::Close => {
                self.opening_report.open = false;
                if let Some(job) = self.opening_report.job.take() {
                    job.cancel();
                }
            }
            OpeningReportMessage::SetExclude(exclude) => {
                self.opening_report.exclude = exclude.filter(|e| !e.trim().is_empty());
                let reference = self
                    .opening_report
                    .reference
                    .clone()
                    .unwrap_or_else(|| self.board_reference());
                self.orep_start(&reference, effects);
            }
            OpeningReportMessage::SetLabels(labels) => self.opening_report.labels = *labels,
            OpeningReportMessage::SetPrefs(prefs) => self.orep_prefs(|p| *p = *prefs, effects),
            OpeningReportMessage::ResetPrefs => self.orep_prefs(
                |p| {
                    *p = OpeningReportPrefs {
                        last_part: p.last_part,
                        last_format: p.last_format,
                        last_dir: p.last_dir.take(),
                        board_size: p.board_size,
                        ..OpeningReportPrefs::default()
                    };
                },
                effects,
            ),
            OpeningReportMessage::Select(selector) => self.orep_select(selector),
            OpeningReportMessage::LoadGame(number) => self.orep_load_game(number, effects),
            OpeningReportMessage::MergeGames => self.orep_merge(effects),
            OpeningReportMessage::Save { path, part, format } => {
                self.orep_save(&path, part, format, effects);
            }
            OpeningReportMessage::ViewHtml => self.orep_view(effects),
            OpeningReportMessage::Favorite(message) => self.orep_favorite(message, effects),
            OpeningReportMessage::Computed { request, outcome } => {
                self.orep_computed(request, *outcome);
            }
            OpeningReportMessage::Saved { request, result } => {
                self.orep_saved(request, result, effects);
            }
            OpeningReportMessage::HtmlWritten { request, result } => {
                if self.opening_report.view.pending != Some(request) {
                    return;
                }
                self.opening_report.view.pending = None;
                match result {
                    Some((path, dir)) => {
                        self.opening_report.view.dir = Some(dir);
                        effects.push(Effect::OpeningReport(OpeningReportEffect::OpenFile {
                            path,
                        }));
                    }
                    None => self.orep_notice(OpeningReportNotice::ViewFailed),
                }
            }
            OpeningReportMessage::FileOpened { ok } => {
                if !ok {
                    self.orep_notice(OpeningReportNotice::ViewFailed);
                }
            }
            OpeningReportMessage::FavoritesLoaded { request, result } => {
                self.orep_favorites_loaded(request, result);
            }
            OpeningReportMessage::FavoritesSaved { request, ok } => {
                if self.opening_report.fav.saving != Some(request) {
                    return;
                }
                self.opening_report.fav.saving = None;
                if ok {
                    // The copy was made with this write.
                    self.opening_report.fav.keep_backup = false;
                } else {
                    self.orep_notice(OpeningReportNotice::FavoritesSaveFailed);
                }
                if std::mem::take(&mut self.opening_report.fav.resave) {
                    self.save_favorites(effects);
                }
            }
            OpeningReportMessage::ReportsDatRead { request, result } => {
                self.orep_dat_read(request, result, effects);
            }
            OpeningReportMessage::BatchItem { request, result } => {
                self.orep_batch_item(request, &result, effects);
            }
        }
    }

    fn orep_prefs(
        &mut self,
        change: impl FnOnce(&mut OpeningReportPrefs),
        effects: &mut Vec<Effect>,
    ) {
        let mut next = (*self.prefs.opening_report).clone();
        change(&mut next);
        let next = next.sanitized();
        if next != *self.prefs.opening_report {
            self.prefs_message(
                PrefsMessage::Set(PrefChange::OpeningReport(Box::new(next))),
                effects,
            );
        }
    }

    fn orep_computed(&mut self, request: RequestId, outcome: ComputeOutcome) {
        if self.opening_report.job.as_ref().map(|j| j.request) != Some(request) {
            return;
        }
        let Some(job) = self.opening_report.job.take() else {
            return;
        };
        if !self.key_is_current(&job.key) {
            return;
        }
        let ui = &mut self.opening_report;
        match outcome {
            ComputeOutcome::Ready { report, generated } => {
                let interrupted = report.core.interrupted;
                ui.shown = Some(Shown {
                    key: job.key,
                    report,
                    options: job.options,
                    generated,
                    database_name: job.database_name,
                });
                if interrupted {
                    self.orep_notice(OpeningReportNotice::Interrupted);
                }
            }
            ComputeOutcome::SearchInterrupted => {
                ui.shown = None;
                ui.search_interrupted = true;
                self.orep_notice(OpeningReportNotice::Interrupted);
            }
            ComputeOutcome::Failed(failure) => {
                ui.shown = None;
                ui.failure = Some(failure);
                self.orep_notice(match failure {
                    ReportFailure::UnsupportedPosition => OpeningReportNotice::UnsupportedPosition,
                    ReportFailure::BadLine => OpeningReportNotice::BadLine,
                    _ => OpeningReportNotice::Failed,
                });
            }
        }
    }

    /// The report on the window, if it is there and still matches the
    /// database; otherwise says why not.
    fn orep_fresh(&mut self) -> Option<(SlotId, crate::opening_report::SharedReport)> {
        let Some(shown) = &self.opening_report.shown else {
            self.orep_notice(OpeningReportNotice::NoReport);
            return None;
        };
        if !self.key_is_current(&shown.key) {
            self.orep_notice(OpeningReportNotice::Stale);
            return None;
        }
        Some((shown.key.slot, shown.report.clone()))
    }

    /// A click on the text: the games of the selection become the filter
    /// of the database of the report (D3601, D3622).
    fn orep_select(&mut self, selector: ReportSelector) {
        if self.list_busy() {
            self.orep_notice(OpeningReportNotice::Busy);
            return;
        }
        let Some((id, report)) = self.orep_fresh() else {
            return;
        };
        let Some(slot) = self.bases.slot_mut(id) else {
            return;
        };
        slot.filter = select_games(&report.core, selector.engine(), slot.total());
        slot.filter_changed();
        slot.list.first = 1;
        self.orep_notice(OpeningReportNotice::FilterSet);
    }

    fn orep_load_game(&mut self, number: u32, effects: &mut Vec<Effect>) {
        let Some((id, report)) = self.orep_fresh() else {
            return;
        };
        let ply = report
            .core
            .lines
            .iter()
            .find(|l| l.game == number)
            .map_or(0, |l| usize::from(l.start_ply));
        self.advanced_load_first(id, number, ply, effects);
    }

    /// "Merge games": the best 50 by average rating, through the merge of
    /// M7-6 (D3614). The batch of that merge skips and counts the games it
    /// cannot merge instead of stopping at the first (m7-6, D3863).
    fn orep_merge(&mut self, effects: &mut Vec<Effect>) {
        let Some((id, report)) = self.orep_fresh() else {
            return;
        };
        let numbers: Vec<u32> = report
            .core
            .best(BestKind::HighRating, MERGE_GAMES)
            .iter()
            .map(|l| l.game)
            .collect();
        if numbers.is_empty() {
            self.orep_notice(OpeningReportNotice::NothingToMerge);
            return;
        }
        self.merge_message(MergeMessage::OpenBatch { slot: id, numbers }, effects);
    }

    /// Everything the program reads or may write later: the files of the
    /// open databases (with the other two files of an SI4 database), the
    /// file of the game on the board and of a save in flight (D2013).
    pub(crate) fn orep_protected(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for (_, slot) in self.bases.iter() {
            if let Some(path) = &slot.path {
                files.push(path.clone());
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("si4"))
                {
                    files.push(path.with_extension("sg4"));
                    files.push(path.with_extension("sn4"));
                }
            }
        }
        if let GameOrigin::PgnFile { path, .. } | GameOrigin::SavedFile { path } = &self.origin {
            files.push(path.clone());
        }
        if let Some(save) = &self.flow.save {
            files.push(save.path.clone());
        }
        files
    }

    fn render_job(&self, part: ReportPart, format: ReportFormat) -> Option<RenderJob> {
        let shown = self.opening_report.shown.as_ref()?;
        Some(RenderJob {
            report: shown.report.clone(),
            options: shown.options.clone(),
            labels: Box::new(self.opening_report.labels.clone()),
            part,
            format,
            database_name: shown.database_name.clone(),
            generated: shown.generated,
        })
    }

    fn orep_save(
        &mut self,
        path: &Path,
        part: ReportPart,
        format: ReportFormat,
        effects: &mut Vec<Effect>,
    ) {
        if self.opening_report.saving.is_some() {
            self.orep_notice(OpeningReportNotice::Busy);
            return;
        }
        let Some(render) = self.render_job(part, format) else {
            self.orep_notice(OpeningReportNotice::NoReport);
            return;
        };
        let path = with_report_extension(path, format);
        let request = self.new_request();
        self.opening_report.saving = Some((request, part, format));
        effects.push(Effect::OpeningReport(OpeningReportEffect::Save(Box::new(
            SaveJob {
                request,
                path,
                render,
                protected: self.orep_protected(),
            },
        ))));
    }

    fn orep_saved(
        &mut self,
        request: RequestId,
        result: Result<PathBuf, SaveFailure>,
        effects: &mut Vec<Effect>,
    ) {
        let Some((pending, part, format)) = self.opening_report.saving else {
            return;
        };
        if pending != request {
            return;
        }
        self.opening_report.saving = None;
        match result {
            Ok(path) => {
                let dir = path
                    .parent()
                    .map(Path::to_path_buf)
                    .filter(|d| !d.as_os_str().is_empty());
                self.orep_prefs(
                    |p| {
                        p.last_part = part;
                        p.last_format = format;
                        if dir.is_some() {
                            p.last_dir = dir;
                        }
                    },
                    effects,
                );
                self.orep_notice(OpeningReportNotice::Saved);
            }
            Err(SaveFailure::ProtectedFile) => self.orep_notice(OpeningReportNotice::SaveProtected),
            Err(_) => self.orep_notice(OpeningReportNotice::SaveFailed),
        }
    }

    fn orep_view(&mut self, effects: &mut Vec<Effect>) {
        let Some(html) = self.render_job(ReportPart::Full, ReportFormat::Html) else {
            self.orep_notice(OpeningReportNotice::NoReport);
            return;
        };
        let request = self.new_request();
        self.opening_report.view.pending = Some(request);
        effects.push(Effect::OpeningReport(OpeningReportEffect::WriteHtml {
            request,
            html: Box::new(html),
            dir: self.opening_report.view.dir.clone(),
        }));
    }

    // ---- favorites ----

    fn favorites_file(&self) -> Option<PathBuf> {
        self.flow.prefs_path.as_deref().map(favorites_path)
    }

    /// Starts the reading of the favorites file the first time. True when
    /// the list can be edited now.
    fn ensure_favorites(&mut self, effects: &mut Vec<Effect>) -> bool {
        let fav = &mut self.opening_report.fav;
        if fav.loaded {
            return true;
        }
        if fav.loading.is_some() {
            return false;
        }
        let Some(path) = self.favorites_file() else {
            self.opening_report.fav.loaded = true;
            return true;
        };
        let request = self.new_request();
        self.opening_report.fav.loading = Some(request);
        effects.push(Effect::OpeningReport(OpeningReportEffect::LoadFavorites {
            request,
            path,
        }));
        false
    }

    fn orep_favorites_loaded(
        &mut self,
        request: RequestId,
        result: Result<LoadedFavorites, crate::error::FileError>,
    ) {
        if self.opening_report.fav.loading != Some(request) {
            return;
        }
        let fav = &mut self.opening_report.fav;
        fav.loading = None;
        match result {
            Ok(loaded) => {
                // Only a successful read allows edits: after a failure the
                // file may be fine and an edit would write over it (D3660).
                fav.loaded = true;
                fav.list = loaded.favorites;
                fav.keep_backup = matches!(
                    loaded.issue,
                    Some(FavoritesIssue::NewerVersion(_) | FavoritesIssue::Dropped(_))
                );
                if matches!(loaded.issue, Some(FavoritesIssue::Unreadable)) {
                    self.orep_notice(OpeningReportNotice::FavoritesCorrupt);
                }
            }
            Err(_) => self.orep_notice(OpeningReportNotice::FavoritesLoadFailed),
        }
    }

    fn save_favorites(&mut self, effects: &mut Vec<Effect>) {
        let Some(path) = self.favorites_file() else {
            return;
        };
        if !self.opening_report.fav.loaded {
            return;
        }
        if self.opening_report.fav.saving.is_some() {
            self.opening_report.fav.resave = true;
            return;
        }
        let request = self.new_request();
        self.opening_report.fav.saving = Some(request);
        effects.push(Effect::OpeningReport(OpeningReportEffect::SaveFavorites {
            request,
            path,
            favorites: self.opening_report.fav.list.clone(),
            backup: self.opening_report.fav.keep_backup,
        }));
    }

    fn favorite_error(&mut self, error: FavoriteError) {
        self.orep_notice(match error {
            FavoriteError::EmptyName => OpeningReportNotice::FavoriteNameEmpty,
            FavoriteError::NameTaken => OpeningReportNotice::FavoriteNameTaken,
            FavoriteError::Missing => OpeningReportNotice::FavoriteMissing,
            _ => OpeningReportNotice::FavoriteLimit,
        });
    }

    fn orep_favorite(&mut self, message: FavoriteMessage, effects: &mut Vec<Effect>) {
        if !self.ensure_favorites(effects) {
            self.orep_notice(OpeningReportNotice::FavoritesLoading);
            return;
        }
        let changed = match message {
            FavoriteMessage::Add { name } => {
                let reference = self.board_reference();
                if reference.start.is_some() {
                    self.orep_notice(OpeningReportNotice::FavoriteStart);
                    return;
                }
                let Some(moves) = moves_text(&Position::new(), &reference.stem) else {
                    self.orep_notice(OpeningReportNotice::FavoriteMoves);
                    return;
                };
                self.opening_report
                    .fav
                    .list
                    .add(&name, &moves)
                    .map(|()| true)
            }
            FavoriteMessage::Rename { index, name } => self
                .opening_report
                .fav
                .list
                .rename(index, &name)
                .map(|()| true),
            FavoriteMessage::Delete(index) => {
                self.opening_report.fav.list.delete(index).map(|_| true)
            }
            FavoriteMessage::MoveUp(index) => self
                .opening_report
                .fav
                .list
                .move_up(index)
                .map(|n| n != index),
            FavoriteMessage::MoveDown(index) => self
                .opening_report
                .fav
                .list
                .move_down(index)
                .map(|n| n != index),
            FavoriteMessage::Import(path) => {
                let request = self.new_request();
                self.opening_report.fav.importing = Some(request);
                effects.push(Effect::OpeningReport(OpeningReportEffect::ReadReportsDat {
                    request,
                    path,
                }));
                return;
            }
            FavoriteMessage::GenerateAll { dir, part, format } => {
                self.orep_batch_start(dir, part, format, effects);
                return;
            }
            FavoriteMessage::StopBatch => {
                self.opening_report_message(OpeningReportMessage::Stop, effects);
                return;
            }
        };
        match changed {
            Ok(true) => self.save_favorites(effects),
            Ok(false) => {}
            Err(error) => self.favorite_error(error),
        }
    }

    fn orep_dat_read(
        &mut self,
        request: RequestId,
        result: Result<ReportsDat, crate::error::FileError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.opening_report.fav.importing != Some(request) {
            return;
        }
        self.opening_report.fav.importing = None;
        match result {
            Ok(dat) => {
                let (added, skipped) = self.opening_report.fav.list.import(dat.favorites);
                self.orep_notice(OpeningReportNotice::FavoritesImported {
                    added,
                    skipped: skipped.saturating_add(dat.ignored),
                });
                if added > 0 {
                    self.save_favorites(effects);
                }
            }
            Err(_) => self.orep_notice(OpeningReportNotice::ImportFailed),
        }
    }

    /// The report of favorite `index`. The game on the board is not
    /// replaced (D3650: the Scid imports the line into the game).
    fn orep_open_favorite(&mut self, index: usize, effects: &mut Vec<Effect>) {
        if !self.ensure_favorites(effects) {
            self.orep_notice(OpeningReportNotice::FavoritesLoading);
            return;
        }
        let Some(favorite) = self.opening_report.fav.list.get(index) else {
            self.orep_notice(OpeningReportNotice::FavoriteMissing);
            return;
        };
        let Some(stem) = parse_line(&favorite.moves) else {
            self.orep_notice(OpeningReportNotice::FavoriteMoves);
            return;
        };
        self.opening_report.exclude = None;
        self.orep_start(&Reference { start: None, stem }, effects);
    }

    // ---- the batch of reports (D3620) ----

    fn orep_batch_start(
        &mut self,
        dir: PathBuf,
        part: ReportPart,
        format: ReportFormat,
        effects: &mut Vec<Effect>,
    ) {
        if self.opening_report.batch.is_some() {
            self.orep_notice(OpeningReportNotice::Busy);
            return;
        }
        let items = self.opening_report.fav.list.items().to_vec();
        if items.is_empty() {
            self.orep_notice(OpeningReportNotice::NoFavorites);
            return;
        }
        let id = self.bases.current();
        let Some(key) = self.slot_key(id) else {
            self.orep_notice(OpeningReportNotice::Unavailable);
            return;
        };
        let request = self.new_request();
        let options = self.orep_options(None);
        let labels = Box::new(self.opening_report.labels.clone());
        self.opening_report.batch = Some(Batch {
            request,
            key,
            items,
            next: 0,
            dir: dir.clone(),
            part,
            format,
            database_name: self.base_name(id),
            options,
            labels,
            written: 0,
            failed: 0,
            used: Vec::new(),
            control: OpenControl::new(),
        });
        self.orep_prefs(
            |p| {
                p.last_part = part;
                p.last_format = format;
                p.last_dir = Some(dir);
            },
            effects,
        );
        self.orep_batch_next(effects);
    }

    /// Sends the next favorite that can be made, or ends the batch.
    fn orep_batch_next(&mut self, effects: &mut Vec<Effect>) {
        loop {
            let Some(batch) = &mut self.opening_report.batch else {
                return;
            };
            let Some(item) = batch.items.get(batch.next).cloned() else {
                let (written, failed) = (batch.written, batch.failed);
                self.opening_report.batch = None;
                self.orep_notice(OpeningReportNotice::BatchDone { written, failed });
                return;
            };
            batch.next += 1;
            let Some(stem) = parse_line(&item.moves) else {
                batch.failed += 1;
                continue;
            };
            let ext = batch.format.extension();
            let stem_name = sanitize_file_name(&item.name);
            let mut file = format!("{stem_name}.{ext}");
            let mut copy = 1u32;
            while batch.used.contains(&file.to_lowercase()) {
                copy += 1;
                file = format!("{stem_name} ({copy}).{ext}");
            }
            batch.used.push(file.to_lowercase());
            let path = batch.dir.join(file);
            let (batch_written, batch_failed) = (batch.written, batch.failed);
            let (key, control, part, format, database_name, options, labels) = (
                batch.key.clone(),
                batch.control.clone(),
                batch.part,
                batch.format,
                batch.database_name.clone(),
                batch.options.clone(),
                batch.labels.clone(),
            );
            // Every item has its own request, so a late answer of an item
            // that was stopped cannot be taken for the next.
            let request = self.new_request();
            if let Some(batch) = &mut self.opening_report.batch {
                batch.request = request;
            }
            let Some(compute) = self.compute_job(
                request,
                key.slot,
                Reference { start: None, stem },
                options,
                control.clone(),
                control,
            ) else {
                // The database is gone: end the batch and tell what was
                // done, counting this item as failed.
                let written = batch_written;
                let failed = batch_failed.saturating_add(1);
                self.opening_report.batch = None;
                self.orep_notice(OpeningReportNotice::BatchDone { written, failed });
                return;
            };
            let job = BatchJob {
                request,
                path,
                compute,
                part,
                format,
                labels,
                database_name,
                protected: self.orep_protected(),
            };
            effects.push(Effect::OpeningReport(OpeningReportEffect::BatchItem(
                Box::new(job),
            )));
            return;
        }
    }

    fn orep_batch_item(
        &mut self,
        request: RequestId,
        result: &Result<PathBuf, BatchFailure>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(batch) = &mut self.opening_report.batch else {
            return;
        };
        if batch.request != request {
            return;
        }
        match result {
            Ok(_) => batch.written += 1,
            Err(BatchFailure::Stopped) => batch.next = batch.items.len(),
            Err(_) => batch.failed += 1,
        }
        self.orep_batch_next(effects);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_is_added_unless_the_name_has_one_of_the_format() {
        let p = |s: &str, f| with_report_extension(Path::new(s), f);
        assert_eq!(p("r", ReportFormat::Html), Path::new("r.html"));
        assert_eq!(p("r.HTML", ReportFormat::Html), Path::new("r.HTML"));
        assert_eq!(p("r.Htm", ReportFormat::Html), Path::new("r.Htm"));
        assert_eq!(p("r.TXT", ReportFormat::Text), Path::new("r.TXT"));
        assert_eq!(p("r.ltx", ReportFormat::Latex), Path::new("r.ltx"));
        assert_eq!(p("r.html", ReportFormat::Text), Path::new("r.html.txt"));
        assert_eq!(p("a.b", ReportFormat::Latex), Path::new("a.b.tex"));
    }

    fn clipbase_report() -> AppState {
        let mut state = AppState::default();
        let mut effects = Vec::new();
        state.opening_report_message(OpeningReportMessage::Generate, &mut effects);
        let Some(Effect::OpeningReport(OpeningReportEffect::Compute(job))) = effects.pop() else {
            panic!("a job was expected");
        };
        let request = job.request;
        let outcome = crate::opening_report::run_compute_for_tests(&job);
        state.opening_report_message(
            OpeningReportMessage::Computed {
                request,
                outcome: Box::new(outcome),
            },
            &mut effects,
        );
        state
    }

    #[test]
    fn the_report_is_stale_when_the_games_under_the_slot_change() {
        // What saving a game, compacting and sorting the database do to
        // the slot (D3621, criterion 27).
        for change in [
            |s: &mut crate::bases::BaseSlot| s.data_changed(),
            |s: &mut crate::bases::BaseSlot| s.clear_sort(),
        ] {
            let mut state = clipbase_report();
            let view = state.opening_report_view();
            assert_eq!(view.phase, ReportPhase::Ready);
            assert!(!view.stale);
            change(state.bases.slot_mut(SlotId::CLIPBASE).expect("clipbase"));
            assert!(state.opening_report_view().stale);
        }
    }

    #[test]
    fn refresh_during_a_batch_starts_no_job() {
        let mut state = clipbase_report();
        let key = state.slot_key(SlotId::CLIPBASE).expect("clipbase key");
        state.opening_report.batch = Some(Batch {
            request: state.new_request(),
            key,
            items: Vec::new(),
            next: 0,
            dir: PathBuf::new(),
            part: ReportPart::Compact,
            format: ReportFormat::Text,
            database_name: String::new(),
            options: state.orep_options(None),
            labels: Box::default(),
            written: 0,
            failed: 0,
            used: Vec::new(),
            control: OpenControl::new(),
        });
        let mut effects = Vec::new();
        state.opening_report_message(OpeningReportMessage::Refresh, &mut effects);
        assert!(effects.is_empty(), "{effects:?}");
        assert!(state.opening_report.job.is_none());
        assert_eq!(
            state.notice,
            Some(AppNotice::OpeningReport(OpeningReportNotice::Busy))
        );
    }

    #[test]
    fn a_batch_that_cannot_start_an_item_ends_with_the_note() {
        let mut state = clipbase_report();
        let key = state.slot_key(SlotId::CLIPBASE).expect("clipbase key");
        // A key of a slot that is not there: no job can be made.
        let missing = ReportKey {
            slot: SlotId::new(0).expect("slot 0"),
            ..key
        };
        state.opening_report.batch = Some(Batch {
            request: state.new_request(),
            key: missing,
            items: vec![crate::opening_report::Favorite {
                name: "One".to_owned(),
                moves: "1.e4".to_owned(),
            }],
            next: 0,
            dir: PathBuf::new(),
            part: ReportPart::Compact,
            format: ReportFormat::Text,
            database_name: String::new(),
            options: state.orep_options(None),
            labels: Box::default(),
            written: 2,
            failed: 0,
            used: Vec::new(),
            control: OpenControl::new(),
        });
        let mut effects = Vec::new();
        state.orep_batch_next(&mut effects);
        assert!(effects.is_empty(), "{effects:?}");
        assert!(state.opening_report.batch.is_none());
        assert_eq!(
            state.notice,
            Some(AppNotice::OpeningReport(OpeningReportNotice::BatchDone {
                written: 2,
                failed: 1
            }))
        );
    }

    #[test]
    fn lines_are_written_and_read_as_the_scid_does() {
        let stem = parse_line("1.e4 c5 2.Nf3").expect("legal line");
        assert_eq!(stem.len(), 3);
        assert_eq!(
            moves_text(&Position::new(), &stem).as_deref(),
            Some("1.e4 c5 2.Nf3")
        );
        assert!(parse_line("1.e4 e4").is_none());
        assert_eq!(parse_line("").map(|m| m.len()), Some(0));
    }
}
